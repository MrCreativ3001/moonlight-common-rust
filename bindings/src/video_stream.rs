use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use moonlight_common::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesKey,
        proto::{
            Instant,
            control::ControlMessage,
            packet::SunshinePing,
            video::{
                VideoStream as VideoStream2, VideoStreamConfig as VideoStreamConfig2,
                VideoStreamError as VideoStreamError2, VideoStreamInput as VideoStreamInput2,
                VideoStreamOutput as VideoStreamOutput2, depayloader::VideoDepayloaderConfig,
            },
        },
        video::{BufferType, ColorSpace, FrameIndex, FrameType, VideoFormat},
    },
};
use uniffi::{Enum, Error, Object, Record, custom_type, export, remote};

#[derive(Debug, thiserror::Error, Error)]
pub enum VideoStreamError {
    #[error("depayoader: {reason}")]
    VideoDepayloader { reason: String },
}

impl From<VideoStreamError2> for VideoStreamError {
    fn from(value: VideoStreamError2) -> Self {
        match value {
            VideoStreamError2::Crypto(err) => Self::VideoDepayloader {
                reason: err.to_string(),
            },
            VideoStreamError2::Depayloader(err) => Self::VideoDepayloader {
                reason: err.to_string(),
            },
        }
    }
}

#[remote(Enum)]
pub enum VideoFormat {
    H264,
    H264High8_444,
    H265,
    H265Main10,
    H265Rext8_444,
    H265Rext10_444,
    Av1Main8,
    Av1Main10,
    Av1High8_444,
    Av1High10_444,
}

#[derive(Debug, Record)]
pub struct VideoStreamConfig {
    pub packet_size: u32,
    pub format: VideoFormat,
    pub server_version: ServerVersion,
    pub fps: u32,
    pub sunshine_ping: Option<SunshinePing>,
    pub sunshine_encryption: Option<AesKey>,
}

#[derive(Debug, Enum)]
pub enum VideoStreamInput {
    Timeout(Instant),
    Receive { now: Instant, data: Vec<u8> },
}

custom_type!(FrameIndex, u32, {
    remote,
    lower: |frame_index| frame_index.0,
    try_lift: |num| Ok(FrameIndex(num)),
});

#[remote(Enum)]
pub enum FrameType {
    PFrame,
    Idr,
}

#[remote(Enum)]
pub enum ColorSpace {
    Rec601,
    Rec709,
    Rec2020,
}

#[remote(Enum)]
pub enum BufferType {
    PicData,
    Sps,
    Pps,
    Vps,
}

#[derive(Debug, Record)]
pub struct VideoFrameBuffer {
    pub buffer_type: BufferType,
    pub data: Vec<u8>,
}

#[derive(Debug, Record)]
pub struct VideoDecodeUnit {
    pub frame_number: FrameIndex,
    pub frame_type: FrameType,
    pub frame_processing_latency: Option<Duration>,
    pub timestamp: Duration,
    pub color_space: ColorSpace,
    pub buffers: Vec<VideoFrameBuffer>,
}

#[derive(Debug, Enum)]
pub enum VideoStreamOutput {
    Send { data: Vec<u8> },
    VideoFrame(VideoDecodeUnit),
    SendControlMessage { message: ControlMessage },
    Timeout(Instant),
}

#[derive(Debug, Object)]
pub struct VideoStream {
    inner: Mutex<VideoStream2>,
}

#[export]
impl VideoStream {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: VideoStreamConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(VideoStream2::new(
                now,
                VideoStreamConfig2 {
                    fps: config.fps,
                    queue: VideoDepayloaderConfig {
                        format: config.format,
                        packet_size: config.packet_size as usize,
                        server_version: config.server_version,
                    },
                    sunshine_encryption: config.sunshine_encryption,
                    sunshine_ping: config.sunshine_ping,
                },
                Arc::new(RustCryptoBackend) as _,
            )),
        })
    }

    pub fn request_idr(&self) {
        let mut inner = self.inner.lock().expect("lock VideoStream");
        inner.request_idr();
    }

    pub fn handle_input(&self, input: VideoStreamInput) -> Result<(), VideoStreamError> {
        let input = match input {
            VideoStreamInput::Timeout(timeout) => VideoStreamInput2::Timeout(timeout),
            VideoStreamInput::Receive { now, ref data } => VideoStreamInput2::Receive { now, data },
        };

        let mut inner = self.inner.lock().expect("lock VideoStream");
        inner.handle_input(input)?;
        Ok(())
    }

    pub fn poll_output(&self) -> Result<VideoStreamOutput, VideoStreamError> {
        let mut inner = self.inner.lock().expect("lock VideoStream");
        let output = inner.poll_output()?;

        let output = match output {
            VideoStreamOutput2::Timeout(timeout) => VideoStreamOutput::Timeout(timeout),
            VideoStreamOutput2::Send { data } => VideoStreamOutput::Send {
                data: data.to_vec(),
            },
            VideoStreamOutput2::VideoFrame(frame) => {
                VideoStreamOutput::VideoFrame(VideoDecodeUnit {
                    frame_number: frame.frame_number,
                    frame_type: frame.frame_type,
                    frame_processing_latency: frame.frame_processing_latency,
                    timestamp: frame.timestamp,
                    color_space: frame.color_space,
                    buffers: frame
                        .buffers
                        .into_iter()
                        .map(|buffer| VideoFrameBuffer {
                            buffer_type: buffer.buffer_type,
                            data: buffer.data.to_vec(),
                        })
                        .collect(),
                })
            }
            VideoStreamOutput2::SendControlMessage { message } => {
                VideoStreamOutput::SendControlMessage { message }
            }
        };

        Ok(output)
    }
}
