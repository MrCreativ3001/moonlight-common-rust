use std::{
    error::Error,
    io,
    net::{Ipv4Addr, SocketAddrV4},
};

use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock},
};
use uuid::Uuid;

use crate::{
    ServerState, ServerVersion,
    high::MoonlightClientError,
    http::{
        ClientIdentifier, ClientInfo, ClientSecret, DEFAULT_UNIQUE_ID, ServerIdentifier,
        app_list::{App, AppListEndpoint, AppListRequest, AppListResponse},
        box_art::{AppBoxArtEndpoint, AppBoxArtRequest},
        cancel::{CancelEndpoint, CancelRequest},
        client::async_client::RequestClient,
        launch::{ClientStreamRequest, LaunchEndpoint},
        pair::{
            PairEndpoint, PairPin, PairingCryptoBackend,
            client::{ClientPairing, ClientPairingError, ClientPairingOutput},
        },
        resume::ResumeEndpoint,
        server_info::{
            ApolloPermissions, ServerInfoEndpoint, ServerInfoRequest, ServerInfoResponse,
        },
        unpair::{UnpairEndpoint, UnpairRequest},
    },
    mac::MacAddress,
    stream::{
        AesIv, AesKey, MoonlightStreamConfig, MoonlightStreamSettings,
        video::ServerCodecModeSupport,
    },
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
        let mut cache_lock = self.cache.write().await;
        let client = self.client.lock().await;

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let http_address = self.http_address();
        let server_info = client
            .send_http::<ServerInfoEndpoint>(client_info, &http_address, &ServerInfoRequest {})
            .await
            .map_err(req_err)?;

        let https_port = server_info.https_port;
        cache_lock.server_info = Some(server_info);

        if cache_lock.authenticated.is_some() {
            let https_address = Self::build_https_address(&self.address, https_port);

            let server_info_secure = client
                .send_https::<ServerInfoEndpoint>(
                    client_info,
                    &https_address,
                    &ServerInfoRequest {},
                )
                .await
                .map_err(req_err)?;

            cache_lock.server_info = Some(server_info_secure);

            let app_list = client
                .send_https::<AppListEndpoint>(client_info, &https_address, &AppListRequest {})
                .await
                .map_err(req_err)?;

            cache_lock.app_list = Some(app_list);
        } else {
            cache_lock.app_list = None;
        }

        drop(cache_lock);
        drop(client);

        Ok(())
    }

    async fn server_info_priv<R>(
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

    pub async fn server_info(&self) -> Result<ServerInfoResponse, MoonlightClientError> {
        self.server_info_priv(|response| response.clone()).await
    }

    pub async fn https_port(&self) -> Result<u16, MoonlightClientError> {
        self.server_info_priv(|info| info.https_port).await
    }

    fn build_https_address(address: &str, https_port: u16) -> String {
        format!("{address}:{https_port}")
    }
    pub async fn https_address(&self) -> Result<String, MoonlightClientError> {
        let https_port = self.https_port().await?;
        Ok(Self::build_https_address(&self.address, https_port))
    }
    pub async fn external_port(&self) -> Result<u16, MoonlightClientError> {
        self.server_info_priv(|info| info.external_port).await
    }

    pub async fn host_name(&self) -> Result<String, MoonlightClientError> {
        self.server_info_priv(|info| info.host_name.clone()).await
    }
    pub async fn version(&self) -> Result<ServerVersion, MoonlightClientError> {
        self.server_info_priv(|info| info.app_version).await
    }

    pub async fn gfe_version(&self) -> Result<String, MoonlightClientError> {
        self.server_info_priv(|info| info.gfe_version.clone()).await
    }
    pub async fn unique_id(&self) -> Result<Uuid, MoonlightClientError> {
        self.server_info_priv(|info| info.unique_id).await
    }

    /// Returns None if unpaired
    pub async fn mac(&self) -> Result<Option<MacAddress>, MoonlightClientError> {
        self.server_info_priv(|info| info.mac).await
    }
    pub async fn local_ip(&self) -> Result<Ipv4Addr, MoonlightClientError> {
        self.server_info_priv(|info| info.local_ip).await
    }

    pub async fn current_game(&self) -> Result<u32, MoonlightClientError> {
        self.server_info_priv(|info| info.current_game).await
    }

    pub async fn state(&self) -> Result<ServerState, MoonlightClientError> {
        self.server_info_priv(|info| info.state).await
    }

    pub async fn max_luma_pixels_hevc(&self) -> Result<u32, MoonlightClientError> {
        self.server_info_priv(|info| info.max_luma_pixels_hevc)
            .await
    }

    pub async fn server_codec_mode_support(
        &self,
    ) -> Result<ServerCodecModeSupport, MoonlightClientError> {
        self.server_info_priv(|info| info.server_codec_mode_support)
            .await
    }

    pub async fn set_identity(
        &self,
        client_identifier: ClientIdentifier,
        client_secret: ClientSecret,
        server_identifier: ServerIdentifier,
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

            let mut cache = self.cache.write().await;

            cache.authenticated = Some(Authenticated {
                client_identifier,
                client_secret,
                server_identifier,
            });
        }

        self.update().await?;

        Ok(())
    }
    pub async fn identity(&self) -> Option<(ClientIdentifier, ClientSecret, ServerIdentifier)> {
        let cache = self.cache.read().await;

        cache.authenticated.as_ref().map(|authenticated| {
            (
                authenticated.client_identifier.clone(),
                authenticated.client_secret.clone(),
                authenticated.server_identifier.clone(),
            )
        })
    }

    pub async fn is_paired(&self) -> Result<bool, MoonlightClientError> {
        let cache = self.cache.read().await;
        Ok(cache.authenticated.is_some())
    }
    async fn check_paired(&self) -> Result<(), MoonlightClientError> {
        if self.is_paired().await? {
            Ok(())
        } else {
            Err(MoonlightClientError::Unauthenticated)
        }
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

        match self
            .pair_impl(
                &http_address,
                &https_address,
                client_identifier,
                client_secret,
                client_info,
                &mut pairing,
                &mut client,
            )
            .await
        {
            Ok(()) => {}
            Err(err) => {
                // Try to unpair
                let _ = client
                    .send_http::<UnpairEndpoint>(client_info, &http_address, &UnpairRequest {})
                    .await;

                return Err(err);
            }
        }

        // Replace client
        {
            let mut client_lock = self.client.lock().await;
            *client_lock = client;
        }

        // Update our info
        self.update().await.map_err(req_err)?;

        Ok(())
    }
    async fn pair_impl<Crypto>(
        &self,
        http_address: &str,
        https_address: &str,
        client_identifier: &ClientIdentifier,
        client_secret: &ClientSecret,
        client_info: ClientInfo<'_>,
        pairing: &mut ClientPairing<Crypto>,
        client: &mut Client,
    ) -> Result<(), MoonlightClientError>
    where
        Crypto: PairingCryptoBackend,
        Crypto::Error: Error + Send + Sync + 'static,
    {
        let mut server_identifier = None;

        loop {
            match pairing.poll_output().map_err(crypto_err)? {
                ClientPairingOutput::SendHttpPairRequest(request) => {
                    let response = client
                        .send_http::<PairEndpoint>(client_info, http_address, &request)
                        .await
                        .map_err(req_err)?;

                    pairing.handle_response(response).map_err(crypto_err)?;
                }
                ClientPairingOutput::SetServerIdentifier(new_server_identifier) => {
                    *client = Client::with_certificates(
                        &client_secret.to_pem(),
                        &client_identifier.to_pem(),
                        &new_server_identifier.to_pem(),
                    )
                    .map_err(req_err)?;

                    server_identifier = Some(new_server_identifier);
                }
                ClientPairingOutput::SendHttpsPairRequest(request) => {
                    assert!(
                        server_identifier.is_some(),
                        "ClientPairing didn't set ServerIdentifier but tried to make a https request"
                    );

                    let response = client
                        .send_https::<PairEndpoint>(client_info, https_address, &request)
                        .await
                        .map_err(req_err)?;

                    pairing.handle_response(response).map_err(crypto_err)?;
                }
                ClientPairingOutput::Success => {
                    {
                        let mut cache_lock = self.cache.write().await;
                        cache_lock.authenticated = Some(Authenticated {
                            client_identifier: client_identifier.clone(),
                            client_secret: client_secret.clone(),
                            server_identifier: server_identifier
                                .take()
                                .expect("PairingClient didn't set a server identifier"),
                        });
                    }

                    return Ok(());
                }
            };
        }
    }

    pub async fn unpair(&self) -> Result<(), MoonlightClientError> {
        self.check_paired().await?;

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
        self.check_paired().await?;

        self.server_info_priv(|info| info.apollo_permissions.clone())
            .await
    }

    pub async fn app_list(&self) -> Result<Vec<App>, MoonlightClientError> {
        let cache = self.cache.read().await;

        if cache.authenticated.is_none() {
            return Err(MoonlightClientError::Unauthenticated);
        }

        if let Some(app_list) = &cache.app_list {
            Ok(app_list.apps.clone())
        } else {
            drop(cache);

            self.update().await?;
            let cache = self.cache.read().await;
            let Some(app_list) = &cache.app_list else {
                unreachable!()
            };

            Ok(app_list.apps.clone())
        }
    }

    pub async fn request_app_image(&self, app_id: u32) -> Result<Vec<u8>, MoonlightClientError> {
        self.check_paired().await?;

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

    /// Starts a stream.
    /// The returned [MoonlightStreamConfig] must be passed into a stream implementation.
    ///
    /// Before starting the stream you should adjust the settings using [MoonlightStreamSettings::adjust_for_server].
    pub async fn start_stream(
        &self,
        app_id: u32,
        settings: &MoonlightStreamSettings,
        aes_key: AesKey,
        aes_iv: AesIv,
        launch_url_query_parameters: &str,
    ) -> Result<MoonlightStreamConfig, MoonlightClientError> {
        // Clearing cache so we refresh and can see if there's a game -> launch or resume?
        self.update().await?;

        let address = self.address.clone();
        let https_address = self.https_address().await?;

        let current_game = self.current_game().await?;

        let request = ClientStreamRequest {
            app_id,
            mode_width: settings.width,
            mode_height: settings.height,
            mode_fps: settings.fps,
            hdr: settings.hdr,
            sops: settings.sops,
            local_audio_play_mode: settings.local_audio_play_mode,
            gamepads_attached_mask: settings.gamepads_attached.bits() as i32,
            gamepads_persist_after_disconnect: settings.gamepads_persist_after_disconnect,
            ri_key: aes_key,
            ri_key_id: aes_iv,
            additional_query_parameters: launch_url_query_parameters.to_string(),
        };

        let client_info = ClientInfo {
            unique_id: &self.client_unique_id,
            uuid: Uuid::new_v4(),
        };

        let rtsp_session_url = {
            let client = self.client.lock().await;

            if current_game == 0 {
                let launch_response = client
                    .send_https::<LaunchEndpoint>(client_info, &https_address, &request)
                    .await
                    .map_err(req_err)?;

                launch_response.rtsp_session_url
            } else {
                let resume_response = client
                    .send_https::<ResumeEndpoint>(client_info, &https_address, &request)
                    .await
                    .map_err(req_err)?;

                resume_response.rtsp_session_url
            }
        };

        let app_version = self.version().await?;
        let server_codec_mode_support = self.server_codec_mode_support().await?;
        let gfe_version = self.gfe_version().await?.to_owned();
        let apollo_permissions = self.apollo_permissions().await?;

        Ok(MoonlightStreamConfig {
            address,
            gfe_version: Some(gfe_version),
            server_codec_mode_support,
            rtsp_session_url,
            remote_input_aes_iv: aes_iv,
            remote_input_aes_key: aes_key,
            version: app_version,
            apollo_permissions,
        })
    }

    pub async fn cancel(&self) -> Result<bool, MoonlightClientError> {
        self.check_paired().await?;

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

        if !response.cancelled {
            return Ok(false);
        }

        self.update().await?;

        let current_game = self.current_game().await?;
        if current_game != 0 {
            // We're not the device that opened this session
            return Ok(false);
        }

        Ok(response.cancelled)
    }
}
