use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    Endpoint, ParseError, TextResponse, helper::parse_xml_child_text, launch::ClientStreamRequest,
};

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

#[derive(Debug, Clone)]
pub struct ResumeResponse {
    pub resume: u32,
    /// The rtsp url for this resume request.
    ///
    /// See [wolf docs](https://games-on-whales.github.io/wolf/stable/protocols/rtsp.html) for more details:
    pub rtsp_session_url: String,
}

impl TextResponse for ResumeResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
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

        Ok(ResumeResponse {
            resume: parse_xml_child_text(root, "resume")?.parse()?,
            rtsp_session_url: parse_xml_child_text(root, "sessionUrl0")?.to_string(),
        })
    }
}
