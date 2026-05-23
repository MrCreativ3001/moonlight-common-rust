use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    pin, select, spawn,
    sync::{Mutex, Notify},
    task::JoinHandle,
    time::sleep_until,
};
use tracing::{debug, error, info, warn};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
        audio::{AudioConfig, AudioFrame, OpusMultistreamConfig},
        proto::{
            MoonlightStreamInput, MoonlightStreamProtoError, MoonlightStreamSetup,
            MoonlightStreamSetupOutput,
            audio::{AudioStreamError, AudioStreamInput, AudioStreamOutput},
            control::{
                ClientInputEvent, ControlStream, ControlStreamEvent, ControlStreamInput,
                ControlStreamOutput,
                packet::ControlPacket,
                peer::{ControlError, ControlHostAction},
            },
            crypto::CryptoBackend,
            microphone::foundation::{
                FoundationMicStream, FoundationMicStreamError, FoundationMicStreamInput,
                FoundationMicStreamOutput,
            },
            video::{VideoStreamError, VideoStreamInput, VideoStreamOutput},
        },
        tokio::signal::StopSignal,
        video::{DecodeResult, VideoCapabilities, VideoDecodeUnit, VideoSetup},
    },
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
    #[error("exceeded first frame timeout")]
    FirstFrameTimeout,
}

#[async_trait]
pub trait MoonlightStreamHandler {
    async fn video_capabilities(&self) -> VideoCapabilities {
        VideoCapabilities::default()
    }
    async fn setup_video(&self, setup: VideoSetup) -> Result<(), MoonlightStreamError>;
    async fn on_video_frame(&self, frame: VideoDecodeUnit<&[u8]>) -> DecodeResult;

    async fn setup_audio(
        &self,
        audio_config: AudioConfig,
        opus_config: OpusMultistreamConfig,
    ) -> Result<(), MoonlightStreamError>;
    async fn on_audio_frame(&self, frame: AudioFrame<&[u8]>);

    async fn on_control_packet(&self, packet: ControlPacket);

    async fn on_stop(&self);
}

pub struct MoonlightStream {
    features: HostFeatures,
    inner: Arc<Inner>,
}

struct Inner {
    stop: StopSignal,
    tasks: Mutex<Tasks>,
    handler: Arc<dyn MoonlightStreamHandler + Send + Sync>,
    control_stream: Mutex<Option<ControlStream<Arc<dyn CryptoBackend + Send + Sync>>>>,
    control_stream_notify: Notify,
    foundation_mic: Mutex<Option<FoundationMicStream<Arc<dyn CryptoBackend + Send + Sync>>>>,
    foundation_mic_notify: Notify,
    first_frame: Mutex<FirstFrame>,
    first_frame_notify: Notify,
}

#[derive(Debug, Default)]
struct FirstFrame {
    video: bool,
    audio: bool,
    control: bool,
}

#[derive(Debug, Default)]
struct Tasks {
    cleaned_up_tasks: bool,
    audio: Option<JoinHandle<()>>,
    video: Option<JoinHandle<()>>,
    control: Option<JoinHandle<()>>,
    foundation_microphone: Option<JoinHandle<()>>,
}

fn handle_error(inner: &Inner, error: MoonlightStreamError) {
    error!(error = ?error, "an error occured");
    inner.stop.stop();
}

impl MoonlightStream {
    pub fn launch_query_parameters() -> &'static str {
        MoonlightStreamSetup::<DisabledCryptoBackend>::launch_query_parameters()
    }

    /// # Cancel Safety
    ///
    /// This function this not cancel safe.
    pub async fn connect<Crypto>(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        crypto_backend: Crypto,
        handler: Arc<dyn MoonlightStreamHandler + Send + Sync>,
    ) -> Result<Self, MoonlightStreamError>
    where
        Crypto: CryptoBackend + Clone + 'static,
    {
        let crypto_backend: Arc<dyn CryptoBackend + Send + Sync> = Arc::new(crypto_backend);
        let setup = MoonlightStreamSetup::new(
            Instant::now(),
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
            first_frame: Default::default(),
            first_frame_notify: Notify::new(),
        });

        let features = match Self::connect_inner(setup, inner.clone()).await {
            Ok(value) => value,
            Err(err) => {
                Self::stop_inner(&inner).await;

                return Err(err);
            }
        };

        // Wait until all streams are connected
        let deadline = Instant::now() + Duration::from_secs(10);

        loop {
            let sleep = sleep_until(deadline.into());

            select! {
                _ = sleep => {
                    Self::stop_inner(&inner).await;

                    return Err(MoonlightStreamError::FirstFrameTimeout);
                }
                _ = inner.first_frame_notify.notified() => {
                    let guard = inner.first_frame.lock().await;

                    if guard.audio && guard.video && guard.control {
                        break;
                    }
                }
            }
        }

        Ok(MoonlightStream { features, inner })
    }

    async fn connect_inner(
        mut setup: MoonlightStreamSetup<Arc<dyn CryptoBackend + Send + Sync>>,
        inner: Arc<Inner>,
    ) -> Result<HostFeatures, MoonlightStreamError> {
        let mut tcp_stream = None;
        let mut buffer = vec![0; 2048];

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

                    let socket = UdpSocket::bind("0.0.0.0:0").await?;
                    socket.connect(addr).await?;

                    let mut buffer = vec![0; 4096];

                    let handle = spawn({
                        let inner = inner.clone();

                        async move {
                            let mut first_frame = true;

                            loop {
                                if inner.stop.is_notified() {
                                    break;
                                }

                                let poll_output = match audio_stream.poll_output() {
                                    Ok(value) => value,
                                    Err(err) => {
                                        handle_error(&inner, err.into());
                                        break;
                                    }
                                };

                                let mut timeout = match poll_output {
                                    AudioStreamOutput::AudioFrame(frame) => {
                                        if first_frame {
                                            let mut guard = inner.first_frame.lock().await;
                                            guard.audio = true;

                                            inner.first_frame_notify.notify_one();

                                            first_frame = false;
                                        }

                                        inner
                                            .handler
                                            .on_audio_frame(AudioFrame {
                                                timestamp: frame.timestamp,
                                                buffer: &frame.buffer,
                                            })
                                            .await;
                                        continue;
                                    }
                                    AudioStreamOutput::Send { data } => {
                                        if let Err(err) = socket.send(data).await {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                        continue;
                                    }
                                    AudioStreamOutput::Timeout(instant) => instant,
                                };

                                // Cap duration at 1 to allow for stop signal
                                timeout = timeout.max(Instant::now() + Duration::from_secs(1));

                                select! {
                                    _ = sleep_until(timeout.into()) => {
                                        if let Err(err) = audio_stream.handle_input(AudioStreamInput::Timeout(Instant::now())) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                    res = socket.recv(&mut buffer) => {
                                        let len = match res {
                                            Ok(value) => value,
                                            Err(err) => {
                                                handle_error(&inner, err.into());
                                                break;
                                            }
                                        };

                                        if let Err(err) = audio_stream.handle_input(AudioStreamInput::Receive {
                                            now: Instant::now(),
                                            data: &buffer[0..len],
                                        }) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                }
                            }

                            debug!("stopping audio task");
                        }
                    });

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
                    inner.handler.setup_video(setup).await?;

                    let socket = UdpSocket::bind("0.0.0.0:0").await?;
                    socket.connect(addr).await?;

                    let mut buffer = vec![0; 4096];

                    let handle = spawn({
                        let inner = inner.clone();

                        async move {
                            let mut first_frame = true;
                            let mut decode_result = DecodeResult::Ok;

                            loop {
                                if inner.stop.is_notified() {
                                    break;
                                }

                                if matches!(decode_result, DecodeResult::NeedIdr) {
                                    video_stream.request_idr();
                                    decode_result = DecodeResult::Ok;
                                }

                                let poll_output = match video_stream.poll_output() {
                                    Ok(value) => value,
                                    Err(err) => {
                                        handle_error(&inner, err.into());
                                        break;
                                    }
                                };

                                let mut timeout = match poll_output {
                                    VideoStreamOutput::Send { data } => {
                                        if let Err(err) = socket.send(data).await {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                        continue;
                                    }
                                    VideoStreamOutput::SendControlMessage { message } => {
                                        let mut control_stream = inner.control_stream.lock().await;

                                        let Some(control_stream) = &mut *control_stream else {
                                            // The control stream should stop the stream, if it's not present
                                            continue;
                                        };

                                        if let Err(err) = control_stream.handle_input(
                                            ControlStreamInput::Message {
                                                now: Instant::now(),
                                                message,
                                            },
                                        ) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }

                                        continue;
                                    }
                                    VideoStreamOutput::VideoFrame(frame) => {
                                        if first_frame {
                                            let mut guard = inner.first_frame.lock().await;
                                            guard.video = true;

                                            inner.first_frame_notify.notify_one();

                                            first_frame = false;
                                        }

                                        decode_result = inner.handler.on_video_frame(frame).await;
                                        continue;
                                    }
                                    VideoStreamOutput::Timeout(timeout) => timeout,
                                };
                                drop(poll_output);

                                // Cap duration at 1 to allow for stop signal
                                timeout = timeout.max(Instant::now() + Duration::from_secs(1));

                                select! {
                                    _ = sleep_until(timeout.into()) => {
                                        if let Err(err) = video_stream.handle_input(VideoStreamInput::Timeout(Instant::now())) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                    res = socket.recv(&mut buffer) => {
                                        let len = match res {
                                            Ok(value) => value,
                                            Err(err) => {
                                                handle_error(&inner, err.into());
                                                break;
                                            }
                                        };

                                        if let Err(err) = video_stream.handle_input(VideoStreamInput::Receive {
                                            now: Instant::now(),
                                            data: &buffer[0..len],
                                        }) {
                                            handle_error(&inner, err.into());
                                            break;
                                        }
                                    }
                                }
                            }

                            debug!("stopping video task");
                        }
                    });

                    let mut tasks = inner.tasks.lock().await;

                    debug_assert!(tasks.video.is_none());
                    tasks.video = Some(handle);
                    continue;
                }
                MoonlightStreamSetupOutput::FoundationStartMic { addr, mic_stream } => {
                    let inner = inner.clone();

                    let socket = UdpSocket::bind("0.0.0.0").await?;
                    socket.connect(addr).await?;

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

                                let timeout = {
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
                                                break instant;
                                            }
                                        }
                                    }
                                };

                                let input = select! {
                                    _ = sleep_until(timeout.into()) => {
                                        FoundationMicStreamInput::Timeout(Instant::now())
                                    },
                                    _ = inner.foundation_mic_notify.notified() => {
                                        FoundationMicStreamInput::Timeout(Instant::now())
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

                    let socket = UdpSocket::bind("0.0.0.0:0").await?;
                    socket.connect(addr).await?;

                    let mut buffer = vec![0; 4096];

                    {
                        let mut guard = inner.control_stream.lock().await;
                        debug_assert!(guard.is_none());
                        *guard = Some(control_stream);
                    }

                    let handle = spawn({
                        let inner = inner.clone();

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
                                    )) => timeout,
                                    ControlStreamOutput::Event(event) => match event {
                                        ControlStreamEvent::Connect => {
                                            let mut guard = inner.first_frame.lock().await;
                                            guard.control = true;

                                            inner.first_frame_notify.notify_one();

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
                                timeout = timeout.max(Instant::now() + Duration::from_secs(1));

                                select! {
                                    _ = sleep_until(timeout.into()) => {
                                        let mut control_stream = inner.control_stream.lock().await;
                                        let Some(control_stream) = control_stream.as_mut() else {
                                            // next iteration will close stream
                                            continue;
                                        };

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Timeout(Instant::now())) {
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

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Timeout(Instant::now())) {
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

                                        if let Err(err)= control_stream.handle_input(ControlStreamInput::Receive { now: Instant::now(), addr, data: &buffer[0..len] }) {
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

            let timeout = sleep_until(timeout.into());
            pin!(timeout);

            select! {
                res = tcp_stream.as_mut().expect("tcp stream").read(&mut buffer), if tcp_stream.is_some() => {
                    let len = res?;

                    if len == 0 {
                        setup.handle_input(MoonlightStreamInput::TcpDisconnected(Instant::now()))?;
                    } else {
                        setup.handle_input(MoonlightStreamInput::TcpReceive { now: Instant::now(), data: &buffer[0..len] })?;
                    }
                }
                _ = timeout => {
                    setup.handle_input(MoonlightStreamInput::Timeout(Instant::now()))?;
                }
            };
        };

        Ok(features)
    }

    /// # Cancel Safety
    ///
    /// This function is not cancel safe.
    pub async fn send_input(&self, input: ClientInputEvent) -> Result<(), ControlError> {
        self.use_control_stream(|control_stream| control_stream.batch_input(input))
            .await?;

        Ok(())
    }
    /// # Cancel Safety
    ///
    /// This function is not cancel safe.
    pub async fn send_input_raw(&self, packet: ControlPacket) -> Result<(), ControlError> {
        self.use_control_stream(|control_stream| control_stream.send_raw(packet))
            .await?;

        Ok(())
    }

    async fn use_control_stream<R>(
        &self,
        f: impl FnOnce(
            &mut ControlStream<Arc<dyn CryptoBackend + Send + Sync>>,
        ) -> Result<R, ControlError>,
    ) -> Result<R, ControlError> {
        let mut control_stream_guard = self.inner.control_stream.lock().await;

        if let Some(control_stream) = &mut *control_stream_guard {
            let result = f(control_stream);

            drop(control_stream_guard);
            self.inner.control_stream_notify.notify_one();

            result
        } else {
            Err(ControlError::NotConnected)
        }
    }

    /// # Cancel Safety
    ///
    /// This function is not cancel safe.
    pub async fn send_microphone_opus_data(&self, timestamp: Duration, frame: &[u8]) -> bool {
        let mut foundation_mic_guard = self.inner.foundation_mic.lock().await;

        if let Some(stream) = &mut *foundation_mic_guard {
            match stream.send_microphone_opus_data(timestamp, frame) {
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

    pub async fn host_features(&self) -> HostFeatures {
        self.features.clone()
    }

    pub async fn is_connected(&self) -> bool {
        self.inner.stop.is_notified()
    }

    /// # Cancel Safety
    ///
    /// This function is not cancel safe.
    pub async fn stop(&self) {
        Self::stop_inner(&self.inner).await;
    }
    async fn stop_inner(inner: &Inner) {
        if inner.stop.is_notified() {
            debug!("not stopping stream because its already stopped");
            return;
        }
        inner.stop.stop();

        let mut tasks = inner.tasks.lock().await;
        tasks.wait_or_abort_tasks().await;
    }
}

impl Drop for MoonlightStream {
    fn drop(&mut self) {
        let inner = self.inner.clone();

        spawn(async move {
            let mut tasks = inner.tasks.lock().await;

            debug!("performing task cleanup in drop");

            let mut tasks = tasks.take();

            tasks.wait_or_abort_tasks().await;
        });
    }
}

impl Tasks {
    async fn wait_or_abort_tasks(&mut self) {
        if self.cleaned_up_tasks {
            debug!("not cleaning up tasks because they were already cleaned up");
            return;
        }

        debug!("trying to join all tasks");

        // All tasks should have their own cleanup logic, which means this deadline shouldn't be used, but just to be sure.
        let deadline = Instant::now() + Duration::from_secs(20);

        Self::try_join_or_abort(&mut self.audio, deadline, "audio").await;
        Self::try_join_or_abort(&mut self.video, deadline, "video").await;
        Self::try_join_or_abort(&mut self.control, deadline, "control").await;
        Self::try_join_or_abort(
            &mut self.foundation_microphone,
            deadline,
            "foundation microphone",
        )
        .await;

        info!("fully terminated the stream");

        self.cleaned_up_tasks = true;
    }
    async fn try_join_or_abort<T>(
        handle: &mut Option<JoinHandle<T>>,
        deadline: Instant,
        name: &str,
    ) {
        if let Some(mut handle) = handle.take() {
            let sleep = sleep_until(deadline.into());

            select! {
                _ = sleep => {
                    debug!("aborting {name} task because deadline was reached");

                    // abort the handle
                    handle.abort();
                }
                _ = &mut handle => {
                    debug!("{name} task was cleaned up");

                    // fallthrough
                }
            }
        } else {
            debug!("{name} handle doesn't exists");
        }
    }

    fn take(&mut self) -> Self {
        Tasks {
            cleaned_up_tasks: self.cleaned_up_tasks,
            audio: self.audio.take(),
            video: self.video.take(),
            control: self.control.take(),
            foundation_microphone: self.foundation_microphone.take(),
        }
    }
}
