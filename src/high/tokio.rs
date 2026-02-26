use std::{
    error::Error,
    io,
    net::{Ipv4Addr, SocketAddrV4},
};

use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock},
    task::JoinError,
};
use uuid::Uuid;

use crate::{
    Error, MoonlightError, ServerState, ServerVersion,
    http::{
        ClientIdentifier, ClientInfo, ClientSecret, DEFAULT_UNIQUE_ID, ServerIdentifier,
        app_list::{App, AppListEndpoint, AppListRequest, AppListResponse},
        box_art::{AppBoxArtEndpoint, AppBoxArtRequest},
        cancel::{CancelEndpoint, CancelRequest},
        client::async_client::RequestClient,
        pair::{
            PairEndpoint, PairPin, PairResponse, PairingCryptoBackend,
            client::{ClientPairing, ClientPairingError, ClientPairingOutput},
        },
        server_info::{
            ApolloPermissions, ServerInfoEndpoint, ServerInfoRequest, ServerInfoResponse,
        },
        unpair::{UnpairEndpoint, UnpairRequest},
    },
    mac::MacAddress,
    stream::video::ServerCodecModeSupport,
};

pub async fn broadcast_magic_packet(mac: MacAddress) -> Result<(), io::Error> {
    let mut magic_packet = [0u8; 6 * 17];

    magic_packet[0..6].copy_from_slice(&[255, 255, 255, 255, 255, 255]);
    for i in 1..17 {
        magic_packet[(i * 6)..((i + 1) * 6)].copy_from_slice(&mac.to_bytes());
    }

    let broadcast = SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 255), 9);

    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    socket.set_broadcast(true)?;
    socket.send_to(&magic_packet, &broadcast).await?;

    Ok(())
}

#[derive(Debug, Error)]
pub enum MoonlightClientError {
    #[error("{0}")]
    Moonlight(#[from] MoonlightError),
    #[error("{0}")]
    BlockingJoin(#[from] JoinError),
    #[error("this action requires pairing")]
    NotPaired,
    #[error("{0}")]
    StreamConfig(#[from] StreamConfigError),
    #[error("the host is likely offline")]
    LikelyOffline,
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("request: {0}")]
    Backend(Box<dyn Error + Send + Sync>),
    #[error("pairing: {0}")]
    Pairing(ClientPairingError<Box<dyn Error + Send + Sync>>),
}

#[derive(Debug, Error)]
pub enum StreamConfigError {
    #[error("hdr not supported")]
    NotSupportedHdr,
    #[error("4k not supported")]
    NotSupported4k,
    #[error("4k not supported: Your device must support HEVC or AV1 to stream at 4k")]
    NotSupported4kCodecMissing,
    #[error("4k not supported: Update GeForce Experience")]
    NotSupported4kUpdateGfe,
}

pub struct MoonlightHost<Client> {
    client_unique_id: String,
    client: Mutex<Client>,
    address: String,
    http_port: u16,
    cache: RwLock<Cache>,
}

#[derive(Debug, Default)]
struct Cache {
    authenticated: Option<Authenticated>,
    server_info: Option<ServerInfoResponse>,
    app_list: Option<AppListResponse>,
}

#[derive(Debug)]
struct Authenticated {
    client_identifier: ClientIdentifier,
    client_secret: ClientSecret,
    server_identifier: ServerIdentifier,
}

fn req_err<Err>(err: Err) -> MoonlightClientError
where
    Err: Error + Send + Sync + 'static,
{
    MoonlightClientError::Backend(Box::new(err))
}
fn crypto_err<Err>(err: ClientPairingError<Err>) -> MoonlightClientError
where
    Err: Error + Send + Sync + 'static,
{
    MoonlightClientError::Pairing(ClientPairingError::from_err(err))
}

/// TODO: some docs
impl<Client> MoonlightHost<Client>
where
    Client: RequestClient,
    <Client as RequestClient>::Error: Error + Send + Sync + 'static,
{
    pub fn new(
        address: String,
        http_port: u16,
        unique_id: Option<String>,
    ) -> Result<Self, MoonlightClientError> {
        Ok(Self {
            client: Mutex::new(Client::with_defaults().map_err(req_err)?),
            client_unique_id: unique_id.unwrap_or_else(|| DEFAULT_UNIQUE_ID.to_string()),
            address,
            http_port,
            cache: Default::default(),
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }
    pub fn http_port(&self) -> u16 {
        self.http_port
    }

    pub fn http_address(&self) -> String {
        format!("{}:{}", self.address, self.http_port)
    }

    pub async fn update(self: &MoonlightHost<Client>) -> Result<(), MoonlightClientError> {
        let client = self.client.lock().await;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let mut cache = Cache::default();

        let http_address = self.http_address();
        let server_info = client
            .send_http::<ServerInfoEndpoint>(client_info, &http_address, &ServerInfoRequest {})
            .await
            .map_err(req_err)?;

        let https_port = server_info.https_port;
        cache.server_info = Some(server_info);

        if self.is_paired() {
            let https_address = Self::build_https_address(&self.address, https_port);

            let server_info_secure = client
                .send_https::<ServerInfoEndpoint>(
                    client_info,
                    &https_address,
                    &ServerInfoRequest {},
                )
                .await
                .map_err(req_err)?;

            cache.server_info = Some(server_info_secure);

            let app_list = client
                .send_https::<AppListEndpoint>(client_info, &http_address, &AppListRequest {})
                .await
                .map_err(req_err)?;

            cache.app_list = Some(app_list);
        }

        {
            let mut cache_lock = self.cache.write().await;
            *cache_lock = cache;
        }

        drop(client);

        Ok(())
    }

    async fn server_info<R>(
        &self,
        f: impl FnOnce(&ServerInfoResponse) -> R,
    ) -> Result<R, MoonlightClientError> {
        let response = self.cache.read().await;

        if let Some(server_info) = &response.server_info {
            Ok(f(server_info))
        } else {
            drop(response);

            self.update().await?;
            let response = self.cache.read().await;
            let Some(server_info) = &response.server_info else {
                unreachable!()
            };

            Ok(f(server_info))
        }
    }

    pub async fn https_port(&self) -> Result<u16, MoonlightClientError> {
        self.server_info(|info| info.https_port).await
    }

    fn build_https_address(address: &str, https_port: u16) -> String {
        format!("{address}:{https_port}")
    }
    pub async fn https_address(&self) -> Result<String, MoonlightClientError> {
        let https_port = self.https_port().await?;
        Ok(Self::build_https_address(&self.address, https_port))
    }
    pub async fn external_port(&self) -> Result<u16, MoonlightClientError> {
        self.server_info(|info| info.external_port).await
    }

    pub async fn host_name(&self) -> Result<String, MoonlightClientError> {
        self.server_info(|info| info.host_name.clone()).await
    }
    pub async fn version(&self) -> Result<ServerVersion, MoonlightClientError> {
        self.server_info(|info| info.app_version).await
    }

    pub async fn gfe_version(&self) -> Result<String, MoonlightClientError> {
        self.server_info(|info| info.gfe_version.clone()).await
    }
    pub async fn unique_id(&self) -> Result<Uuid, MoonlightClientError> {
        self.server_info(|info| info.unique_id).await
    }

    /// Returns None if unpaired
    pub async fn mac(&self) -> Result<Option<MacAddress>, MoonlightClientError> {
        self.server_info(|info| info.mac).await
    }
    pub async fn local_ip(&self) -> Result<Ipv4Addr, MoonlightClientError> {
        self.server_info(|info| info.local_ip).await
    }

    pub async fn current_game(&self) -> Result<u32, MoonlightClientError> {
        self.server_info(|info| info.current_game).await
    }

    pub async fn state(&self) -> Result<ServerState, MoonlightClientError> {
        self.server_info(|info| info.state).await
    }

    pub async fn max_luma_pixels_hevc(&self) -> Result<u32, MoonlightClientError> {
        self.server_info(|info| info.max_luma_pixels_hevc).await
    }

    pub async fn server_codec_mode_support(
        &self,
    ) -> Result<ServerCodecModeSupport, MoonlightClientError> {
        self.server_info(|info| info.server_codec_mode_support)
            .await
    }

    pub async fn set_identity(
        &self,
        client_identifier: &ClientIdentifier,
        client_secret: &ClientSecret,
        server_identifier: &ServerIdentifier,
    ) -> Result<(), MoonlightClientError> {
        let client = Client::with_certificates(
            &client_secret.to_pem(),
            &client_identifier.to_pem(),
            &server_identifier.to_pem(),
        )
        .map_err(req_err)?;

        {
            let mut client_lock = self.client.lock().await;
            *client_lock = client;
        }

        self.update().await?;

        Ok(())
    }

    pub async fn clear_identity(&self) -> Result<(), MoonlightClientError> {
        let client = Client::with_defaults().map_err(req_err)?;

        {
            let mut client_lock = self.client.lock().await;
            *client_lock = client;
        }

        self.update().await?;

        Ok(())
    }

    pub fn is_paired(&self) -> bool {
        // TODO
        false
    }
    fn check_paired(&self) -> Result<(), MoonlightClientError> {
        todo!()
    }

    pub async fn pair<Crypto>(
        &self,
        client_identifier: &ClientIdentifier,
        client_secret: &ClientSecret,
        device_name: String,
        pin: PairPin,
        crypto_provider: Crypto,
    ) -> Result<(), MoonlightClientError>
    where
        Crypto: PairingCryptoBackend,
        Crypto::Error: Error + Send + Sync + 'static,
    {
        let http_address = self.http_address();
        let server_version = self.version().await?;
        let https_address = self.https_address().await?;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let mut client = Client::with_defaults_long_timeout().map_err(req_err)?;

        let mut pairing = ClientPairing::new(
            client_identifier.clone(),
            client_secret.clone(),
            server_version,
            device_name,
            pin,
            crypto_provider,
        )
        .map_err(crypto_err)?;

        // TODO: when any error happens we MUST call the unpair endpoint

        loop {
            match pairing.poll_output().map_err(crypto_err)? {
                ClientPairingOutput::SendHttpPairRequest(request) => {
                    let response = client
                        .send_http::<PairEndpoint>(client_info, &http_address, &request)
                        .await
                        .map_err(req_err)?;

                    pairing.handle_response(response).map_err(crypto_err)?;
                }
                ClientPairingOutput::SetServerIdentifier(server_identifier) => {
                    client = Client::with_certificates(
                        &client_secret.to_pem(),
                        &client_identifier.to_pem(),
                        &server_identifier.to_pem(),
                    )
                    .map_err(req_err)?;
                }
                ClientPairingOutput::SendHttpsPairRequest(request) => {
                    let response = client
                        .send_https::<PairEndpoint>(client_info, &http_address, &request)
                        .await
                        .map_err(req_err)?;

                    pairing.handle_response(response).map_err(crypto_err)?;
                }
                ClientPairingOutput::Success => {
                    // TODO: set the identity of the client and server

                    self.update().await.map_err(req_err)?;

                    break;
                }
            };
        }

        Ok(())
    }

    pub async fn unpair(&self) -> Result<(), MoonlightClientError> {
        self.check_paired()?;

        let https_address = self.https_address().await?;
        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        {
            let mut client = self.client.lock().await;

            client
                .send_https::<UnpairEndpoint>(client_info, &https_address, &UnpairRequest {})
                .await
                .map_err(req_err)?;

            let new_client = Client::with_defaults().map_err(req_err)?;
            *client = new_client;
        }

        Ok(())
    }

    pub async fn apollo_permissions(
        &self,
    ) -> Result<Option<ApolloPermissions>, MoonlightClientError> {
        self.check_paired()?;

        self.server_info(|info| info.apollo_permissions.clone())
            .await
    }

    pub async fn app_list(&self) -> Result<Vec<App>, MoonlightClientError> {
        todo!()
    }

    pub async fn request_app_image(
        &mut self,
        app_id: u32,
    ) -> Result<Vec<u8>, MoonlightClientError> {
        self.check_paired()?;

        let https_address = self.https_address().await?;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let client = { self.client.lock().await.clone() };
        let response = client
            .send_https_with_bytes::<AppBoxArtEndpoint>(
                client_info,
                &https_address,
                &AppBoxArtRequest { app_id },
            )
            .await
            .map_err(req_err)?;

        Ok(response)
    }

    pub async fn cancel(&mut self) -> Result<bool, MoonlightClientError> {
        self.check_paired()?;

        let https_hostport = self.https_address().await?;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let response = {
            let client = self.client.lock().await;

            client
                .send_https::<CancelEndpoint>(client_info, &https_hostport, &CancelRequest {})
                .await
                .map_err(req_err)?
        };

        if !response.cancel {
            return Ok(false);
        }

        self.update().await?;

        let current_game = self.current_game().await?;
        if current_game != 0 {
            // We're not the device that opened this session
            return Ok(false);
        }

        Ok(response.cancel)
    }
}

// TODO: change that feature flags
// mod stream {
//     use openssl::rand::rand_bytes;
//     use uuid::Uuid;
//
//     use crate::{
//         high::{HostError, MoonlightClient, StreamConfigError},
//         http::{
//             ClientInfo,
//             launch::{ClientStreamRequest, host_launch, host_resume},
//             request_client::RequestClient,
//         },
//         pair::PairError,
//         stream::{
//             AesIv, AesKey, EncryptionFlags, MoonlightStreamConfig,
//             audio::AudioConfig,
//             control::ActiveGamepads,
//             video::{ColorRange, ColorSpace, ServerCodecModeSupport, SupportedVideoFormats},
//         },
//     };
//
//     impl<C> MoonlightClient<C>
//     where
//         C: RequestClient,
//     {
//         // Stream config correction
//         pub async fn is_hdr_supported(&mut self) -> Result<bool, HostError<C::Error>> {
//             let server_codec_mode_support = self.server_codec_mode_support().await?;
//
//             Ok(
//                 server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
//                     || server_codec_mode_support.contains(ServerCodecModeSupport::AV1_MAIN10),
//             )
//         }
//         pub async fn is_4k_supported(&mut self) -> Result<bool, HostError<C::Error>> {
//             let is_nvidia = self.is_nvidia_software().await?;
//             let server_codec_mode_support = self.server_codec_mode_support().await?;
//
//             Ok(
//                 server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
//                     || !is_nvidia,
//             )
//         }
//         pub async fn is_4k_supported_gfe(&mut self) -> Result<bool, HostError<C::Error>> {
//             let gfe = self.gfe_version().await?;
//
//             Ok(!gfe.starts_with("2."))
//         }
//
//         pub async fn is_resolution_supported(
//             &mut self,
//             width: usize,
//             height: usize,
//             supported_video_formats: SupportedVideoFormats,
//         ) -> Result<(), HostError<C::Error>> {
//             let resolution_above_4k = width > 4096 || height > 4096;
//
//             if resolution_above_4k && !self.is_4k_supported().await? {
//                 return Err(StreamConfigError::NotSupported4k.into());
//             } else if resolution_above_4k
//                 && supported_video_formats.contains(!SupportedVideoFormats::MASK_H264)
//             {
//                 return Err(StreamConfigError::NotSupported4kCodecMissing.into());
//             } else if height > 2160 && self.is_4k_supported_gfe().await? {
//                 return Err(StreamConfigError::NotSupported4kUpdateGfe.into());
//             }
//
//             Ok(())
//         }
//
//         pub async fn should_disable_sops(
//             &mut self,
//             width: usize,
//             height: usize,
//         ) -> Result<bool, HostError<C::Error>> {
//             // Using an unsupported resolution (not 720p, 1080p, or 4K) causes
//             // GFE to force SOPS to 720p60. This is fine for < 720p resolutions like
//             // 360p or 480p, but it is not ideal for 1440p and other resolutions.
//             // When we detect an unsupported resolution, disable SOPS unless it's under 720p.
//             // FIXME: Detect support resolutions using the serverinfo response, not a hardcoded list
//             const NVIDIA_SUPPORTED_RESOLUTIONS: &[(usize, usize)] =
//                 &[(1280, 720), (1920, 1080), (3840, 2160)];
//
//             let is_nvidia = self.is_nvidia_software().await?;
//
//             Ok(!NVIDIA_SUPPORTED_RESOLUTIONS.contains(&(width, height)) && is_nvidia)
//         }
//
//         pub async fn start_stream(
//             &mut self,
//             app_id: u32,
//             width: u32,
//             height: u32,
//             mut fps: u32,
//             hdr: bool,
//             mut sops: bool,
//             local_audio_play_mode: bool,
//             gamepads_attached: ActiveGamepads,
//             gamepads_persist_after_disconnect: bool,
//             color_space: ColorSpace,
//             color_range: ColorRange,
//             bitrate: u32,
//             packet_size: u32,
//             encryption_flags: EncryptionFlags,
//             audio_configuration: AudioConfig,
//             supported_video_formats: SupportedVideoFormats,
//             launch_url_query_parameters: &str,
//         ) -> Result<MoonlightStreamConfig, HostError<C::Error>> {
//             // Change streaming options if required
//
//             if hdr && !self.is_hdr_supported().await? {
//                 return Err(HostError::StreamConfig(StreamConfigError::NotSupportedHdr));
//             }
//
//             self.is_resolution_supported(width as usize, height as usize, supported_video_formats)
//                 .await?;
//
//             if self.is_nvidia_software().await? {
//                 // Using an FPS value over 60 causes SOPS to default to 720p60,
//                 // so force it to 0 to ensure the correct resolution is set. We
//                 // used to use 60 here but that locked the frame rate to 60 FPS
//                 // on GFE 3.20.3. We don't need this hack for Sunshine.
//                 if fps > 60 {
//                     fps = 0;
//                 }
//
//                 if self
//                     .should_disable_sops(width as usize, height as usize)
//                     .await?
//                 {
//                     sops = false;
//                 }
//             }
//
//             // Clearing cache so we refresh and can see if there's a game -> launch or resume?
//             self.clear_cache();
//
//             let address = self.address.clone();
//             let https_address = self.https_address().await?;
//
//             let current_game = self.current_game().await?;
//
//             let mut aes_key = [0u8; 16];
//             rand_bytes(&mut aes_key).map_err(PairError::from)?;
//
//             let mut aes_iv = [0u8; 4];
//             rand_bytes(&mut aes_iv).map_err(PairError::from)?;
//             let aes_iv = u32::from_be_bytes(aes_iv);
//
//             let request = ClientStreamRequest {
//                 app_id,
//                 mode_width: width,
//                 mode_height: height,
//                 mode_fps: fps,
//                 hdr,
//                 sops,
//                 local_audio_play_mode,
//                 gamepads_attached_mask: gamepads_attached.bits() as i32,
//                 gamepads_persist_after_disconnect,
//                 ri_key: aes_key,
//                 ri_key_id: aes_iv,
//             };
//
//             let client_info = ClientInfo {
//                 unique_id: &self.client_unique_id,
//                 uuid: Uuid::new_v4(),
//             };
//
//             let rtsp_session_url = if current_game == 0 {
//                 let launch_response = host_launch(
//                     &mut self.client,
//                     &https_address,
//                     client_info,
//                     request,
//                     launch_url_query_parameters,
//                 )
//                 .await?;
//
//                 launch_response.rtsp_session_url
//             } else {
//                 let resume_response = host_resume(
//                     &mut self.client,
//                     &https_address,
//                     client_info,
//                     request,
//                     launch_url_query_parameters,
//                 )
//                 .await?;
//
//                 resume_response.rtsp_session_url
//             };
//
//             let app_version = self.version().await?;
//             let server_codec_mode_support = self.server_codec_mode_support().await?;
//             let gfe_version = self.gfe_version().await?.to_owned();
//             let apollo_permissions = self.apollo_permissions().await?;
//
//             Ok(MoonlightStreamConfig {
//                 address,
//                 gfe_version,
//                 server_codec_mode_support,
//                 rtsp_session_url: rtsp_session_url.to_string(),
//                 remote_input_aes_iv: AesIv(aes_iv),
//                 remote_input_aes_key: AesKey(aes_key),
//                 version: app_version,
//                 apollo_permissions,
//             })
//         }
//     }
// }
