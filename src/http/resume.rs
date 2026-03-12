use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    Endpoint, ParseError, TextResponse, helper::parse_xml_child_text, launch::ClientStreamRequest,
};

/// Resumes a session that was already created using a request to [super::launch::LaunchEndpoint].
pub struct ResumeEndpoint;

impl Endpoint for ResumeEndpoint {
    type Request = ClientStreamRequest;
    type Response = ResumeResponse;

    fn path() -> &'static str {
        "/resume"
    }

    fn https_required() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeResponse {
    pub resume: u32,
    /// The rtsp url for this resume request.
    ///
    /// References:
    /// - [wolf docs](https://games-on-whales.github.io/wolf/stable/protocols/rtsp.html) for more details:
    /// - moonlight common c: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Limelight.h#L534-L535
    pub rtsp_session_url: Option<String>,
}

impl TextResponse for ResumeResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        // XML header + root
        body_writer.write_str(r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
        body_writer.write_str(r#"<root status_code="200">"#)?;

        // <resume>
        write!(body_writer, "<resume>{}</resume>", self.resume)?;

        // <sessionUrl0>
        if let Some(rtsp_session_url) = &self.rtsp_session_url {
            write!(
                body_writer,
                "<sessionUrl0>{}</sessionUrl0>",
                rtsp_session_url
            )?;
        }

        // close root
        body_writer.write_str("</root>")?;

        Ok(())
    }
}

impl FromStr for ResumeResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = doc
            .root()
            .children()
            .find(|node| node.tag_name().name() == "root")
            .ok_or(ParseError::XmlRootNotFound)?;

        let rtsp_session_url = match parse_xml_child_text(root, "sessionUrl0") {
            Ok(value) => Some(value.to_string()),
            Err(ParseError::DetailNotFound(_)) => None,
            Err(err) => {
                return Err(err.into());
            }
        };

        Ok(ResumeResponse {
            resume: parse_xml_child_text(root, "resume")?.parse()?,
            rtsp_session_url,
        })
    }
}
