use std::fmt::{self, Display};

use tracing::{Level, debug, instrument};

use crate::webrtc::WebRTCParseError;

#[derive(Debug, Clone, PartialEq)]
pub enum WebRTCLinkHeader {
    IceServer {
        url: String,
        username: Option<String>,
        credential: Option<String>,
    },
}

impl Display for WebRTCLinkHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IceServer {
                url,
                username,
                credential,
            } => {
                write!(f, r#"<{url}>; rel="ice-server""#)?;

                if let Some(username) = username {
                    write!(f, "; username=")?;
                    quote(f, username)?;
                }
                if let Some(credential) = credential {
                    write!(f, "; credential=")?;
                    quote(f, credential)?;
                }
            }
        }
        Ok(())
    }
}

fn quote(f: &mut fmt::Formatter, mut text: &str) -> fmt::Result {
    write!(f, "\"")?;
    while let Some(c) = take(&mut text) {
        match c {
            '\\' | '"' => {
                write!(f, "\\")?;
                write!(f, "{c}")?;
            }
            _ => {
                write!(f, "{c}")?;
            }
        }
    }
    write!(f, "\"")?;
    Ok(())
}

impl WebRTCLinkHeader {
    #[instrument(level = Level::DEBUG)]
    pub fn parse(mut s: &str) -> Vec<Self> {
        let raw_headers = parse_link_header(&mut s).unwrap_or_default();
        debug!(raw_headers = ?raw_headers, "raw headers");

        let mut headers = Vec::new();

        for header in raw_headers {
            if header
                .parameters
                .iter()
                .find(|(name, value)| name == "rel" && value == "ice-server")
                .is_some()
            {
                let username = header
                    .parameters
                    .iter()
                    .find(|(name, _)| name == "username")
                    .map(|(_, value)| value)
                    .cloned();
                let credential = header
                    .parameters
                    .iter()
                    .find(|(name, _)| name == "credential")
                    .map(|(_, value)| value)
                    .cloned();

                headers.push(Self::IceServer {
                    url: header.target,
                    username,
                    credential,
                });
            }
        }

        headers
    }
}

#[derive(Debug)]
struct LinkHeader {
    target: String,
    parameters: Vec<(String, String)>,
}

fn parse_link_header(s: &mut &str) -> Result<Vec<LinkHeader>, WebRTCParseError> {
    take_whitespaces(s);

    let mut links = Vec::new();

    while next(s).is_some() {
        take_whitespaces(s);

        if let Some('<') = next(s) {
            take(s);

            let target_string = take_until(s, '>');

            let Some('>') = next(s) else {
                return Ok(links);
            };

            take(s);

            let link_parameters = parse_parameters(s);

            links.push(LinkHeader {
                target: target_string.to_string(),
                parameters: link_parameters,
            });

            take_whitespaces(s);
            if let Some(',') = next(s) {
                take(s);
                take_whitespaces(s);
            }
        } else {
            return Ok(links);
        }
    }

    Ok(links)
}

fn parse_parameters(s: &mut &str) -> Vec<(String, String)> {
    let mut parameters = Vec::new();

    while next(s).is_some() {
        take_whitespaces(s);

        let Some(';') = next(s) else {
            return parameters;
        };
        take(s);

        take_whitespaces(s);

        let parameter_name = take_while(s, |x| !x.is_whitespace() && !matches!(x, '=' | ';' | ','));

        take_whitespaces(s);

        let parameter_value = match next(s) {
            Some('=') => {
                take(s);

                take_whitespaces(s);

                if let Some('"') = next(s) {
                    parse_quoted_string(s)
                } else {
                    take_while(s, |x| matches!(x, ';' | ',')).to_string()
                }
            }
            _ => String::new(),
        };

        let parameter_name = parameter_name.to_lowercase();

        parameters.push((parameter_name, parameter_value));

        take_whitespaces(s);

        match next(s) {
            None | Some(',') => return parameters,
            _ => {}
        }
    }

    parameters
}

fn parse_quoted_string(s: &mut &str) -> String {
    let mut output = String::new();

    if !matches!(next(s), Some('"')) {
        return String::new();
    }

    take(s);

    while let Some(c) = take(s) {
        match c {
            '\\' => {
                if next(s).is_none() {
                    return output;
                } else {
                    output.push(take(s).expect("value"));
                }
            }
            '"' => {
                return output;
            }
            c => {
                output.push(c);
            }
        }
    }

    output
}

fn take_whitespaces(s: &mut &str) {
    *s = s.trim_start();
}
fn next(s: &mut &str) -> Option<char> {
    s.chars().next()
}
fn take(s: &mut &str) -> Option<char> {
    let output = next(s)?;
    *s = &s[output.len_utf8()..];
    Some(output)
}
fn take_while<'a, F>(s: &mut &'a str, mut condition: F) -> &'a str
where
    F: FnMut(char) -> bool,
{
    if let Some(index) = s.find(|c| !condition(c)) {
        let output = &s[..index];
        *s = &s[index..];
        output
    } else {
        let output = *s;
        *s = "";
        output
    }
}
fn take_until<'a>(s: &mut &'a str, until: char) -> &'a str {
    take_while(s, |x| x != until)
}

#[cfg(test)]
mod test {
    use super::*;

    fn test_eq(expected_text: &str, expected_headers: &[WebRTCLinkHeader]) {
        let headers = WebRTCLinkHeader::parse(expected_text);
        assert_eq!(headers.as_slice(), expected_headers);

        let text = expected_headers
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(text, expected_text);
    }

    #[test]
    pub fn stun_server() {
        test_eq(
            r#"<stun:stun.l.google.com:19302>; rel="ice-server""#,
            &[WebRTCLinkHeader::IceServer {
                url: "stun:stun.l.google.com:19302".to_string(),
                username: None,
                credential: None,
            }],
        )
    }

    #[test]
    pub fn turn_server() {
        test_eq(
            r#"<turn:turn.l.google.com:19302>; rel="ice-server"; username="abc"; credential="def""#,
            &[WebRTCLinkHeader::IceServer {
                url: "turn:turn.l.google.com:19302".to_string(),
                username: Some("abc".to_string()),
                credential: Some("def".to_string()),
            }],
        );
    }

    #[test]
    pub fn multiple_servers() {
        test_eq(
            r#"<stun:stun.l.google.com:19302>; rel="ice-server", <turn:turn.l.google.com:19302>; rel="ice-server"; username="abc"; credential="def""#,
            &[
                WebRTCLinkHeader::IceServer {
                    url: "stun:stun.l.google.com:19302".to_string(),
                    username: None,
                    credential: None,
                },
                WebRTCLinkHeader::IceServer {
                    url: "turn:turn.l.google.com:19302".to_string(),
                    username: Some("abc".to_string()),
                    credential: Some("def".to_string()),
                },
            ],
        );
    }
}
