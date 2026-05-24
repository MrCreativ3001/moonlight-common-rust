//! This module contains all the necessary parts for the Foundation Sunshine microphone protocol

// Can't read chinese but this seems important
// Rtsp Mic: https://github.com/AlkaidLab/foundation-sunshine/blob/master/src/rtsp.cpp#L916
// Mic receive: https://github.com/AlkaidLab/foundation-sunshine/blob/013388962e547698b34a1e6087f44b1ec2b58d17/src/stream.cpp#L1659-L1925
// Client impl: https://github.com/moonlight-stream/moonlight-common-c/pull/123/changes

use std::time::Duration;

use sans_io_time::Instant;

use thiserror::Error;
use tracing::{Level, instrument};

use crate::stream::{
    AesIv, AesKey,
    proto::{
        crypto::CryptoBackend,
        microphone::foundation::payloader::{
            FoundationMicPayloader, FoundationMicPayloaderConfig, FoundationMicPayloaderError,
        },
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
    /// If [Some] the mic stream is encrypted.
    pub encryption: Option<(AesKey, AesIv)>,
}

#[derive(Debug)]
pub enum FoundationMicStreamInput {
    Timeout(Instant),
}

#[derive(Debug)]
pub enum FoundationMicStreamOutput<'a> {
    Send { data: &'a [u8] },
    Timeout(Instant),
}

#[derive(Debug)]
pub struct FoundationMicStream<Crypto> {
    last_now: Instant,
    payloader: FoundationMicPayloader<Crypto>,
}

impl<Crypto> FoundationMicStream<Crypto>
where
    Crypto: CryptoBackend,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: FoundationMicStreamConfig, crypto_backend: Crypto) -> Self {
        Self {
            last_now: now,
            payloader: FoundationMicPayloader::new(
                FoundationMicPayloaderConfig {
                    encryption: config.encryption,
                },
                crypto_backend,
            ),
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

    pub fn poll_output(
        &mut self,
    ) -> Result<FoundationMicStreamOutput<'_>, FoundationMicStreamError> {
        let packet = self.payloader.poll_packet()?;

        if let Some(packet) = packet {
            Ok(FoundationMicStreamOutput::Send { data: packet })
        } else {
            Ok(FoundationMicStreamOutput::Timeout(
                self.last_now + Duration::from_secs(1),
            ))
        }
    }

    pub fn handle_input(
        &mut self,
        input: FoundationMicStreamInput,
    ) -> Result<(), FoundationMicStreamError> {
        match input {
            FoundationMicStreamInput::Timeout(now) => {
                self.last_now = now;

                Ok(())
            }
        }
    }
}
