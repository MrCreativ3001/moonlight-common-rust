use sans_io_time::Instant as SansInstant;
use std::{
    collections::VecDeque,
    future::pending,
    io,
    pin::pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    select, spawn,
    sync::Notify,
    time::{Instant, sleep, sleep_until},
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
    tokio::driver::StreamDriver,
    video::{VideoCapabilities, VideoSetup},
};

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

#[derive(Debug)]
#[non_exhaustive]
pub enum MoonlightStreamEvent {
    Audio(AudioStreamEvent),
    Video(VideoStreamEvent),
    Control(ControlStreamEvent),
}

impl From<AudioStreamEvent> for MoonlightStreamEvent {
    fn from(value: AudioStreamEvent) -> Self {
        Self::Audio(value)
    }
}
impl From<VideoStreamEvent> for MoonlightStreamEvent {
    fn from(value: VideoStreamEvent) -> Self {
        Self::Video(value)
    }
}
impl From<ControlStreamEvent> for MoonlightStreamEvent {
    fn from(value: ControlStreamEvent) -> Self {
        Self::Control(value)
    }
}

pub struct MoonlightStream {
    host_features: HostFeatures,
    audio_setup: OpusMultistreamConfig,
    video_setup: VideoSetup,
    audio_stream: StreamDriver<AudioStream>,
    video_stream: StreamDriver<VideoStream>,
    control_stream: StreamDriver<ControlStream>,
    foundation_mic_stream: Option<StreamDriver<FoundationMicStream>>,
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

        let base_time = Instant::now();

        let mut setup = MoonlightStreamSetup::new(
            SansInstant::from_std(base_time.into_std()),
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
                _ = sleep_until(timeout.to_std(base_time.into_std()).into()) => {
                    setup.handle_input(MoonlightStreamInput::Timeout(SansInstant::from_std(base_time.into_std())))?;
                    continue;
                }
                result = tcp_stream.as_mut().expect("tcp stream should exist in this state").read(&mut buffer), if tcp_stream.is_some() => {
                    let len = result?;

                    let now = SansInstant::from_std(base_time.into_std());
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

        debug!("binding all streams");
        let (audio_stream, video_stream, mut control_stream, foundation_mic_stream) = try_join!(
            StreamDriver::new(audio_stream.expect("audio stream")),
            StreamDriver::new(video_stream.expect("video stream")),
            StreamDriver::new(control_stream.expect("control stream")),
            async {
                if let Some(foundation_mic_stream) = foundation_mic_stream {
                    StreamDriver::new(foundation_mic_stream).await.map(Some)
                } else {
                    Ok(None)
                }
            }
        )?;

        debug!("waiting for control stream connection");
        // Wait for enet connection
        let mut sleep = pin!(sleep(Duration::from_secs(20)));
        loop {
            select! {
                _ = &mut sleep => {
                    return Err(MoonlightStreamError::ConnectionTimeout);
                }
                result = control_stream.drive() => {
                    let event = result?;

                    match event {
                        ControlStreamEvent::Connect => {
                            info!("control stream connected");
                            break;
                        }
                        event => warn!(event = ?event, "got control stream event before being connected"),
                    }
                },
            }
        }

        Ok(Self {
            host_features,
            audio_setup: audio_setup.expect("audio setup"),
            audio_stream,
            video_setup: video_setup.expect("video setup"),
            video_stream,
            control_stream,
            foundation_mic_stream,
        })
    }

    /// If this instance can be discarded
    pub fn is_alive(&mut self) -> bool {
        // TODO: can this function be immutable?
        !self.control_stream.stream_mut().can_discard()
    }

    pub fn audio_setup(&self) -> OpusMultistreamConfig {
        self.audio_setup.clone()
    }
    pub fn video_setup(&self) -> VideoSetup {
        self.video_setup
    }

    pub fn estimated_rtt(&self) -> Result<EstimatedRttInfo, ControlError> {
        self.control_stream.stream().estimated_rtt()
    }

    pub fn send_input(&mut self, input: ClientInputEvent) -> Result<(), ControlError> {
        self.control_stream.stream_mut().batch_input(input)
    }
    pub fn send_raw(&mut self, packet: ControlPacket) -> Result<(), ControlError> {
        self.control_stream.stream_mut().send_raw(packet)
    }

    pub fn disconnect(&mut self) -> Result<(), ControlError> {
        self.control_stream.stream_mut().disconnect(0)
    }

    pub fn send_microphone_opus_data(&mut self, timestamp: Duration, frame: &[u8]) -> bool {
        if let Some(foundation_mic) = self.foundation_mic_stream.as_mut() {
            if let Err(err) = foundation_mic
                .stream_mut()
                .send_microphone_opus_data(timestamp, frame)
            {
                warn!(error = %err, "failed to send microphone data");
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    pub async fn drive(&mut self) -> Result<MoonlightStreamEvent, MoonlightStreamError> {
        select! {
            result = self.audio_stream.drive() => result.map(MoonlightStreamEvent::from),
            result = self.video_stream.drive() => result.map(MoonlightStreamEvent::from),
            result = self.control_stream.drive() => result.map(MoonlightStreamEvent::from),
            _ = async {
                if let Some(stream) = self.foundation_mic_stream.as_mut() {
                    stream.drive().await
                } else {
                    pending().await
                }
            } => unreachable!(),
        }
    }

    pub fn host_features(&self) -> HostFeatures {
        self.host_features.clone()
    }
}
