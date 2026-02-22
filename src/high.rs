//!
//! The high level api of the moonlight wrapper
//!

use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4},
};

use pem::Pem;
use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock},
    task::JoinError,
};
use uuid::Uuid;

use crate::{
    Error, MoonlightError, PairPin, PairStatus, ServerState, ServerVersion,
    http::{
        ClientInfo, DEFAULT_UNIQUE_ID, ParseError,
        app_list::{App, AppListEndpoint, AppListRequest, AppListResponse},
        box_art::{AppBoxArtEndpoint, AppBoxArtRequest},
        client::async_client::RequestClient,
        host_info::{ApolloPermissions, HostInfoEndpoint, HostInfoRequest, HostInfoResponse},
        unpair::{UnpairEndpoint, UnpairRequest},
    },
    mac::MacAddress,
    pair::{ClientAuth, PairSuccess, host_pair},
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
pub enum HostError<RequestError> {
    #[error("{0}")]
    Moonlight(#[from] MoonlightError),
    #[error("{0}")]
    BlockingJoin(#[from] JoinError),
    #[error("this action requires pairing")]
    NotPaired,
    #[error("{0}")]
    Api(RequestError),
    #[error("{0}")]
    StreamConfig(#[from] StreamConfigError),
    #[error("the host is likely offline")]
    LikelyOffline,
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

pub struct MoonlightClient<Client> {
    client_unique_id: String,
    client: Mutex<Client>,
    address: String,
    http_port: u16,
    cache: RwLock<Cache>,
}

#[derive(Debug, Default)]
struct Cache {
    host_info: Option<HostInfoResponse>,
    app_list: Option<AppListResponse>,
}

impl<C> MoonlightClient<C>
where
    C: RequestClient,
{
    pub fn new(
        address: String,
        http_port: u16,
        unique_id: Option<String>,
    ) -> Result<Self, HostError<C::Error>> {
        Ok(Self {
            client: Mutex::new(C::with_defaults().map_err(|err| HostError::Api(err))?),
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

    pub async fn update(&self) -> Result<(), HostError<C::Error>> {
        let client = self.client.lock().await;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let mut cache = Cache::default();

        let http_address = self.http_address();
        let host_info = client
            .send_http::<HostInfoEndpoint>(client_info, &http_address, &HostInfoRequest {})
            .await
            .map_err(|err| HostError::Api(err))?;

        let https_port = host_info.https_port;
        cache.host_info = Some(host_info);

        if self.is_paired() == PairStatus::Paired {
            let https_address = Self::build_https_address(&self.address, https_port);

            let host_info_secure = client
                .send_https::<HostInfoEndpoint>(client_info, &http_address, &HostInfoRequest {})
                .await
                .map_err(|err| HostError::Api(err))?;

            cache.host_info = Some(host_info_secure);

            let app_list = client
                .send_https::<AppListEndpoint>(client_info, &http_address, &AppListRequest {})
                .await
                .map_err(|err| HostError::Api(err))?;

            cache.app_list = Some(app_list);
        }

        {
            let mut cache_lock = self.cache.write().await;
            *cache_lock = cache;
        }

        drop(client);

        Ok(())
    }

    async fn host_info(&self) -> Result<&HostInfoResponse, HostError<C::Error>> {
        todo!()
    }

    pub async fn https_port(&self) -> Result<u16, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.https_port)
    }

    fn build_https_address(address: &str, https_port: u16) -> String {
        format!("{address}:{https_port}")
    }
    pub async fn https_address(&self) -> Result<String, HostError<C::Error>> {
        let https_port = self.https_port().await?;
        Ok(Self::build_https_address(&self.address, https_port))
    }
    pub async fn external_port(&self) -> Result<u16, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.external_port)
    }

    pub async fn host_name(&self) -> Result<&str, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.host_name.as_str())
    }
    pub async fn version(&self) -> Result<ServerVersion, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.app_version)
    }

    pub async fn gfe_version(&self) -> Result<&str, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.gfe_version.as_str())
    }
    pub async fn unique_id(&self) -> Result<Uuid, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.unique_id)
    }

    /// Returns None if unpaired
    pub async fn mac(&self) -> Result<Option<MacAddress>, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.mac)
    }
    pub async fn local_ip(&self) -> Result<Ipv4Addr, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.local_ip)
    }

    pub async fn current_game(&self) -> Result<u32, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.current_game)
    }

    pub async fn state(&self) -> Result<ServerState, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.state)
    }

    pub async fn max_luma_pixels_hevc(&self) -> Result<u32, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.max_luma_pixels_hevc)
    }

    pub async fn server_codec_mode_support(
        &self,
    ) -> Result<ServerCodecModeSupport, HostError<C::Error>> {
        let info = self.host_info().await?;
        Ok(info.server_codec_mode_support)
    }

    pub async fn set_identity(
        &self,
        client_auth: &ClientAuth,
        server_certificate: &Pem,
    ) -> Result<(), HostError<C::Error>> {
        let client = C::with_certificates(
            &client_auth.private_key,
            &client_auth.certificate,
            server_certificate,
        )
        .map_err(|err| HostError::Api(err))?;

        {
            let mut client_lock = self.client.lock().await;
            *client_lock = client;
        }

        self.update().await?;

        Ok(())
    }

    pub async fn clear_identity(&self) -> Result<(), HostError<C::Error>> {
        let client = C::with_defaults().map_err(HostError::Api)?;

        {
            let mut client_lock = self.client.lock().await;
            *client_lock = client;
        }

        self.update().await?;

        Ok(())
    }

    pub fn is_paired(&self) -> PairStatus {
        if self.paired.is_some() {
            PairStatus::Paired
        } else {
            PairStatus::NotPaired
        }
    }

    pub fn client_private_key(&self) -> Option<&Pem> {
        self.paired.as_ref().map(|x| &x.client_private_key)
    }
    pub fn client_certificate(&self) -> Option<&Pem> {
        self.paired.as_ref().map(|x| &x.client_certificate)
    }
    pub fn server_certificate(&self) -> Option<&Pem> {
        self.paired.as_ref().map(|x| &x.server_certificate)
    }

    pub async fn pair(
        &self,
        auth: &ClientAuth,
        device_name: String,
        pin: PairPin,
    ) -> Result<(), HostError<C::Error>> {
        let http_address = self.http_address();
        let server_version = self.version().await?;
        let https_address = self.https_address().await?;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let mut client = C::with_defaults_long_timeout().map_err(HostError::Api)?;

        let PairSuccess {
            server_certificate,
            client: new_client,
        } = host_pair(
            &mut client,
            &http_address,
            &https_address,
            client_info,
            &auth.private_key,
            &auth.certificate,
            &device_name,
            server_version,
            pin,
        )
        .await?;

        self.client = new_client;

        self.paired = Some(Paired {
            client_private_key: auth.private_key.clone(),
            client_certificate: auth.certificate.clone(),
            server_certificate,
            cache_app_list: None,
        });

        let Some(info) = self.cache_info.as_mut() else {
            unreachable!()
        };
        info.pair_status = PairStatus::Paired;

        self.clear_cache();

        Ok(())
    }

    fn check_paired(&self) -> Result<(), HostError<C::Error>> {
        if self.is_paired() == PairStatus::Paired {
            Ok(())
        } else {
            Err(HostError::NotPaired)
        }
    }

    pub async fn unpair(&self) -> Result<(), HostError<C::Error>> {
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
                .map_err(HostError::Api)?;

            let new_client = C::with_defaults().map_err(HostError::Api)?;
            *client = new_client;
        }

        Ok(())
    }

    pub async fn apollo_permissions(
        &self,
    ) -> Result<Option<ApolloPermissions>, HostError<C::Error>> {
        self.check_paired()?;

        let host_info = self.host_info().await?;

        Ok(host_info.apollo_permissions.clone())
    }

    pub async fn app_list(&self) -> Result<Vec<App>, HostError<C::Error>> {
        self.check_paired()?;

        let https_address = self.https_address().await?;
        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let Some(paired) = self.paired.as_mut() else {
            return Err(HostError::NotPaired);
        };

        // Recache
        if paired.cache_app_list.is_none() {
            let response = host_app_list(&mut self.client, &https_address, client_info).await?;

            paired.cache_app_list = Some(response);
        }

        let Some(cache_app_list) = &paired.cache_app_list else {
            unreachable!()
        };

        Ok(cache_app_list.apps.as_slice())
    }

    pub async fn request_app_image(
        &mut self,
        app_id: u32,
    ) -> Result<C::Bytes, HostError<C::Error>> {
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
            .await;

        Ok(response)
    }

    pub async fn cancel(&mut self) -> Result<bool, HostError<C::Error>> {
        self.check_paired()?;

        let https_hostport = self.https_address().await?;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let response = host_cancel(&mut self.client, &https_hostport, client_info).await?;

        self.clear_cache();

        let current_game = self.current_game().await?;
        if current_game != 0 {
            // We're not the device that opened this session
            return Ok(false);
        }

        Ok(response)
    }
}

// TODO: change that feature flags
#[cfg(all(feature = "stream_c", feature = "stream_proto"))]
mod stream {
    use openssl::rand::rand_bytes;
    use uuid::Uuid;

    use crate::{
        high::{HostError, MoonlightClient, StreamConfigError},
        http::{
            ClientInfo,
            launch::{ClientStreamRequest, host_launch, host_resume},
            request_client::RequestClient,
        },
        pair::PairError,
        stream::{
            AesIv, AesKey, EncryptionFlags, MoonlightStreamConfig,
            audio::AudioConfig,
            control::ActiveGamepads,
            video::{ColorRange, ColorSpace, ServerCodecModeSupport, SupportedVideoFormats},
        },
    };

    impl<C> MoonlightClient<C>
    where
        C: RequestClient,
    {
        // Stream config correction
        pub async fn is_hdr_supported(&mut self) -> Result<bool, HostError<C::Error>> {
            let server_codec_mode_support = self.server_codec_mode_support().await?;

            Ok(
                server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
                    || server_codec_mode_support.contains(ServerCodecModeSupport::AV1_MAIN10),
            )
        }
        pub async fn is_4k_supported(&mut self) -> Result<bool, HostError<C::Error>> {
            let is_nvidia = self.is_nvidia_software().await?;
            let server_codec_mode_support = self.server_codec_mode_support().await?;

            Ok(
                server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
                    || !is_nvidia,
            )
        }
        pub async fn is_4k_supported_gfe(&mut self) -> Result<bool, HostError<C::Error>> {
            let gfe = self.gfe_version().await?;

            Ok(!gfe.starts_with("2."))
        }

        pub async fn is_resolution_supported(
            &mut self,
            width: usize,
            height: usize,
            supported_video_formats: SupportedVideoFormats,
        ) -> Result<(), HostError<C::Error>> {
            let resolution_above_4k = width > 4096 || height > 4096;

            if resolution_above_4k && !self.is_4k_supported().await? {
                return Err(StreamConfigError::NotSupported4k.into());
            } else if resolution_above_4k
                && supported_video_formats.contains(!SupportedVideoFormats::MASK_H264)
            {
                return Err(StreamConfigError::NotSupported4kCodecMissing.into());
            } else if height > 2160 && self.is_4k_supported_gfe().await? {
                return Err(StreamConfigError::NotSupported4kUpdateGfe.into());
            }

            Ok(())
        }

        pub async fn should_disable_sops(
            &mut self,
            width: usize,
            height: usize,
        ) -> Result<bool, HostError<C::Error>> {
            // Using an unsupported resolution (not 720p, 1080p, or 4K) causes
            // GFE to force SOPS to 720p60. This is fine for < 720p resolutions like
            // 360p or 480p, but it is not ideal for 1440p and other resolutions.
            // When we detect an unsupported resolution, disable SOPS unless it's under 720p.
            // FIXME: Detect support resolutions using the serverinfo response, not a hardcoded list
            const NVIDIA_SUPPORTED_RESOLUTIONS: &[(usize, usize)] =
                &[(1280, 720), (1920, 1080), (3840, 2160)];

            let is_nvidia = self.is_nvidia_software().await?;

            Ok(!NVIDIA_SUPPORTED_RESOLUTIONS.contains(&(width, height)) && is_nvidia)
        }

        pub async fn start_stream(
            &mut self,
            app_id: u32,
            width: u32,
            height: u32,
            mut fps: u32,
            hdr: bool,
            mut sops: bool,
            local_audio_play_mode: bool,
            gamepads_attached: ActiveGamepads,
            gamepads_persist_after_disconnect: bool,
            color_space: ColorSpace,
            color_range: ColorRange,
            bitrate: u32,
            packet_size: u32,
            encryption_flags: EncryptionFlags,
            audio_configuration: AudioConfig,
            supported_video_formats: SupportedVideoFormats,
            launch_url_query_parameters: &str,
        ) -> Result<MoonlightStreamConfig, HostError<C::Error>> {
            // Change streaming options if required

            if hdr && !self.is_hdr_supported().await? {
                return Err(HostError::StreamConfig(StreamConfigError::NotSupportedHdr));
            }

            self.is_resolution_supported(width as usize, height as usize, supported_video_formats)
                .await?;

            if self.is_nvidia_software().await? {
                // Using an FPS value over 60 causes SOPS to default to 720p60,
                // so force it to 0 to ensure the correct resolution is set. We
                // used to use 60 here but that locked the frame rate to 60 FPS
                // on GFE 3.20.3. We don't need this hack for Sunshine.
                if fps > 60 {
                    fps = 0;
                }

                if self
                    .should_disable_sops(width as usize, height as usize)
                    .await?
                {
                    sops = false;
                }
            }

            // Clearing cache so we refresh and can see if there's a game -> launch or resume?
            self.clear_cache();

            let address = self.address.clone();
            let https_address = self.https_address().await?;

            let current_game = self.current_game().await?;

            let mut aes_key = [0u8; 16];
            rand_bytes(&mut aes_key).map_err(PairError::from)?;

            let mut aes_iv = [0u8; 4];
            rand_bytes(&mut aes_iv).map_err(PairError::from)?;
            let aes_iv = u32::from_be_bytes(aes_iv);

            let request = ClientStreamRequest {
                app_id,
                mode_width: width,
                mode_height: height,
                mode_fps: fps,
                hdr,
                sops,
                local_audio_play_mode,
                gamepads_attached_mask: gamepads_attached.bits() as i32,
                gamepads_persist_after_disconnect,
                ri_key: aes_key,
                ri_key_id: aes_iv,
            };

            let client_info = ClientInfo {
                unique_id: &self.client_unique_id,
                uuid: Uuid::new_v4(),
            };

            let rtsp_session_url = if current_game == 0 {
                let launch_response = host_launch(
                    &mut self.client,
                    &https_address,
                    client_info,
                    request,
                    launch_url_query_parameters,
                )
                .await?;

                launch_response.rtsp_session_url
            } else {
                let resume_response = host_resume(
                    &mut self.client,
                    &https_address,
                    client_info,
                    request,
                    launch_url_query_parameters,
                )
                .await?;

                resume_response.rtsp_session_url
            };

            let app_version = self.version().await?;
            let server_codec_mode_support = self.server_codec_mode_support().await?;
            let gfe_version = self.gfe_version().await?.to_owned();
            let apollo_permissions = self.apollo_permissions().await?;

            Ok(MoonlightStreamConfig {
                address,
                gfe_version,
                server_codec_mode_support,
                rtsp_session_url: rtsp_session_url.to_string(),
                remote_input_aes_iv: AesIv(aes_iv),
                remote_input_aes_key: AesKey(aes_key),
                version: app_version,
                apollo_permissions,
            })
        }
    }
}
