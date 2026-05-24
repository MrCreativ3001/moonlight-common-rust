use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use sans_io_time::Instant;

use crate::stream::proto::audio::{AudioStreamConfig, AudioStreamError};

#[cfg(not(feature = "rustcrypto"))]
type Crypto = crate::crypto::disabled::DisabledCryptoBackend;
#[cfg(feature = "rustcrypto")]
type Crypto = crate::crypto::rustcrypto::RustCryptoBackend;

#[derive(uniffi::Object)]
pub struct AudioStream {
    inner: Mutex<super::AudioStream<Crypto>>,
}

#[derive(Debug, uniffi::Enum)]
pub enum AudioStreamInput {
    Timeout(Instant),
    Receive { now: Instant, data: Vec<u8> },
}

#[derive(Debug, uniffi::Record)]
pub struct AudioFrame {
    pub timestamp: Duration,
    pub buffer: Vec<u8>,
}

#[derive(Debug, uniffi::Enum)]
pub enum AudioStreamOutput {
    Send { data: Vec<u8> },
    // TODO: use lifetime?
    AudioFrame(AudioFrame),
    Timeout(Instant),
}

#[uniffi::export]
impl AudioStream {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: AudioStreamConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(super::AudioStream::new(now, config, Crypto::default())),
        })
    }

    pub fn handle_input(&self, input: AudioStreamInput) -> Result<(), AudioStreamError> {
        let mut value = self.inner.lock().expect("lock AudioStream");

        let input = match input {
            AudioStreamInput::Timeout(timeout) => super::AudioStreamInput::Timeout(timeout),
            AudioStreamInput::Receive { now, ref data } => {
                super::AudioStreamInput::Receive { now, data }
            }
        };

        value.handle_input(input)
    }

    pub fn poll_output(&self) -> Result<AudioStreamOutput, AudioStreamError> {
        let mut value = self.inner.lock().expect("lock AudioStream");

        let output = value.poll_output()?;

        Ok(match output {
            super::AudioStreamOutput::Timeout(timeout) => AudioStreamOutput::Timeout(timeout),
            super::AudioStreamOutput::Send { data } => AudioStreamOutput::Send {
                data: data.to_vec(),
            },
            super::AudioStreamOutput::AudioFrame(frame) => {
                AudioStreamOutput::AudioFrame(AudioFrame {
                    timestamp: frame.timestamp,
                    buffer: frame.buffer,
                })
            }
        })
    }
}
