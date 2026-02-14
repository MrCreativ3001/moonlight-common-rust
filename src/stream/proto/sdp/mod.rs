use std::{
    fmt::{self, Display, Formatter},
    net::{AddrParseError, IpAddr, Ipv4Addr, Ipv6Addr},
    num::ParseIntError,
    str::FromStr,
};

use thiserror::Error;

pub mod client;
pub mod server;

#[derive(Debug, PartialEq)]
pub struct SdpAttribute {
    pub key: String,
    pub value: Option<String>,
}

pub fn sdp_attr(key: impl ToString, value: impl ToString) -> SdpAttribute {
    SdpAttribute {
        key: key.to_string(),
        value: Some(value.to_string()),
    }
}

#[derive(Debug, PartialEq)]
pub enum SdpNetworkType {
    In,
}

impl Display for SdpNetworkType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let str = match self {
            Self::In => "IN",
        };

        write!(f, "{}", str)
    }
}

impl FromStr for SdpNetworkType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "IN" => Ok(Self::In),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdpMediaType {
    Video,
}

impl Display for SdpMediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Video => "video",
        };
        write!(f, "{s}")
    }
}

impl FromStr for SdpMediaType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "video" => Ok(Self::Video),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SdpMedia {
    pub media_type: SdpMediaType,
    pub port: u16,
}

/// `o=<username> <session_id> <session_version> <network_type> <ip.type> <ip.address>`
#[derive(Debug, PartialEq)]
pub struct SdpOrigin {
    pub username: String,
    pub session_id: usize,
    pub session_version: usize,
    pub network_type: SdpNetworkType,
    pub ip: IpAddr,
}

impl Display for SdpOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} ",
            self.username, self.session_id, self.session_version, self.network_type,
        )?;

        match self.ip {
            IpAddr::V4(v4) => write!(f, "IPv4 {}", v4)?,
            IpAddr::V6(v6) => write!(f, "IPv6 {}", v6)?,
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ParseSdpOriginError {
    #[error("invalid line: \"{line}\"")]
    InvalidLine { line: String },
    #[error("failed to parse int: {0}")]
    ParseInt(#[from] ParseIntError),
    #[error("failed to parse network type: \"{network_type}\"")]
    ParseNetworkType { network_type: String },
    #[error("failed to parse ip address type: {ip_type}")]
    ParseIpAddrType { ip_type: String },
    #[error("failed to parse ip address: {0}")]
    ParseIpAddr(#[from] AddrParseError),
}

impl FromStr for SdpOrigin {
    type Err = ParseSdpOriginError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split(" ");
        let Some(username) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };

        let Some(session_id_str) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };
        let session_id = session_id_str.parse()?;

        let Some(session_version_str) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };
        let session_version = session_version_str.parse()?;

        let Some(network_type_str) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };
        let network_type = match network_type_str.parse() {
            Ok(value) => value,
            Err(_) => {
                return Err(ParseSdpOriginError::ParseNetworkType {
                    network_type: network_type_str.to_string(),
                });
            }
        };

        let Some(ip_type_str) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };
        let Some(ip_str) = split.next() else {
            return Err(ParseSdpOriginError::InvalidLine {
                line: s.to_string(),
            });
        };

        let ip = match ip_type_str {
            "IPv4" => ip_str.parse::<Ipv4Addr>()?.into(),
            "IPv6" => ip_str.parse::<Ipv6Addr>()?.into(),
            _ => {
                return Err(ParseSdpOriginError::ParseIpAddrType {
                    ip_type: ip_type_str.to_string(),
                });
            }
        };

        Ok(Self {
            username: username.to_string(),
            session_id,
            session_version,
            network_type,
            ip,
        })
    }
}

/// A SessionDescription.
///
/// References:
/// - [SessionDescription](https://docs.rs/sdp/0.10.0/sdp/description/session/struct.SessionDescription.html)
#[derive(Debug, Default, PartialEq)]
pub struct Sdp {
    pub version: Option<usize>,
    pub origin: Option<SdpOrigin>,
    pub session: Option<String>,
    pub attributes: Vec<SdpAttribute>,
    pub media: Vec<SdpMedia>,
    pub time: Option<(u32, u32)>,
}

#[derive(Debug, Error)]
pub enum ParseSdpError {
    #[error("invalid fmtp line: {0}")]
    InvalidFmtpLine(String),
    #[error("failed to parse int: {0}")]
    ParseInt(#[from] ParseIntError),
    #[error("failed to parse origin: {0}")]
    ParseOrigin(#[from] ParseSdpOriginError),
    #[error("failed to parse media line: \"{line}\"")]
    ParseMediaLine { line: String },
    #[error("failed to parse media line: \"{line}\"")]
    ParseTimeLine { line: String },
}

impl FromStr for Sdp {
    type Err = ParseSdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lines = s.lines();

        let mut sdp = Sdp::default();

        for line in lines {
            if let Some(version) = line.strip_prefix("v=") {
                let version = version.parse()?;

                sdp.version = Some(version);
            } else if let Some(origin) = line.strip_prefix("o=") {
                let origin = SdpOrigin::from_str(origin)?;

                sdp.origin = Some(origin);
            } else if let Some(session) = line.strip_prefix("s=") {
                sdp.session = Some(session.trim().to_string());
            } else if let Some(attribute) = line.strip_prefix("a=") {
                let (key, value) = match attribute.trim().split_once(":") {
                    Some((key, value)) => (key, Some(value)),
                    None => (attribute, None),
                };

                sdp.attributes.push(SdpAttribute {
                    key: key.to_string(),
                    value: value.map(str::to_string),
                });
            } else if let Some(media) = line.strip_prefix("m=") {
                let mut split = media.split(" ");
                let Some(type_str) = split.next() else {
                    return Err(ParseSdpError::ParseMediaLine {
                        line: line.to_string(),
                    });
                };
                let Ok(media_type) = SdpMediaType::from_str(type_str) else {
                    return Err(ParseSdpError::ParseMediaLine {
                        line: line.to_string(),
                    });
                };

                let Some(port_str) = split.next() else {
                    return Err(ParseSdpError::ParseMediaLine {
                        line: line.to_string(),
                    });
                };
                let port = port_str.parse()?;

                sdp.media.push(SdpMedia { media_type, port });
            } else if let Some(time) = line.strip_prefix("t=") {
                let mut split = time.split(" ");

                let Some(t0_str) = split.next() else {
                    return Err(ParseSdpError::ParseTimeLine {
                        line: line.to_string(),
                    });
                };
                let t0 = t0_str.parse()?;

                let Some(t1_str) = split.next() else {
                    return Err(ParseSdpError::ParseTimeLine {
                        line: line.to_string(),
                    });
                };
                let t1 = t1_str.parse()?;

                sdp.time = Some((t0, t1));
            } else {
                // TODO: maybe debug or warn?

                // ignore
            }
        }

        Ok(sdp)
    }
}

impl Display for Sdp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(version) = self.version {
            writeln!(f, "v={version}")?;
        }
        if let Some(origin) = &self.origin {
            writeln!(f, "o={origin}")?;
        }
        if let Some(session) = &self.session {
            writeln!(f, "s={session}")?;
        }

        for attribute in &self.attributes {
            writeln!(
                f,
                "a={}:{}",
                attribute.key,
                attribute.value.as_deref().unwrap_or("")
            )?;
        }

        if let Some((a, b)) = self.time {
            writeln!(f, "t={a} {b}")?;
        }

        for media in &self.media {
            writeln!(f, "m={} {}", media.media_type, media.port)?;
        }

        Ok(())
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use crate::stream::proto::sdp::{
        Sdp, SdpAttribute, SdpMedia, SdpMediaType, SdpNetworkType, SdpOrigin, sdp_attr,
    };

    #[test]
    fn test_sdp() {
        let assert_sdp_eq = |sdp: Sdp, text: &str| {
            let test_sdp = format!("{}", sdp);
            assert_eq!(test_sdp.as_str(), text, "Display fail: \n{test_sdp}");

            let test_sdp = text.parse::<Sdp>().unwrap();
            assert_eq!(test_sdp, sdp, "Parse fail: \n{test_sdp:#?}");
        };

        // test server sdp
        assert_sdp_eq(
            Sdp {
                version: None,
                origin: None,
                session: None,
                attributes: vec![
                    SdpAttribute {
                        key: "x-ss-general.featureFlags".into(),
                        value: Some("3".into()),
                    },
                    SdpAttribute {
                        key: "x-ss-general.encryptionSupported".into(),
                        value: Some("5".into()),
                    },
                    SdpAttribute {
                        key: "x-ss-general.encryptionRequested".into(),
                        value: Some("1".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=21101".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=21101".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=642012453".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=660012345".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=85301245367".into()),
                    },
                    SdpAttribute {
                        key: "fmtp".into(),
                        value: Some("97 surround-params=88001234567".into()),
                    },
                ],
                media: vec![],
                time: None,
            },
            r#"a=x-ss-general.featureFlags:3
a=x-ss-general.encryptionSupported:5
a=x-ss-general.encryptionRequested:1
a=fmtp:97 surround-params=21101
a=fmtp:97 surround-params=21101
a=fmtp:97 surround-params=642012453
a=fmtp:97 surround-params=660012345
a=fmtp:97 surround-params=85301245367
a=fmtp:97 surround-params=88001234567
"#,
        );

        // test client sdp
        assert_sdp_eq(
            Sdp {
                version: Some(0),
                origin: Some(SdpOrigin {
                    username: "android".to_string(),
                    session_id: 0,
                    session_version: 14,
                    network_type: SdpNetworkType::In,
                    ip: Ipv4Addr::new(192, 168, 178, 140).into(),
                }),
                session: Some("NVIDIA Streaming Client".to_string()),
                attributes: vec![
                    sdp_attr("x-ml-general.featureFlags", "3"),
                    sdp_attr("x-ss-general.encryptionEnabled", "5"),
                    sdp_attr("x-ss-video[0].chromaSamplingType", "1"),
                    sdp_attr("x-nv-video[0].clientViewportWd", "1920"),
                    sdp_attr("x-nv-video[0].clientViewportHt", "1080"),
                    sdp_attr("x-nv-video[0].maxFPS", "60"),
                    sdp_attr("x-nv-video[0].packetSize", "1024"),
                    sdp_attr("x-nv-video[0].rateControlMode", "4"),
                    sdp_attr("x-nv-video[0].timeoutLengthMs", "7000"),
                    sdp_attr("x-nv-video[0].framesWithInvalidRefThreshold", "0"),
                    sdp_attr("x-nv-video[0].initialBitrateKbps", "3200"),
                    sdp_attr("x-nv-video[0].initialPeakBitrateKbps", "3200"),
                    sdp_attr("x-nv-vqos[0].bw.minimumBitrateKbps", "3200"),
                    sdp_attr("x-nv-vqos[0].bw.maximumBitrateKbps", "3200"),
                    sdp_attr("x-ml-video.configuredBitrateKbps", "4000"),
                    sdp_attr("x-nv-vqos[0].fec.enable", "1"),
                    sdp_attr("x-nv-vqos[0].videoQualityScoreUpdateTime", "5000"),
                    sdp_attr("x-nv-vqos[0].qosTrafficType", "5"),
                    sdp_attr("x-nv-aqos.qosTrafficType", "4"),
                    sdp_attr("x-nv-general.featureFlags", "167"),
                    sdp_attr("x-ny-general.useReliableUdp", "13"),
                    sdp_attr("x-nv-vqos[0].fec.minRequiredFecPackets", "2"),
                    sdp_attr("x-nv-vqos[0].bllFec.enable", "0"),
                    sdp_attr("x-nv-vqos[0].drc.enable", "0"),
                    sdp_attr("x-nv-general.enableRecoveryMode", "0"),
                    sdp_attr("x-nv-video[0].videoEncoderSlicesPerFrame", "1"),
                    sdp_attr("x-nv-clientSupportHevc", "0"),
                    sdp_attr("x-nv-vqos[0].bitStreamFormat", "0"),
                    sdp_attr("x-nv-video[0].dynamic RangeMode", "0"),
                    sdp_attr("x-nv-video[0].maxNumReferenceFrames", "1"),
                    sdp_attr("x-nv-video[0].clientRefreshRateX100", "6000"),
                    sdp_attr("x-nv-audio.surround.numChannels", "2"),
                    sdp_attr("x-nv-audio.surround.channelMask", "3"),
                    sdp_attr("x-nv-audio.surround.enable", "0"),
                    sdp_attr("x-nv-audio.surround.AudioQuality", "0"),
                    sdp_attr("x-nv-aqos.packetDuration", "5"),
                    sdp_attr("x-nv-video[0].encoderCscMode", "5"),
                ],
                media: vec![SdpMedia {
                    media_type: SdpMediaType::Video,
                    port: 47998,
                }],
                time: Some((0, 0)),
            },
            r#"v=0
o=android 0 14 IN IPv4 192.168.178.140
s=NVIDIA Streaming Client
a=x-ml-general.featureFlags:3
a=x-ss-general.encryptionEnabled:5
a=x-ss-video[0].chromaSamplingType:1
a=x-nv-video[0].clientViewportWd:1920
a=x-nv-video[0].clientViewportHt:1080
a=x-nv-video[0].maxFPS:60
a=x-nv-video[0].packetSize:1024
a=x-nv-video[0].rateControlMode:4
a=x-nv-video[0].timeoutLengthMs:7000
a=x-nv-video[0].framesWithInvalidRefThreshold:0
a=x-nv-video[0].initialBitrateKbps:3200
a=x-nv-video[0].initialPeakBitrateKbps:3200
a=x-nv-vqos[0].bw.minimumBitrateKbps:3200
a=x-nv-vqos[0].bw.maximumBitrateKbps:3200
a=x-ml-video.configuredBitrateKbps:4000
a=x-nv-vqos[0].fec.enable:1
a=x-nv-vqos[0].videoQualityScoreUpdateTime:5000
a=x-nv-vqos[0].qosTrafficType:5
a=x-nv-aqos.qosTrafficType:4
a=x-nv-general.featureFlags:167
a=x-ny-general.useReliableUdp:13
a=x-nv-vqos[0].fec.minRequiredFecPackets:2
a=x-nv-vqos[0].bllFec.enable:0
a=x-nv-vqos[0].drc.enable:0
a=x-nv-general.enableRecoveryMode:0
a=x-nv-video[0].videoEncoderSlicesPerFrame:1
a=x-nv-clientSupportHevc:0
a=x-nv-vqos[0].bitStreamFormat:0
a=x-nv-video[0].dynamic RangeMode:0
a=x-nv-video[0].maxNumReferenceFrames:1
a=x-nv-video[0].clientRefreshRateX100:6000
a=x-nv-audio.surround.numChannels:2
a=x-nv-audio.surround.channelMask:3
a=x-nv-audio.surround.enable:0
a=x-nv-audio.surround.AudioQuality:0
a=x-nv-aqos.packetDuration:5
a=x-nv-video[0].encoderCscMode:5
t=0 0
m=video 47998
"#,
        );
    }
}
