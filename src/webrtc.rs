//! This module contains common types and functions needed for game moonlight over webrtc.
//! It doesn't contain a full webrtc implementation.

use sdp_types::{Attribute, Session};

pub use sdp_types as sdp;

const CONTROL_STREAM_SIMPLE: &str = "x-moonlight-control-stream";
const CONTROL_STREAM_ENET: &str = "x-moonlight-control-stream:enet";

/// This contains moonlight feature support of a WebRTC session.
#[derive(Debug, Default)]
pub struct WebRtcClientFeatures {
    /// If the simple control stream is supported.
    ///
    /// The server will create the data channel.
    /// This data channel MUST be reliable and ordered.
    ///
    /// All messages sent over this channel SHOULD be binary messages.
    /// The contents of each message are equal to the serialized data of a [ControlPacket](crate::stream::proto::control::packet::ControlPacket).
    pub control_stream_simple: bool,
    /// If the control stream over enet is supported.
    ///
    /// The server will create the data channel.
    /// This data channel MUST be unreliable, unordered and MUST have the [`RTCDataChannel.protocol`](https://developer.mozilla.org/en-US/docs/Web/API/RTCDataChannel/protocol) field set to `enet`.
    ///
    /// This control stream will use enet over a data channel.
    /// The contents of each enet packet are equal to the serialized data of a [ControlPacket](crate::stream::proto::control::packet::ControlPacket).
    pub control_stream_enet: bool,
    // TODO: other extensions: e.g. apollo?
}

impl WebRtcClientFeatures {
    /// Get all custom WebRtc values from the session.
    pub fn from_session(session: &Session) -> Self {
        let mut this = Self::default();

        for attribute in &session.attributes {
            if attribute.attribute == CONTROL_STREAM_SIMPLE {
                this.control_stream_simple = true;
            }
            if attribute.attribute == CONTROL_STREAM_ENET {
                this.control_stream_enet = true;
            }
        }

        this
    }
    /// Removes all custom WebRtc values from the session.
    pub fn remove_from_session(session: &mut Session) {
        session
            .attributes
            .retain(|attribute| !attribute.attribute.starts_with("x-moonlight"));
    }

    /// Add all features to the session.
    pub fn apply_to(&self, session: &mut Session) {
        Self::remove_from_session(session);

        if self.control_stream_simple {
            session.attributes.push(Attribute {
                attribute: CONTROL_STREAM_SIMPLE.to_string(),
                value: None,
            });
        }
        if self.control_stream_enet {
            session.attributes.push(Attribute {
                attribute: CONTROL_STREAM_ENET.to_string(),
                value: None,
            });
        }
    }
}
