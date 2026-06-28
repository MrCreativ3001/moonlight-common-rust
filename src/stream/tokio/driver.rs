use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Instant as StdInstant,
};

use sans_io_time::Instant;
use tokio::{
    io::ReadBuf,
    net::UdpSocket,
    spawn,
    time::{Sleep, sleep_until},
};
use tracing::{Instrument, Span, debug, error};

use crate::stream::{proto::runtime::UdpStream, tokio::MoonlightStreamError};

pub async fn bind_udp_stream<Stream>(
    stream: Stream,
    span: Span,
) -> Result<StreamRef<Stream>, MoonlightStreamError>
where
    Stream: TokioStreamExt + 'static,
    MoonlightStreamError: From<Stream::Error>,
{
    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    let stream_ref = StreamRef(Arc::new(StreamInner {
        state: Mutex::new(State {
            base_instant: StdInstant::now(),
            stream,
            waker: None,
            timer: None,
            timer_deadline: None,
            socket,
            recv_buffer: vec![0; 4096],
        }),
        notify: Stream::Notifier::default(),
    }));

    let mut driver = StreamDriver(stream_ref.clone());
    spawn(
        async move {
            if let Err(err) = (&mut driver).await {
                error!(error = %err, "stream driver errored");
            }

            Stream::on_stop(driver.0.notify());

            debug!("stopped driver");
        }
        .instrument(span),
    );

    Ok(stream_ref)
}

pub(super) trait TokioStreamExt: UdpStream {
    type Notifier: Default + Send + Sync + 'static;

    fn on_event(event: Self::Event, notify: &Self::Notifier);
    fn on_stop(notify: &Self::Notifier);
}

#[derive(Clone)]
struct StreamDriver<Stream>(StreamRef<Stream>)
where
    Stream: TokioStreamExt;

impl<Stream> Future for StreamDriver<Stream>
where
    Stream: TokioStreamExt,
    MoonlightStreamError: From<Stream::Error>,
{
    type Output = Result<(), MoonlightStreamError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.0.state.lock().expect("StreamDriver::poll");

        if let Err(err) = guard.drive(cx) {
            return Poll::Ready(Err(err));
        }
        guard.forward_events(&self.0.0.notify);

        Poll::Pending
    }
}

pub(super) struct StreamRef<Stream>(Arc<StreamInner<Stream>>)
where
    Stream: TokioStreamExt;

impl<Stream> Clone for StreamRef<Stream>
where
    Stream: TokioStreamExt,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Stream> StreamRef<Stream>
where
    Stream: TokioStreamExt,
    MoonlightStreamError: From<Stream::Error>,
{
    pub fn stream_mut<T>(&self, f: impl FnOnce(&mut Stream) -> (bool, T)) -> T {
        let mut guard = self.0.state.lock().expect("use_stream");

        let (should_wake, result) = f(&mut guard.stream);
        if should_wake {
            guard.wake();
        }

        result
    }

    pub fn notify(&self) -> &Stream::Notifier {
        &self.0.notify
    }
}

struct StreamInner<Stream>
where
    Stream: TokioStreamExt,
{
    state: Mutex<State<Stream>>,
    notify: Stream::Notifier,
}

struct State<Stream> {
    base_instant: StdInstant,
    stream: Stream,
    waker: Option<Waker>,
    timer: Option<Pin<Box<Sleep>>>,
    timer_deadline: Option<Instant>,
    socket: UdpSocket,
    recv_buffer: Vec<u8>,
}

impl<Stream> State<Stream>
where
    Stream: TokioStreamExt,
    MoonlightStreamError: From<Stream::Error>,
{
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn forward_events(&mut self, notify: &Stream::Notifier) {
        while let Some(event) = self.stream.poll_event() {
            Stream::on_event(event, notify);
        }
    }

    fn drive(&mut self, cx: &mut Context<'_>) -> Result<(), MoonlightStreamError> {
        self.waker = Some(cx.waker().clone());

        loop {
            self.drive_send(cx)?;

            if self.drive_recv(cx)? {
                continue;
            }
            if self.drive_timer(cx)? {
                continue;
            }

            break;
        }

        Ok(())
    }

    fn drive_send(&mut self, cx: &mut Context<'_>) -> Result<(), MoonlightStreamError> {
        // While we can send
        while let Some((addr, send)) = self.stream.pending_send() {
            // See if we can write
            if self.socket.poll_send_ready(cx).is_pending() {
                return Ok(());
            }

            // Try to send the packet
            match self.socket.try_send_to(send, addr) {
                Ok(_) => {
                    self.stream.consume_send();
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            }
        }

        Ok(())
    }

    fn drive_recv(&mut self, cx: &mut Context<'_>) -> Result<bool, MoonlightStreamError> {
        let mut buffer = ReadBuf::new(&mut self.recv_buffer);

        if let Poll::Ready(result) = self.socket.poll_recv_from(cx, &mut buffer) {
            let addr = result?;

            self.stream.handle_receive(
                Instant::from_std(self.base_instant),
                addr,
                buffer.filled(),
            )?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn drive_timer(&mut self, cx: &mut Context<'_>) -> Result<bool, MoonlightStreamError> {
        // If we've got a deadline
        let Some(deadline) = self.stream.poll_timeout() else {
            self.timer = None;
            self.timer_deadline = None;
            return Ok(false);
        };

        // Adjust or Create timer, if necessary
        if let Some(timer) = &mut self.timer {
            // See if timer needs to be changed
            if self
                .timer_deadline
                .expect("timer deadline should exist in this state")
                != deadline
            {
                timer
                    .as_mut()
                    .reset(deadline.to_std(self.base_instant).into());
            }
        } else {
            // Create new timer
            self.timer = Some(Box::pin(sleep_until(
                deadline.to_std(self.base_instant).into(),
            )));
        }
        self.timer_deadline = Some(deadline);

        let timer = self
            .timer
            .as_mut()
            .expect("timer should exist in this state");

        // See if deadline expired
        if timer.as_mut().poll(cx).is_ready() {
            self.timer = None;
            self.timer_deadline = None;

            self.stream
                .handle_timeout(Instant::from_std(self.base_instant))?;
            return Ok(true);
        }

        Ok(false)
    }
}
