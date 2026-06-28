use std::{
    net::SocketAddr,
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
            packet::SunshinePing,
            runtime::UdpStream,
            video::{
                VideoStream as VideoStream2, VideoStreamConfig as VideoStreamConfig2,
                VideoStreamEvent, depayloader::VideoDepayloaderConfig,
            },
        },
        video::{BufferType, ColorSpace, FrameIndex, FrameType, VideoFormat},
    },
};
use uniffi::{Object, Record, custom_type, export, remote};

use crate::{MoonlightError, UdpTransmit};

#[derive(Debug, Record)]
pub struct VideoStreamConfig {
    pub addr: SocketAddr,
    pub packet_size: u32,
    pub format: VideoFormat,
    pub server_version: ServerVersion,
    pub fps: u32,
    pub sunshine_ping: Option<SunshinePing>,
    pub sunshine_encryption: Option<AesKey>,
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

#[remote(Enum)]
pub enum VideoStreamEvent {
    OnFrame,
    SignalIdr,
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
                    addr: config.addr,
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

    // -- Sans IO

    pub fn poll_event(&self) -> Option<VideoStreamEvent> {
        let mut inner = self.inner.lock().expect("lock VideoStream");
        inner.poll_event()
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        let inner = self.inner.lock().expect("lock VideoStream");
        inner.poll_timeout()
    }

    pub fn poll_packet(&self) -> Option<UdpTransmit> {
        let mut inner = self.inner.lock().expect("lock VideoStream");

        let result = inner.pending_send().map(|(addr, contents)| UdpTransmit {
            addr,
            contents: contents.to_vec(),
        });
        inner.consume_send();

        result
    }

    pub fn handle_receive(
        &self,
        now: Instant,
        addr: SocketAddr,
        contents: Vec<u8>,
    ) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock VideoStream");
        inner.handle_receive(now, addr, &contents)?;
        Ok(())
    }

    pub fn handle_timeout(&self, now: Instant) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock VideoStream");
        inner.handle_timeout(now)?;
        Ok(())
    }
}
