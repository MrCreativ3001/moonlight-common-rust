use std::{
    collections::VecDeque,
    fmt::{self, Debug, Formatter},
    mem,
    net::SocketAddr,
    time::Duration,
};

use sans_io_time::Instant;

use rusty_enet::error::PeerSendError;
use tracing::{Level, debug, info, instrument, trace, warn};

use crate::{
    ServerVersion,
    http::server_info::ApolloPermissions,
    stream::{
        AesKey,
        control::EstimatedRttInfo,
        proto::{
            DynCryptoBackend,
            control::{
                input_batcher::{ClientInputEvent, InputBatcher},
                packet::{
                    ControlPacket, ControlPacketConfig, ControlPacketNotSupported, EnetChannel,
                    PERIODIC_PING_INTERVAL, PERIODIC_PING_VERSION,
                },
                peer::{
                    ControlConnectConfig, ControlEncryptionMethod, ControlError, ControlHost,
                    ControlHostConfig, ControlHostEvent, ControlPeerConfig, ControlPeerId,
                    ControlPeerRole,
                },
            },
            enet::EnetError,
            runtime::UdpStream,
            soonest,
        },
    },
};

pub mod packet;

mod encryption;
pub mod input_batcher;
pub mod peer;

#[cfg(test)]
mod test;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L39-L44
const BATCH_INTERVAL_MS: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    pub encryption: Option<(ControlEncryptionMethod, AesKey)>,
    pub apollo_permissions: Option<ApolloPermissions>,
}

#[derive(Debug)]
pub enum ControlStreamEvent {
    /// The [ControlStream] has successfully connected to the server:
    /// - Packets can now be sent
    Connect,
    /// The [ControlStream] received a packet.
    Packet(ControlPacket),
    /// The [ControlStream] got disconnected
    Disconnect,
}

/// This is the high level client control stream.
///
/// It does:
/// - automatic input batching using [Self::batch_input] and sending in a predefined interval
/// - regular periodic ping to avoid disconnects
pub struct ControlStream {
    server_version: ServerVersion,
    apollo_permissions: Option<ApolloPermissions>,
    addr: SocketAddr,
    peer: ControlPeerId,
    peer_connected: bool,
    last_now: Instant,
    last_ping: Option<Instant>,
    buffered_packets: Vec<ControlPacket>,
    last_batch_send: Instant,
    batcher: InputBatcher,
    host: ControlHost,
    events: VecDeque<ControlStreamEvent>,
}

impl ControlStream {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(
        now: Instant,
        config: ControlStreamConfig,
        crypto_backend: DynCryptoBackend,
    ) -> Result<Self, ControlError> {
        if config.server_version.major < 5 {
            // Servers below v5 use tcp and don't have encryption support
            // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L849-L856
            return Err(ControlError::VersionNotSupported(config.server_version));
        }

        // All values that could lead to an error are controlled by us and won't cause errors
        // -> This cannot fail
        #[allow(clippy::unwrap_used)]
        let mut host = ControlHost::new(
            now,
            ControlHostConfig {
                peer_count: 1,
                peer_channel_count: EnetChannel::CHANNEL_COUNT,
            },
            crypto_backend,
        )
        .unwrap();

        let packets = ControlPacketConfig::new(config.server_version, config.encryption.is_some())
            .ok_or(ControlError::VersionNotSupported(config.server_version))?;

        // All values that could lead to an error are controlled by us and won't cause errors
        // -> This cannot fail
        #[allow(clippy::unwrap_used)]
        let peer = host
            .connect(
                config.addr,
                ControlConnectConfig {
                    channel_count: EnetChannel::CHANNEL_COUNT,
                    sunshine_connect_data: config.sunshine_connect_data,
                    config: ControlPeerConfig {
                        role: ControlPeerRole::Client,
                        encryption: config.encryption,
                        packets,
                    },
                },
            )
            .unwrap();

        Ok(Self {
            server_version: config.server_version,
            apollo_permissions: config.apollo_permissions,
            addr: config.addr,
            peer,
            peer_connected: false,
            last_now: now,
            last_ping: (config.server_version >= PERIODIC_PING_VERSION).then_some(now),
            buffered_packets: vec![],
            last_batch_send: now,
            batcher: InputBatcher::default(),
            host,
            events: Default::default(),
        })
    }

    pub fn estimated_rtt(&self) -> Result<EstimatedRttInfo, ControlError> {
        self.host
            .peer_estimated_rtt(self.peer)
            .ok_or(ControlError::NotConnected)
    }

    /// This will intelligently batch or instantly send the input based on if it makes sense to do so.
    pub fn batch_input(&mut self, input: ClientInputEvent) -> Result<(), ControlError> {
        trace!(input = ?input, "batching input for control stream");

        self.check_input_supported(&input)?;

        for packet in self.batcher.batch_input(input) {
            self.send_raw(packet)?;
        }

        Ok(())
    }

    /// Will send all batched inputs now.
    ///
    /// This is automatically done if you're following the default event loop of this struct.
    pub fn send_batched_inputs_now(&mut self) -> Result<(), ControlError> {
        self.last_batch_send = self.last_now;

        for packet in self.batcher.remove_batched_inputs() {
            self.send_raw(packet)?;
        }

        Ok(())
    }

    fn check_input_supported(&self, input: &ClientInputEvent) -> Result<(), ControlError> {
        if let Some(permissions) = &self.apollo_permissions {
            match input {
                ClientInputEvent::ControllerConnect { .. }
                | ClientInputEvent::ControllerDisconnect { .. }
                | ClientInputEvent::ControllerState { .. } => {
                    if !permissions.contains(ApolloPermissions::INPUT_CONTROLLER) {
                        return Err(ControlError::ApolloPermissionDenied);
                    }
                }
                ClientInputEvent::Keyboard { .. } => {
                    if !permissions.contains(ApolloPermissions::INPUT_KEYBOARD) {
                        return Err(ControlError::ApolloPermissionDenied);
                    }
                }
                ClientInputEvent::MouseButton { .. }
                | ClientInputEvent::MouseMoveAbsolute { .. }
                | ClientInputEvent::MouseMoveRelative { .. }
                | ClientInputEvent::MouseScrollHorizontal { .. }
                | ClientInputEvent::MouseScrollVertical { .. } => {
                    if !permissions.contains(ApolloPermissions::INPUT_MOUSE) {
                        return Err(ControlError::ApolloPermissionDenied);
                    }
                }
                ClientInputEvent::Touch { .. } => {
                    if !permissions.contains(ApolloPermissions::INPUT_TOUCH) {
                        return Err(ControlError::ApolloPermissionDenied);
                    }
                }
                ClientInputEvent::Pen { .. } => {
                    if !permissions.contains(ApolloPermissions::INPUT_PEN) {
                        return Err(ControlError::ApolloPermissionDenied);
                    }
                }
            }
        }

        match input {
            ClientInputEvent::MouseScrollHorizontal { .. }
                if !self.server_version.is_sunshine_like() =>
            {
                Err(ControlError::PacketNotSupported(ControlPacketNotSupported))
            }
            _ => Ok(()),
        }
    }

    pub fn disconnect(&mut self, disconnect_data: u32) -> Result<(), ControlError> {
        self.host.disconnect(self.peer, disconnect_data)?;

        Ok(())
    }
    pub fn can_discard(&mut self) -> bool {
        self.host.can_discard()
    }

    pub fn send_raw(&mut self, packet: ControlPacket) -> Result<(), ControlError> {
        self.send_inner(packet, false)
    }
    pub(crate) fn send_inner(
        &mut self,
        packet: ControlPacket,
        force_packet: bool,
    ) -> Result<(), ControlError> {
        if force_packet && !self.peer_connected {
            trace!(force_packet = force_packet, packet = ?packet, "buffering forced packet");

            self.buffered_packets.push(packet);
            return Ok(());
        }

        let (channel, kind) = packet.channel(self.server_version);

        self.host.send(self.peer, channel, kind, packet)?;

        Ok(())
    }

    fn do_batching(&mut self) -> Result<(), ControlError> {
        if !self.batcher.is_dirty() {
            return Ok(());
        }

        if self.last_batch_send + BATCH_INTERVAL_MS <= self.last_now {
            self.send_batched_inputs_now()?;
        }

        Ok(())
    }

    /// Returns the time when the next ping must be sent
    fn do_ping(&mut self) -> Result<(), ControlError> {
        // If this server doesn't support the periodic ping
        let Some(last_ping) = self.last_ping else {
            trace!("server doesn't support periodic ping, not sending periodic ping");
            return Ok(());
        };

        if self.last_now >= last_ping + PERIODIC_PING_INTERVAL {
            match self.send_raw(ControlPacket::PeriodicPing) {
                Ok(()) => {}
                Err(ControlError::Enet(EnetError::PeerSendError(PeerSendError::NotConnected)))
                | Err(ControlError::NotConnected) => {
                    trace!(
                        self = ?self,
                        "not sending periodic ping because the control stream (via enet) is not connected yet."
                    );
                    // We are not connected yet -> we cannot send a ping
                    return Ok(());
                }
                Err(err) => return Err(err),
            }

            trace!(
                last_ping = ?last_ping,
                now = ?self.last_now,
                "sending periodic ping"
            );

            self.last_ping = Some(self.last_now);
        }

        Ok(())
    }

    fn do_update(&mut self, now: Instant) -> Result<(), ControlError> {
        self.last_now = now;

        if self.peer_connected {
            debug_assert_eq!(self.buffered_packets.len(), 0);
        }
        if self.host.can_discard() {
            trace!("erroring with NotConnected because the host can be discarded");
            // This only happens when there's no peer in the connection
            // -> we must've disconnected somehow -> this object is not useable anymore
            return Err(ControlError::NotConnected);
        }

        // Handle events
        while let Some(event) = self.host.poll_event() {
            trace!(event = ?event, "control host event");

            let event = match event {
                ControlHostEvent::Connected {
                    id,
                    sunshine_connect_data: _,
                } => {
                    if id != self.peer {
                        debug!("unknown peer connected to control stream: disconnecting them");

                        // Nobody should connect to this peer, but if they do just instantly disconnect them
                        let _ = self.host.disconnect_now(id, 0);
                        continue;
                    }

                    self.peer_connected = true;

                    // Send all buffered packets
                    for packet in mem::take(&mut self.buffered_packets) {
                        self.send_raw(packet)?;
                    }

                    info!("connected control stream");

                    ControlStreamEvent::Connect
                }
                ControlHostEvent::Receive {
                    id,
                    channel_id: _,
                    packet,
                } => {
                    if id != self.peer {
                        // ignore other peers
                        continue;
                    }

                    #[allow(clippy::single_match)]
                    match &packet {
                        ControlPacket::ServerTermination { .. } => {
                            // Disconnect instanly:
                            // We used to wait for a ENET_EVENT_TYPE_DISCONNECT event, but since
                            // GFE 3.20.3.63 we don't get one for 10 seconds after we first get
                            // this termination message. The termination message should be reliable
                            // enough to end the stream now, rather than waiting for an explicit
                            // disconnect. The server will also not acknowledge our disconnect
                            // message once it sends this message, so we mark the peer as fully
                            // disconnected now to avoid delays waiting for an ack that will
                            // never arrive.
                            // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1362-L1375
                            self.host.disconnect_now(self.peer, 0)?;

                            // The enet disconnect event will be called next poll
                        }
                        _ => {}
                    }

                    ControlStreamEvent::Packet(packet)
                }
                ControlHostEvent::Disconnected { id } => {
                    if id != self.peer {
                        // ignore other peers
                        continue;
                    }

                    self.peer_connected = false;

                    info!("disconnected control stream");

                    ControlStreamEvent::Disconnect
                }
            };

            trace!("pushing event");
            self.events.push_back(event);
        }

        // Handle batching
        self.do_batching()?;

        // Handle periodic ping
        self.do_ping()?;

        Ok(())
    }
}

impl Drop for ControlStream {
    fn drop(&mut self) {
        info!("terminated control stream");
    }
}

impl Debug for ControlStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[ControlStream]")
    }
}

impl UdpStream for ControlStream {
    type Error = ControlError;

    type Event = ControlStreamEvent;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
        self.host.pending_send()
    }

    fn consume_send(&mut self) {
        self.host.consume_send();
    }

    fn poll_timeout(&self) -> Option<Instant> {
        let batch_timeout = if self.batcher.is_dirty() {
            Some(self.last_batch_send + BATCH_INTERVAL_MS)
        } else {
            None
        };

        let ping_timeout = self.last_ping.unwrap_or(self.last_now) + PERIODIC_PING_INTERVAL;

        let host_timeout = self.host.poll_timeout();

        soonest(soonest(batch_timeout, ping_timeout), host_timeout)
    }

    fn poll_event(&mut self) -> Option<Self::Event> {
        self.events.pop_front()
    }

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        if self.addr != addr {
            trace!(stream_addr = %self.addr, recv_addr = %addr, "received packet from non stream address");
            return Ok(());
        }

        self.host.handle_receive(now, addr, data)?;

        self.do_update(now)?;

        Ok(())
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error> {
        self.host.handle_timeout(now)?;

        self.do_update(now)?;

        Ok(())
    }
}
