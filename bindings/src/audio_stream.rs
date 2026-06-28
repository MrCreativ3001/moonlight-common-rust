use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use uniffi::{Object, Record, custom_type, deps::anyhow::anyhow, export, remote};

use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        SunshineEncryption,
        audio::OpusMultistreamConfig as OpusMultistreamConfig2,
        proto::{
            Instant,
            audio::{
                AudioStream as AudioStream2, AudioStreamConfig as AudioStreamConfig2,
                AudioStreamEvent,
            },
            packet::SunshinePing,
            runtime::UdpStream,
        },
    },
};

use crate::{MoonlightError, UdpTransmit};

#[derive(Debug, Record)]
pub struct OpusMultistreamConfig {
    pub sample_rate: u32,
    pub channel_count: u32,
    pub streams: u32,
    pub coupled_streams: u32,
    pub samples_per_frame: u32,
    pub mapping: OpusMultistreamMapping,
}

#[derive(Debug, Clone, Copy)]
pub struct OpusMultistreamMapping(pub [u8; 8]);

custom_type!(OpusMultistreamMapping, Vec<u8>, {
    lower: |mapping| mapping.0.to_vec(),
    try_lift: |vec| Ok(OpusMultistreamMapping(vec.as_array::<8>().copied().ok_or_else(|| anyhow!("The length of the OpusMultistreamMapping must be 8 bytes! (current: {})", vec.len()))?)),
});

#[derive(Debug, Record)]
pub struct AudioStreamConfig {
    pub addr: SocketAddr,
    pub opus_config: OpusMultistreamConfig,
    pub fec: bool,
    pub sunshine_encryption: Option<SunshineEncryption>,
    pub sunshine_ping: Option<SunshinePing>,
}

#[remote(Enum)]
pub enum AudioStreamEvent {
    OnFrame,
}

#[derive(Debug, Object)]
pub struct AudioStream {
    inner: Mutex<AudioStream2>,
}

#[export]
impl AudioStream {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: AudioStreamConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(AudioStream2::new(
                now,
                AudioStreamConfig2 {
                    addr: config.addr,
                    opus_config: OpusMultistreamConfig2 {
                        sample_rate: config.opus_config.sample_rate,
                        channel_count: config.opus_config.channel_count,
                        streams: config.opus_config.streams,
                        coupled_streams: config.opus_config.coupled_streams,
                        samples_per_frame: config.opus_config.samples_per_frame,
                        mapping: config.opus_config.mapping.0,
                    },
                    fec: config.fec,
                    sunshine_encryption: config.sunshine_encryption,
                    sunshine_ping: config.sunshine_ping,
                },
                Arc::new(RustCryptoBackend),
            )),
        })
    }

    // -- Sans IO

    pub fn poll_event(&self) -> Option<AudioStreamEvent> {
        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.poll_event()
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        let inner = self.inner.lock().expect("lock AudioStream");
        inner.poll_timeout()
    }

    pub fn poll_packet(&self) -> Option<UdpTransmit> {
        let mut inner = self.inner.lock().expect("lock AudioStream");

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
        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.handle_receive(now, addr, &contents)?;
        Ok(())
    }

    pub fn handle_timeout(&self, now: Instant) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.handle_timeout(now)?;
        Ok(())
    }
}
