use sans_io_time::Instant;
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant as StdInstant},
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    select, spawn,
    sync::Notify,
    time::{sleep, sleep_until},
    try_join,
};
use tracing::{Instrument, Level, debug, info, info_span, instrument, trace, warn};

use crate::stream::{
    HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
    audio::{AudioFrame, OpusMultistreamConfig},
    control::EstimatedRttInfo,
    proto::{
        DynCryptoBackend, MoonlightStreamInput, MoonlightStreamProtoError, MoonlightStreamSetup,
        MoonlightStreamSetupOutput,
        audio::{AudioStream, AudioStreamError, AudioStreamEvent},
        control::{
            ControlStream, ControlStreamEvent, input_batcher::ClientInputEvent,
            packet::ControlPacket, peer::ControlError,
        },
        microphone::foundation::{FoundationMicStream, FoundationMicStreamError},
        video::{VideoStream, VideoStreamError, VideoStreamEvent, frame::OwnedVideoFrame},
    },
    tokio::driver::{StreamRef, TokioStreamExt},
    video::{VideoCapabilities, VideoSetup},
};
use driver::bind_udp_stream;

mod driver;

#[derive(Debug, Error)]
pub enum MoonlightStreamError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("setup: {0}")]
    Setup(#[from] MoonlightStreamProtoError),
    #[error("audio: {0}")]
    Audio(#[from] AudioStreamError),
    #[error("video: {0}")]
    Video(#[from] VideoStreamError),
    #[error("control: {0}")]
    Control(#[from] ControlError),
    #[error("foundation mic: {0}")]
    FoundationMic(#[from] FoundationMicStreamError),
    #[error("connection timed out")]
    ConnectionTimeout,
    #[error("the stream was already closed")]
    Closed,
}

#[derive(Clone)]
pub struct MoonlightStream(Arc<Inner>);

struct Inner {
    stopped: AtomicBool,
    host_features: HostFeatures,
    audio_setup: OpusMultistreamConfig,
    audio_stream: StreamRef<AudioStream>,
    video_setup: VideoSetup,
    video_stream: StreamRef<VideoStream>,
    control_stream: StreamRef<ControlStream>,
    foundation_mic_stream: Option<StreamRef<FoundationMicStream>>,
}

impl MoonlightStream {
    #[instrument(level = Level::DEBUG, skip_all, name = "stream")]
    pub async fn connect(
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        crypto_backend: DynCryptoBackend,
        video_capabilities: VideoCapabilities,
    ) -> Result<Self, MoonlightStreamError> {
        debug!(config = ?config, settings = ?settings, video_capabilities = ?video_capabilities, "stream connect");

        let base_time = StdInstant::now();

        let mut setup = MoonlightStreamSetup::new(
            Instant::from_std(base_time),
            config,
            settings,
            crypto_backend,
            video_capabilities,
        )?;

        let mut buffer = vec![0; 4096];
        let mut tcp_stream = None;

        let host_features;

        let mut audio_setup = None;
        let mut video_setup = None;

        let mut audio_stream = None;
        let mut video_stream = None;
        let mut control_stream = None;
        let mut foundation_mic_stream = None;

        loop {
            let timeout = match setup.poll_output()? {
                MoonlightStreamSetupOutput::TcpConnect { addr } => {
                    tcp_stream = Some(TcpStream::connect(addr).await?);
                    continue;
                }
                MoonlightStreamSetupOutput::TcpWrite { data } => {
                    let tcp_stream = tcp_stream.as_mut().expect("tcp write");

                    tcp_stream.write_all(&data).await?;
                    continue;
                }
                MoonlightStreamSetupOutput::Timeout(timeout) => timeout,
                MoonlightStreamSetupOutput::StartAudioStream {
                    config,
                    audio_stream: new_audio_stream,
                } => {
                    audio_setup = Some(config);
                    audio_stream = Some(new_audio_stream);

                    continue;
                }
                MoonlightStreamSetupOutput::StartVideoStream {
                    setup,
                    video_stream: new_video_stream,
                } => {
                    video_setup = Some(setup);
                    video_stream = Some(new_video_stream);

                    continue;
                }
                MoonlightStreamSetupOutput::StartControlStream {
                    control_stream: new_control_stream,
                } => {
                    control_stream = Some(new_control_stream);

                    continue;
                }
                MoonlightStreamSetupOutput::FoundationStartMic {
                    mic_stream: new_foundation_mic_stream,
                } => {
                    foundation_mic_stream = Some(new_foundation_mic_stream);

                    continue;
                }
                MoonlightStreamSetupOutput::Connected { features } => {
                    host_features = features;
                    break;
                }
            };

            select! {
                _ = sleep_until(timeout.to_std(base_time).into()) => {
                    setup.handle_input(MoonlightStreamInput::Timeout(Instant::from_std(base_time)))?;
                    continue;
                }
                result = tcp_stream.as_mut().expect("tcp stream should exist in this state").read(&mut buffer), if tcp_stream.is_some() => {
                    let len = result?;

                    let now = Instant::from_std(base_time);
                    if len == 0 {
                        setup.handle_input(MoonlightStreamInput::TcpDisconnected(now))?;
                    } else {
                        setup.handle_input(MoonlightStreamInput::TcpReceive {
                            now,
                            data: &buffer[0..len],
                        })?;
                    }
                }
            };
        }

        let (audio_stream, video_stream, control_stream, foundation_mic_stream) = try_join!(
            bind_udp_stream(
                audio_stream.expect("audio stream"),
                info_span!("audio_stream")
            ),
            bind_udp_stream(
                video_stream.expect("video stream"),
                info_span!("video_stream")
            ),
            bind_udp_stream(
                control_stream.expect("control stream"),
                info_span!("control_stream")
            ),
            async {
                if let Some(foundation_mic_stream) = foundation_mic_stream {
                    bind_udp_stream(foundation_mic_stream, info_span!("foundation_mic_stream"))
                        .await
                        .map(Some)
                } else {
                    Ok(None)
                }
            }
        )?;

        // Wait for enet connection
        select! {
            _ = sleep(Duration::from_secs(20)) => {
                // TODO: stop the connection
            }
            _ = control_stream.notify().on_connect.notified() => {
                // fall through
            },
        }

        let this = Self(Arc::new(Inner {
            stopped: AtomicBool::new(false),
            host_features,
            audio_setup: audio_setup.expect("audio setup"),
            audio_stream,
            video_setup: video_setup.expect("video setup"),
            video_stream,
            control_stream,
            foundation_mic_stream,
        }));

        // idr logic
        spawn({
            let stream = this.clone();
            async move {
                loop {
                    stream
                        .0
                        .video_stream
                        .notify()
                        .on_request_idr
                        .notified()
                        .await;

                    if stream.is_stopped() {
                        return;
                    }

                    let _ = stream.send_raw(ControlPacket::RequestIdr);
                }
            }
            .instrument(info_span!("idr_request"))
        });

        // See if any stream crashed
        spawn({
            let stream = this.clone();
            async move {
                select! {
                    _ = stream.0.audio_stream.notify().on_stop.notified() => {
                        info!("audio stream has stopped. stopping main stream");
                    }
                    _ = stream.0.video_stream.notify().on_stop.notified() => {
                        info!("video stream was stopped. stopping main stream");
                    }
                    _ = stream.0.control_stream.notify().on_stop.notified() => {
                        info!("control stream was stopped. stopping main stream");
                    }
                }

                stream.stop();
            }
            .instrument(info_span!("stopper"))
        });

        Ok(this)
    }

    pub async fn estimated_rtt(&self) -> Result<EstimatedRttInfo, ControlError> {
        self.0
            .control_stream
            .stream_mut(|stream| (false, stream.estimated_rtt()))
    }

    pub fn send_input(&self, input: ClientInputEvent) -> Result<(), ControlError> {
        self.0
            .control_stream
            .stream_mut(|stream| (true, stream.batch_input(input)))
    }
    pub fn send_raw(&self, packet: ControlPacket) -> Result<(), ControlError> {
        self.0
            .control_stream
            .stream_mut(|stream| (true, stream.send_raw(packet)))
    }

    pub fn send_microphone_opus_data(&self, timestamp: Duration, frame: &[u8]) -> bool {
        if self.is_stopped() {
            return false;
        }

        if let Some(foundation_mic) = &self.0.foundation_mic_stream {
            match foundation_mic
                .stream_mut(|stream| (true, stream.send_microphone_opus_data(timestamp, frame)))
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

    pub fn audio_setup(&self) -> OpusMultistreamConfig {
        self.0.audio_setup.clone()
    }
    pub async fn poll_audio_frame(&self) -> Result<AudioFrame<Vec<u8>>, MoonlightStreamError> {
        loop {
            if let Some(frame) = self
                .0
                .audio_stream
                .stream_mut(|stream| (false, stream.poll_frame()))
            {
                return Ok(frame);
            }

            if self.is_stopped() {
                return Err(MoonlightStreamError::Closed);
            }

            self.0.audio_stream.notify().on_frame.notified().await;
        }
    }

    pub fn video_setup(&self) -> VideoSetup {
        self.0.video_setup
    }
    pub async fn poll_video_frame(&self) -> Result<OwnedVideoFrame, MoonlightStreamError> {
        loop {
            if let Some(frame) = self
                .0
                .video_stream
                .stream_mut(|stream| (false, stream.poll_frame()))
            {
                return Ok(frame);
            }

            if self.is_stopped() {
                return Err(MoonlightStreamError::Closed);
            }

            self.0.video_stream.notify().on_frame.notified().await;
        }
    }

    pub async fn poll_packet(&self) -> Result<ControlPacket, MoonlightStreamError> {
        loop {
            if let Some(packet) = self
                .0
                .control_stream
                .notify()
                .packets
                .lock()
                .expect("MoonlightStream::poll_packet")
                .pop_front()
            {
                return Ok(packet);
            }

            if self.is_stopped() {
                return Err(MoonlightStreamError::Closed);
            }

            self.0.control_stream.notify().on_packet.notified().await;
        }
    }

    pub fn host_features(&self) -> HostFeatures {
        self.0.host_features.clone()
    }

    pub fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);

        // Notify all listeners so they register the stop
        self.0.audio_stream.notify().on_frame.notify_one();

        self.0.video_stream.notify().on_frame.notify_one();
        self.0.video_stream.notify().on_request_idr.notify_one();

        self.0.control_stream.notify().on_packet.notify_one();

        // Use the control stream notify to stop the stopping task spawned in the constructor
        self.0.control_stream.notify().on_stop.notify_one();
    }
}

impl Drop for MoonlightStream {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct AudioNotify {
    on_frame: Notify,
    on_stop: Notify,
}

impl TokioStreamExt for AudioStream {
    type Notifier = AudioNotify;
    fn on_event(event: Self::Event, notify: &Self::Notifier) {
        trace!(event = ?event, "audio stream event");

        match event {
            AudioStreamEvent::OnFrame => notify.on_frame.notify_one(),
        }
    }

    fn on_stop(notify: &Self::Notifier) {
        notify.on_stop.notify_one();
    }
}

#[derive(Default)]
struct VideoNotify {
    on_frame: Notify,
    on_request_idr: Notify,
    on_stop: Notify,
}

impl TokioStreamExt for VideoStream {
    type Notifier = VideoNotify;

    fn on_event(event: Self::Event, notify: &Self::Notifier) {
        trace!(event = ?event, "video stream event");

        match event {
            VideoStreamEvent::OnFrame => notify.on_frame.notify_one(),
            VideoStreamEvent::SignalIdr => notify.on_request_idr.notify_one(),
        }
    }

    fn on_stop(notify: &Self::Notifier) {
        notify.on_stop.notify_one();
    }
}

#[derive(Default)]
struct ControlNotify {
    on_connect: Notify,
    on_packet: Notify,
    packets: Mutex<VecDeque<ControlPacket>>,
    on_stop: Notify,
}

impl TokioStreamExt for ControlStream {
    type Notifier = ControlNotify;

    fn on_event(event: Self::Event, notify: &Self::Notifier) {
        trace!(event = ?event, "control stream event");

        match event {
            ControlStreamEvent::Connect => notify.on_connect.notify_one(),
            ControlStreamEvent::Packet(packet) => {
                let mut packets = notify
                    .packets
                    .lock()
                    .expect("<ControlStream as TokioStreamExt>::on_event");
                packets.push_back(packet);
                notify.on_packet.notify_one();
            }
            ControlStreamEvent::Disconnect => {
                notify.on_stop.notify_one();
            }
        }
    }

    fn on_stop(notify: &Self::Notifier) {
        notify.on_stop.notify_one();
    }
}

impl TokioStreamExt for FoundationMicStream {
    type Notifier = ();

    fn on_event(_event: Self::Event, _notify: &Self::Notifier) {
        // do nothing
    }

    fn on_stop(_notify: &Self::Notifier) {
        // TODO: also stop the stream?
    }
}
