//! This module contains all the necessary parts for the Foundation Sunshine microphone protocol

// Can't read chinese but this seems important
// Rtsp Mic: https://github.com/AlkaidLab/foundation-sunshine/blob/master/src/rtsp.cpp#L916
// Mic receive: https://github.com/AlkaidLab/foundation-sunshine/blob/013388962e547698b34a1e6087f44b1ec2b58d17/src/stream.cpp#L1659-L1925
// Client impl: https://github.com/moonlight-stream/moonlight-common-c/pull/123/changes

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use sans_io_time::Instant;

use thiserror::Error;
use tracing::{Level, instrument};

use crate::stream::{
    SunshineEncryption,
    proto::{
        DynCryptoBackend,
        microphone::foundation::{
            packet::FOUNDATION_MAX_MIC_PACKET_SIZE,
            payloader::{
                FoundationMicPayloader, FoundationMicPayloaderConfig, FoundationMicPayloaderError,
            },
        },
        runtime::UdpStream,
    },
};

/// References:
/// - <https://github.com/qiin2333/moonlight-common-c/blob/7ed14144d1aef1d6d234ea98b17eedc083a5ac36/src/RtspConnection.c#L1438-L1439>
pub const FOUNDATION_DEFAULT_MIC_PORT: u16 = 47996;

pub mod packet;
pub mod payloader;
pub mod rtsp;

#[cfg(test)]
#[allow(clippy::unwrap_used, unused)]
mod test;

#[derive(Debug, Error)]
pub enum FoundationMicStreamError {
    #[error("payloader: {0}")]
    Payloader(#[from] FoundationMicPayloaderError),
}

#[derive(Debug)]
pub struct FoundationMicStreamConfig {
    pub addr: SocketAddr,
    /// If [Some] the mic stream is encrypted.
    pub encryption: Option<SunshineEncryption>,
}

#[derive(Debug)]
pub struct FoundationMicStream {
    addr: SocketAddr,
    payloader: FoundationMicPayloader,
    current_packet: Vec<u8>,
}

impl FoundationMicStream {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(
        now: Instant,
        config: FoundationMicStreamConfig,
        crypto_backend: DynCryptoBackend,
    ) -> Self {
        Self {
            addr: config.addr,
            payloader: FoundationMicPayloader::new(
                FoundationMicPayloaderConfig {
                    encryption: config.encryption,
                },
                crypto_backend,
            ),
            current_packet: vec![0; FOUNDATION_MAX_MIC_PACKET_SIZE],
        }
    }

    pub fn send_microphone_opus_data(
        &mut self,
        timestamp: Duration,
        frame: &[u8],
    ) -> Result<(), FoundationMicStreamError> {
        self.payloader.push_frame(timestamp, frame)?;

        Ok(())
    }

    fn update(&mut self) {
        if self.current_packet.is_empty()
            && let Some(packet) = self.payloader.poll_packet()
        {
            self.current_packet.extend_from_slice(packet);
        }
    }
}

impl UdpStream for FoundationMicStream {
    type Error = FoundationMicStreamError;

    type Event = Infallible;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
        if !self.current_packet.is_empty() {
            Some((self.addr, &self.current_packet))
        } else {
            None
        }
    }

    fn consume_send(&mut self) {
        self.current_packet.clear();
        self.update();
    }

    fn poll_timeout(&self) -> Option<Instant> {
        None
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        None
    }

    fn handle_receive(
        &mut self,
        _now: Instant,
        _addr: SocketAddr,
        _data: &[u8],
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn handle_timeout(&mut self, _now: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}
