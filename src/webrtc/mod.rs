//! This module contains common types and functions needed for game moonlight over webrtc.
//! It doesn't contain a full webrtc implementation.
//!
#![doc = include_str!("./protocol.md")]

pub mod launch;

pub const LINK_MICROPHONE: &str = "<urn:moonlight:microphone>; rel=\"urn:whep:microphone\"";
pub const LINK_CONTROL_STREAM_SIMPLE: &str = "<urn:moonlight:control>; rel=\"urn:whep:control\"";
pub const LINK_CONTROL_STREAM_ENET: &str = "<urn:moonlight:control-enet>; rel=\"urn:whep:control\"";
