use std::{
    collections::{HashMap, VecDeque},
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    time::Duration,
};

use sans_io_time::Instant;

use thiserror::Error;
use tracing::{Level, debug, info, instrument, trace};

use crate::stream::{
    AesKey,
    proto::{
        DynCryptoBackend,
        crypto::CryptoError,
        packet::SunshinePing,
        ping::{PingSender, PingSenderConfig, PingSenderState},
        runtime::UdpStream,
        video::{
            depayloader::{VideoDepayloader, VideoDepayloaderConfig, VideoDepayloaderError},
            packet::FrameType,
        },
    },
    video::{ColorSpace, FrameIndex, VideoDecodeUnit},
};

pub mod depayloader;
#[allow(unused)]
mod nal;
mod packet;
pub mod payloader;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test;

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
pub enum VideoStreamEvent {
    OnFrame,
    SignalIdr,
}

#[derive(Debug, Clone)]
pub struct VideoStreamConfig {
    pub addr: SocketAddr,
    pub queue: VideoDepayloaderConfig,
    pub fps: u32,
    pub sunshine_ping: Option<SunshinePing>,
    pub sunshine_encryption: Option<AesKey>,
}

pub struct VideoStream {
    #[allow(unused)]
    crypto_backend: DynCryptoBackend,
    #[allow(unused)]
    aes_key: Option<AesKey>,
    addr: SocketAddr,
    last_now: Instant,
    ping_sender: PingSender,
    depayloader: VideoDepayloader,
    first_frame: Option<Instant>,
    last_frame: Instant,
    current_frame: Option<FrameIndex>,
    frames_first_seen: HashMap<FrameIndex, Instant>,
    waiting_for_idr_since: Option<Instant>,
    events: VecDeque<VideoStreamEvent>,
}

impl VideoStream {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: VideoStreamConfig, crypto_backend: DynCryptoBackend) -> Self {
        let depayloader = VideoDepayloader::new(config.queue);

        Self {
            crypto_backend,
            aes_key: config.sunshine_encryption,
            addr: config.addr,
            last_now: now,
            ping_sender: PingSender::new(
                now,
                PingSenderConfig {
                    sunshine_ping: config.sunshine_ping,
                },
            ),
            first_frame: None,
            frames_first_seen: Default::default(),
            depayloader,
            // the stream starts at frame index 1
            current_frame: None,
            last_frame: now,
            waiting_for_idr_since: Some(now),
            events: Default::default(),
        }
    }

    fn do_idr_request(&mut self) -> Result<(), VideoStreamError> {
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

            self.events.push_back(VideoStreamEvent::SignalIdr);
        }

        Ok(())
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

    fn update(&mut self, now: Instant) -> Result<(), VideoStreamError> {
        let mut frame_to_return = None;

        // Add the first seen numbers
        for frame_index in self.depayloader.known_frames() {
            self.frames_first_seen.entry(frame_index).or_insert(now);
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
                    debug!(now = ?now, frame_metadata = ?frame, "received idr");
                    self.waiting_for_idr_since = None;

                    frame_to_return = Some(frame.frame_index);
                }
            }
        }

        // Look if the next frame is available
        if let Some(frame_index) = frame_to_return {
            if self.first_frame.is_none() {
                self.first_frame = Some(now);
            }

            self.last_frame = now;

            self.current_frame = Some(frame_index);
            self.events.push_back(VideoStreamEvent::OnFrame);

            return Ok(());
        }

        self.do_idr_request()?;

        Ok(())
    }

    pub fn poll_frame(&mut self) -> Option<VideoDecodeUnit<&[u8]>> {
        if let Some(current_frame) = self.current_frame {
            let frame = self.depayloader.frame(current_frame).ok()??;
            self.current_frame = Some(FrameIndex(*current_frame + 1));

            Some(VideoDecodeUnit {
                frame_number: frame.metadata.frame_index,
                frame_type: frame.parsed_frame_type,
                frame_processing_latency: frame.metadata.host_processing_latency,
                timestamp: frame.metadata.timestamp,
                color_space: ColorSpace::Rec709,
                buffers: frame.buffers,
            })
        } else {
            None
        }
    }

    pub fn request_idr(&mut self) {
        info!("requesting idr on behalf of the video decoder");

        // Push event
        self.events.push_back(VideoStreamEvent::SignalIdr);
    }
}

impl Debug for VideoStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[VideoStream]")
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        info!("terminated video stream");
    }
}

impl UdpStream for VideoStream {
    type Error = VideoStreamError;

    type Event = VideoStreamEvent;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
        self.ping_sender.pending_send().map(|x| (self.addr, x))
    }
    fn consume_send(&mut self) {
        self.ping_sender.consume_send()
    }

    fn poll_timeout(&self) -> Option<Instant> {
        Some(self.last_now + self.wait_until_idr())
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        self.events.pop_front()
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error> {
        self.last_now = now;

        self.ping_sender.handle_timeout(now);
        self.update(now)?;

        Ok(())
    }

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.last_now = now;

        self.ping_sender.handle_timeout(now);

        if self.addr != addr {
            self.update(now)?;

            trace!(stream_addr = %self.addr, recv_addr = %addr, "received packet from non stream address");
            return Ok(());
        }

        if !matches!(self.ping_sender.state(), PingSenderState::Finished) {
            info!(now = ?now, "received first video packet");

            self.ping_sender.set_finished();
        }

        self.depayloader.handle_packet(data)?;

        self.update(now)?;

        Ok(())
    }
}
