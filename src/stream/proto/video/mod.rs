use std::{
    collections::HashMap,
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
                FrameIndex, VideoDepayloader, VideoDepayloaderConfig, VideoDepayloaderError,
                VideoDepayloaderOutput, VideoFrame,
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

/// The time window a frame has for all packets to be received
const FULL_FRAME_RECEIVE_TIMEOUT: Duration = Duration::from_millis(100);
/// A final timeout that is used when nothing happened
const STALL_TIMEOUT: Duration = Duration::from_millis(2000);
/// The time between each idr request
const IDR_REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug, Error)]
pub enum VideoStreamError {
    #[error("depayloader: {0}")]
    Depayloader(#[from] VideoDepayloaderError),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Debug)]
pub enum VideoStreamInput<'a> {
    Timeout(Instant),
    Receive { now: Instant, data: &'a [u8] },
}

#[derive(Debug)]
pub enum VideoStreamOutput<'a> {
    Send {
        data: Vec<u8>,
    },
    VideoFrame(VideoFrame<&'a [u8]>),
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
}

// TODO: maybe rename this into video stream proto?
pub struct VideoStream<Crypto> {
    crypto_backend: Crypto,
    aes_key: Option<AesKey>,
    last_now: Instant,
    state: State,
    depayloader: VideoDepayloader,
    first_frame: Option<Instant>,
    last_frame: Instant,
    frames_first_seen: HashMap<u32, Instant>,
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
            aes_key: config.sunshine_encryption,
            state: State::SendPing {
                last_send: None,
                sunshine_ping: config.sunshine_ping.map(|payload| SunshinePingPacket {
                    payload,
                    sequence_number: 0,
                }),
            },
            last_now: now,
            first_frame: None,
            frames_first_seen: Default::default(),
            depayloader,
            last_frame: now,
            waiting_for_idr_since: Some(now),
        }
    }

    fn do_request_idr(&mut self) -> Result<Option<VideoStreamOutput>, VideoStreamError> {
        // request an idr if needed
        let timeout = self.wait_until_idr();

        let mut request_idr = false;

        if timeout.is_zero() {
            if let Some(last_idr_request) = self.waiting_for_idr_since {
                if self.last_now - last_idr_request >= IDR_REQUEST_TIMEOUT {
                    request_idr = true;
                }
            } else {
                request_idr = true;
            }
        }

        if request_idr {
            self.waiting_for_idr_since = Some(self.last_now);

            info!(time_until_idr = ?timeout, now = ?self.last_now, waiting_for_idr_since = ?self.waiting_for_idr_since, "requesting idr and unsyncing depayloader");

            todo!();

            return Ok(Some(VideoStreamOutput::SendControlMessage {
                message: ControlMessage(ControlMessageInner::SendPacket {
                    packet: ControlPacket::RequestIdr,
                    force: true,
                }),
            }));
        }

        Ok(Some(VideoStreamOutput::Timeout(self.last_now + timeout)))
    }
    fn wait_until_idr(&self) -> Duration {
        todo!();

        // let highest = status.highest_seen_frame_index.unwrap_or(0);

        // let frame = self.depayloader.is_frame_available(FrameIndex(0));

        // let current_frame_first_seen = self.frames_first_seen.get(&current);

        // // Default when we're stuck
        // let mut timeout = STALL_TIMEOUT.saturating_sub(self.last_now - self.last_frame);

        // // After which time the frame can be seen as lost based on current time
        // let current_frame_until_dropped =
        //     current_frame_first_seen.map(|current_frame_first_seen| {
        //         FULL_FRAME_RECEIVE_TIMEOUT.saturating_sub(self.last_now - *current_frame_first_seen)
        //     });

        // // likely skipped frame
        // if highest > current
        //     && let Some(current_frame_dropped) = current_frame_until_dropped
        // {
        //     timeout = timeout.min(current_frame_dropped);
        // }

        // // incomplete frame
        // if let Some(frame) = frame
        //     && let Some(total) = frame.total_data_packets
        //     && frame.received_data_packets < total
        //     && let Some(current_frame_until_dropped) = current_frame_until_dropped
        // {
        //     timeout = timeout.min(current_frame_until_dropped);
        // }

        // timeout
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
                // TODO: Delete old frame first packet receive

                // TODO: implement this
                if let Some(frame) = self.depayloader.frame(FrameIndex(0))? {
                    if self.first_frame.is_none() {
                        self.first_frame = Some(self.last_now);
                    }

                    let mut should_return_frame = false;

                    if frame.metadata.frame_type == FrameType::Idr {
                        debug!(now = ?self.last_now, "received idr");
                        self.waiting_for_idr_since = None;

                        todo!();

                        should_return_frame = true;
                    } else if self.waiting_for_idr_since.is_some() {
                        // TODO: keep some frames because the idr might arrive late
                        debug!(now = ?self.last_now, "dropping received frame because waiting for an idr");
                    } else {
                        should_return_frame = true;
                    }

                    if should_return_frame {
                        self.last_frame = self.last_now;

                        return Ok(VideoStreamOutput::VideoFrame(frame));
                    }
                }

                // TODO: fix
                // if let Some(timeout) = self.do_request_idr()? {
                //     return Ok(timeout);
                // }

                // TODO: fix
                Ok(VideoStreamOutput::Timeout(Instant::now()))
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
                // if rand::random_bool(0.07) {
                //     info!("TESTING: dropping packet");
                //     return Ok(());
                // }

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

// TODO: test idr requesting logic?
