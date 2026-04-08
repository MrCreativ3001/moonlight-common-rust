use std::{
    any::Any,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::{Arc, Condvar, Mutex},
    thread::{JoinHandle, sleep, spawn},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Span, debug, debug_span, error, info, info_span, trace, warn};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        MoonlightStreamConfig, MoonlightStreamSettings,
        audio::{AudioConfig, AudioDecoder},
        connection::ConnectionListener,
        proto::{
            MOONLIGHT_STREAM_SETUP_TCP_CONNECT_TIMEOUT, MoonlightStreamInput,
            MoonlightStreamProtoError, MoonlightStreamSetup, MoonlightStreamSetupOutput,
            audio::{AudioStream, AudioStreamError, AudioStreamInput, AudioStreamOutput},
            control::{
                ClientInputEvent, ControlStream, ControlStreamEvent, ControlStreamInput,
                ControlStreamOutput,
                packet::ControlPacket,
                peer::{ControlError, ControlHostAction, ControlHostInput},
            },
            crypto::CryptoBackend,
            video::{VideoStream, VideoStreamError, VideoStreamInput, VideoStreamOutput},
        },
        std::{ringbuffer::RingBuffer, signal::StopSignal},
        video::{ColorSpace, FrameType, VideoDecodeUnit, VideoDecoder, VideoFrameBuffer},
    },
};

mod ringbuffer;
mod signal;

#[derive(Debug, Error)]
pub enum MoonlightStreamError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("proto: {0}")]
    Proto(#[from] MoonlightStreamProtoError),
    #[error("audio stream: {0}")]
    Audio(#[from] AudioStreamError),
    #[error("video stream: {0}")]
    Video(#[from] VideoStreamError),
    #[error("control stream: {0}")]
    Control(#[from] ControlError),
    #[error("thread join: {0:?}")]
    ThreadJoin(Box<dyn Any + Send + 'static>),
    #[error("exceeded first frame timeout")]
    FirstFrameTimeout,
}

pub struct MoonlightStream {
    inner: Arc<SharedInner>,
    threads: Threads,
    stop: StopSignal,
}

#[derive(Debug, Default)]
struct Threads {
    audio_stream: Option<JoinHandle<()>>,
    video_stream: Option<JoinHandle<()>>,
    control_stream_sender: Option<JoinHandle<()>>,
    control_stream_receiver: Option<JoinHandle<()>>,
}

impl Threads {
    /// This won't call the stop signal!
    fn try_join_all(&mut self, mut on_error: impl FnMut(Box<dyn Any + Send + 'static>)) {
        debug!("trying to join all threads of this stream");

        if let Some(audio_stream) = self.audio_stream.take() {
            if let Err(err) = audio_stream.join() {
                on_error(err);
            }
        } else {
            debug!("audio_stream_thread doesn't exist");
        }
        if let Some(video_stream) = self.video_stream.take() {
            if let Err(err) = video_stream.join() {
                on_error(err);
            }
        } else {
            debug!("video_stream_thread doesn't exist");
        }
        if let Some(control_stream_sender) = self.control_stream_sender.take() {
            if let Err(err) = control_stream_sender.join() {
                on_error(err);
            }
        } else {
            debug!("control_stream_sender_thread doesn't exist");
        }
        if let Some(control_stream_receiver) = self.control_stream_receiver.take() {
            if let Err(err) = control_stream_receiver.join() {
                on_error(err);
            }
        } else {
            debug!("control_stream_receiver_thread doesn't exist");
        }

        debug!("finished thread cleanup");
    }
    fn take(&mut self) -> Self {
        Self {
            audio_stream: self.audio_stream.take(),
            video_stream: self.video_stream.take(),
            control_stream_sender: self.control_stream_sender.take(),
            control_stream_receiver: self.control_stream_receiver.take(),
        }
    }
}

struct SharedInner {
    control_stream: Mutex<Option<ControlStream<Arc<dyn CryptoBackend + Send>>>>,
    control_notify: Condvar,
    first_frame: Mutex<FirstFrame>,
    first_frame_notify: Condvar,
}

#[derive(Debug, Default)]
struct FirstFrame {
    has_audio: bool,
    has_video: bool,
    has_control: bool,
}

// TODO: how to handle errors, maybe in the connection listener?

impl MoonlightStream {
    pub fn launch_query_parameters() -> &'static str {
        MoonlightStreamSetup::<DisabledCryptoBackend>::launch_query_parameters()
    }

    pub fn connect<Crypto>(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        video_decoder: impl VideoDecoder + Send + 'static,
        audio_decoder: impl AudioDecoder + Send + 'static,
        connection_listener: impl ConnectionListener + Send + 'static,
        crypto_backend: Crypto,
    ) -> Result<Self, MoonlightStreamError>
    where
        Crypto: CryptoBackend + Clone + 'static,
    {
        let stop = StopSignal::new();

        let mut threads = Threads::default();

        let inner = match Self::connect_inner(
            config,
            settings,
            video_decoder,
            audio_decoder,
            connection_listener,
            crypto_backend,
            &mut threads,
            &stop,
        ) {
            Ok(value) => value,
            Err(err) => {
                error!(error = ?err, "failed to start moonlight stream");
                info!("cleaning up all threads");

                // stop, wait all threads, then error
                stop.stop();

                threads.try_join_all(|err| {
                    warn!(error = ?err, "error whilst joining thread");
                });

                return Err(err);
            }
        };

        Ok(Self {
            inner,
            threads,
            stop,
        })
    }
    fn connect_inner<Crypto>(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        video_decoder: impl VideoDecoder + Send + 'static,
        audio_decoder: impl AudioDecoder + Send + 'static,
        connection_listener: impl ConnectionListener + Send + 'static,
        crypto_backend: Crypto,
        threads: &mut Threads,
        stop: &StopSignal,
    ) -> Result<Arc<SharedInner>, MoonlightStreamError>
    where
        Crypto: CryptoBackend + Clone + 'static,
    {
        let span = info_span!("stream");
        let _enter = span.enter();

        let crypto_backend: Arc<dyn CryptoBackend + Send + 'static> = Arc::new(crypto_backend);

        let mut tcp_stream: Option<TcpStream> = None;
        let mut recv_buffer = vec![0; 2048];

        let mut audio_decoder = Some(audio_decoder);
        let mut video_decoder = Some(video_decoder);
        let mut connection_listener = Some(connection_listener);

        let shared_inner = Arc::new(SharedInner {
            control_notify: Condvar::new(),
            control_stream: Mutex::new(None),
            first_frame: Mutex::new(FirstFrame::default()),
            first_frame_notify: Condvar::new(),
        });

        let mut setup =
            MoonlightStreamSetup::new(Instant::now(), config, settings, crypto_backend)?;

        loop {
            match setup.poll_output()? {
                MoonlightStreamSetupOutput::Timeout(timeout) => {
                    let sleep_duration = timeout.saturating_duration_since(Instant::now());

                    if let Some(stream) = tcp_stream.as_mut() {
                        stream.set_read_timeout(Some(sleep_duration))?;

                        let len = match stream.read(&mut recv_buffer) {
                            Ok(len) => len,
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    io::ErrorKind::ConnectionReset
                                        | io::ErrorKind::ConnectionAborted
                                ) =>
                            {
                                0
                            }
                            Err(err) => return Err(err.into()),
                        };
                        trace!(bytes_len = len, "receive tcp bytes");

                        if len == 0 {
                            setup.handle_input(MoonlightStreamInput::TcpDisconnected(
                                Instant::now(),
                            ))?;

                            tcp_stream = None;
                        } else {
                            setup.handle_input(MoonlightStreamInput::TcpReceive {
                                now: Instant::now(),
                                data: &recv_buffer[0..len],
                            })?;
                        }

                        continue;
                    } else {
                        sleep(sleep_duration);

                        // Timeout is a fallthrough
                    }
                }
                MoonlightStreamSetupOutput::TcpConnect { addr } => {
                    trace!(addr = ?addr, "opening tcp stream");

                    let new_stream = TcpStream::connect_timeout(
                        &addr,
                        MOONLIGHT_STREAM_SETUP_TCP_CONNECT_TIMEOUT,
                    )?;
                    new_stream.set_nodelay(true)?;

                    tcp_stream = Some(new_stream);
                }
                MoonlightStreamSetupOutput::TcpWrite { data } => {
                    let tcp_stream = tcp_stream.as_mut().expect("MoonlightStreamSetup issued a TcpWrite action, however no TcpStream is currently connected!");

                    tcp_stream.write_all(&data)?;
                }
                MoonlightStreamSetupOutput::StartAudioStream { addr, audio_stream } => {
                    let audio_decoder = audio_decoder
                        .take()
                        .expect("audio decoder was already taken");

                    threads.audio_stream = Some(audio_thread(
                        info_span!("audio_stream"),
                        stop.clone(),
                        addr,
                        audio_stream,
                        audio_decoder,
                        shared_inner.clone(),
                    ));
                }
                MoonlightStreamSetupOutput::StartVideoStream { addr, video_stream } => {
                    let video_decoder = video_decoder
                        .take()
                        .expect("video decoder was already taken");

                    threads.video_stream = Some(video_thread(
                        info_span!("video_stream"),
                        stop.clone(),
                        addr,
                        video_stream,
                        video_decoder,
                        shared_inner.clone(),
                    ));
                }
                MoonlightStreamSetupOutput::StartControlStream {
                    addr,
                    control_stream: new_control_stream,
                } => {
                    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0")?);
                    socket.connect(addr)?;

                    {
                        let mut control_stream = shared_inner
                            .control_stream
                            .lock()
                            .expect("failed to lock ControlStream");
                        *control_stream = Some(new_control_stream);
                    }

                    threads.control_stream_sender = Some(control_thread_sender(
                        info_span!("control_stream_sender"),
                        stop.clone(),
                        socket.clone(),
                        connection_listener
                            .take()
                            .expect("connection listener was already taken"),
                        shared_inner.clone(),
                    ));

                    threads.control_stream_receiver = Some(control_thread_receiver(
                        info_span!("control_stream_sender"),
                        stop.clone(),
                        socket.clone(),
                        shared_inner.clone(),
                    ));
                }
                MoonlightStreamSetupOutput::Connected => break,
            }

            setup.handle_input(MoonlightStreamInput::Timeout(Instant::now()))?;
        }

        drop(tcp_stream);

        // wait until all streams are connected: audio, video, control
        let maximum_timeout = Instant::now() + Duration::from_secs(20);
        loop {
            let first_frame = shared_inner
                .first_frame
                .lock()
                .expect("failed to get FirstFrame");

            if Instant::now() > maximum_timeout {
                debug!(first_frame = ?first_frame, "exceeded FirstFrame timeout");
                return Err(MoonlightStreamError::FirstFrameTimeout);
            }

            let (first_frame, _) = shared_inner
                .first_frame_notify
                .wait_timeout(first_frame, Duration::from_secs(1))
                .expect("failed to get FirstFrame");

            if first_frame.has_audio && first_frame.has_video && first_frame.has_control {
                break;
            }
        }

        info!("started moonlight stream");

        Ok(shared_inner)
    }

    pub fn send_input(&self, input: ClientInputEvent) -> Result<(), ControlError> {
        trace!(input = ?input, "received input from application");

        self.use_control_stream(|stream| stream.batch_input(input))?;

        Ok(())
    }

    fn use_control_stream(
        &self,
        f: impl FnOnce(
            &mut ControlStream<Arc<dyn CryptoBackend + Send + 'static>>,
        ) -> Result<(), ControlError>,
    ) -> Result<(), ControlError> {
        if self.stop.is_notified() {
            trace!("couldn't aquire control stream because the stream was stopped");
            return Err(ControlError::NotConnected);
        }

        {
            let mut control_stream = self
                .inner
                .control_stream
                .lock()
                .expect("failed to lock ControlStream");
            let control_stream = control_stream
                .as_mut()
                .expect("failed to get ControlStream");

            f(control_stream)?;
        }

        // this should notify both the sender and receiver
        self.inner.control_notify.notify_all();

        Ok(())
    }

    pub fn stop(mut self) {
        // TODO: when dropping the connection should be closed in another thread, only stop should wait until the connection closed successful, maybe with result

        self.stop.stop();

        self.threads.try_join_all(|err| {
            warn!(error = ?err,"error whilst joining thread");
        });

        info!("fully terminated the stream");
    }
}

impl Drop for MoonlightStream {
    fn drop(&mut self) {
        if self.stop.is_notified() {
            // We already clean up, likely using stop
            return;
        }

        debug!("MoonlightStream was dropped, performing cleanup in another thread");

        let mut threads = self.threads.take();

        spawn(move || {
            threads.try_join_all(|err| {
                warn!(error = ?err,"error whilst joining thread");
            });

            info!("fully terminated the stream");
        });
    }
}

// IMPORTANT: after this point errors shouldn't be propagated via ? because we also need to stop the stream
fn handle_error(stop: &StopSignal, error: MoonlightStreamError) {
    error!(error = ?error, "an error occured");
    stop.stop();
}

const UDP_BUFFER_CAPACITY: usize = 4096;

fn udp_receiver(
    span: Span,
    stop: StopSignal,
    socket: Arc<UdpSocket>,
    buffer: Arc<RingBuffer>,
) -> JoinHandle<()> {
    spawn(move || {
        let _enter = span.enter();

        let mut recv_buffer = vec![0; buffer.max_packet_size()];

        // Set read timeout to regularly check for the stop signal
        if let Err(err) = socket.set_read_timeout(Some(Duration::from_secs(1))) {
            handle_error(&stop, err.into());
            return;
        }

        loop {
            match socket.recv(&mut recv_buffer) {
                Ok(len) => {
                    buffer.push(&recv_buffer[0..len]);
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    // handles read timeout
                }
                Err(err) => {
                    handle_error(&stop, err.into());
                    break;
                }
            }

            if stop.is_notified() {
                break;
            }
        }

        debug!("stopped udp_receiver");
    })
}

fn audio_thread<Crypto>(
    span: Span,
    stop: StopSignal,
    addr: SocketAddr,
    mut audio_stream: AudioStream<Crypto>,
    mut audio_decoder: impl AudioDecoder + Send + 'static,
    shared_inner: Arc<SharedInner>,
) -> JoinHandle<()>
where
    Crypto: CryptoBackend + 'static,
{
    spawn(move || {
        let _enter = span.enter();

        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(value) => value,
            Err(err) => {
                handle_error(&stop, err.into());
                return;
            }
        };
        let socket = Arc::new(socket);
        if let Err(err) = socket.connect(addr) {
            handle_error(&stop, err.into());
            return;
        }

        let ring_buffer = Arc::new(RingBuffer::new(100, UDP_BUFFER_CAPACITY));

        let receive_handle = udp_receiver(
            debug_span!("udp_receiver"),
            stop.clone(),
            socket.clone(),
            ring_buffer.clone(),
        );

        let mut buffer = vec![0; ring_buffer.max_packet_size()];
        let mut started = false;

        loop {
            let poll_output = match audio_stream.poll_output() {
                Ok(value) => value,
                Err(err) => {
                    handle_error(&stop, err.into());
                    break;
                }
            };

            let timeout = match poll_output {
                AudioStreamOutput::Send { data } => {
                    if let Err(err) = socket.send(&data) {
                        handle_error(&stop, err.into());
                        break;
                    }
                    continue;
                }
                AudioStreamOutput::Setup { opus_config } => {
                    // TODO: audio config
                    audio_decoder.setup(AudioConfig::STEREO, opus_config);
                    continue;
                }
                AudioStreamOutput::AudioSample(sample) => {
                    if !started {
                        let mut first_frame = shared_inner
                            .first_frame
                            .lock()
                            .expect("failed to get FirstFrame");
                        first_frame.has_audio = true;

                        audio_decoder.start();

                        started = true;
                    }

                    audio_decoder.decode_and_play_sample(sample);
                    continue;
                }
                AudioStreamOutput::Timeout(timeout) => timeout,
            };

            let mut timeout = timeout.saturating_duration_since(Instant::now());

            // Will likely never happen, but we need to regularly check on the stop signal
            timeout = timeout.min(Duration::from_secs(1));

            let input = match ring_buffer.pop(&mut buffer, Some(timeout)) {
                None => AudioStreamInput::Timeout(Instant::now()),
                Some(len) => AudioStreamInput::Receive {
                    now: Instant::now(),
                    data: &mut buffer[0..len],
                },
            };

            if let Err(err) = audio_stream.handle_input(input) {
                handle_error(&stop, err.into());
                break;
            }

            if stop.is_notified() {
                break;
            }
        }

        // handle receive thread
        if let Err(err) = receive_handle.join() {
            handle_error(&stop, MoonlightStreamError::ThreadJoin(err));
        }

        debug!("stopped audio_thread");
    })
}

fn video_thread<Crypto>(
    span: Span,
    stop: StopSignal,
    addr: SocketAddr,
    mut video_stream: VideoStream<Crypto>,
    mut video_decoder: impl VideoDecoder + Send + 'static,
    shared_inner: Arc<SharedInner>,
) -> JoinHandle<()>
where
    Crypto: CryptoBackend + 'static,
{
    spawn(move || {
        let _enter = span.enter();

        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(value) => value,
            Err(err) => {
                handle_error(&stop, err.into());
                return;
            }
        };
        let socket = Arc::new(socket);
        if let Err(err) = socket.connect(addr) {
            handle_error(&stop, err.into());
            return;
        }

        let ring_buffer = Arc::new(RingBuffer::new(100, UDP_BUFFER_CAPACITY));

        let receive_handle = udp_receiver(
            debug_span!("udp_receiver"),
            stop.clone(),
            socket.clone(),
            ring_buffer.clone(),
        );

        let mut buffer = vec![0; ring_buffer.max_packet_size()];
        let mut started = false;

        loop {
            let poll_output = match video_stream.poll_output() {
                Ok(value) => value,
                Err(err) => {
                    handle_error(&stop, err.into());
                    break;
                }
            };

            let timeout = match poll_output {
                VideoStreamOutput::Send { data } => {
                    if let Err(err) = socket.send(&data) {
                        handle_error(&stop, err.into());
                        break;
                    }
                    continue;
                }
                // TODO: setup?
                // VideoStreamOutput::Setup => {
                //     // TODO: audio config
                //
                //     continue;
                // }
                VideoStreamOutput::VideoFrame(frame) => {
                    if !started {
                        let mut first_frame = shared_inner
                            .first_frame
                            .lock()
                            .expect("failed to get FirstFrame");
                        first_frame.has_video = true;

                        video_decoder.start();

                        started = true;
                    }

                    let buffers = frame
                        .buffers
                        .iter()
                        .map(|x| VideoFrameBuffer {
                            buffer_type: x.buffer_type,
                            data: x.data.as_slice(),
                        })
                        .collect::<Vec<_>>();

                    let decode_unit = VideoDecodeUnit {
                        frame_number: frame.frame_index as i32,
                        color_space: ColorSpace::Rec709,
                        // TODO
                        frame_type: FrameType::PFrame,
                        frame_processing_latency: frame.host_processing_latency,
                        hdr_active: false,
                        timestamp: frame.timestamp,
                        buffers: &buffers,
                    };

                    video_decoder.submit_decode_unit(decode_unit);
                    continue;
                }
                VideoStreamOutput::SendControlMessage { message } => {
                    // grab the control stream and send a packet

                    let mut control_stream = shared_inner
                        .control_stream
                        .lock()
                        .expect("failed to get lock on ControlStream");
                    let control_stream = control_stream
                        .as_mut()
                        .expect("failed to get ControlStream");

                    if let Err(err) = control_stream.handle_input(ControlStreamInput::Message {
                        now: Instant::now(),
                        message,
                    }) {
                        handle_error(&stop, err.into());
                        break;
                    }

                    continue;
                }
                VideoStreamOutput::Timeout(timeout) => timeout,
            };

            let mut timeout = timeout.saturating_duration_since(Instant::now());

            // Will likely never happen, but we need to regularly check on the stop signal
            timeout = timeout.min(Duration::from_secs(1));

            let input = match ring_buffer.pop(&mut buffer, Some(timeout)) {
                None => VideoStreamInput::Timeout(Instant::now()),
                Some(len) => VideoStreamInput::Receive {
                    now: Instant::now(),
                    data: &mut buffer[0..len],
                },
            };

            if let Err(err) = video_stream.handle_input(input) {
                handle_error(&stop, err.into());
                break;
            }

            if stop.is_notified() {
                break;
            }
        }

        // handle receive thread
        if let Err(err) = receive_handle.join() {
            handle_error(&stop, MoonlightStreamError::ThreadJoin(err));
        }

        debug!("stopped video_thread");
    })
}

fn control_thread_sender(
    span: Span,
    stop: StopSignal,
    socket: Arc<UdpSocket>,
    mut connection_listener: impl ConnectionListener + Send + 'static,
    shared_inner: Arc<SharedInner>,
) -> JoinHandle<()> {
    spawn(move || {
        let _enter = span.enter();

        let mut final_shutdown_timeout = None;

        let mut timeout = Duration::ZERO;

        'outer: loop {
            let mut control_stream = shared_inner
                .control_stream
                .lock()
                .expect("failed to get lock on ControlStream");
            {
                let control_stream = control_stream
                    .as_mut()
                    .expect("failed to get ControlStream");

                // Check for shutdown
                if let Some(final_shutdown_timeout) = final_shutdown_timeout
                    && Instant::now() > final_shutdown_timeout
                {
                    warn!("failed to gracefully close the stream. exiting now");

                    return;
                }

                if stop.is_notified() && final_shutdown_timeout.is_none() {
                    // This will only get called once on shutdown
                    final_shutdown_timeout = Some(Instant::now() + Duration::from_secs(10));

                    // TODO: figure the disconnect code out
                    if let Err(err) = control_stream.disconnect(0) {
                        handle_error(&stop, err.into());
                        break 'outer;
                    }
                }

                if stop.is_notified() && control_stream.can_discard() {
                    break 'outer;
                }
            }

            // Wait on Condvar
            let (mut control_stream, _) = shared_inner
                .control_notify
                .wait_timeout(control_stream, timeout)
                .expect("failed to wait on ControlStream");
            let control_stream = control_stream
                .as_mut()
                .expect("failed to get ControlStream");

            // Do event loop for control stream
            let deadline = loop {
                if control_stream.can_discard() {
                    debug!(
                        "stopping control_thread_sender because the ControlStream can be discarded"
                    );
                    break 'outer;
                }

                let poll_output = match control_stream.poll_output() {
                    Ok(value) => value,
                    Err(err) => {
                        handle_error(&stop, err.into());
                        break 'outer;
                    }
                };

                match poll_output {
                    ControlStreamOutput::Action(ControlHostAction::SendUdp { addr, data }) => {
                        if let Err(err) = socket.send_to(&data, addr) {
                            handle_error(&stop, err.into());
                            break 'outer;
                        }
                        continue;
                    }
                    ControlStreamOutput::Event(ControlStreamEvent::Connect) => {
                        let mut first_frame = shared_inner
                            .first_frame
                            .lock()
                            .expect("failed to get FirstFrame");
                        first_frame.has_control = true;
                        continue;
                    }
                    ControlStreamOutput::Event(ControlStreamEvent::Packet(packet)) => {
                        match packet {
                            ControlPacket::HdrMode { enabled, sunshine } => {
                                connection_listener.set_hdr_mode(enabled, sunshine);
                            }
                            // TODO: add other host->client packets
                            _ => {
                                debug!(packet = ?packet, "received unrecognized packet by std implementation")
                            }
                        }
                        continue;
                    }
                    ControlStreamOutput::Event(ControlStreamEvent::Disconnect) => {
                        stop.stop();
                        continue;
                    }
                    ControlStreamOutput::Action(ControlHostAction::Timeout(timeout)) => {
                        break timeout;
                    }
                }
            };

            timeout = deadline.saturating_duration_since(Instant::now());

            // Will likely never happen, but we need to regularly check on the stop signal
            timeout = timeout.min(Duration::from_secs(1));
        }

        debug!("stopped control_thread_sender");
    })
}

fn control_thread_receiver(
    span: Span,
    stop: StopSignal,
    socket: Arc<UdpSocket>,
    control: Arc<SharedInner>,
) -> JoinHandle<()> {
    spawn(move || {
        let _enter = span.enter();

        // This should be more than enough
        let mut recv_buffer = vec![0; 2048];

        // Set read timeout to regularly check for the stop signal
        if let Err(err) = socket.set_read_timeout(Some(Duration::from_secs(1))) {
            handle_error(&stop, err.into());
            return;
        }

        let mut final_shutdown_timeout = None;

        loop {
            // Check if this thread needs to shutdown
            if let Some(final_shutdown_timeout) = final_shutdown_timeout
                && Instant::now() > final_shutdown_timeout
            {
                warn!("failed to gracefully close the stream. exiting now");

                return;
            }

            if stop.is_notified() && final_shutdown_timeout.is_none() {
                final_shutdown_timeout = Some(Instant::now() + Duration::from_secs(10));
            }

            // Receive data and create input
            let input = match socket.recv_from(&mut recv_buffer) {
                Ok((len, addr)) => ControlStreamInput::Host(ControlHostInput::Receive {
                    now: Instant::now(),
                    addr,
                    data: &mut recv_buffer[0..len],
                }),
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    // handles read timeout
                    ControlStreamInput::Host(ControlHostInput::Timeout(Instant::now()))
                }
                Err(err) => {
                    handle_error(&stop, err.into());
                    break;
                }
            };

            {
                // Give input into ControlStream
                let mut control_stream = control
                    .control_stream
                    .lock()
                    .expect("failed to get lock on ControlStream");
                let control_stream = control_stream
                    .as_mut()
                    .expect("failed to get ControlStream");

                if let Err(err) = control_stream.handle_input(input) {
                    handle_error(&stop, err.into());
                    break;
                }

                // Check if we can discard the control stream
                if stop.is_notified() && control_stream.can_discard() {
                    break;
                }
            }
        }

        debug!("stopped control_thread_receiver");
    })
}
