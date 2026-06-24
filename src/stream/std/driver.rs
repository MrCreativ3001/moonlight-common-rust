use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant as StdInstant};
use std::{io, thread};

use sans_io_time::Instant;
use tracing::{Level, Span, debug, instrument, trace};

use crate::stream::{proto::stream::UdpStream, std::MoonlightStreamError};

const UDP_BUFFER_CAPACITY: usize = 4096;

pub struct SyncUdpDriver<Stream> {
    addr: SocketAddr,
    socket: UdpSocket,
    base_time: StdInstant,
    stream_condvar: Condvar,
    stream: Mutex<Stream>,
    stopped: AtomicBool,
}

impl<Stream> SyncUdpDriver<Stream>
where
    Stream: UdpStream,
    MoonlightStreamError: From<Stream::Error>,
{
    pub fn connect(
        base_time: StdInstant,
        addr: SocketAddr,
        stream: Stream,
    ) -> Result<Self, io::Error> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        Ok(Self {
            addr,
            socket,
            base_time,
            stream_condvar: Condvar::new(),
            stream: Mutex::new(stream),
            stopped: AtomicBool::new(false),
        })
    }

    #[instrument(level = Level::TRACE, skip(self, f))]
    pub fn stream_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Stream) -> R,
    {
        let mut guard = self.stream.lock().expect("lock stream failed");
        let res = f(&mut guard);
        self.stream_condvar.notify_all();
        res
    }

    pub fn run(&self) -> Result<(), MoonlightStreamError> {
        if self.is_stopped() {
            return Ok(());
        }

        let span = Span::current();

        thread::scope::<_, Result<(), MoonlightStreamError>>(|scope| {
            debug!("starting udp driver threads");

            let send =
                scope.spawn(|| span.in_scope(|| self.blocking_send().inspect_err(|_| self.stop())));
            let recv =
                scope.spawn(|| span.in_scope(|| self.blocking_recv().inspect_err(|_| self.stop())));
            let timeout = scope
                .spawn(|| span.in_scope(|| self.blocking_timeout().inspect_err(|_| self.stop())));

            let send_res = send.join();
            let recv_res = recv.join();
            let timeout_res = timeout.join();

            send_res.map_err(MoonlightStreamError::ThreadJoin)??;
            recv_res.map_err(MoonlightStreamError::ThreadJoin)??;
            timeout_res.map_err(MoonlightStreamError::ThreadJoin)??;

            Ok(())
        })?;

        self.stop();

        Ok(())
    }

    #[instrument(level = Level::TRACE, skip(self))]
    fn blocking_send(&self) -> Result<(), MoonlightStreamError> {
        debug!("started sending thread");

        // This handles sending packets

        let mut len = 0;
        let mut buffer = vec![0; UDP_BUFFER_CAPACITY];
        let mut stream = self.stream.lock().expect("lock stream failed");

        self.socket
            .set_write_timeout(Some(Duration::from_secs(1)))?;
        loop {
            if self.is_stopped() {
                break;
            }

            if len != 0 {
                // We were blocked to see if this thread should stop
            } else if let Some(pending_send) = stream.pending_send() {
                trace!(pending_send = ?pending_send, "got pending sending buffer");

                len = pending_send.len();
                buffer[0..len].copy_from_slice(pending_send);

                stream.consume_send();
            } else {
                // Wait for pending packet
                trace!("waiting for change of stream");
                stream = self.stream_condvar.wait(stream).expect("wait on stream");
                continue;
            }
            drop(stream);

            trace!("sending packet");
            // Send packet
            match self.socket.send_to(&buffer[0..len], self.addr) {
                Ok(_) => {
                    trace!(packet = &buffer[0..len], "successfully sent packet");
                    // Submit packet using len = 0
                    len = 0;
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if self.is_stopped() {
                        return Ok(());
                    }
                }
                Err(err) => return Err(err.into()),
            }
            trace!("finished sending packet");

            // Re-lock stream
            stream = self.stream.lock().expect("lock stream failed");
        }

        Ok(())
    }

    #[instrument(level = Level::TRACE, skip(self))]
    fn blocking_recv(&self) -> Result<(), MoonlightStreamError> {
        debug!("started receiving thread");

        // This handles receiving packets

        let mut buffer = vec![0; UDP_BUFFER_CAPACITY];

        self.socket.set_read_timeout(Some(Duration::from_secs(1)))?;
        loop {
            let (len, addr) = match self.socket.recv_from(&mut buffer) {
                Ok(value) => value,
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if self.is_stopped() {
                        return Ok(());
                    }
                    continue;
                }
                Err(err) => return Err(err.into()),
            };

            if self.is_stopped() {
                break;
            }

            if addr != self.addr {
                // Discard packet
                continue;
            }
            trace!(packet = ?buffer[0..len], "received packet");

            let mut stream = self.stream.lock().expect("lock stream failed");
            stream.handle_receive(Instant::from_std(self.base_time), &buffer[0..len])?;

            self.stream_condvar.notify_all();
        }

        Ok(())
    }

    #[instrument(level = Level::TRACE, skip(self))]
    fn blocking_timeout(&self) -> Result<(), MoonlightStreamError> {
        debug!("started timeout thread");

        // This handles timeouts

        let mut stream = self.stream.lock().expect("lock stream failed");
        loop {
            if self.is_stopped() {
                break;
            }

            let deadline = stream.poll_timeout().map(|x| x.to_std(self.base_time));

            if let Some(deadline) = deadline {
                let timeout = deadline
                    .checked_duration_since(StdInstant::now())
                    // Make sure that the other threads can have access
                    .unwrap_or(Duration::from_millis(1));

                trace!(timeout = ?timeout, "waiting on stream");

                let (new_stream, result) = self
                    .stream_condvar
                    .wait_timeout(stream, timeout)
                    .expect("wait on stream");
                stream = new_stream;

                if result.timed_out() {
                    trace!("handling timeout");

                    stream
                        .handle_timeout(Instant::from_std(self.base_time))
                        .map_err(MoonlightStreamError::from)?;
                    self.stream_condvar.notify_all();
                } else {
                    trace!("not handling timeout because the timeout wasn't reached");
                }
            } else {
                trace!("waiting on stream without timeout");

                stream = self.stream_condvar.wait(stream).expect("wait on stream");
            }
        }

        Ok(())
    }

    /// Returns [None] if the stream was stopped
    pub fn blocking_poll_event(&self) -> Option<Stream::Event> {
        let mut stream = self.stream.lock().expect("lock stream failed");

        loop {
            if self.is_stopped() {
                return None;
            }

            if let Some(event) = stream.poll_event() {
                return Some(event);
            } else {
                trace!("no events found");
            }

            trace!("waiting on stream for events");
            stream = self
                .stream_condvar
                .wait(stream)
                .expect("wait on stream failed");
        }
    }
}

impl<Stream> SyncUdpDriver<Stream> {
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.stream_condvar.notify_all();
    }
}
