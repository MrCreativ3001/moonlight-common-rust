use std::{
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use reed_solomon_erasure::{galois_8::ReedSolomon, matrix::Matrix};
use thiserror::Error;
use tracing::{Level, debug, instrument, trace};

use crate::stream::{
    AesIv, AesKey,
    audio::OpusMultistreamConfig,
    proto::{
        audio::{
            depayloader::{
                AudioDepayloader, AudioDepayloaderConfig, AudioDepayloaderError, AudioSample,
            },
            packet::{RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS, RTP_AUDIO_TOTAL_SHARDS},
        },
        crypto::CryptoContext,
        packet::SunshinePingPacket,
        rtsp::moonlight::SunshinePing,
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
const MAXIMUM_SAMPLE_WAIT: Duration = Duration::from_millis(1000);

#[derive(Debug)]
pub struct AudioStreamConfig {
    pub addr: SocketAddr,
    pub opus_config: OpusMultistreamConfig,
    /// See: https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L28-L44
    pub fec: bool,
    pub sunshine_ping: Option<SunshinePing>,
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
    Receive {
        now: Instant,
        from: SocketAddr,
        data: &'a [u8],
    },
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

pub struct AudioStream {
    addr: SocketAddr,
    opus_config: OpusMultistreamConfig,
    encryption: Option<(Arc<dyn CryptoContext>, AesKey, AesIv)>,
    last_now: Instant,
    last_sample: Instant,
    state: State,
    queue: AudioDepayloader,
}

impl AudioStream {
    #[instrument(level = Level::DEBUG)]
    pub fn new(now: Instant, config: AudioStreamConfig) -> Self {
        Self {
            addr: config.addr,
            opus_config: config.opus_config,
            encryption: None,
            last_now: now,
            last_sample: now,
            state: State::SendPing {
                last_send: None,
                sunshine_ping: config.sunshine_ping.map(|payload| SunshinePingPacket {
                    payload,
                    sequence_number: 0,
                }),
            },
            queue: AudioDepayloader::new(AudioDepayloaderConfig { fec: config.fec }),
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
            AudioStreamInput::Receive { now, from, data } => {
                self.last_now = now;

                if from != self.addr {
                    return Ok(());
                }

                if matches!(self.state, State::SendPing { .. }) {
                    self.state = State::Setup;
                }

                // TODO: encryption?
                if let Some(_) = self.encryption {
                    todo!();
                } else {
                    self.queue.handle_packet(data)?;
                }

                Ok(())
            }
        }
    }
}

impl Debug for AudioStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[AudioStream]")
    }
}

pub(crate) fn create_audio_reed_solomon() -> ReedSolomon {
    // Our implementation doesn't generate a correct rs matrix: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpAudioQueue.c#L52-L59
    let parity: [u8; 8] = [0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c];

    let mut matrix = Matrix::new(RTP_AUDIO_TOTAL_SHARDS, RTP_AUDIO_DATA_SHARDS);

    for row in 0..RTP_AUDIO_DATA_SHARDS {
        for col in 0..RTP_AUDIO_DATA_SHARDS {
            matrix.set(row, col, if row == col { 1 } else { 0 });
        }
    }

    for row in 0..RTP_AUDIO_FEC_SHARDS {
        for col in 0..RTP_AUDIO_DATA_SHARDS {
            matrix.set(
                RTP_AUDIO_DATA_SHARDS + row,
                col,
                parity[row * RTP_AUDIO_DATA_SHARDS + col],
            );
        }
    }

    // This won't panic because all values are controlled by us and are correct for the rs implementation
    #[allow(clippy::unwrap_used)]
    ReedSolomon::new_with_matrix(RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS, matrix).unwrap()
}
