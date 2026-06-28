use std::{net::SocketAddr, pin::pin, time::Duration};

use futures::{FutureExt, future::Fuse, select, try_join};
use sans_io_time::Instant;
use thiserror::Error;

use crate::stream::{
    HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
    audio::OpusMultistreamConfig,
    proto::{
        DynCryptoBackend, MoonlightStreamInput, MoonlightStreamProtoError, MoonlightStreamSetup,
        MoonlightStreamSetupOutput,
        audio::{AudioStream, AudioStreamError},
        control::{ControlStream, ControlStreamEvent, peer::ControlError},
        microphone::foundation::{FoundationMicStream, FoundationMicStreamError},
        video::{VideoStream, VideoStreamError},
    },
    video::{VideoCapabilities, VideoSetup},
};

pub trait UdpStream: Send + Sync {
    type Error;

    type Event;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])>;
    fn consume_send(&mut self);

    fn poll_timeout(&self) -> Option<Instant>;

    fn poll_event(&mut self) -> Option<Self::Event>;

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error>;

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error>;
}
