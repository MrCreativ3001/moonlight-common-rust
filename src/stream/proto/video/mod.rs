use std::{
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, debug, instrument, trace};

use crate::{
    ServerVersion,
    stream::{
        AesKey,
        proto::{
            crypto::{CipherAlgorithm, CipherFlags, CryptoContext},
            packet::SunshinePingPacket,
            rtsp::moonlight::SunshinePing,
            video::{
                depayloader::{VideoDepayloader, VideoDepayloaderConfig, VideoFrame},
                packet::EncryptedVideoHeader,
            },
        },
    },
};

pub mod depayloader;
mod nal;
mod packet;
pub mod payloader;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test;

const PING_RETRY: Duration = Duration::from_millis(500);

#[derive(Debug, Error)]
pub enum VideoStreamError {}

#[derive(Debug)]
pub enum VideoStreamInput<'a> {
    Timeout(Instant),
    Receive { now: Instant, data: &'a [u8] },
}

#[derive(Debug)]
pub enum VideoStreamOutput {
    SendUdp { to: SocketAddr, data: Vec<u8> },
    ConnectTcp { to: SocketAddr },
    DisconnectTcp,
    VideoFrame(VideoFrame),
    Timeout(Instant),
}

#[derive(Debug, Clone)]
pub struct VideoStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub queue: VideoDepayloaderConfig,
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
enum FirstFrame {
    ConnectTcp,
    WaitTcp,
    DisconnectTcp,
    Finished,
}

pub struct VideoStream {
    addr: SocketAddr,
    encryption: Option<(Arc<dyn CryptoContext>, AesKey)>,
    last_now: Instant,
    state: State,
    queue: VideoDepayloader,
    /// Only on gfe v3
    /// I don't know who made this, but you just need to connect to a specific port via tcp and then instantly close the connection
    /// Interesting...
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoStream.c#L362-L364
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoStream.c#L266-L275
    first_frame_connect: FirstFrame,
}

impl VideoStream {
    #[instrument(level = Level::DEBUG)]
    pub fn new(now: Instant, config: VideoStreamConfig) -> Self {
        Self {
            addr: config.addr,
            // TODO: get crypto context when config.encrypted
            encryption: None,
            state: State::SendPing {
                last_send: None,
                sunshine_ping: config.sunshine_ping.map(|payload| SunshinePingPacket {
                    payload,
                    sequence_number: 0,
                }),
            },
            last_now: now,
            queue: VideoDepayloader::new(config.queue),
            first_frame_connect: if config.server_version.major == 3 {
                FirstFrame::ConnectTcp
            } else {
                FirstFrame::Finished
            },
        }
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

                debug!(packet = ?packet, "Sending initial video ping");

                last_send.replace(self.last_now);

                Ok(VideoStreamOutput::SendUdp {
                    to: self.addr,
                    data: packet,
                })
            }
            State::ReceiveVideo => {
                if let Some(frame) = self.queue.poll_frame().unwrap() {
                    return Ok(VideoStreamOutput::VideoFrame(frame));
                }

                Ok(VideoStreamOutput::Timeout(
                    // TODO: set video timeout and then do exit
                    self.last_now + Duration::from_secs(1),
                ))
            }
        }

        // TODO: also implement tcp first frame
    }

    pub fn handle_input(&mut self, input: VideoStreamInput) -> Result<(), VideoStreamError> {
        match input {
            VideoStreamInput::Timeout(now) => {
                self.last_now = now;

                Ok(())
            }
            VideoStreamInput::Receive { now, data } => {
                self.last_now = now;

                self.state = State::ReceiveVideo;

                let data = if let Some((crypto_context, key)) = self.encryption.as_ref() {
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
                    let size = crypto_context
                        .decrypt(
                            CipherAlgorithm::AesGcm,
                            CipherFlags::empty(),
                            &key, // TODO: get key <---
                            &encryption_header.iv,
                            Some(&encryption_header.tag),
                            &data[32..],
                            &mut decrypted,
                        )
                        .unwrap();
                    decrypted.resize(size, 0);

                    decrypted
                } else {
                    data.to_vec()
                };

                self.queue.handle_packet(&data).unwrap();

                Ok(())
            }
        }
    }
}

impl Debug for VideoStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[VideoStream]")
    }
}
