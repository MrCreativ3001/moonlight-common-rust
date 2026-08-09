use std::{
    collections::VecDeque,
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use sans_io_time::Instant;

use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{Level, debug, info, instrument, trace};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        SunshineEncryption,
        audio::{AudioFrame, OpusMultistreamConfig},
        proto::{
            DynCryptoBackend,
            audio::{
                depayloader::{AudioDepayloader, AudioDepayloaderConfig, AudioDepayloaderError},
                packet::{RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS},
            },
            packet::SunshinePing,
            ping::{PingSender, PingSenderConfig, PingSenderState},
            runtime::UdpStream,
        },
    },
};

pub mod depayloader;
mod packet;
pub mod payloader;

#[cfg(test)]
#[allow(clippy::unwrap_used, unused)]
mod test;

// TODO: this needs to be adjustable based on the audio sample length
/// The maximum time to wait for a sample
const MAXIMUM_SAMPLE_WAIT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct AudioStreamConfig {
    pub addr: SocketAddr,
    pub opus_config: OpusMultistreamConfig,
    /// See: <https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L28-L44>
    pub fec: bool,
    pub sunshine_ping: Option<SunshinePing>,
    /// If [Some] the audio stream is encrypted.
    pub sunshine_encryption: Option<SunshineEncryption>,
}

#[derive(Debug, Error)]
pub enum AudioStreamError {
    #[error("audio queue: {0}")]
    Queue(#[from] AudioDepayloaderError),
}

#[derive(Debug)]
pub enum AudioStreamEvent {
    OnFrame(AudioFrame<Bytes>),
}

pub struct AudioStream {
    addr: SocketAddr,
    last_frame: Instant,
    dropped_frames: bool,
    ping_sender: PingSender,
    depayloader: AudioDepayloader,
    events: VecDeque<AudioStreamEvent>,
}

impl AudioStream {
    pub fn new_unencrypted(now: Instant, config: AudioStreamConfig) -> Self {
        Self::new(now, config, Arc::new(DisabledCryptoBackend) as _)
    }
}

impl AudioStream {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: AudioStreamConfig, crypto_backend: DynCryptoBackend) -> Self {
        debug!("new audio stream");

        Self {
            addr: config.addr,
            last_frame: now,
            dropped_frames: true,
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
            events: Default::default(),
        }
    }

    fn poll_depayloader(&mut self, now: Instant) -> Result<(), AudioStreamError> {
        while let Some(frame) = self.depayloader.poll_frame()? {
            self.last_frame = now;
            self.events.push_back(AudioStreamEvent::OnFrame(AudioFrame {
                timestamp: frame.timestamp,
                buffer: frame.buffer.into(),
            }));
        }

        Ok(())
    }
}

impl Debug for AudioStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[AudioStream]")
    }
}

impl Drop for AudioStream {
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

impl UdpStream for AudioStream {
    type Error = AudioStreamError;

    type Event = AudioStreamEvent;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
        self.ping_sender.pending_send().map(|x| (self.addr, x))
    }
    fn consume_send(&mut self) {
        self.ping_sender.consume_send();
    }

    fn poll_timeout(&self) -> Option<Instant> {
        if !self.dropped_frames {
            None
        } else {
            Some(self.last_frame + MAXIMUM_SAMPLE_WAIT)
        }
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        self.events.pop_front()
    }

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        if self.addr != addr {
            trace!(stream_addr = %self.addr, recv_addr = %addr, "received packet from non stream address");
            return Ok(());
        }

        self.depayloader.handle_packet(data)?;

        if !matches!(self.ping_sender.state(), PingSenderState::Finished) {
            self.ping_sender.set_finished();
        }

        self.handle_timeout(now)?;

        Ok(())
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error> {
        self.ping_sender.handle_timeout(now);

        if self.last_frame + MAXIMUM_SAMPLE_WAIT < now {
            if !self.dropped_frames {
                debug!(
                    "Dropping audio frame because it took too long to receive: Last Frame: {:?}, Current Time: {:?}",
                    self.last_frame, now
                );
            }

            self.dropped_frames = true;

            self.depayloader.try_skip_samples()?;
        }

        self.poll_depayloader(now)?;

        Ok(())
    }
}
