use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, debug, info, instrument, trace, trace_span};

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

pub struct VideoStream<Crypto> {
    crypto_backend: Crypto,
    aes_key: Option<AesKey>,
    last_now: Instant,
    state: State,
    depayloader: VideoDepayloader,
    first_frame: Option<Instant>,
    last_frame: Instant,
    current_frame: Option<FrameIndex>,
    frames_first_seen: HashMap<FrameIndex, Instant>,
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
            // the stream starts at frame index 1
            current_frame: None,
            last_frame: now,
            waiting_for_idr_since: Some(now),
        }
    }

    fn do_request_idr(&mut self) -> Result<VideoStreamOutput<'static>, VideoStreamError> {
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

            return Ok(VideoStreamOutput::SendControlMessage {
                message: ControlMessage(ControlMessageInner::SendPacket {
                    packet: ControlPacket::RequestIdr,
                    force: true,
                }),
            });
        }

        Ok(VideoStreamOutput::Timeout(self.last_now + timeout))
    }
    fn wait_until_idr(&self) -> Duration {
        // Default when we're stuck
        let mut timeout = STALL_TIMEOUT.saturating_sub(self.last_now - self.last_frame);

        let Some(current) = self.current_frame else {
            return timeout;
        };
        let highest = self.depayloader.known_frames().max().unwrap_or(current);

        let frame_known = self.depayloader.is_frame_known(FrameIndex(0));

        let current_frame_first_seen = self.frames_first_seen.get(&current);

        // After which time the frame can be seen as lost based on current time
        let current_frame_until_dropped =
            current_frame_first_seen.map(|current_frame_first_seen| {
                FULL_FRAME_RECEIVE_TIMEOUT.saturating_sub(self.last_now - *current_frame_first_seen)
            });

        // likely skipped frame
        if highest > current
            && let Some(current_frame_dropped) = current_frame_until_dropped
        {
            timeout = timeout.min(current_frame_dropped);
        }

        // incomplete frame
        if frame_known && let Some(current_frame_until_dropped) = current_frame_until_dropped {
            timeout = timeout.min(current_frame_until_dropped);
        }

        timeout
    }

    pub fn poll_output(&mut self) -> Result<VideoStreamOutput<'_>, VideoStreamError> {
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
                let mut frame_to_return = None;

                // Add the first seen numbers
                for frame_index in self.depayloader.known_frames() {
                    self.frames_first_seen
                        .entry(frame_index)
                        .or_insert(self.last_now);
                }

                if let Some(current_frame) = self.current_frame {
                    // discard the old frame
                    {
                        // TODO: maybe add a lower bound in the depayloader directly?
                        let mut known_frames_len = 0;
                        let mut known_frames: [FrameIndex; 200] = [FrameIndex(0); _];
                        let mut known_frames_iter = self.depayloader.known_frames();
                        for known_frame in known_frames_iter.by_ref() {
                            if known_frames.len() <= known_frames_len {
                                break;
                            }

                            known_frames[known_frames_len] = known_frame;
                            known_frames_len += 1;
                        }
                        drop(known_frames_iter);

                        for known_frame in known_frames[0..known_frames_len].iter() {
                            if *known_frame < current_frame {
                                self.depayloader.discard_frame(*known_frame);
                            }
                        }
                    }

                    // If we're synced just use the current frame
                    if self.depayloader.is_frame_available(current_frame) {
                        frame_to_return = Some(current_frame);
                    }
                }

                if self.waiting_for_idr_since.is_some() || self.current_frame.is_none() {
                    // Search for idrs, if we're waiting for one, or we're not in sync
                    for frame_index in self.depayloader.available_frames() {
                        let frame = self
                            .depayloader
                            .frame_metadata(frame_index)?
                            .expect("frame is available but couldn't be produced");

                        if frame.frame_type == FrameType::Idr {
                            debug!(now = ?self.last_now, frame_metadata = ?frame, "received idr");
                            self.waiting_for_idr_since = None;

                            frame_to_return = Some(frame.frame_index);
                        }
                    }
                }

                if let Some(frame_index) = frame_to_return {
                    if self.first_frame.is_none() {
                        self.first_frame = Some(self.last_now);
                    }

                    let frame = self
                        .depayloader
                        .frame(frame_index)?
                        .expect("failed to get frame");

                    self.current_frame = Some(FrameIndex(*frame_index + 1));
                    self.last_frame = self.last_now;

                    return Ok(VideoStreamOutput::VideoFrame(frame));
                }

                self.do_request_idr()
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

                self.depayloader.handle_packet(&data)?;

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
