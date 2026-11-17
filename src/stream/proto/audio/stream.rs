use bytes::Bytes;
use core::fmt;
use sans_io_time::Instant;
use std::{
    collections::VecDeque,
    fmt::{Debug, Formatter},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tracing::{Level, debug, info, instrument, trace};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    error::Error,
    stream::{
        SunshineEncryption,
        audio::{AudioFrame, OpusMultistreamConfig},
        proto::{
            DynCryptoBackend,
            audio::depayloader::{AudioDepayloader, AudioDepayloaderConfig, AudioDepayloaderError},
            packet::SunshinePing,
            ping::{PingSender, PingSenderConfig},
            runtime::{Receive, Transmit, UdpStream},
            soonest,
        },
    },
};

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;

#[derive(Debug)]
pub struct AudioStreamConfig {
    pub addr: SocketAddr,
    pub opus_config: OpusMultistreamConfig,
    pub audio_packet_duration: Duration,
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
    max_sample_wait: Duration,
    first_packet: Option<Instant>,
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
            // maximum wait for a audio packet to be lost
            //
            // References: https://github.com/moonlight-stream/moonlight-common-c/blob/f900dd4767759c7b9d0e93bcea666b55c69ea62f/src/RtpAudioQueue.c#L544-L548
            max_sample_wait: (config.audio_packet_duration * 4) + Duration::from_millis(10),
            first_packet: None,
            last_frame: now,
            dropped_frames: true,
            ping_sender: PingSender::new(
                now,
                PingSenderConfig {
                    sunshine_ping: config.sunshine_ping,
                },
            ),
            depayloader: AudioDepayloader::new(
                AudioDepayloaderConfig {
                    fec: config.fec,
                    encryption: config.sunshine_encryption,
                },
                crypto_backend,
            ),
            events: Default::default(),
        }
    }

    fn poll_depayloader(&mut self, now: Instant) -> Result<(), Error> {
        while let Some(frame) = self.depayloader.poll_frame()? {
            self.last_frame = now;
            self.dropped_frames = false;

            self.events.push_back(AudioStreamEvent::OnFrame(frame));
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

impl UdpStream for AudioStream {
    type Event = AudioStreamEvent;

    fn poll_transmit(&mut self) -> Option<Transmit> {
        self.ping_sender.poll_transmit().map(|data| Transmit {
            destination: self.addr,
            data,
        })
    }

    fn poll_timeout(&self) -> Option<Instant> {
        let timeout = {
            // Wait for first packet
            #[allow(clippy::question_mark)]
            if self.first_packet.is_none() {
                None
            } else {
                if !self.dropped_frames {
                    None
                } else {
                    Some(self.last_frame + self.max_sample_wait)
                }
            }
        };

        soonest(timeout, self.ping_sender.poll_timeout())
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        self.events.pop_front()
    }

    fn handle_receive(&mut self, now: Instant, receive: Receive) -> Result<(), Error> {
        if self.addr != receive.source {
            trace!(stream_addr = %self.addr, recv_addr = %receive.source, "received packet from non stream address");
            return Ok(());
        }

        self.depayloader.handle_packet(receive.data)?;

        if self.first_packet.is_none() {
            info!(now = %now, "received first audio packet");

            self.first_packet = Some(now);
        }

        self.handle_timeout(now)?;

        Ok(())
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Error> {
        self.ping_sender.handle_timeout(now);

        if self.last_frame + self.max_sample_wait < now {
            if !self.dropped_frames {
                info!(
                    last_frame = %self.last_frame,
                    now = %now,
                    "dropping audio frame because it took too long to receive",
                );
            }

            // Wait another max_sample_wait before skipping again.
            // Without this the deadline stays in the past while no packet arrives.
            self.last_frame = now;

            self.dropped_frames = true;
        }
        if self.dropped_frames {
            self.depayloader.try_skip_samples()?;
        }

        self.poll_depayloader(now)?;

        Ok(())
    }

    fn recv_buffer_hint(&self) -> Option<usize> {
        // See https://github.com/moonlight-stream/moonlight-common-c/blob/e41355ea01670fd4c830b384009d31dd0339a705/src/AudioStream.c#L96
        None
    }
}
