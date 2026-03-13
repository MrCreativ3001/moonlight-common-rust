//!
//! This module contains the core of the Moonlight Sans-IO Protocol implementation.
//! The entrypoint is the [MoonlightStreamProto] struct.
//!

use std::{
    fmt::Debug,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{Level, debug, instrument};

use crate::{
    ServerVersion,
    stream::{
        EncryptionFlags, MoonlightStreamConfig, MoonlightStreamSettings, StreamingConfig,
        audio::{AudioConfig, OpusMultistreamConfig},
        proto::{
            audio::{AudioStream, AudioStreamConfig, AudioStreamError},
            control::{
                ControlMessage, ControlMessageInner, ControlStream, ControlStreamConfig,
                ControlStreamError, packet::ControlPacket,
            },
            rtsp::{
                Rtsp, RtspError, RtspInput, RtspOutput,
                moonlight::{
                    DEFAULT_AUDIO_PORT, ParseMoonlightRtspResponseError, RtspDescribeResponse,
                    RtspOptionsResponse, RtspPlayResponse, RtspSetupAudioResponse,
                    RtspSetupControlResponse, RtspSetupVideoResponse, send_rtsp_announce,
                    send_rtsp_control_setup, send_rtsp_describe, send_rtsp_options, send_rtsp_play,
                    send_rtsp_setup_audio, send_rtsp_video_setup,
                },
                raw::{RtspAddr, RtspAddrParseError},
            },
            sdp::{
                client::{ClientSdp, MoonlightFeatureFlags, SunshineEncryptionFlags},
                server::ServerSdp,
            },
            video::{
                VideoStream, VideoStreamConfig, VideoStreamError,
                depayloader::VideoDepayloaderConfig,
            },
        },
        video::{DEFAULT_VIDEO_PORT, VideoFormat},
    },
};

// TODO: replace simplelog with tracingfor this common crate!

// TODO: avoid heap alloc by using 'a in certain structs (e.g. RtspRequest / Response, Sdp)

// TODO: rename video/audio queue to depay?

// TODO: implement apollo extensions: https://github.com/ClassicOldSong/moonlight-common-c/commit/84af637de7718d1bb390332f0e37a4c6d59e6b78
// Detect apollo based on: if we have a "Permission" field in the xml?
// - https://github.com/LizardByte/Sunshine/blob/c9e0bb864ed263da6dd5c2fff5541c268f94cfaf/src/nvhttp.cpp#L679-L770
// - https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/nvhttp.cpp#L882-L1013
// - OTP pairing?

// TODO: replace Instant with a custom Timestamp struct for Wasm compat

pub mod audio;
pub mod control;
pub mod crypto;
pub mod video;

mod rtsp;
mod sdp;

mod enet;
mod packet;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test;

// TODO: move all defaults ports to some better location
pub const DEFAULT_RTSP_PORT: u16 = 48010;

#[derive(Debug, Error)]
pub enum MoonlightStreamProtoError {
    #[error("rtsp: {0}")]
    Rtsp(#[from] RtspError),
    #[error("parse rtsp response: {0}")]
    RtspParse(#[from] ParseMoonlightRtspResponseError),
    #[error("sunshine returned the wrong session id: \"{session}\"")]
    WrongSessionId {
        expected_session: String,
        session: String,
    },
    #[error("audio stream: {0}")]
    AudioStream(#[from] AudioStreamError),
    #[error("video stream: {0}")]
    VideoStream(#[from] VideoStreamError),
    #[error("control stream: {0}")]
    ControlStream(#[from] ControlStreamError),
}

#[derive(Debug)]
pub enum MoonlightStreamInput<'a> {
    Timeout(Instant),
    TcpConnect(Instant),
    TcpReceive {
        now: Instant,
        data: &'a [u8],
    },
    TcpDisconnect(Instant),
    UdpReceive {
        now: Instant,
        source: SocketAddr,
        data: &'a [u8],
    },
}

#[derive(Debug)]
pub enum MoonlightStreamOutput {
    Timeout(Instant),
    Action(MoonlightStreamAction),
    Event(MoonlightStreamEvent),
}

#[derive(Debug)]
pub enum MoonlightStreamAction {
    ConnectTcp {
        addr: SocketAddr,
    },
    SendTcp {
        data: Vec<u8>,
    },
    SendUdp {
        to: SocketAddr,
        data: Vec<u8>,
    },
    /// Can only be called once by the implementation
    StartAudioStream {
        addr: SocketAddr,
        audio_stream: AudioStream,
    },
    /// Can only be called once by the implementation
    StartVideoStream {
        addr: SocketAddr,
        video_stream: VideoStream,
    },
    /// Can only be called once by the implementation
    StartControlStream {
        addr: SocketAddr,
        control_stream: ControlStream,
    },
    /// Send a control message to the [ControlStream] returned by [MoonlightStreamAction::StartControlStream]
    SendControlMessage {
        message: ControlMessage,
    },
}

#[derive(Debug, PartialEq)]
pub enum MoonlightStreamEvent {
    // TODO
}

#[derive(Debug)]
struct Sdp {
    server_sdp: ServerSdp,
    client_sdp: ClientSdp,
    opus_config: OpusMultistreamConfig,
    video_format: VideoFormat,
}

///
/// The entrypoint of the Moonlight Sans-IO Protocol implementation.
///
/// Use the [MoonlightStreamProto::new] function to create a new stream.
///
/// ## Usage
/// ```
/// // TODO
/// ```
///
pub struct MoonlightStreamProto {
    client_settings: MoonlightStreamSettings,
    rtsp: Rtsp,
    sdp: Option<Sdp>,
    server_version: ServerVersion,
    session_id: Option<String>,
    last_now: Instant,
    state: State,
}

#[derive(Debug)]
enum State {
    RtspOptionsReceive,
    RtspDescribeReceive,
    SetupAudio,
    RtspSetupAudioReceive { response: RtspSetupAudioResponse },
    SetupVideo,
    RtspSetupVideoReceive { response: RtspSetupVideoResponse },
    SetupControl,
    RtspSetupControlReceive { response: RtspSetupControlResponse },
    RtspAnnounceReceive,
    RtspPlayReceive,
    ControlRequestIdr,
    ControlStartB,
    ControlAllowPackets,
    Connected,
}

impl MoonlightStreamProto {
    ///
    /// The parameter [MoonlightStreamConfig] contains all the important technical details while the [MoonlightStreamSettings] are settings that the user can modify to enhance their streaming experience.
    ///
    /// To obtain a [MoonlightStreamConfig] you can use a [MoonlightClient](crate::high::MoonlightClient) and call the [MoonlightClient::start_stream](crate::high::MoonlightClient::start_stream) function.
    ///
    #[instrument(level = Level::DEBUG, err)]
    pub fn new(
        now: Instant,
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
    ) -> Result<Self, MoonlightStreamProtoError> {
        // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L976-L994
        #[allow(clippy::wildcard_in_or_patterns)]
        let client_version = match config.version.major {
            3 => 10,
            4 => 11,
            5 => 12,
            6 => 13,
            7 | _ => 14,
        };

        let rtsp_addr: RtspAddr = match config.rtsp_session_url {
            None => {
                let ip: IpAddr = config
                    .address
                    .parse()
                    .map_err(RtspAddrParseError::from)
                    .map_err(RtspError::from)?;
                let addr = SocketAddr::new(ip, DEFAULT_RTSP_PORT);

                debug!(rtsp_addr = %addr, "No rtsp address given, generating using given information");

                RtspAddr {
                    encrypted: false,
                    addr,
                }
            }
            Some(rtsp_url) => rtsp_url.parse().map_err(RtspError::from)?,
        };

        let mut this = Self {
            client_settings: settings,
            last_now: now,
            // TODO: how to get this?
            rtsp: Rtsp::new(rtsp_addr, client_version)?,
            server_version: config.version,
            sdp: None,
            session_id: None,
            state: State::RtspOptionsReceive,
        };

        send_rtsp_options(&mut this.rtsp);

        Ok(this)
    }

    pub fn poll_output(&mut self) -> Result<MoonlightStreamOutput, MoonlightStreamProtoError> {
        let mut timeout;
        loop {
            match self.rtsp.poll_output()? {
                RtspOutput::Connect { addr } => {
                    return Ok(MoonlightStreamOutput::Action(
                        MoonlightStreamAction::ConnectTcp { addr },
                    ));
                }
                RtspOutput::Write { data } => {
                    return Ok(MoonlightStreamOutput::Action(
                        MoonlightStreamAction::SendTcp { data },
                    ));
                }
                RtspOutput::Response(response) => {
                    match &mut self.state {
                        State::RtspOptionsReceive => {
                            let _options = RtspOptionsResponse::try_from_response(&response)?;

                            send_rtsp_describe(&mut self.rtsp);
                            self.state = State::RtspDescribeReceive;
                        }
                        State::RtspDescribeReceive => {
                            let describe = RtspDescribeResponse::try_from_response(&response)?;

                            debug!(sdp = ?describe.sdp, "Received Server Sdp");

                            // The server won't send more information about itself so we can already create our client sdp
                            let (client_sdp, opus_config, video_format) =
                                self.generate_client_sdp(&describe.sdp)?;
                            let server_sdp = describe.sdp;

                            let sdp = Sdp {
                                server_sdp,
                                client_sdp,
                                opus_config,
                                video_format,
                            };
                            debug!(sdp = ?sdp, "Generated Client Sdp");
                            self.sdp = Some(sdp);

                            send_rtsp_setup_audio(&mut self.rtsp, None);
                            self.state = State::SetupAudio;
                        }
                        State::SetupAudio => {
                            let audio_setup = RtspSetupAudioResponse::try_from_response(&response)?;
                            // IMPORTANT: setup audio now: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/AudioStream.c#L87-L110
                            let ip = self.rtsp.target_addr().addr.ip();

                            // This won't panic because the sdp was created before this
                            #[allow(clippy::unwrap_used)]
                            let opus_config = self.sdp.as_ref().unwrap().opus_config.clone();

                            let addr =
                                SocketAddr::new(ip, audio_setup.port.unwrap_or(DEFAULT_AUDIO_PORT));

                            let audio_stream = AudioStream::new(
                                self.last_now,
                                AudioStreamConfig {
                                    addr,
                                    opus_config,
                                    // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpAudioQueue.c#L28-L44
                                    // Older versions of GFE violate some invariants that our FEC code requires, so we turn it off for
                                    // anything older than GFE 3.19 just to be safe. GFE seems to have changed to the "modern" behavior
                                    // between GFE 3.18 and 3.19.
                                    //
                                    // In the case of GFE 3.13, it does send FEC packets but it requires very special handling because:
                                    // a) data and FEC shards may vary in size
                                    // b) FEC blocks can start on boundaries that are not multiples of RTPA_DATA_SHARDS
                                    //
                                    // It doesn't seem worth it to sink a bunch of hours into figure out how to properly handle audio FEC
                                    // for a 3 year old version of GFE that almost nobody uses. Instead, we'll just disable the FEC queue
                                    // entirely and pass all audio data straight to the decoder.
                                    // TODO: want fec disabled
                                    fec: self.server_version >= ServerVersion::new(7, 1, 415, 0),
                                    sunshine_ping: audio_setup.sunshine_ping.clone(),
                                    // TODO: encryption?
                                    sunshine_encryption: None,
                                },
                            );

                            self.state = State::RtspSetupAudioReceive {
                                response: audio_setup,
                            };

                            return Ok(MoonlightStreamOutput::Action(
                                MoonlightStreamAction::StartAudioStream { addr, audio_stream },
                            ));
                        }
                        // RtspSetupAudioReceive down
                        State::SetupVideo => {
                            let video_setup = RtspSetupVideoResponse::try_from_response(&response)?;

                            // Session id exists at this point
                            #[allow(clippy::unwrap_used)]
                            let session_id = self.session_id.as_ref().unwrap();

                            if &video_setup.session_id != session_id {
                                return Err(MoonlightStreamProtoError::WrongSessionId {
                                    expected_session: session_id.to_string(),
                                    session: video_setup.session_id.to_string(),
                                });
                            }

                            let ip = self.rtsp.target_addr().addr.ip();

                            // This is allowed because sdp is initialized in states before
                            #[allow(clippy::unwrap_used)]
                            let sdp = self.sdp.as_mut().unwrap();

                            let video_port = video_setup.port.unwrap_or(DEFAULT_VIDEO_PORT);
                            let addr = SocketAddr::new(ip, video_port);

                            // TODO: this is using another port? https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L562
                            // Update sdp video port
                            sdp.client_sdp.video_port = Some(video_port);

                            let video_stream = VideoStream::new(
                                self.last_now,
                                VideoStreamConfig {
                                    server_version: self.server_version,
                                    addr,
                                    queue: VideoDepayloaderConfig {
                                        // Packet size will always exist
                                        #[allow(clippy::unwrap_used)]
                                        packet_size: sdp.client_sdp.packet_size.unwrap() as usize,
                                    },
                                    sunshine_ping: video_setup.sunshine_ping.clone(),
                                    sunshine_encryption: None, // TODO <--
                                },
                            );

                            self.state = State::RtspSetupVideoReceive {
                                response: video_setup,
                            };

                            return Ok(MoonlightStreamOutput::Action(
                                MoonlightStreamAction::StartVideoStream { addr, video_stream },
                            ));
                        }
                        State::SetupControl => {
                            let control_setup =
                                RtspSetupControlResponse::try_from_response(&response)?;

                            // Session id exists at this point
                            #[allow(clippy::unwrap_used)]
                            let session_id = self.session_id.as_ref().unwrap();

                            if &control_setup.session_id != session_id {
                                return Err(MoonlightStreamProtoError::WrongSessionId {
                                    expected_session: session_id.to_string(),
                                    session: control_setup.session_id.to_string(),
                                });
                            }

                            let ip = self.rtsp.target_addr().addr.ip();
                            let addr = SocketAddr::new(
                                ip,
                                control_setup.port.unwrap_or(DEFAULT_VIDEO_PORT),
                            );

                            let control_stream = ControlStream::new(
                                self.last_now,
                                ControlStreamConfig {
                                    server_version: self.server_version,
                                    addr,
                                    sunshine_connect_data: control_setup.sunshine_connect_data,
                                    sunshine_encryption: None, // TODO <--
                                },
                            );

                            self.state = State::RtspSetupControlReceive {
                                response: control_setup,
                            };

                            return Ok(MoonlightStreamOutput::Action(
                                MoonlightStreamAction::StartControlStream {
                                    addr,
                                    control_stream,
                                },
                            ));
                        }
                        State::RtspAnnounceReceive => {
                            // Session id exists at this point
                            #[allow(clippy::unwrap_used)]
                            let session_id = self.session_id.as_ref().unwrap();

                            send_rtsp_play(&mut self.rtsp, session_id.clone());

                            // We can never receive a response from the play
                            self.state = State::RtspPlayReceive;

                            continue;
                        }
                        State::RtspPlayReceive => {
                            let _response = RtspPlayResponse::try_from_response(&response)?;

                            self.state = State::ControlRequestIdr;
                        }
                        State::Connected => {
                            // TODO: this should at least print some warning?
                        }
                        _ => {}
                    }

                    continue;
                }
                RtspOutput::Timeout => {
                    // TODO: manage timeout and disconnect
                    timeout = self.last_now + Duration::from_secs(1);
                }
            }

            // This doesn't require any rtsp actions
            match &mut self.state {
                State::RtspSetupAudioReceive { response } => {
                    send_rtsp_video_setup(
                        &mut self.rtsp,
                        self.server_version,
                        Some(response.session_id.clone()),
                    );

                    self.session_id = Some(response.session_id.clone());

                    self.state = State::SetupVideo;
                    continue;
                }
                State::RtspSetupVideoReceive { response } => {
                    // Session id exists at this point
                    #[allow(clippy::unwrap_used)]
                    let session_id = self.session_id.as_ref().unwrap();

                    send_rtsp_control_setup(&mut self.rtsp, Some(session_id.clone()));

                    self.state = State::SetupControl;
                    continue;
                }
                State::RtspSetupControlReceive { response } => {
                    // Session id exists at this point
                    #[allow(clippy::unwrap_used)]
                    let session_id = self.session_id.as_ref().unwrap();

                    // This won't panic because this state can only be reached when there's a client sdp set in RtspDescribeReceive
                    #[allow(clippy::unwrap_used)]
                    let sdp = self.sdp.as_ref().unwrap();

                    send_rtsp_announce(&mut self.rtsp, session_id.clone(), sdp.client_sdp.clone());

                    self.state = State::RtspAnnounceReceive;
                    continue;
                }
                State::ControlRequestIdr => {
                    self.state = State::ControlStartB;

                    return Ok(MoonlightStreamOutput::Action(
                        MoonlightStreamAction::SendControlMessage {
                            message: ControlMessage(ControlMessageInner::SendPacket {
                                packet: ControlPacket::RequestIdr,
                            }),
                        },
                    ));
                }
                State::ControlStartB => {
                    self.state = State::ControlAllowPackets;

                    return Ok(MoonlightStreamOutput::Action(
                        MoonlightStreamAction::SendControlMessage {
                            message: ControlMessage(ControlMessageInner::SendPacket {
                                packet: ControlPacket::StartB,
                            }),
                        },
                    ));
                }
                State::ControlAllowPackets => {
                    self.state = State::Connected;

                    return Ok(MoonlightStreamOutput::Action(
                        MoonlightStreamAction::SendControlMessage {
                            message: ControlMessage(ControlMessageInner::AllowOtherPackets),
                        },
                    ));
                }
                _ => {}
            }

            // This happens when we have a timeout
            break;
        }

        Ok(MoonlightStreamOutput::Timeout(timeout))
    }

    pub fn handle_input(
        &mut self,
        input: MoonlightStreamInput,
    ) -> Result<(), MoonlightStreamProtoError> {
        let last_now = self.last_now;
        // TODO: all sans io structs MUST be updated via timeout even if it isn't their event

        match input {
            MoonlightStreamInput::Timeout(now) => {
                self.last_now = now;
            }
            MoonlightStreamInput::TcpConnect(now) => {
                self.last_now = now;

                self.rtsp.handle_input(RtspInput::Connect)?;
            }
            MoonlightStreamInput::TcpReceive { now, data } => {
                self.last_now = now;

                self.rtsp.handle_input(RtspInput::Receive(data))?;
            }
            MoonlightStreamInput::TcpDisconnect(now) => {
                self.last_now = now;

                self.rtsp.handle_input(RtspInput::Disconnect)?;
            }
            _ => todo!(),
        }

        Ok(())
    }

    fn generate_client_sdp(
        &self,
        server_sdp: &ServerSdp,
    ) -> Result<(ClientSdp, OpusMultistreamConfig, VideoFormat), MoonlightStreamProtoError> {
        // TODO: implement other changes from that fn: https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/SdpGenerator.c#L255-L543

        // -- Moonlight Features
        let mut moonlight_features = MoonlightFeatureFlags::empty();

        if self.server_version.is_sunshine_like() {
            moonlight_features |=
                MoonlightFeatureFlags::FEC_STATUS | MoonlightFeatureFlags::SESSION_ID_V1;
        }

        // -- Encryption
        let server_encryption_requested = server_sdp
            .sunshine_encryption_requested
            .unwrap_or(SunshineEncryptionFlags::empty());
        let server_encryption_supported = server_sdp
            .sunshine_encryption_requested
            .unwrap_or(SunshineEncryptionFlags::empty());

        let mut sunshine_encryption = SunshineEncryptionFlags::empty();

        if self.server_version.is_sunshine_like() {
            // New-style control stream encryption is low overhead, so we enable it any time it is supported
            if server_encryption_supported.contains(SunshineEncryptionFlags::CONTROL_V2) {
                // TODO: enable this when this is actually implemented inside the MoonlightControlStream
                // sunshine_encryption |= SunshineEncryptionFlags::CONTROL_V2;
            }

            // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/SdpGenerator.c#L280-L289
            // If video encryption is supported by the host and desired by the client, use it
            if server_encryption_supported.contains(SunshineEncryptionFlags::VIDEO)
                && !self
                    .client_settings
                    .encryption_flags
                    .contains(EncryptionFlags::VIDEO)
            {
                sunshine_encryption |= SunshineEncryptionFlags::VIDEO;
            }
            // If video encryption is explicitly requested by the host but *not* by the client,
            // we'll encrypt anyway (since we are capable of doing so) and print a warning.
            if server_encryption_requested.contains(SunshineEncryptionFlags::VIDEO)
                && !self
                    .client_settings
                    .encryption_flags
                    .contains(EncryptionFlags::VIDEO)
            {
                sunshine_encryption |= SunshineEncryptionFlags::VIDEO;
                // TODO: print warning
            }

            // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/SdpGenerator.c#L291-L300
            // If audio encryption is supported by the host and desired by the client, use it
            if server_encryption_supported.contains(SunshineEncryptionFlags::AUDIO)
                && !self
                    .client_settings
                    .encryption_flags
                    .contains(EncryptionFlags::AUDIO)
            {
                sunshine_encryption |= SunshineEncryptionFlags::AUDIO;
            }
            // If audio encryption is explicitly requested by the host but *not* by the client,
            // we'll encrypt anyway (since we are capable of doing so) and print a warning.
            if server_encryption_requested.contains(SunshineEncryptionFlags::AUDIO)
                && !self
                    .client_settings
                    .encryption_flags
                    .contains(EncryptionFlags::AUDIO)
            {
                sunshine_encryption |= SunshineEncryptionFlags::AUDIO;
                // TODO: print warning
            }
        }

        // -- Select Audio
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtspConnection.c#L733-L834
        let audio_packet_duration = Duration::from_millis(5);

        let mut opus_config = if self.client_settings.audio_config == AudioConfig::STEREO {
            OpusMultistreamConfig {
                sample_rate: 48000,
                samples_per_frame: 48 * audio_packet_duration.as_millis() as u32,
                channel_count: 2,
                streams: 1,
                coupled_streams: 1,
                mapping: [0, 1, 0, 0, 0, 0, 0, 0],
            }
        } else {
            // TODO: figure this out: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtspConnection.c#L750-L830
            todo!()
        };

        // -- Select Video Format
        // TODO: find out negotiated formats
        let negotiated_video_format = VideoFormat::H264;

        // TODO: only generate the sdp in the announce stage so we know the video port
        let client_sdp = ClientSdp::new(
            StreamingConfig::Local,
            self.server_version,
            // TODO: what is target_ip actually
            self.rtsp.target_addr().addr.ip(),
            moonlight_features,
            sunshine_encryption,
            negotiated_video_format,
            self.client_settings.width,
            self.client_settings.height,
            self.client_settings.fps,
            self.client_settings.fps_x100,
            self.client_settings.packet_size,
            self.client_settings.bitrate,
            // TODO: is it actually 0.0.0.0?
            "0.0.0.0".to_string(),
            self.rtsp.target_addr().addr.port(),
            50, // TODO: <--
            self.client_settings.audio_config,
            false, // <-- TODO
            1,     // TODO: <--
            1,     // TODO: <--
            self.client_settings.color_space,
            self.client_settings.color_range,
            0,
        );

        Ok((client_sdp, opus_config, negotiated_video_format))
    }
}

// Other notes:
// Dimensions over 4096 are only supported with HEVC on NVENC: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1118C13-L1121C14
