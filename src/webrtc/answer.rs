use std::str::FromStr;

use sdp_types::Session;

use crate::webrtc::{WebRTCParseError, bool_str, parse_bool, push};

pub struct WebRTCSessionAnswer {
    /// The name of the app that was started.
    pub app_name: Option<String>,
    /// If the server supports microphone passthrough.
    pub microphone: bool,
}

impl FromStr for WebRTCSessionAnswer {
    type Err = WebRTCParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let session = Session::parse(s.as_bytes())?;

        Self::from_sdp(&session)
    }
}

impl WebRTCSessionAnswer {
    pub fn from_sdp(session: &Session) -> Result<Self, WebRTCParseError> {
        let mut app_name = None;
        let mut microphone = false;

        for attr in &session.attributes {
            let Some(value) = &attr.value else {
                continue;
            };

            match attr.attribute.as_str() {
                "x-moonlight-app-name" => app_name = attr.value.clone(),
                "x-moonlight-microphone" => {
                    microphone = parse_bool("x-moonlight-microphone", value)?;
                }
                _ => {}
            }
        }

        Ok(Self {
            app_name,
            microphone,
        })
    }

    pub fn apply(&self, session: &mut Session) {
        if let Some(app_name) = &self.app_name {
            push(session, "x-moonlight-app-name", app_name);
        }
        if self.microphone {
            push(session, "x-moonlight-microphone", bool_str(true));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use sdp_types::Session;

    fn base_session() -> Session {
        Session::parse(
            b"v=0\r\n\
              o=- 0 0 IN IP4 127.0.0.1\r\n\
              s=-\r\n\
              t=0 0\r\n",
        )
        .unwrap()
    }

    #[test]
    fn parses_empty_answer() {
        let session = base_session();

        let answer = WebRTCSessionAnswer::from_sdp(&session).unwrap();

        assert_eq!(answer.app_name, None);
        assert!(!answer.microphone);
    }

    #[test]
    fn parses_app_name() {
        let session = Session::parse(
            b"v=0\r\n\
              o=- 0 0 IN IP4 127.0.0.1\r\n\
              s=-\r\n\
              t=0 0\r\n\
              a=x-moonlight-app-name:Steam\r\n",
        )
        .unwrap();

        let answer = WebRTCSessionAnswer::from_sdp(&session).unwrap();

        assert_eq!(answer.app_name.as_deref(), Some("Steam"));
        assert!(!answer.microphone);
    }

    #[test]
    fn parses_microphone_flag() {
        let session = Session::parse(
            b"v=0\r\n\
              o=- 0 0 IN IP4 127.0.0.1\r\n\
              s=-\r\n\
              t=0 0\r\n\
              a=x-moonlight-microphone:1\r\n",
        )
        .unwrap();

        let answer = WebRTCSessionAnswer::from_sdp(&session).unwrap();

        assert!(answer.microphone);
    }

    #[test]
    fn rejects_invalid_microphone_flag() {
        let session = Session::parse(
            b"v=0\r\n\
              o=- 0 0 IN IP4 127.0.0.1\r\n\
              s=-\r\n\
              t=0 0\r\n\
              a=x-moonlight-microphone:not-a-bool\r\n",
        )
        .unwrap();

        assert!(WebRTCSessionAnswer::from_sdp(&session).is_err());
    }

    #[test]
    fn apply_writes_attributes() {
        let mut session = base_session();

        WebRTCSessionAnswer {
            app_name: Some("Steam".to_string()),
            microphone: true,
        }
        .apply(&mut session);

        let parsed = WebRTCSessionAnswer::from_sdp(&session).unwrap();

        assert_eq!(parsed.app_name.as_deref(), Some("Steam"));
        assert!(parsed.microphone);
    }

    #[test]
    fn apply_omits_false_microphone() {
        let mut session = base_session();

        WebRTCSessionAnswer {
            app_name: Some("Steam".to_string()),
            microphone: false,
        }
        .apply(&mut session);

        let parsed = WebRTCSessionAnswer::from_sdp(&session).unwrap();

        assert_eq!(parsed.app_name.as_deref(), Some("Steam"));
        assert!(!parsed.microphone);
    }

    #[test]
    fn parses_from_str() {
        let s = "v=0\r\n\
                 o=- 0 0 IN IP4 127.0.0.1\r\n\
                 s=-\r\n\
                 t=0 0\r\n\
                 a=x-moonlight-app-name:Steam\r\n\
                 a=x-moonlight-microphone:1\r\n";

        let answer = WebRTCSessionAnswer::from_str(s).unwrap();

        assert_eq!(answer.app_name.as_deref(), Some("Steam"));
        assert!(answer.microphone);
    }
}
