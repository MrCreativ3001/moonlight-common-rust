use sans_io_time::Instant as SInstant;
use std::{
    any::Any,
    io::{self, Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread::{self, sleep, spawn},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, debug, info, info_span, instrument, trace, warn};

use crate::stream::{
    HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
    audio::{AudioConfig, AudioDecoder, AudioFrame},
    connection::ConnectionListener,
    proto::{
        DynCryptoBackend, MOONLIGHT_STREAM_SETUP_TCP_CONNECT_TIMEOUT, MoonlightStreamInput,
        MoonlightStreamProtoError, MoonlightStreamSetup, MoonlightStreamSetupOutput,
        audio::{AudioStream, AudioStreamError, AudioStreamEvent},
        control::{
            ControlStream, ControlStreamEvent,
            input_batcher::ClientInputEvent,
            packet::{ControlPacket, TerminationReason},
            peer::ControlError,
        },
        crypto::CryptoBackend,
        microphone::foundation::{FoundationMicStream, FoundationMicStreamError},
        video::{VideoStream, VideoStreamError, VideoStreamEvent},
    },
    std::driver::SyncUdpDriver,
    video::VideoDecoder,
};

mod driver;

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
    #[error("foundation mic stream: {0}")]
    FoundationMic(#[from] FoundationMicStreamError),
    #[error("thread join: {0:?}")]
    ThreadJoin(Box<dyn Any + Send + 'static>),
    #[error("exceeded connection timeout")]
    ConnectionTimeout,
}

pub struct MoonlightStream {
    inner: Arc<Inner>,
}
// TODO: how to handle errors, maybe in the connection listener?

impl MoonlightStream {
    pub fn launch_query_parameters() -> &'static str {
        MoonlightStreamSetup::launch_query_parameters()
    }

    pub fn connect(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        mut video_decoder: impl VideoDecoder + Send + 'static,
        mut audio_decoder: impl AudioDecoder + Send + 'static,
        connection_listener: impl ConnectionListener + Send + 'static,
        crypto_backend: DynCryptoBackend,
    ) -> Result<Self, MoonlightStreamError> {
        let base_time = Instant::now();

        let span = info_span!("stream");
        let _enter = span.enter();

        let crypto_backend: Arc<dyn CryptoBackend + Send + 'static> = Arc::new(crypto_backend);

        let mut tcp_stream: Option<TcpStream> = None;
        let mut recv_buffer = vec![0; 2048];

        let mut host_features = HostFeatures::default();
        let video_capabilities = video_decoder.capabilities();

        let mut audio_stream = None;
        let mut video_stream = None;
        let mut control_stream = None;
        let mut foundation_mic_stream = None;

        let (on_enet_connect_sender, on_enet_connect) = mpsc::channel::<()>();

        let mut setup = MoonlightStreamSetup::new(
            SInstant::from_std(base_time),
            config,
            settings,
            crypto_backend,
            video_capabilities,
        )?;

        loop {
            match setup.poll_output()? {
                MoonlightStreamSetupOutput::Timeout(timeout) => {
                    let sleep_duration =
                        timeout.saturating_duration_since(SInstant::from_std(base_time));

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
                                SInstant::from_std(base_time),
                            ))?;

                            tcp_stream = None;
                        } else {
                            setup.handle_input(MoonlightStreamInput::TcpReceive {
                                now: SInstant::from_std(base_time),
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
                MoonlightStreamSetupOutput::StartAudioStream {
                    addr,
                    config,
                    audio_stream: new_audio_stream,
                } => {
                    // TODO: get audio config?
                    audio_decoder.setup(AudioConfig::STEREO, config);

                    debug_assert!(audio_stream.is_none());
                    audio_stream = Some(SyncUdpDriver::connect(base_time, addr, new_audio_stream)?);
                }
                MoonlightStreamSetupOutput::StartVideoStream {
                    addr,
                    setup,
                    video_stream: new_video_stream,
                } => {
                    video_decoder.setup(setup);

                    debug_assert!(video_stream.is_none());
                    video_stream = Some(SyncUdpDriver::connect(base_time, addr, new_video_stream)?);
                }
                MoonlightStreamSetupOutput::FoundationStartMic {
                    addr,
                    mic_stream: new_mic_stream,
                } => {
                    debug_assert!(foundation_mic_stream.is_none());
                    foundation_mic_stream =
                        Some(SyncUdpDriver::connect(base_time, addr, new_mic_stream)?);
                }
                MoonlightStreamSetupOutput::StartControlStream {
                    addr,
                    control_stream: new_control_stream,
                } => {
                    debug_assert!(control_stream.is_none());
                    control_stream =
                        Some(SyncUdpDriver::connect(base_time, addr, new_control_stream)?);
                }
                MoonlightStreamSetupOutput::Connected { features } => {
                    host_features = features;
                    break;
                }
            }

            setup.handle_input(MoonlightStreamInput::Timeout(SInstant::from_std(base_time)))?;
        }

        drop(tcp_stream);

        info!("finished setup, launching threads");

        let inner = Arc::new(Inner {
            host_features,
            streams: Streams {
                audio: audio_stream.expect("audio stream"),
                video: video_stream.expect("video stream"),
                control: control_stream.expect("control_stream"),
                foundation_mic: foundation_mic_stream,
            },
            stopped: AtomicBool::new(false),
        });

        // TODO: use this handle in the stop fn
        let handle = spawn({
            let inner = inner.clone();
            let span = span.clone();
            move || {
                let _enter = span.enter();

                inner.run(
                    on_enet_connect_sender,
                    video_decoder,
                    audio_decoder,
                    connection_listener,
                )
            }
        });

        debug!("started threads, waiting for enet to connect");

        // wait until only control stream is connected: https://github.com/MrCreativ3001/moonlight-common-rust/issues/4
        match on_enet_connect.recv_timeout(Duration::from_secs(20)) {
            Ok(_) => {}
            Err(RecvTimeoutError::Disconnected) => {
                debug!("enet connect failed");
                let error = handle.join();

                error.map_err(MoonlightStreamError::ThreadJoin)??;
                unreachable!()
            }
            Err(RecvTimeoutError::Timeout) => {
                debug!("connection timeout on connect");
                return Err(MoonlightStreamError::ConnectionTimeout);
            }
        }

        info!("started moonlight stream");

        Ok(Self { inner })
    }

    pub fn send_input(&self, input: ClientInputEvent) -> Result<(), ControlError> {
        trace!(input = ?input, "received input from application");

        self.inner
            .streams
            .control
            .stream_mut(|stream| stream.batch_input(input))?;

        Ok(())
    }
    pub fn send_input_raw(&self, packet: ControlPacket) -> Result<(), ControlError> {
        trace!(packet = ?packet, "received packet from application");

        self.inner
            .streams
            .control
            .stream_mut(|stream| stream.send_raw(packet))?;

        Ok(())
    }

    /// Use [host_features](Self::host_features) to detect support for microphone.
    pub fn send_microphone_opus_data(&self, timestamp: Duration, frame: &[u8]) -> bool {
        if let Some(foundation_mic) = &self.inner.streams.foundation_mic {
            match foundation_mic
                .stream_mut(|stream| stream.send_microphone_opus_data(timestamp, frame))
            {
                Ok(_) => true,
                Err(err) => {
                    warn!(error = ?err, "failed to send foundation microphone data");

                    false
                }
            }
        } else {
            false
        }
    }

    pub fn host_features(&self) -> HostFeatures {
        self.inner.host_features.clone()
    }

    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::Acquire)
    }

    #[instrument(level = Level::DEBUG, skip(self))]
    pub fn stop(&self) {
        // Stop self
        self.inner.stop();

        info!("stopped all streams");
    }
}

impl Drop for MoonlightStream {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Inner {
    streams: Streams,
    stopped: AtomicBool,
    host_features: HostFeatures,
}
struct Streams {
    audio: SyncUdpDriver<AudioStream>,
    video: SyncUdpDriver<VideoStream>,
    control: SyncUdpDriver<ControlStream>,
    foundation_mic: Option<SyncUdpDriver<FoundationMicStream>>,
}

impl Inner {
    fn run(
        &self,
        on_enet_connect: mpsc::Sender<()>,
        mut video_decoder: impl VideoDecoder + Send + 'static,
        mut audio_decoder: impl AudioDecoder + Send + 'static,
        mut connection_listener: impl ConnectionListener + Send + 'static,
    ) -> Result<(), MoonlightStreamError> {
        let audio = info_span!("audio_stream");
        let video = info_span!("video_stream");
        let control = info_span!("control_stream");
        let foundation_mic = info_span!("foundation_mic");

        thread::scope::<_, Result<_, MoonlightStreamError>>(|scope| {
            let audio_run = scope
                .spawn(|| audio.in_scope(|| self.streams.audio.run().inspect_err(|_| self.stop())));
            let audio_events = scope.spawn(|| {
                audio.in_scope(|| {
                    while let Some(event) = self.streams.audio.blocking_poll_event() {
                        trace!(event = ?event, "event");

                        match event {
                            AudioStreamEvent::Connected => {
                                audio_decoder.start();
                            }
                            AudioStreamEvent::Frame(frame) => {
                                audio_decoder.decode_and_play_sample(AudioFrame {
                                    timestamp: frame.timestamp,
                                    buffer: &frame.buffer,
                                });
                            }
                        }
                    }
                    audio_decoder.stop();
                })
            });

            let video_run = scope
                .spawn(|| video.in_scope(|| self.streams.video.run().inspect_err(|_| self.stop())));
            let video_events = scope.spawn(|| {
                video.in_scope(|| {
                    while let Some(event) = self.streams.video.blocking_poll_event() {
                        trace!(event = ?event, "event");

                        match event {
                            VideoStreamEvent::Connected => {
                                video_decoder.start();
                            }
                            VideoStreamEvent::FrameAvailable => {
                                self.streams.video.stream_mut(|stream| {
                                    while let Some(frame) = stream.poll_frame() {
                                        video_decoder.submit_decode_unit(frame);
                                    }
                                });
                            }
                            VideoStreamEvent::SignalIdr => {
                                self.streams.control.stream_mut(|stream| {
                                    let _ = stream.send_raw(ControlPacket::RequestIdr);
                                });
                            }
                        }
                    }
                    video_decoder.stop();
                })
            });

            let control_run = scope.spawn(|| {
                control.in_scope(|| self.streams.control.run().inspect_err(|_| self.stop()))
            });
            let control_events = scope.spawn(|| {
                control.in_scope(|| {
                    debug!("starting control events");

                    let mut error_code = 0;

                    while let Some(event) = self.streams.control.blocking_poll_event() {
                        trace!(event = ?event, "event");

                        match event {
                            ControlStreamEvent::Connect => {
                                let _ = on_enet_connect.send(());
                            }
                            ControlStreamEvent::Packet(control_packet) => match control_packet {
                                // TODO: other control packets
                                ControlPacket::ServerTermination { reason } => {
                                    error_code = match reason {
                                        TerminationReason::Long(reason) => reason as i32,
                                        TerminationReason::Short(reason) => reason as i32,
                                    };
                                }
                                _ => {}
                            },
                            ControlStreamEvent::Disconnect => {
                                info!("stopping stream because control stream got disconnected");
                                self.stopped.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }

                    connection_listener.connection_terminated(error_code);

                    debug!("stopping control events");
                })
            });

            let foundation_mic_run =
                if let Some(stream_foundation_mic) = &self.streams.foundation_mic {
                    Some(scope.spawn(|| {
                        foundation_mic
                            .in_scope(|| stream_foundation_mic.run().inspect_err(|_| self.stop()))
                    }))
                } else {
                    None
                };
            // No need for foundation mic events, it's only sending

            // -- Join all threads
            let audio_run_res = audio_run.join();
            let audio_events_res = audio_events.join();

            let video_run_res = video_run.join();
            let video_events_res = video_events.join();

            let control_run_res = control_run.join();
            let control_events_res = control_events.join();

            let foundation_mic_res = foundation_mic_run.map(|x| x.join());

            // -- Handle possible errors
            audio_run_res.map_err(MoonlightStreamError::ThreadJoin)??;
            audio_events_res.map_err(MoonlightStreamError::ThreadJoin)?;

            video_run_res.map_err(MoonlightStreamError::ThreadJoin)??;
            video_events_res.map_err(MoonlightStreamError::ThreadJoin)?;

            control_run_res.map_err(MoonlightStreamError::ThreadJoin)??;
            control_events_res.map_err(MoonlightStreamError::ThreadJoin)?;

            foundation_mic_res
                .transpose()
                .map_err(MoonlightStreamError::ThreadJoin)?
                .transpose()?;

            Ok(())
        })?;

        self.stop();

        Ok(())
    }

    fn stop(&self) {
        // Stop self
        self.stopped.store(true, Ordering::Release);

        // Stop all streams
        self.streams.audio.stop();
        self.streams.video.stop();
        self.streams.control.stop();
        if let Some(foundation_mic) = &self.streams.foundation_mic {
            foundation_mic.stop();
        }
    }
}
