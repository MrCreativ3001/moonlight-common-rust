use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use uniffi::{Enum, Object, Record, custom_type, deps::anyhow::anyhow, export};

use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        SunshineEncryption,
        audio::OpusMultistreamConfig as OpusMultistreamConfig2,
        proto::{
            Instant,
            audio::{
                AudioStream as AudioStream2, AudioStreamConfig as AudioStreamConfig2,
                AudioStreamInput as AudioStreamInput2, AudioStreamOutput as AudioStreamOutput2,
            },
            packet::SunshinePing,
        },
    },
};

use crate::MoonlightError;

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

#[derive(Debug, Enum)]
pub enum AudioStreamInput {
    Timeout(Instant),
    Receive { now: Instant, data: Vec<u8> },
}

#[derive(Debug, Enum)]
pub enum AudioStreamOutput {
    Timeout(Instant),
    Send { data: Vec<u8> },
    AudioFrame { timestamp: Duration, frame: Vec<u8> },
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

    pub fn handle_input(&self, input: AudioStreamInput) -> Result<(), MoonlightError> {
        let input = match input {
            AudioStreamInput::Timeout(timeout) => AudioStreamInput2::Timeout(timeout),
            AudioStreamInput::Receive { now, ref data } => AudioStreamInput2::Receive { now, data },
        };

        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.handle_input(input)?;
        Ok(())
    }

    pub fn poll_output(&self) -> Result<AudioStreamOutput, MoonlightError> {
        let mut inner = self.inner.lock().expect("lock AudioStream");
        let output = inner.poll_output()?;

        let output = match output {
            AudioStreamOutput2::Timeout(timeout) => AudioStreamOutput::Timeout(timeout),
            AudioStreamOutput2::Send { data } => AudioStreamOutput::Send {
                data: data.to_vec(),
            },
            AudioStreamOutput2::AudioFrame(frame) => AudioStreamOutput::AudioFrame {
                timestamp: frame.timestamp,
                frame: frame.buffer.to_vec(),
            },
        };

        Ok(output)
    }
}
