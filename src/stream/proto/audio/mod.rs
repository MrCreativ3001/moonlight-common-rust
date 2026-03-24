use std::{
    error::Error,
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    time::{Duration, Instant},
};

use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{Level, debug, instrument};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        AesIv, AesKey,
        audio::{AudioSample, OpusMultistreamConfig},
        proto::{
            audio::{
                depayloader::{AudioDepayloader, AudioDepayloaderConfig, AudioDepayloaderError},
                packet::{RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS},
            },
            crypto::CryptoBackend,
            packet::SunshinePingPacket,
            rtsp::moonlight::SunshinePing,
        },
    },
};

const PING_RETRY: Duration = Duration::from_millis(500);

pub mod depayloader;
mod packet;
pub mod payloader;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test;

// TODO: this needs to be adjustable based on the audio sample length
/// The maximum time to wait for a sample
const MAXIMUM_SAMPLE_WAIT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct AudioStreamConfig {
    pub addr: SocketAddr,
    pub opus_config: OpusMultistreamConfig,
    /// See: https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L28-L44
    pub fec: bool,
    pub sunshine_ping: Option<SunshinePing>,
    /// If [Some] the audio stream is encrypted.
    pub sunshine_encryption: Option<(AesKey, AesIv)>,
}

#[derive(Debug, Error)]
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
pub enum AudioStreamOutput {
    Send { to: SocketAddr, data: Vec<u8> },
    Setup { opus_config: OpusMultistreamConfig },
    AudioSample(AudioSample),
    Timeout(Instant),
}

#[derive(Debug)]
enum State {
    SendPing {
        last_send: Option<Instant>,
        sunshine_ping: Option<SunshinePingPacket>,
    },
    Setup,
    ReceiveAudio,
}

pub struct AudioStream<Crypto> {
    addr: SocketAddr,
    opus_config: OpusMultistreamConfig,
    last_now: Instant,
    last_sample: Instant,
    state: State,
    queue: AudioDepayloader<Crypto>,
}

impl AudioStream<DisabledCryptoBackend> {
    pub fn new_unencrypted(now: Instant, config: AudioStreamConfig) -> Self {
        Self::new(now, config, DisabledCryptoBackend)
    }
}

impl<Crypto> AudioStream<Crypto>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: AudioStreamConfig, crypto_backend: Crypto) -> Self {
        Self {
            addr: config.addr,
            opus_config: config.opus_config,
            last_now: now,
            last_sample: now,
            state: State::SendPing {
                last_send: None,
                sunshine_ping: config.sunshine_ping.map(|payload| SunshinePingPacket {
                    payload,
                    sequence_number: 0,
                }),
            },
            queue: AudioDepayloader::new(
                AudioDepayloaderConfig {
                    fec: config.fec,
                    encryption: config.sunshine_encryption,
                },
                crypto_backend,
            ),
        }
    }

    pub fn poll_output(&mut self) -> Result<AudioStreamOutput, AudioStreamError> {
        match &mut self.state {
            State::SendPing {
                last_send,
                sunshine_ping,
            } => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/master/src/AudioStream.c#L38-L65
                if let Some(last_send) = last_send
                    && *last_send + PING_RETRY > self.last_now
                {
                    return Ok(AudioStreamOutput::Timeout(*last_send + PING_RETRY));
                }

                let packet = if let Some(ping) = sunshine_ping.as_mut() {
                    ping.sequence_number += 1;

                    let mut data = [0; 20];
                    ping.serialize(&mut data);
                    data.to_vec()
                } else {
                    // Just some magic bytes
                    vec![0x50, 0x49, 0x4E, 0x47]
                };
                debug!(packet = ?packet, "Sending initial audio ping");

                last_send.replace(self.last_now);

                Ok(AudioStreamOutput::Send {
                    to: self.addr,
                    data: packet,
                })
            }
            State::Setup => {
                self.state = State::ReceiveAudio;

                Ok(AudioStreamOutput::Setup {
                    opus_config: self.opus_config.clone(),
                })
            }
            State::ReceiveAudio => {
                if let Some(data) = self.queue.poll_sample()? {
                    self.last_sample = self.last_now;

                    return Ok(AudioStreamOutput::AudioSample(data));
                } else if self.last_sample + MAXIMUM_SAMPLE_WAIT < self.last_now {
                    // TODO: use the timestamp to better estimate when we should skip samples
                    debug!(
                        "Dropping audio sample because it took too long to receive: Last Sample: {:?}, Current Time: {:?}",
                        self.last_sample, self.last_now
                    );

                    self.queue.try_skip_samples()?;

                    self.last_sample = self.last_now;
                    if let Some(data) = self.queue.poll_sample()? {
                        return Ok(AudioStreamOutput::AudioSample(data));
                    }
                }

                Ok(AudioStreamOutput::Timeout(
                    self.last_now + MAXIMUM_SAMPLE_WAIT,
                ))
            }
        }
    }

    pub fn handle_input(&mut self, input: AudioStreamInput) -> Result<(), AudioStreamError> {
        match input {
            AudioStreamInput::Timeout(now) => {
                self.last_now = now;

                Ok(())
            }
            AudioStreamInput::Receive { now, data } => {
                self.last_now = now;

                if matches!(self.state, State::SendPing { .. }) {
                    self.state = State::Setup;
                }

                self.queue.handle_packet(data)?;

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
