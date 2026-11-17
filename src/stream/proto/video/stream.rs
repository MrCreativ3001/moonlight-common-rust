use core::fmt;
use std::{
    collections::{HashMap, VecDeque},
    fmt::{Debug, Formatter},
    net::SocketAddr,
    time::Duration,
};

use sans_io_time::Instant;
use tracing::{Level, debug, info, instrument, trace};

use crate::{
    error::Error,
    stream::{
        proto::{
            DynCryptoBackend,
            packet::SunshinePing,
            ping::{PingSender, PingSenderConfig},
            runtime::{Receive, Transmit, UdpStream},
            soonest,
            video::{
                depayloader::{VideoDepayloader, VideoDepayloaderConfig},
                frame::OwnedVideoFrame,
                packet::VIDEO_RECV_BUFFERED_PACKETS,
            },
        },
        video::{FrameIndex, ServerFrameType},
    },
};

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;

/// The time window a frame has for all packets to be received
const FULL_FRAME_RECEIVE_TIMEOUT: Duration = Duration::from_millis(100);
/// The time between each idr request
const IDR_REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug)]
pub enum VideoStreamEvent {
    OnFrame(OwnedVideoFrame),
    SignalIdr,
}

#[derive(Debug, Clone)]
pub struct VideoStreamConfig {
    pub addr: SocketAddr,
    pub queue: VideoDepayloaderConfig,
    pub fps: u32,
    pub sunshine_ping: Option<SunshinePing>,
}

pub struct VideoStream {
    addr: SocketAddr,
    last_now: Instant,
    ping_sender: PingSender,
    depayloader: VideoDepayloader,
    first_packet: Option<Instant>,
    first_frame: Option<Instant>,
    last_frame: Instant,
    current_frame: Option<FrameIndex>,
    frames_first_seen: HashMap<FrameIndex, Instant>,
    idr_deadline: Instant,
    events: VecDeque<VideoStreamEvent>,
}

impl VideoStream {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, config: VideoStreamConfig, crypto_backend: DynCryptoBackend) -> Self {
        debug!("new video stream");

        let depayloader = VideoDepayloader::new(config.queue, crypto_backend);

        Self {
            addr: config.addr,
            last_now: now,
            ping_sender: PingSender::new(
                now,
                PingSenderConfig {
                    sunshine_ping: config.sunshine_ping,
                },
            ),
            first_packet: None,
            first_frame: None,
            frames_first_seen: Default::default(),
            depayloader,
            // the stream starts at frame index 1
            current_frame: None,
            last_frame: now,
            idr_deadline: now + IDR_REQUEST_TIMEOUT,
            events: Default::default(),
        }
    }

    fn do_idr_request(&mut self) -> Result<(), Error> {
        if self.last_now >= self.idr_deadline {
            // move idr deadline forward
            self.idr_deadline = self.last_now + IDR_REQUEST_TIMEOUT;

            // unsync video stream
            self.current_frame = None;

            info!(now = ?self.last_now, waiting_for_idr_since = ?self.idr_deadline, "requesting idr and unsyncing video stream");

            self.events.push_back(VideoStreamEvent::SignalIdr);
        }

        Ok(())
    }

    fn update(&mut self, now: Instant) -> Result<(), Error> {
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
        } else {
            // Search for idrs, if we're waiting for one, or we're not in sync
            for frame_index in self.depayloader.available_frames() {
                let frame = self
                    .depayloader
                    .frame_metadata(frame_index)
                    .expect("the frame should have metadata at this point");

                if frame.frame_type == ServerFrameType::Idr {
                    debug!(now = ?now, frame_metadata = ?frame, "received idr");

                    frame_to_return = Some(frame.frame_index);
                }
            }
        }

        // Look if the next frame is available
        if let Some(frame_index) = frame_to_return {
            if self.first_frame.is_none() {
                debug!(now = %now, "received first frame");
                self.first_frame = Some(now);
            }

            self.last_frame = now;
            self.idr_deadline = self.last_now + FULL_FRAME_RECEIVE_TIMEOUT;

            self.current_frame = Some(FrameIndex(frame_index.0 + 1));

            let frame = self
                .depayloader
                .take_frame(frame_index)
                .expect("VideoStream::update, take_frame");
            self.events.push_back(VideoStreamEvent::OnFrame(frame));

            return Ok(());
        }

        self.do_idr_request()?;

        Ok(())
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
    type Event = VideoStreamEvent;

    fn poll_transmit(&mut self) -> Option<Transmit> {
        let data = self.ping_sender.poll_transmit()?;

        Some(Transmit {
            destination: self.addr,
            data,
        })
    }

    fn poll_timeout(&self) -> Option<Instant> {
        soonest(self.idr_deadline, self.ping_sender.poll_timeout())
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        self.events.pop_front()
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Error> {
        self.last_now = now;

        self.ping_sender.handle_timeout(now);
        self.update(now)?;

        Ok(())
    }

    fn handle_receive(&mut self, now: Instant, receive: Receive) -> Result<(), Error> {
        self.last_now = now;

        self.ping_sender.handle_timeout(now);

        if self.addr != receive.source {
            self.update(now)?;

            trace!(stream_addr = %self.addr, recv_addr = %receive.source, "received packet from non stream address");
            return Ok(());
        }

        if self.first_packet.is_none() {
            info!(now = %now, "received first video packet");

            self.first_packet = Some(now);
        }

        self.depayloader.handle_packet(receive.data)?;

        self.update(now)?;

        Ok(())
    }

    fn recv_buffer_hint(&self) -> Option<usize> {
        // See https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoStream.c#L331-L333
        Some(VIDEO_RECV_BUFFERED_PACKETS * self.depayloader.packet_size())
    }
}
