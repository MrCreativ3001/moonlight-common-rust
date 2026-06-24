use sans_io_time::Instant as SInstant;
use std::{
    io,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    pin, select, spawn,
    sync::{Mutex, Notify, mpsc},
    task::JoinHandle,
    time::sleep_until,
};
use tracing::{debug, error, info, warn};

use crate::stream::{
    HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
    audio::{AudioConfig, AudioFrame, OpusMultistreamConfig},
    proto::{
        DynCryptoBackend, MoonlightStreamInput, MoonlightStreamProtoError, MoonlightStreamSetup,
        MoonlightStreamSetupOutput,
        audio::{AudioStreamError, AudioStreamInput, AudioStreamOutput},
        control::{
            ControlStream, ControlStreamEvent, ControlStreamInput, ControlStreamOutput,
            input_batcher::ClientInputEvent,
            packet::ControlPacket,
            peer::{ControlError, ControlHostAction},
        },
        crypto::CryptoBackend,
        microphone::foundation::{
            FoundationMicStream, FoundationMicStreamError, FoundationMicStreamInput,
            FoundationMicStreamOutput,
        },
        stream::{AsyncUdpDriver, AsyncUdpSocket, Runtime},
        video::{VideoStreamError, VideoStreamEvent, VideoStreamInput, VideoStreamOutput},
    },
    tokio::signal::StopSignal,
    video::{DecodeResult, VideoCapabilities, VideoDecodeUnit, VideoSetup},
};

// TODO: move to using tokio::time::Instant

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
    #[error("foundation mic stream: {0}")]
    FoundationMic(#[from] FoundationMicStreamError),
    #[error("exceeded connection timeout")]
    ConnectionTimeout,
}

pub struct MoonlightStream {
    features: HostFeatures,
    inner: Arc<Inner>,
}

struct Inner {
    stop: StopSignal,
}

fn handle_error(inner: &Inner, error: MoonlightStreamError) {
    error!(error = ?error, "an error occured");
    inner.stop.stop();
}

impl MoonlightStream {
    pub fn launch_query_parameters() -> &'static str {
        MoonlightStreamSetup::launch_query_parameters()
    }

    /// # Cancel Safety
    ///
    /// This function this not cancel safe.
    pub async fn connect(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        crypto_backend: DynCryptoBackend,
    ) -> Result<Self, MoonlightStreamError> {
        let crypto_backend: Arc<dyn CryptoBackend + Send + Sync> = Arc::new(crypto_backend);
        let base_time = Instant::now();

        let setup = MoonlightStreamSetup::new(
            SInstant::from_std(base_time),
            config,
            settings,
            crypto_backend,
            handler.video_capabilities().await,
        )?;

        let inner = Arc::new(Inner {
            stop: StopSignal::new(),
            tasks: Mutex::new(Tasks::default()),
            handler,
            control_stream: Default::default(),
            control_stream_notify: Notify::new(),
            foundation_mic: Default::default(),
            foundation_mic_notify: Notify::new(),
        });

        let features = match Self::connect_inner(base_time, setup, inner.clone()).await {
            Ok(value) => value,
            Err(err) => {
                Self::stop_inner(&inner).await;

                return Err(err);
            }
        };

        Ok(MoonlightStream { features, inner })
    }

    async fn connect_inner(
        base_time: Instant,
        mut setup: MoonlightStreamSetup,
        inner: Arc<Inner>,
    ) -> Result<HostFeatures, MoonlightStreamError> {
        let mut tcp_stream = None;
        let mut buffer = vec![0; 2048];

        let (on_enet_connect_sender, mut on_enet_connect) = mpsc::channel(1);

        let features = loop {
            let timeout = match setup.poll_output()? {
                MoonlightStreamSetupOutput::Timeout(timeout) => timeout,
                MoonlightStreamSetupOutput::TcpConnect { addr } => {
                    tcp_stream = Some(TcpStream::connect(addr).await?);
                    continue;
                }
                MoonlightStreamSetupOutput::TcpWrite { data } => {
                    tcp_stream
                        .as_mut()
                        .expect("tcp stream")
                        .write_all(&data)
                        .await?;
                    continue;
                }
                MoonlightStreamSetupOutput::StartAudioStream {
                    addr,
                    config,
                    mut audio_stream,
                } => {
                    // TODO: use audio config
                    inner
                        .handler
                        .setup_audio(AudioConfig::STEREO, config)
                        .await?;

                    let socket = bind_any_and_connect_udp_socket(addr).await?;

                    let mut buffer = vec![0; 4096];

                    todo!();

                    let mut tasks = inner.tasks.lock().await;

                    debug_assert!(tasks.audio.is_none());
                    tasks.audio = Some(handle);
                    continue;
                }
                MoonlightStreamSetupOutput::StartVideoStream {
                    addr,
                    setup,
                    mut video_stream,
                } => {
                    let mut buffer = vec![0; 4096];

                    let mut driver =
                        AsyncUdpDriver::connect(TokioRuntime, addr, video_stream).await?;

                    spawn(async move {
                        loop {
                            match driver.poll_event().await.unwrap() {
                                VideoStreamEvent::FrameAvailable => {
                                    todo!();
                                }
                                VideoStreamEvent::SignalIdr => {
                                    todo!();
                                }
                            }
                        }
                    });

                    let mut tasks = inner.tasks.lock().await;

                    debug_assert!(tasks.video.is_none());
                    tasks.video = Some(handle);
                    continue;
                }
                MoonlightStreamSetupOutput::FoundationStartMic { addr, mic_stream } => {
                    let inner = inner.clone();

                    let socket = bind_any_and_connect_udp_socket(addr).await?;

                    {
                        let mut guard = inner.foundation_mic.lock().await;
                        debug_assert!(guard.is_none());
                        *guard = Some(mic_stream);
                    }

                    let handle = spawn({
                        let inner = inner.clone();

                        async move {
                            'outer: loop {
                                if inner.stop.is_notified() {
                                    break;
                                }

                                let mut timeout = {
                                    let mut mic_stream = inner.foundation_mic.lock().await;
                                    let Some(mic_stream) = mic_stream.as_mut() else {
                                        debug!("stopping because of missing control stream");
                                        inner.stop.stop();
                                        break;
                                    };

                                    loop {
                                        let poll_output = match mic_stream.poll_output() {
                                            Ok(value) => value,
                                            Err(err) => {
                                                handle_error(&inner, err.into());
                                                break 'outer;
                                            }
                                        };

                                        match poll_output {
                                            FoundationMicStreamOutput::Send { data } => {
                                                if let Err(err) = socket.send(data).await {
                                                    handle_error(&inner, err.into());
                                                    break 'outer;
                                                }
                                                continue;
                                            }
                                            FoundationMicStreamOutput::Timeout(instant) => {
                                                break instant.to_std(base_time);
                                            }
                                        }
                                    }
                                };

                                // Cap duration at 1 to allow for stop signal
                                timeout = timeout.min(Instant::now() + Duration::from_secs(1));

                                let input = select! {
                                    _ = sleep_until(timeout.into()) => {
                                        FoundationMicStreamInput::Timeout(SInstant::from_std(base_time))
                                    },
                                    _ = inner.foundation_mic_notify.notified() => {
                                        FoundationMicStreamInput::Timeout(SInstant::from_std(base_time))
                                    }
                                };

                                let mut mic_stream = inner.foundation_mic.lock().await;
                                let Some(mic_stream) = mic_stream.as_mut() else {
                                    debug!("stopping because of missing control stream");
                                    inner.stop.stop();
                                    break;
                                };
                                if let Err(err) = mic_stream.handle_input(input) {
                                    handle_error(&inner, err.into());
                                    break 'outer;
                                }

                                continue;
                            }
                        }
                    });

                    let mut tasks = inner.tasks.lock().await;

                    tasks.foundation_microphone = Some(handle);
                    continue;
                }
                MoonlightStreamSetupOutput::StartControlStream {
                    addr,
                    control_stream,
                } => {
                    let inner = inner.clone();

                    let socket = bind_any_and_connect_udp_socket(addr).await?;

                    let mut buffer = vec![0; 4096];

                    {
                        let mut guard = inner.control_stream.lock().await;
                        debug_assert!(guard.is_none());
                        *guard = Some(control_stream);
                    }

                    let handle = spawn({
                        let inner = inner.clone();
                        let on_enet_connect_sender = on_enet_connect_sender.clone();

                        async move {
                            let mut final_shutdown_deadline = None;

                            loop {
                                if inner.stop.is_notified() && final_shutdown_deadline.is_none() {
                                    final_shutdown_deadline =
                                        Some(Instant::now() + Duration::from_secs(10));
                                    debug!(
                                        shutdown_deadline = ?final_shutdown_deadline,
                                        "setting control stream shutdown deadline"
                                    );

                                    // Signal stop event to the control stream
                                    let mut control_stream = inner.control_stream.lock().await;
                                    let Some(control_stream) = control_stream.as_mut() else {
                                        debug!("stopping because of missing control stream");
                                        inner.stop.stop();
                                        break;
                                    };

                                    if let Err(err) = control_stream.disconnect(0) {
                                        handle_error(&inner, err.into());
                                        break;
                                    }

                                    // Do not stop yet, try graceful shutdown
                                }

                                // Look for shutdown of control stream
                                if let Some(final_shutdown_deadline) = final_shutdown_deadline
                                    && Instant::now() >= final_shutdown_deadline
                                {
                                    debug!("stopping control stream because of final deadline");
                                    break;
                                }

                                if let Some(final_shutdown_deadline) = final_shutdown_deadline
                                    && final_shutdown_deadline < Instant::now()
                                {
                                    warn!("failed to gracefully close the stream. exiting now");
                                    break;
                                }

                                let poll_output = {
                                    let mut control_stream = inner.control_stream.lock().await;
                                    let Some(control_stream) = control_stream.as_mut() else {
                                        debug!("stopping because of missing control stream");
                                        inner.stop.stop();
                                        break;
                                    };

                                    if control_stream.can_discard() {
                                        debug!(
                                            "stopping control stream because it can be discarded"
                                        );
                                        break;
                                    }

                                    match control_stream.poll_output() {
                                        Ok(value) => value,
                                        Err(err) => {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                };

                                let mut timeout = match poll_output {
                                    ControlStreamOutput::Action(ControlHostAction::SendUdp {
                                        addr: send_addr,
                                        data,
                                    }) => {
                                        if send_addr != addr {
                                            warn!(
                                                address = %addr,
                                                "control stream tried to send to another address than the host address!"
                                            );
                                            continue;
                                        }

                                        if let Err(err) = socket.send(&data).await {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                        continue;
                                    }
                                    ControlStreamOutput::Action(ControlHostAction::Timeout(
                                        timeout,
                                    )) => timeout.to_std(base_time),
                                    ControlStreamOutput::Event(event) => match event {
                                        ControlStreamEvent::Connect => {
                                            let _ = on_enet_connect_sender.send(()).await;

                                            continue;
                                        }
                                        ControlStreamEvent::Packet(control_packet) => {
                                            inner.handler.on_control_packet(control_packet).await;
                                            continue;
                                        }
                                        ControlStreamEvent::Disconnect => {
                                            inner.stop.stop();
                                            continue;
                                        }
                                    },
                                };

                                // Cap duration at 1 to allow for stop signal
                                timeout = timeout.min(Instant::now() + Duration::from_secs(1));

                                select! {
                                    _ = sleep_until(timeout.into()) => {
                                        let mut control_stream = inner.control_stream.lock().await;
                                        let Some(control_stream) = control_stream.as_mut() else {
                                            // next iteration will close stream
                                            continue;
                                        };

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Timeout(SInstant::from_std(base_time))) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                    _ = inner.control_stream_notify.notified() => {
                                        let mut control_stream = inner.control_stream.lock().await;
                                        let Some(control_stream) = control_stream.as_mut() else {
                                            // next iteration will close stream
                                            continue;
                                        };

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Timeout(SInstant::from_std(base_time))) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                    res = socket.recv(&mut buffer) => {
                                        let mut control_stream = inner.control_stream.lock().await;
                                        let Some(control_stream) = control_stream.as_mut() else {
                                            // next iteration will close stream
                                            continue;
                                        };

                                        let len = match res {
                                            Ok(value) => value,
                                            Err(err) => {
                                                handle_error(&inner, err.into());
                                                break;
                                            }
                                        };

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Receive { now: SInstant::from_std(base_time), addr, data: &buffer[0..len] }) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                }
                            }

                            debug!("stopping control task");
                        }
                    });

                    let mut tasks = inner.tasks.lock().await;

                    tasks.control = Some(handle);
                    continue;
                }
                MoonlightStreamSetupOutput::Connected { features } => {
                    break features;
                }
            };

            let timeout = sleep_until(timeout.to_std(base_time).into());
            pin!(timeout);

            select! {
                res = tcp_stream.as_mut().expect("tcp stream").read(&mut buffer), if tcp_stream.is_some() => {
                    let len = res?;

                    if len == 0 {
                        setup.handle_input(MoonlightStreamInput::TcpDisconnected(SInstant::from_std(base_time)))?;
                    } else {
                        setup.handle_input(MoonlightStreamInput::TcpReceive { now: SInstant::from_std(base_time), data: &buffer[0..len] })?;
                    }
                }
                _ = timeout => {
                    setup.handle_input(MoonlightStreamInput::Timeout(SInstant::from_std(base_time)))?;
                }
            };
        };

        // Wait until all the control stream is connected
        let deadline = Instant::now() + Duration::from_secs(10);

        select! {
            _ = sleep_until(deadline.into()) => {
                debug!("no enet connection could be established");
                return Err(MoonlightStreamError::ConnectionTimeout);
            }
            _ = on_enet_connect.recv() => {
                // fallthrough
            }
        }

        Ok(features)
    }

    pub fn host_features(&self) -> HostFeatures {
        self.features.clone()
    }

    pub async fn is_connected(&self) -> bool {
        self.inner.stop.is_notified()
    }

    pub fn stop(&self) {
        self.inner.stop.stop();
    }
}

struct TokioRuntime {
    base_time: Instant,
}

impl Runtime for TokioRuntime {
    fn now(&self) -> SInstant {
        SInstant::from_std(self.base_time)
    }

    async fn sleep_until(&self, deadline: SInstant) {
        sleep_until(deadline.to_std(self.base_time).into()).await
    }

    type Socket = UdpSocket;

    async fn connect_udp_socket(
        &self,
        addr: SocketAddr,
    ) -> Result<Self::Socket, <Self::Socket as AsyncUdpSocket>::Error> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(addr).await?;

        disable_udp_conn_reset(&socket);

        Ok(socket)
    }
}

impl AsyncUdpSocket for UdpSocket {
    type Error = io::Error;

    async fn recv(&self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        UdpSocket::recv(self, buffer).await
    }

    async fn writable(&self) -> Result<(), Self::Error> {
        UdpSocket::writable(self).await
    }

    fn try_send(&self, buffer: &[u8]) -> Result<bool, Self::Error> {
        match UdpSocket::try_send(self, buffer) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
    }
}

/// On Windows, a connected UDP socket returns `WSAECONNRESET` (10054) on the
/// next `recv`/`send` after a previously sent datagram triggered an ICMP
/// port-unreachable. This is the `SIO_UDP_CONNRESET` behavior and it is enabled
/// by default. In the GameStream protocol the host only opens its RTP ports
/// after the RTSP `PLAY`, so the pings the client sends beforehand elicit a
/// transient port-unreachable. The resulting 10054 is reported on `recv`/`send`
/// and is treated as fatal by the audio/video stream threads, which tears the
/// stream down before the first frame is ever received.
///
/// Disable the behavior so transient ICMP port-unreachables no longer surface
/// as socket errors, matching what native Moonlight clients do on Windows.
#[cfg(windows)]
fn disable_udp_conn_reset(socket: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{SIO_UDP_CONNRESET, WSAIoctl};

    let enable: u32 = 0; // FALSE: stop raising WSAECONNRESET on ICMP port-unreachable
    let mut bytes_returned: u32 = 0;
    let _ = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as _,
            SIO_UDP_CONNRESET,
            &enable as *const u32 as *const _,
            core::mem::size_of::<u32>() as u32,
            core::ptr::null_mut(),
            0,
            &mut bytes_returned,
            core::ptr::null_mut(),
            None,
        )
    };
}

#[cfg(not(windows))]
fn disable_udp_conn_reset(_socket: &UdpSocket) {}
