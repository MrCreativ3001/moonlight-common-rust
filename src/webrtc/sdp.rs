use sdp_types::{Attribute, Session};

pub use sdp_types as sdp;

const CONTROL_STREAM_SIMPLE: &str = "x-moonlight-control-stream";
const CONTROL_STREAM_ENET: &str = "x-moonlight-control-stream:enet";
const MICROPHONE_STREAM_IDENTIFICATION: &str = "x-moonlight-microphone";

/// This contains the feature support of a webrtc session description.
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
    /// The media stream id + track id of the track that the microphone stream is associated with.
    pub microphone_msid: Option<(String, String)>,
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

        for media in &session.medias {
            let mut is_microphone = false;
            let mut msid: Option<(String, String)> = None;

            for attribute in &media.attributes {
                match attribute.attribute.as_str() {
                    "msid" => {
                        if let Some(value) = &attribute.value {
                            let mut parts = value.split_whitespace();
                            if let (Some(stream_id), Some(track_id)) = (parts.next(), parts.next())
                            {
                                msid = Some((stream_id.to_string(), track_id.to_string()));
                            }
                        }
                    }

                    MICROPHONE_STREAM_IDENTIFICATION => {
                        is_microphone = true;
                    }

                    _ => {}
                }
            }

            if is_microphone {
                this.microphone_msid = msid;
            }
        }

        this
    }
    /// Removes all Moonlight WebRtc values from the session.
    pub fn remove_from_session(session: &mut Session) {
        session
            .attributes
            .retain(|attribute| !attribute.attribute.starts_with("x-moonlight"));

        for media in &mut session.medias {
            media
                .attributes
                .retain(|attribute| !attribute.attribute.starts_with("x-moonlight"));
        }
    }

    /// Add all moonlight features to the session.
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

        if let Some((ref stream_id, ref track_id)) = self.microphone_msid {
            for media in &mut session.medias {
                let mut matches = false;

                if let Ok(values) = media.get_attribute_values("msid") {
                    for value in values.into_iter().flatten() {
                        let mut parts = value.split_whitespace();
                        if let (Some(sid), Some(tid)) = (parts.next(), parts.next())
                            && sid == stream_id
                            && tid == track_id
                        {
                            matches = true;
                            break;
                        }
                    }
                }

                if matches {
                    media.attributes.push(Attribute {
                        attribute: MICROPHONE_STREAM_IDENTIFICATION.to_string(),
                        value: None,
                    });
                }
            }
        }
    }
}

// TODO: tests
