use std::{
    fmt::{self, Debug, Formatter},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, debug, info, instrument};

use crate::stream::{
    AesKey,
    proto::{
        ControlMessageInner,
        control::{ControlMessage, packet::ControlPacket},
        crypto::{CipherAlgorithm, CryptoBackend, CryptoError},
        packet::SunshinePingPacket,
        rtsp::moonlight::SunshinePing,
        video::{
            depayloader::{
                VideoDepayloader, VideoDepayloaderConfig, VideoDepayloaderOutput,
                VideoDepayloaderReport, VideoFrame,
            },
            packet::{EncryptedVideoHeader, FrameType},
        },
    },
};

pub mod depayloader;
mod nal;
mod packet;
pub mod payloader;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test;

const PING_RETRY: Duration = Duration::from_millis(500);

#[derive(Debug, Error)]
pub enum VideoStreamError {
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Debug)]
pub enum VideoStreamInput<'a> {
    Timeout(Instant),
    Receive { now: Instant, data: &'a [u8] },
}

#[derive(Debug)]
pub enum VideoStreamOutput {
    Send {
        data: Vec<u8>,
    },
    VideoFrame(VideoFrame),
    /// Send a control message to the [ControlStream](super::control::ControlStream).
    SendControlMessage {
        message: ControlMessage,
    },
    Timeout(Instant),
}

#[derive(Debug, Clone)]
pub struct VideoStreamConfig {
    pub queue: VideoDepayloaderConfig,
    pub fps: u32,
    pub sunshine_ping: Option<SunshinePing>,
    pub sunshine_encryption: Option<AesKey>,
}

enum State {
    SendPing {
        last_send: Option<Instant>,
        sunshine_ping: Option<SunshinePingPacket>,
    },
    ReceiveVideo,
    SendFecReport {
        fec_report: VideoDepayloaderReport,
    },
}

// TODO: maybe rename this into video stream proto?
pub struct VideoStream<Crypto> {
    crypto_backend: Crypto,
    aes_key: Option<AesKey>,
    frame_rate: u32,
    last_now: Instant,
    state: State,
    depayloader: VideoDepayloader,
    last_frame: Option<Instant>,
    waiting_for_idr_since: Option<Instant>,
}

impl<Crypto> VideoStream<Crypto>
where
    Crypto: CryptoBackend,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: VideoStreamConfig, crypto_backend: Crypto) -> Self {
        let depayloader = VideoDepayloader::new(config.queue);

        Self {
            crypto_backend,
            frame_rate: config.fps,
            aes_key: config.sunshine_encryption,
            state: State::SendPing {
                last_send: None,
                sunshine_ping: config.sunshine_ping.map(|payload| SunshinePingPacket {
                    payload,
                    sequence_number: 0,
                }),
            },
            last_now: now,
            last_frame: None,
            depayloader,
            waiting_for_idr_since: Some(now),
        }
    }

    fn duration_until_frame_drop(&self) -> Option<Duration> {
        self.last_frame.map(|last_frame| {
            (Duration::from_secs_f32(1.0 / self.frame_rate as f32) + Duration::from_millis(10))
                .saturating_sub(self.last_now.saturating_duration_since(last_frame))
        })
    }

    pub fn poll_output(&mut self) -> Result<VideoStreamOutput, VideoStreamError> {
        match &mut self.state {
            State::SendPing {
                last_send,
                sunshine_ping,
            } => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/VideoStream.c#L54-L82
                if let Some(last_send) = last_send
                    && *last_send + PING_RETRY > self.last_now
                {
                    return Ok(VideoStreamOutput::Timeout(*last_send + PING_RETRY));
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

                debug!(packet = ?packet, "sending initial video ping");

                last_send.replace(self.last_now);

                Ok(VideoStreamOutput::Send { data: packet })
            }
            State::ReceiveVideo => {
                if let VideoDepayloaderOutput::Frame {
                    frame,
                    report: fec_report,
                } = self.depayloader.poll_output().unwrap()
                {
                    if frame.frame_type == FrameType::Idr {
                        debug!(now = ?self.last_now, "received idr");
                        self.waiting_for_idr_since = None;
                    } else if self.waiting_for_idr_since.is_some() {
                        debug!(now = ?self.last_now, "dropping received frame because waiting for an idr");

                        return Ok(VideoStreamOutput::Timeout(
                            self.last_now + Duration::from_secs(1),
                        ));
                    }

                    self.state = State::SendFecReport { fec_report };
                    self.last_frame = Some(self.last_now);

                    return Ok(VideoStreamOutput::VideoFrame(frame));
                }

                // Frame dropping logic
                let duration_until_frame_drop = self.duration_until_frame_drop();
                if let Some(duration_until_frame_drop) = duration_until_frame_drop
                    && duration_until_frame_drop.is_zero()
                {
                    self.last_frame = Some(self.last_now);
                    self.depayloader
                        .discard_frame(self.depayloader.status().current_frame_index)
                        .unwrap();

                    if self.waiting_for_idr_since.is_none()
                        || self
                            .waiting_for_idr_since
                            .is_some_and(|x| self.last_now - x > Duration::from_secs(1))
                    {
                        info!(
                            now = ?self.last_now,
                            last_frame = ?self.last_frame,
                            depayloader_status = ?self.depayloader.status(),
                            "requesting idr because frame took too long to receive"
                        );

                        // Request idr if we're not already waiting
                        self.waiting_for_idr_since = Some(self.last_now);
                        return Ok(VideoStreamOutput::SendControlMessage {
                            message: ControlMessage(ControlMessageInner::SendPacket {
                                packet: ControlPacket::RequestIdr,
                                force: true,
                            }),
                        });
                    }
                }

                Ok(VideoStreamOutput::Timeout(
                    // TODO: set video timeout and then do exit
                    duration_until_frame_drop
                        .map(|x| self.last_now + x)
                        .unwrap_or_else(|| self.last_now + Duration::from_secs(1)),
                ))
            }
            State::SendFecReport { fec_report } => {
                let message = ControlMessageInner::SendPacket {
                    force: false,
                    packet: ControlPacket::FrameFec {
                        frame_index: fec_report.frame_index,
                        highest_received_sequence_number: fec_report
                            .highest_received_sequence_number,
                        next_contiguous_sequence_number: fec_report.next_contiguous_sequence_number,
                        missing_packets_before_highest_received: fec_report
                            .missing_packets_before_highest_received,
                        total_data_packets: fec_report.total_data_packets,
                        total_parity_packets: fec_report.total_parity_packets,
                        received_data_packets: fec_report.received_data_packets,
                        received_parity_packets: fec_report.received_parity_packets,
                        fec_percentage: fec_report.fec_percentage,
                        multi_fec_block_index: fec_report.multi_fec_block_index,
                        multi_fec_block_count: fec_report.multi_fec_block_count,
                    },
                };

                self.state = State::ReceiveVideo;

                // TODO: for some reason sunshine doesn't support this??!?
                // Ok(VideoStreamOutput::SendControlMessage {
                //     message: ControlMessage(message),
                // })
                Ok(VideoStreamOutput::Timeout(self.last_now))
            }
        }
    }

    pub fn handle_input(&mut self, input: VideoStreamInput) -> Result<(), VideoStreamError> {
        match input {
            VideoStreamInput::Timeout(now) => {
                self.last_now = now;

                Ok(())
            }
            VideoStreamInput::Receive { now, data } => {
                self.last_now = now;

                if matches!(self.state, State::SendPing { .. }) {
                    info!(now = ?self.last_now, "received first video packet");
                }

                self.state = State::ReceiveVideo;

                // TODO: remove this randomness, just for testing
                if rand::random_bool(0.03) {
                    info!("TESTING: dropping packet");
                    return Ok(());
                }

                // TODO: move this into the depayloader
                let data = if let Some(aes_key) = self.aes_key.as_ref() {
                    // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/VideoStream.c#L213-L220

                    // TODO: check size before access
                    let encryption_header = EncryptedVideoHeader::deserialize(
                        data[0..EncryptedVideoHeader::SIZE]
                            .as_array::<{ EncryptedVideoHeader::SIZE }>()
                            .unwrap(),
                    );

                    // TODO: store this buffer inside ourself's struct because the size is known, but check just to be careful beforehand!: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/VideoStream.c#L96
                    let mut decrypted = vec![0; data.len() - EncryptedVideoHeader::SIZE];

                    // TODO: fix unwrap
                    let size = self.crypto_backend.decrypt(
                        CipherAlgorithm::Aes128Gcm,
                        &aes_key, // TODO: get key <---
                        &encryption_header.iv,
                        Some(&encryption_header.tag),
                        &data[32..],
                        &mut decrypted,
                    )?;
                    decrypted.resize(size, 0);

                    decrypted
                } else {
                    data.to_vec()
                };

                self.depayloader.handle_packet(&data).unwrap();

                Ok(())
            }
        }
    }
}

impl<Crypto> Debug for VideoStream<Crypto> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[VideoStream]")
    }
}

impl<Crypto> Drop for VideoStream<Crypto> {
    fn drop(&mut self) {
        info!("terminated video stream");
    }
}
