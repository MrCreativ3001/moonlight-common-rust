//!
//! This module contains the core of the Moonlight Sans-IO Protocol implementation.
//! The entrypoint is the [MoonlightStreamProto] struct.
//!

use std::{
    fmt::Debug,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use thiserror::Error;
use tracing::{Level, debug, info, instrument, warn};

use crate::{
    ServerVersion,
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        EncryptionFlags, HostFeatures, MoonlightStreamConfig, MoonlightStreamSettings,
        RawHostFeatures, StreamingConfig,
        audio::{AudioConfig, OpusMultistreamConfig},
        proto::{
            audio::{AudioStream, AudioStreamConfig},
            control::{
                ControlStream, ControlStreamConfig, packet::ControlPacket,
                peer::ControlEncryptionMethod,
            },
            crypto::CryptoBackend,
            ip_addr::is_private_network_address,
            microphone::foundation::{
                FOUNDATION_DEFAULT_MIC_PORT, FoundationMicStream, FoundationMicStreamConfig,
                rtsp::{RtspSetupFoundationMicRequest, RtspSetupFoundationMicResponse},
            },
            rtsp::{
                client::{RtspClient, RtspClientConfig, RtspClientError, RtspInput, RtspOutput},
                moonlight::{
                    DEFAULT_AUDIO_PORT, ParseMoonlightRtspResponseError, RtspAnnounceRequest,
                    RtspDescribeRequest, RtspDescribeResponse, RtspOptionsRequest, RtspPlayRequest,
                    RtspSetupAudioRequest, RtspSetupAudioResponse, RtspSetupControlRequest,
                    RtspSetupControlResponse, RtspSetupVideoRequest, RtspSetupVideoResponse,
                },
                raw::{RtspAddr, RtspAddrParseError},
            },
            sdp::{
                client::{ClientSdp, MoonlightFeatureFlags, SunshineEncryptionFlags},
                server::ServerSdp,
            },
            video::{VideoStream, VideoStreamConfig, depayloader::VideoDepayloaderConfig},
        },
        video::{
            DEFAULT_VIDEO_PORT, ServerCodecModeSupport, VideoCapabilities, VideoFormat,
            VideoFormats, VideoSetup,
        },
    },
};

// TODO: implement apollo extensions: https://github.com/ClassicOldSong/moonlight-common-c/commit/84af637de7718d1bb390332f0e37a4c6d59e6b78
// Detect apollo based on: if we have a "Permission" field in the xml?
// - https://github.com/LizardByte/Sunshine/blob/c9e0bb864ed263da6dd5c2fff5541c268f94cfaf/src/nvhttp.cpp#L679-L770
// - https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/nvhttp.cpp#L882-L1013
// - OTP pairing?

pub mod audio;
pub mod control;
pub mod crypto;
pub mod microphone;
pub mod ping;
pub mod video;

pub mod runtime;

pub mod rtsp;
pub mod sdp;

pub mod packet;

mod enet;
pub(crate) mod fec;
mod ip_addr;

pub(crate) type DynCryptoBackend = Arc<dyn CryptoBackend + 'static>;

pub use sans_io_time::Instant;

// TODO: move all defaults ports to some better location
pub const DEFAULT_RTSP_PORT: u16 = 48010;

#[derive(Debug, Error)]
pub enum MoonlightStreamProtoError {
    #[error("rtsp: {0}")]
    Rtsp(#[from] RtspClientError),
    #[error("parse rtsp response: {0}")]
    RtspParse(#[from] ParseMoonlightRtspResponseError),
    #[error("sunshine returned the wrong session id: \"{session}\"")]
    WrongSessionId {
        expected_session: String,
        session: String,
    },
}

pub const MOONLIGHT_STREAM_SETUP_TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug)]
pub enum MoonlightStreamInput<'a> {
    Timeout(Instant),
    TcpReceive { now: Instant, data: &'a [u8] },
    TcpDisconnected(Instant),
}

#[derive(Debug)]
pub enum MoonlightStreamSetupOutput {
    Timeout(Instant),
    /// Connect to the address using tcp
    TcpConnect {
        addr: SocketAddr,
    },
    /// Write using tcp the already connected tcp stream
    ///
    /// See also:
    /// - [Self::TcpConnect]
    TcpWrite {
        data: Vec<u8>,
    },
    /// Can only be called once by the implementation
    StartAudioStream {
        config: OpusMultistreamConfig,
        audio_stream: AudioStream,
    },
    /// Can only be called once by the implementation
    StartVideoStream {
        setup: VideoSetup,
        video_stream: VideoStream,
    },
    /// Can only be called once by the implementation
    FoundationStartMic {
        mic_stream: FoundationMicStream,
    },
    /// Can only be called once by the implementation
    StartControlStream {
        control_stream: ControlStream,
    },
    /// The stream is now fully started and the [MoonlightStreamSetup] can be discarded
    Connected {
        features: HostFeatures,
    },
}

#[derive(Debug)]
struct Sdp {
    client_sdp: ClientSdp,
    opus_config: OpusMultistreamConfig,
    video_format: VideoFormat,
}

// TODO: improve the robustness of this impl, calling poll_output multiple times without handle_input should either work or crash

///
/// The entrypoint of the Moonlight Sans-IO Protocol implementation.
///
/// Use the [MoonlightStreamProto::new] function to create a new stream.
///
/// ## Usage
///
// TODO
///
pub struct MoonlightStreamSetup {
    client_config: MoonlightStreamConfig,
    client_settings: MoonlightStreamSettings,
    video_capabilities: VideoCapabilities,
    crypto_backend: DynCryptoBackend,
    rtsp: RtspClient,
    sdp: Option<Sdp>,
    server_version: ServerVersion,
    session_id: Option<String>,
    last_now: Instant,
    state: State,
    host_features: HostFeatures,
}

#[derive(Debug)]
enum State {
    RtspOptionsReceive,
    RtspDescribeReceive,
    SetupAudio,
    RtspSetupAudioReceive {
        response: RtspSetupAudioResponse,
    },
    SetupVideo,
    RtspSetupVideoReceive {
        _response: RtspSetupVideoResponse,
    },
    SetupFoundationMic,
    RtspSetupFoundationMicReceive {
        _response: RtspSetupFoundationMicResponse,
    },
    SetupControl,
    RtspSetupControlReceive {
        _response: RtspSetupControlResponse,
    },
    RtspAnnounceReceive,
    RtspPlayReceive,
    Connected,
}

impl MoonlightStreamSetup {
    pub fn new_unencrypted(
        now: Instant,
        config: MoonlightStreamConfig,
        settings: MoonlightStreamSettings,
        video_capabilities: VideoCapabilities,
    ) -> Result<Self, MoonlightStreamProtoError> {
        Self::new(
            now,
            config,
            settings,
            Arc::new(DisabledCryptoBackend) as _,
            video_capabilities,
        )
    }
}

impl MoonlightStreamSetup {
    pub fn launch_query_parameters() -> &'static str {
        "&corever=1"
    }

    ///
    /// The parameter [MoonlightStreamConfig] contains all the important technical details while the [MoonlightStreamSettings] are settings that the user can modify to enhance their streaming experience.
    ///
    /// To obtain a [MoonlightStreamConfig] you can use a [MoonlightClient](crate::high::MoonlightClient) and call the [MoonlightClient::start_stream](crate::high::MoonlightClient::start_stream) function.
    ///
    #[instrument(level = Level::DEBUG, skip(crypto_backend), err)]
    pub fn new(
        now: Instant,
        config: MoonlightStreamConfig,
        mut settings: MoonlightStreamSettings,
        crypto_backend: DynCryptoBackend,
        video_capabilities: VideoCapabilities,
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

        let ip: IpAddr = config
            .address
            .parse()
            .map_err(RtspAddrParseError::from)
            .map_err(RtspClientError::from)?;

        let rtsp_addr: RtspAddr = match config.rtsp_session_url {
            None => {
                let addr = SocketAddr::new(ip, DEFAULT_RTSP_PORT);

                debug!(rtsp_addr = %addr, "No rtsp address given, generating using given information");

                RtspAddr {
                    encrypted: false,
                    addr,
                }
            }
            Some(ref rtsp_url) => rtsp_url.parse().map_err(RtspClientError::from)?,
        };

        // https://github.com/moonlight-stream/moonlight-common-c/blob/e41355ea01670fd4c830b384009d31dd0339a705/src/Connection.c#L398-L425
        if settings.streaming_remotely == StreamingConfig::Auto {
            if is_private_network_address(rtsp_addr.addr.ip()) {
                settings.streaming_remotely = StreamingConfig::Local;
            } else {
                settings.streaming_remotely = StreamingConfig::Remote;

                if rtsp_addr.addr.ip().is_ipv4() {
                    // Cap packet size at 1024 for remote IPv4 streaming to avoid fragmentation.
                    info!("Packet size capped at 1024 bytes for remote IPv4 streaming");
                    settings.packet_size = 1024;
                } else {
                    // IPv6 guarantees a minimum MTU of 1280 before fragmentation, so use a higher
                    // packet size cap for remote IPv6 streaming (when not using NAT64 which isn't
                    // end-to-end IPv6 traffic).
                    info!("Packet size capped at 1184 bytes for remote IPv6 streaming");
                    settings.packet_size = 1184;
                }
            }
        }

        let mut this = Self {
            client_settings: settings,
            video_capabilities,
            crypto_backend: crypto_backend.clone(),
            last_now: now,
            rtsp: RtspClient::new(
                RtspClientConfig {
                    remote_addr: SocketAddr::new(ip, rtsp_addr.addr.port()),
                    rtsp_target: rtsp_addr,
                    client_version,
                    aes_key: Some(config.encryption.aes_key),
                },
                crypto_backend,
            ),
            server_version: config.version,
            client_config: config,
            sdp: None,
            session_id: None,
            state: State::RtspOptionsReceive,
            host_features: HostFeatures::default(),
        };

        // For Wolf: Allow no response for an RtspOptions
        this.rtsp.send_no_response(
            RtspOptionsRequest {
                target: this.rtsp.target_addr(),
            }
            .into_request(this.server_version),
        )?;

        Ok(this)
    }

    pub fn poll_output(&mut self) -> Result<MoonlightStreamSetupOutput, MoonlightStreamProtoError> {
        let mut timeout;
        loop {
            match self.rtsp.poll_output()? {
                RtspOutput::Connect { addr } => {
                    return Ok(MoonlightStreamSetupOutput::TcpConnect { addr });
                }
                RtspOutput::Write { data } => {
                    return Ok(MoonlightStreamSetupOutput::TcpWrite { data });
                }
                RtspOutput::Response {
                    response: Some(response),
                } => {
                    match &mut self.state {
                        // RtspOptionsReceive, see below i no response
                        State::RtspDescribeReceive => {
                            let describe = RtspDescribeResponse::try_from_response(&response)?;

                            debug!(sdp = ?describe.sdp, "received server sdp");

                            // The server won't send more information about itself so we can already create our client sdp
                            let (client_sdp, opus_config, video_format) =
                                self.generate_client_sdp(&describe.sdp)?;
                            let server_sdp = describe.sdp;

                            // Enable host features based on sdp
                            debug_assert_eq!(
                                self.host_features,
                                HostFeatures::default(),
                                "set host features after the server sdp was initialized!"
                            );
                            self.host_features = server_sdp
                                .sunshine_feature_flags
                                .clone()
                                .unwrap_or(RawHostFeatures::empty())
                                .into_host_features(self.server_version);

                            let sdp = Sdp {
                                client_sdp,
                                opus_config,
                                video_format,
                            };
                            debug!(sdp = ?sdp, "generated client sdp");
                            self.sdp = Some(sdp);

                            self.rtsp.send(
                                RtspSetupAudioRequest {
                                    target: self.rtsp.target_addr(),
                                    session_id: None,
                                }
                                .into_request(self.server_version),
                            )?;
                            self.state = State::SetupAudio;
                        }
                        State::SetupAudio => {
                            let audio_setup = RtspSetupAudioResponse::try_from_response(&response)?;
                            // IMPORTANT: setup audio now: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/AudioStream.c#L87-L110
                            let ip = self.rtsp.target_addr().addr.ip();

                            // This won't panic because the sdp was created before this
                            #[allow(clippy::unwrap_used)]
                            let sdp = self.sdp.as_ref().unwrap();

                            // Get configurations
                            let opus_config = sdp.opus_config.clone();

                            let encrypted = sdp
                                .client_sdp
                                .sunshine_encryption
                                .unwrap_or(SunshineEncryptionFlags::empty())
                                .contains(SunshineEncryptionFlags::AUDIO);

                            let addr =
                                SocketAddr::new(ip, audio_setup.port.unwrap_or(DEFAULT_AUDIO_PORT));

                            let audio_stream = AudioStream::new(
                                self.last_now,
                                AudioStreamConfig {
                                    addr,
                                    opus_config: opus_config.clone(),
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
                                    fec: self.server_version >= ServerVersion::new(7, 1, 415, 0),
                                    sunshine_ping: audio_setup.sunshine_ping.clone(),
                                    sunshine_encryption: encrypted
                                        .then_some(self.client_config.encryption),
                                },
                                self.crypto_backend.clone(),
                            );

                            self.state = State::RtspSetupAudioReceive {
                                response: audio_setup,
                            };

                            info!("starting audio stream");

                            return Ok(MoonlightStreamSetupOutput::StartAudioStream {
                                config: opus_config,
                                audio_stream,
                            });
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
                                    addr,
                                    queue: VideoDepayloaderConfig {
                                        server_version: self.server_version,
                                        // Packet size will always exist
                                        #[allow(clippy::unwrap_used)]
                                        packet_size: sdp.client_sdp.packet_size.unwrap() as usize,
                                        format: sdp.video_format,
                                    },
                                    fps: self.client_settings.fps,
                                    sunshine_ping: video_setup.sunshine_ping.clone(),
                                    sunshine_encryption: None, // TODO <--
                                },
                                self.crypto_backend.clone(),
                            );

                            self.state = State::RtspSetupVideoReceive {
                                _response: video_setup,
                            };

                            info!("starting video stream");

                            return Ok(MoonlightStreamSetupOutput::StartVideoStream {
                                setup: VideoSetup {
                                    format: sdp.video_format,
                                    width: sdp
                                        .client_sdp
                                        .client_viewport_width
                                        .expect("width in client sdp"),
                                    height: sdp
                                        .client_sdp
                                        .client_viewport_height
                                        .expect("height in client sdp"),
                                    redraw_rate: sdp.client_sdp.max_fps.expect("fps in client sdp"),
                                },
                                video_stream,
                            });
                        }
                        State::SetupFoundationMic => {
                            let mic_setup =
                                RtspSetupFoundationMicResponse::try_from_response(&response)?;

                            // Session id exists at this point
                            #[allow(clippy::unwrap_used)]
                            let session_id = self.session_id.as_ref().unwrap();

                            if &mic_setup.session_id != session_id {
                                return Err(MoonlightStreamProtoError::WrongSessionId {
                                    expected_session: session_id.to_string(),
                                    session: mic_setup.session_id.to_string(),
                                });
                            }

                            let ip = self.rtsp.target_addr().addr.ip();

                            // This is allowed because sdp is initialized in states before
                            #[allow(clippy::unwrap_used)]
                            let sdp = self.sdp.as_mut().unwrap();

                            let mic_port = mic_setup.port.unwrap_or(FOUNDATION_DEFAULT_MIC_PORT);
                            let addr = SocketAddr::new(ip, mic_port);

                            let encrypted = sdp
                                .client_sdp
                                .sunshine_encryption
                                .unwrap_or(SunshineEncryptionFlags::empty())
                                .contains(SunshineEncryptionFlags::FOUNDATION_MIC);

                            let mic_stream = FoundationMicStream::new(
                                self.last_now,
                                FoundationMicStreamConfig {
                                    addr,
                                    encryption: encrypted.then_some(self.client_config.encryption),
                                },
                                self.crypto_backend.clone(),
                            );

                            self.state = State::RtspSetupFoundationMicReceive {
                                _response: mic_setup,
                            };

                            info!("starting microphone stream");

                            // Enable microphone extension in features
                            self.host_features.microphone = true;
                            self.host_features.extensions.foundation_microphone = true;

                            return Ok(MoonlightStreamSetupOutput::FoundationStartMic {
                                mic_stream,
                            });
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

                            // Sdp is initialized by now
                            #[allow(clippy::unwrap_used)]
                            let sdp = self.sdp.as_ref().unwrap();

                            let should_enable_encryption =
                                self.server_version >= ServerVersion::new(7, 1, 431, 0);
                            let encryption = if should_enable_encryption
                                && sdp
                                    .client_sdp
                                    .sunshine_encryption
                                    .unwrap_or(SunshineEncryptionFlags::empty())
                                    .contains(SunshineEncryptionFlags::CONTROL_V2)
                            {
                                Some((
                                    ControlEncryptionMethod::Sunshine,
                                    self.client_config.encryption.aes_key,
                                ))
                            } else if should_enable_encryption {
                                Some((
                                    ControlEncryptionMethod::Nvidia,
                                    self.client_config.encryption.aes_key,
                                ))
                            } else {
                                None
                            };

                            // We control all values and those values don't fail -> this cannot panic
                            let mut control_stream = ControlStream::new(
                                self.last_now,
                                ControlStreamConfig {
                                    server_version: self.server_version,
                                    addr,
                                    sunshine_connect_data: control_setup.sunshine_connect_data,
                                    encryption,
                                    apollo_permissions: self
                                        .client_config
                                        .apollo_permissions
                                        .clone(),
                                },
                                self.crypto_backend.clone(),
                            )
                            .expect("failed to create control stream");

                            // Buffer RequestIdr and StartB for connect because they should be the first packets
                            // This won't panic, because we have control over all values and they don't panic
                            #[allow(clippy::unwrap_used)]
                            control_stream
                                .send_inner(ControlPacket::RequestIdr, true)
                                .expect("failed to send / buffer RequestIdr");
                            #[allow(clippy::unwrap_used)]
                            control_stream
                                .send_inner(ControlPacket::StartB, true)
                                .expect("failed to send / buffer StartB");

                            self.state = State::RtspSetupControlReceive {
                                _response: control_setup,
                            };

                            info!("starting control stream");

                            return Ok(MoonlightStreamSetupOutput::StartControlStream {
                                control_stream,
                            });
                        }
                        State::RtspAnnounceReceive => {
                            // Session id exists at this point
                            #[allow(clippy::unwrap_used)]
                            let session_id = self.session_id.as_ref().unwrap();

                            // For GFE 3.22 compatibility, we must start the audio ping thread before the RTSP handshake.
                            // It will not reply to our RTSP PLAY request until the audio ping has been received.
                            self.rtsp.send_no_response(
                                RtspPlayRequest {
                                    session_id: session_id.to_owned(),
                                }
                                .into_request(self.server_version),
                            )?;

                            info!("sending final rtsp play command");

                            // We can never receive a response from the play
                            self.state = State::RtspPlayReceive;

                            continue;
                        }
                        _ => {}
                    }

                    continue;
                }
                RtspOutput::Response { .. } => match &mut self.state {
                    State::RtspOptionsReceive => {
                        self.rtsp.send(
                            RtspDescribeRequest {
                                target: self.rtsp.target_addr(),
                            }
                            .into_request(self.server_version),
                        )?;
                        self.state = State::RtspDescribeReceive;
                        continue;
                    }
                    State::RtspPlayReceive => {
                        // move to next state
                        self.state = State::Connected;

                        continue;
                    }
                    // this cannot happen because it's only reachable, when using [RtspClient::send_no_response]
                    _ => unreachable!("received empty rtsp response in state {:?}", self.state),
                },
                RtspOutput::Timeout => {
                    // TODO: manage timeout and disconnect
                    timeout = self.last_now + Duration::from_secs(1);
                }
            }

            // This doesn't require any rtsp actions
            match &mut self.state {
                State::RtspSetupAudioReceive { response } => {
                    self.rtsp.send(
                        RtspSetupVideoRequest {
                            target: self.rtsp.target_addr(),
                            session_id: Some(response.session_id.clone()),
                        }
                        .into_request(self.server_version),
                    )?;

                    self.session_id = Some(response.session_id.clone());

                    self.state = State::SetupVideo;
                    continue;
                }
                State::RtspSetupVideoReceive { _response: _ } => {
                    // Session id exists at this point
                    #[allow(clippy::unwrap_used)]
                    let session_id = self.session_id.as_ref().unwrap();

                    if self.client_config.foundation_enable_mic {
                        self.rtsp.send(
                            RtspSetupFoundationMicRequest {
                                target: self.rtsp.target_addr(),
                                session_id: Some(session_id.clone()),
                            }
                            .into_request(self.server_version),
                        )?;

                        self.state = State::SetupFoundationMic;
                    } else {
                        self.rtsp.send(
                            RtspSetupControlRequest {
                                session_id: Some(session_id.clone()),
                            }
                            .into_request(self.server_version),
                        )?;

                        self.state = State::SetupControl;
                    }
                    continue;
                }
                State::RtspSetupFoundationMicReceive { _response } => {
                    // Session id exists at this point
                    #[allow(clippy::unwrap_used)]
                    let session_id = self.session_id.as_ref().unwrap();

                    self.rtsp.send(
                        RtspSetupControlRequest {
                            session_id: Some(session_id.clone()),
                        }
                        .into_request(self.server_version),
                    )?;

                    self.state = State::SetupControl;
                    continue;
                }
                State::RtspSetupControlReceive { _response: _ } => {
                    // Session id exists at this point
                    #[allow(clippy::unwrap_used)]
                    let session_id = self.session_id.as_ref().unwrap();

                    // This won't panic because this state can only be reached when there's a client sdp set in RtspDescribeReceive
                    #[allow(clippy::unwrap_used)]
                    let sdp = self.sdp.as_ref().unwrap();

                    self.rtsp.send(
                        RtspAnnounceRequest {
                            session_id: session_id.clone(),
                            sdp: sdp.client_sdp.clone(),
                        }
                        .into_request(self.server_version),
                    )?;

                    self.state = State::RtspAnnounceReceive;
                    continue;
                }
                State::Connected => {
                    // Configure other features of the host
                    self.host_features.extensions.apollo_permissions =
                        self.client_config.apollo_permissions.clone();

                    return Ok(MoonlightStreamSetupOutput::Connected {
                        features: self.host_features.clone(),
                    });
                }
                _ => {}
            }

            // This happens when we have a timeout
            break;
        }

        Ok(MoonlightStreamSetupOutput::Timeout(timeout))
    }

    #[instrument(level = Level::TRACE, skip(self))]
    pub fn handle_input(
        &mut self,
        input: MoonlightStreamInput,
    ) -> Result<(), MoonlightStreamProtoError> {
        let _last_now = self.last_now;
        // TODO: all sans io structs MUST be updated via timeout even if it isn't their event

        match input {
            MoonlightStreamInput::Timeout(now) => {
                self.last_now = now;
            }
            MoonlightStreamInput::TcpReceive { now, data } => {
                self.last_now = now;

                self.rtsp.handle_input(RtspInput::Receive(data))?;
            }
            MoonlightStreamInput::TcpDisconnected(now) => {
                self.last_now = now;

                self.rtsp.handle_input(RtspInput::Disconnected)?;
            }
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
            .sunshine_encryption_supported
            .unwrap_or(SunshineEncryptionFlags::empty());

        let mut sunshine_encryption = SunshineEncryptionFlags::empty();

        if self.server_version.is_sunshine_like() {
            // New-style control stream encryption is low overhead, so we enable it any time it is supported
            if server_encryption_supported.contains(SunshineEncryptionFlags::CONTROL_V2) {
                sunshine_encryption |= SunshineEncryptionFlags::CONTROL_V2;
            }

            let client_wants_video = self
                .client_settings
                .encryption_flags
                .contains(EncryptionFlags::VIDEO);

            // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/SdpGenerator.c#L280-L289
            // If video encryption is supported by the host and desired by the client, use it
            if server_encryption_supported.contains(SunshineEncryptionFlags::VIDEO)
                && client_wants_video
            {
                sunshine_encryption |= SunshineEncryptionFlags::VIDEO;
            }
            // If video encryption is explicitly requested by the host but *not* by the client,
            // we'll encrypt anyway (since we are capable of doing so) and print a warning.
            if server_encryption_requested.contains(SunshineEncryptionFlags::VIDEO)
                && !client_wants_video
            {
                sunshine_encryption |= SunshineEncryptionFlags::VIDEO;
                warn!(
                    "Server requested video encryption; enabling it even though the client disabled it"
                );
            }

            let client_wants_audio = self
                .client_settings
                .encryption_flags
                .contains(EncryptionFlags::AUDIO);

            // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/SdpGenerator.c#L291-L300
            // If audio encryption is supported by the host and desired by the client, use it
            if server_encryption_supported.contains(SunshineEncryptionFlags::AUDIO)
                && client_wants_audio
            {
                sunshine_encryption |= SunshineEncryptionFlags::AUDIO;
            }
            // If audio encryption is explicitly requested by the host but *not* by the client,
            // we'll encrypt anyway (since we are capable of doing so) and print a warning.
            if server_encryption_requested.contains(SunshineEncryptionFlags::AUDIO)
                && !client_wants_audio
            {
                sunshine_encryption |= SunshineEncryptionFlags::AUDIO;
                warn!(
                    "Server requested audio encryption; enabling it even though the client disabled it"
                );
            }

            if self.server_version.is_foundation() {
                // See
                // https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/SdpGenerator.c#L302-L309
                // If microphone encryption is supported by the host and audio encryption is enable, enable it
                // Microphone encryption follows audio encryption - if audio is encrypted, mic should be too
                if server_encryption_supported.contains(SunshineEncryptionFlags::FOUNDATION_MIC)
                    && sunshine_encryption.contains(SunshineEncryptionFlags::AUDIO)
                {
                    sunshine_encryption |= SunshineEncryptionFlags::FOUNDATION_MIC;
                }

                // Enable mic encryption if the host explicitly requests it
                if server_encryption_requested.contains(SunshineEncryptionFlags::FOUNDATION_MIC) {
                    sunshine_encryption |= SunshineEncryptionFlags::FOUNDATION_MIC;
                }
            }
        }

        // -- Select Audio
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtspConnection.c#L733-L834
        let audio_packet_duration = Duration::from_millis(5);

        let mut high_quality_audio = false;
        let opus_config = if self.client_settings.audio_config == AudioConfig::STEREO {
            OpusMultistreamConfig {
                sample_rate: 48000,
                samples_per_frame: 48 * audio_packet_duration.as_millis() as u32,
                channel_count: 2,
                streams: 1,
                coupled_streams: 1,
                mapping: [0, 1, 0, 0, 0, 0, 0, 0],
            }
        } else {
            // TODO: is this correct?

            // See
            // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtspConnection.c#L750-L830

            // Figure out if the preferred audio config is supported, else just use stereo
            let mut selected_config = OpusMultistreamConfig {
                sample_rate: 48000,
                samples_per_frame: 48 * audio_packet_duration.as_millis() as u32,
                channel_count: 2,
                streams: 1,
                coupled_streams: 1,
                mapping: [0, 1, 0, 0, 0, 0, 0, 0],
            };

            for opus_config in &server_sdp.audio_surround_params {
                if opus_config.channel_count == self.client_settings.audio_config.channel_count {
                    high_quality_audio = true;
                    selected_config = opus_config.clone();
                    break;
                }
            }

            selected_config
        };

        // -- Select Video Format
        // Av1 is not supported in this implementation currently
        // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/RtspConnection.c#L1090-L1136
        let negotiated_video_format;

        if self
            .client_config
            .server_codec_mode_support
            .intersects(ServerCodecModeSupport::MASK_HEVC)
            && self
                .client_settings
                .supported_video_formats
                .intersects(VideoFormats::MASK_H265)
        {
            // H265 Rext 10
            if self
                .client_config
                .server_codec_mode_support
                .contains(ServerCodecModeSupport::HEVC_REXT10_444)
                && self
                    .client_settings
                    .supported_video_formats
                    .contains(VideoFormats::H265_REXT10_444)
            {
                negotiated_video_format = VideoFormat::H265Rext10_444;
            } else
            // H265 Main 10
            if self
                .client_config
                .server_codec_mode_support
                .contains(ServerCodecModeSupport::HEVC_MAIN10)
                && self
                    .client_settings
                    .supported_video_formats
                    .contains(VideoFormats::H265_MAIN10)
            {
                negotiated_video_format = VideoFormat::H265Main10;
            } else
            // H265 Rext 8
            if self
                .client_config
                .server_codec_mode_support
                .contains(ServerCodecModeSupport::HEVC_REXT8_444)
                && self
                    .client_settings
                    .supported_video_formats
                    .contains(VideoFormats::H265_REXT8_444)
            {
                negotiated_video_format = VideoFormat::H265Rext8_444;
            } else {
                negotiated_video_format = VideoFormat::H265;
            }
        } else {
            // H264 High 8
            if self
                .client_config
                .server_codec_mode_support
                .contains(ServerCodecModeSupport::H264_HIGH8_444)
                && self
                    .client_settings
                    .supported_video_formats
                    .contains(VideoFormats::H264_HIGH8_444)
            {
                negotiated_video_format = VideoFormat::H264High8_444;
            } else {
                // Default H264
                negotiated_video_format = VideoFormat::H264;
            }
        }

        // Repair percent
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/SdpGenerator.c#L224-L230
        let mut fec_repair_percent = 20;
        if self.client_settings.width >= 3840 && self.client_settings.height >= 2160 {
            // When streaming 4K, lower FEC levels to reduce stream overhead
            fec_repair_percent = 5;
        }

        // This seems configurable but i don't know what it does exactly, but 1 works
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/SdpGenerator.c#L424-L429
        let slices_per_frame = self.video_capabilities.slices_per_frame.unwrap_or(1);

        // Reference Frame Invalidation, See
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/SdpGenerator.c#L462-L474
        let reference_frame_invalidation_supported = match negotiated_video_format {
            VideoFormat::H264 | VideoFormat::H264High8_444 => {
                self.video_capabilities.reference_frame_invalidation_h264
            }
            VideoFormat::H265
            | VideoFormat::H265Rext10_444
            | VideoFormat::H265Main10
            | VideoFormat::H265Rext8_444 => {
                self.video_capabilities.reference_frame_invalidation_h264
            }
            VideoFormat::Av1Main8
            | VideoFormat::Av1Main10
            | VideoFormat::Av1High8_444
            | VideoFormat::Av1High10_444 => {
                self.video_capabilities.reference_frame_invalidation_h265
            }
        };
        let max_num_reference_frames = if reference_frame_invalidation_supported {
            // If the decoder supports reference frame invalidation, that indicates it also supports
            // the maximum number of reference frames allowed by the codec. Even if we can't use RFI
            // due to lack of host support, we can still allow the host to pick a number of reference
            // frames greater than 1 to improve encoding efficiency.
            0
        } else {
            1
        };

        // TODO: only generate the sdp in the announce stage so we know the video port
        let client_sdp = ClientSdp::new(
            StreamingConfig::Local,
            self.server_version,
            self.rtsp.target_addr().addr.ip(),
            moonlight_features,
            sunshine_encryption,
            // Enable encryption
            13,
            negotiated_video_format,
            self.client_settings.width,
            self.client_settings.height,
            self.client_settings.fps,
            self.client_settings.fps_x100,
            self.client_settings.packet_size,
            self.client_settings.bitrate,
            "0.0.0.0".to_string(),
            self.rtsp.target_addr().addr.port(),
            fec_repair_percent,
            self.client_settings.audio_config,
            high_quality_audio,
            slices_per_frame,
            max_num_reference_frames,
            self.client_settings.color_space,
            self.client_settings.color_range,
            0,
        );

        Ok((client_sdp, opus_config, negotiated_video_format))
    }
}

// Other notes:
// Dimensions over 4096 are only supported with HEVC on NVENC: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1118C13-L1121C14

fn soonest(a: impl Into<Option<Instant>>, b: impl Into<Option<Instant>>) -> Option<Instant> {
    let a = a.into();
    let b = b.into();

    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
