use std::{
    fmt::{self, Debug, Formatter},
    time::Duration,
};

use sans_io_time::Instant;

use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{Level, debug, info, instrument};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        SunshineEncryption,
        audio::{AudioFrame, OpusMultistreamConfig},
        proto::{
            audio::{
                depayloader::{AudioDepayloader, AudioDepayloaderConfig, AudioDepayloaderError},
                packet::{RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS},
            },
            crypto::CryptoBackend,
            packet::SunshinePing,
            ping::{
                PingSender, PingSenderConfig, PingSenderInput, PingSenderOutput, PingSenderState,
            },
        },
    },
};

pub mod depayloader;
mod packet;
pub mod payloader;

#[cfg(feature = "uniffi")]
#[doc(hidden)]
pub mod uniffi_impl;

#[cfg(test)]
#[allow(clippy::unwrap_used, unused)]
mod test;

// TODO: this needs to be adjustable based on the audio sample length
/// The maximum time to wait for a sample
const MAXIMUM_SAMPLE_WAIT: Duration = Duration::from_millis(100);

#[derive(Debug)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AudioStreamConfig {
    pub opus_config: OpusMultistreamConfig,
    /// See: <https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L28-L44>
    pub fec: bool,
    pub sunshine_ping: Option<SunshinePing>,
    /// If [Some] the audio stream is encrypted.
    pub sunshine_encryption: Option<SunshineEncryption>,
}

#[derive(Debug, Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum AudioStreamError {
    #[error("audio queue: {0}")]
    Queue(#[from] AudioDepayloaderError),
}

#[derive(Debug)]
pub enum AudioStreamInput<'a> {
    Timeout(Instant),
    Receive { now: Instant, data: &'a [u8] },
}

#[derive(Debug)]
pub enum AudioStreamOutput<'a> {
    Send { data: &'a [u8] },
    // TODO: use lifetime?
    AudioFrame(AudioFrame<Vec<u8>>),
    Timeout(Instant),
}

pub struct AudioStream<Crypto> {
    last_now: Instant,
    last_sample: Instant,
    ping_sender: PingSender,
    depayloader: AudioDepayloader<Crypto>,
}

impl AudioStream<DisabledCryptoBackend> {
    pub fn new_unencrypted(now: Instant, config: AudioStreamConfig) -> Self {
        Self::new(now, config, DisabledCryptoBackend)
    }
}

impl<Crypto> AudioStream<Crypto>
where
    Crypto: CryptoBackend,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: AudioStreamConfig, crypto_backend: Crypto) -> Self {
        Self {
            last_now: now,
            last_sample: now,
            ping_sender: PingSender::new(
                now,
                PingSenderConfig {
                    sunshine_ping: config.sunshine_ping,
                },
            ),
            depayloader: AudioDepayloader::new(
                // TODO: use opus config for packet size?
                AudioDepayloaderConfig {
                    fec: config.fec,
                    encryption: config.sunshine_encryption,
                },
                crypto_backend,
            ),
        }
    }

    pub fn poll_output(&mut self) -> Result<AudioStreamOutput<'_>, AudioStreamError> {
        if !matches!(self.ping_sender.state(), PingSenderState::Finished) {
            return match self.ping_sender.poll_output() {
                PingSenderOutput::Send { data } => Ok(AudioStreamOutput::Send { data }),
                PingSenderOutput::Timeout(timeout) => Ok(AudioStreamOutput::Timeout(timeout)),
                PingSenderOutput::Finished => unreachable!(),
            };
        }

        if let Some(data) = self.depayloader.poll_frame()? {
            self.last_sample = self.last_now;

            return Ok(AudioStreamOutput::AudioFrame(data));
        } else if self.last_sample + MAXIMUM_SAMPLE_WAIT < self.last_now {
            // TODO: use the timestamp to better estimate when we should skip samples
            debug!(
                "Dropping audio sample because it took too long to receive: Last Sample: {:?}, Current Time: {:?}",
                self.last_sample, self.last_now
            );

            self.depayloader.try_skip_samples()?;

            self.last_sample = self.last_now;
            if let Some(data) = self.depayloader.poll_frame()? {
                return Ok(AudioStreamOutput::AudioFrame(data));
            }
        }

        Ok(AudioStreamOutput::Timeout(
            self.last_now + MAXIMUM_SAMPLE_WAIT,
        ))
    }

    pub fn handle_input(&mut self, input: AudioStreamInput) -> Result<(), AudioStreamError> {
        match input {
            AudioStreamInput::Timeout(now) => {
                self.last_now = now;
                self.ping_sender.handle_input(PingSenderInput::Timeout(now));

                Ok(())
            }
            AudioStreamInput::Receive { now, data } => {
                self.last_now = now;
                self.ping_sender.handle_input(PingSenderInput::Timeout(now));

                if !matches!(self.ping_sender.state(), PingSenderState::Finished) {
                    info!(now = ?now, "received first audio packet");

                    self.ping_sender.set_finished();
                }

                self.depayloader.handle_packet(data)?;

                Ok(())
            }
        }
    }
}

impl<Crypto> Debug for AudioStream<Crypto> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[AudioStream]")
    }
}

impl<Crypto> Drop for AudioStream<Crypto> {
    fn drop(&mut self) {
        info!("terminated audio stream");
    }
}

pub(crate) fn create_audio_reed_solomon() -> ReedSolomon {
    // Normal rs implementation don't generate a correct rs matrix: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpAudioQueue.c#L52-L59
    let parity: [u8; 8] = [0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c];

    // This won't panic because all values are controlled by us and are correct for the rs implementation
    #[allow(clippy::unwrap_used)]
    let mut reed_solomon = ReedSolomon::new(RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS).unwrap();

    // This won't panic because all values are controlled by us and are correct for the rs implementation
    #[allow(clippy::unwrap_used)]
    reed_solomon.set_parity_matrix(&parity).unwrap();

    reed_solomon
}
