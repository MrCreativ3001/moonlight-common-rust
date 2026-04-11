use std::{
    collections::HashSet,
    fmt::{self, Debug, Formatter},
    mem,
    net::SocketAddr,
    time::{Duration, Instant},
};

use rusty_enet::{PacketKind, error::PeerSendError};
use tracing::{Level, debug, info, instrument, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        AesKey,
        control::{
            ActiveGamepads, ControllerButtons, ControllerCapabilities, ControllerType, KeyAction,
            KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction,
        },
        proto::{
            control::{
                packet::{
                    ControlPacket, ControlPacketConfig, ControlPacketNotSupported,
                    PERIODIC_PING_INTERVAL, PERIODIC_PING_VERSION,
                },
                peer::{
                    ControlConnectConfig, ControlEncryptionMethod, ControlError, ControlHost,
                    ControlHostAction, ControlHostConfig, ControlHostEvent, ControlHostInput,
                    ControlHostOutput, ControlPeerConfig, ControlPeerId, ControlPeerRole,
                },
            },
            crypto::CryptoBackend,
            enet::EnetError,
        },
    },
};

pub mod packet;

mod encryption;
pub mod peer;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test;

// TODO: where's the difference between v1 and v2 headers?

const CHANNEL_GENERIC: u8 = 0x00;
const CHANNEL_URGENT: u8 = 0x01; // IDR and reference frame invalidation requests
const CHANNEL_KEYBOARD: u8 = 0x02;
const CHANNEL_MOUSE: u8 = 0x03;
const CHANNEL_PEN: u8 = 0x04;
const CHANNEL_TOUCH: u8 = 0x05;
const CHANNEL_UTF8: u8 = 0x06;
const CHANNEL_GAMEPAD_BASE: u8 = 0x10; // 0x10 to 0x1F by controller index
const CHANNEL_SENSOR_BASE: u8 = 0x20; // 0x20 to 0x2F by controller index
const CHANNEL_COUNT: usize = 0x30;

/// A message from the [MoonlightStreamProto](super::MoonlightStreamProto) to the [ControlStream]
#[derive(Debug)]
pub struct ControlMessage(pub(super) ControlMessageInner);
#[derive(Debug)]
pub(super) enum ControlMessageInner {
    /// Sends a packet regardless of the [Self::AllowOtherPackets] option
    SendPacket { packet: ControlPacket, force: bool },
}

#[derive(Debug, Clone)]
pub enum ClientInputEvent {
    Keyboard {
        action: KeyAction,
        flags: KeyFlags,
        key_code: KeyCode,
        modifiers: KeyModifiers,
    },
    MouseMoveRelative {
        delta_x: i16,
        delta_y: i16,
    },
    MouseMoveAbsolute {
        x: i16,
        y: i16,
        reference_width: i16,
        reference_height: i16,
    },
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
    },
    MouseScrollVertical {
        /// This value might be clamped to [LI_WHEEL_DELTA]
        scroll_y: i16,
    },
    /// Sunshine extension
    MouseScrollHorizontal {
        scroll_x: i16,
    },
    ControllerConnect {
        controller_number: u8,
        ty: ControllerType,
        capabilities: ControllerCapabilities,
        supported_buttons: ControllerButtons,
    },
    ControllerState {
        controller_number: u8,
        pressed_buttons: ControllerButtons,
        left_trigger: f32,
        right_trigger: f32,
        left_stick_x: f32,
        left_stick_y: f32,
        right_stick_x: f32,
        right_stick_y: f32,
    },
    ControllerDisconnect {
        controller_number: u8,
    },
    // TODO: touch events?
    // TODO: pen events?
}

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L39-L44
const BATCH_INTERVAL_MS: Duration = Duration::from_millis(1);

/// Some data about the input
#[derive(Debug)]
struct Inputs {
    last_send: Instant,
    dirty: bool,
    // pressed keys
    pressed_keys: HashSet<KeyCode>,
    // mouse move relative
    mouse_delta_x: i16,
    mouse_delta_y: i16,
    // mouse move absolute
    mouse_absolute_x: i16,
    mouse_absolute_y: i16,
    mouse_absolute_reference_width: i16,
    mouse_absolute_reference_height: i16,
    // mouse scroll
    mouse_scroll_x: i16,
    mouse_scroll_y: i16,
    // connected controllers
    gamepads: ActiveGamepads,
}

#[derive(Debug)]
pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    pub encryption: Option<(ControlEncryptionMethod, AesKey)>,
}

#[derive(Debug)]
pub enum ControlStreamInput<'a> {
    Receive {
        now: Instant,
        addr: SocketAddr,
        data: &'a [u8],
    },
    Timeout(Instant),
    /// A message received from the main [MoonlightStreamProto](super::MoonlightStreamProto) or the [VideoStream](super::video::VideoStream)
    Message {
        now: Instant,
        message: ControlMessage,
    },
}

#[derive(Debug)]
pub enum ControlStreamOutput {
    Action(ControlHostAction),
    Event(ControlStreamEvent),
}

#[derive(Debug)]
pub enum ControlStreamEvent {
    /// The control has successfully connected to the server:
    /// - Packets can now be sent
    Connect,
    /// The [ControlStream] received a packet.
    Packet(ControlPacket),
    /// The control stream got disconnected
    Disconnect,
}

/// This is the high level client control stream.
///
/// It does:
/// - automatic input batching using [Self::batch_input] and sending in a predefined interval
/// - regular periodic ping to avoid disconnects
///
/// When used in combination with the [MoonlightStreamProto](super::MoonlightStreamProto) and the [VideoStream](super::video::VideoStream) it handles:
/// - automatic idr requests
/// - sending a disconnect packet on stop
///
/// If it's not used with that it won't do these things, however you can manually do them using the [Self::send_raw] function.
pub struct ControlStream<Crypto> {
    server_version: ServerVersion,
    peer: ControlPeerId,
    peer_connected: bool,
    last_now: Instant,
    last_ping: Option<Instant>,
    allow_packets: bool,
    buffered_packets: Vec<ControlPacket>,
    inputs: Inputs,
    host: ControlHost<Crypto>,
}

impl<Crypto> ControlStream<Crypto>
where
    Crypto: CryptoBackend,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(
        now: Instant,
        config: ControlStreamConfig,
        crypto_backend: Crypto,
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
                peer_channel_count: CHANNEL_COUNT,
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
                    channel_count: CHANNEL_COUNT,
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
            peer,
            peer_connected: false,
            allow_packets: false,
            last_now: now,
            last_ping: (config.server_version >= PERIODIC_PING_VERSION).then_some(now),
            buffered_packets: vec![],
            inputs: Inputs {
                last_send: now,
                dirty: false,
                pressed_keys: Default::default(),
                mouse_delta_x: 0,
                mouse_delta_y: 0,
                mouse_absolute_x: 0,
                mouse_absolute_y: 0,
                mouse_absolute_reference_width: 0,
                mouse_absolute_reference_height: 0,
                mouse_scroll_x: 0,
                mouse_scroll_y: 0,
                gamepads: ActiveGamepads::empty(),
            },
            host,
        })
    }

    /// This will intelligently batch or instantly send the input based on if it makes sense to do so.
    pub fn batch_input(&mut self, input: ClientInputEvent) -> Result<(), ControlError> {
        trace!(input = ?input, "batching input for control stream");

        self.check_input_supported(&input)?;

        match input {
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                self.inputs.dirty = true;

                self.inputs.mouse_delta_x = self.inputs.mouse_delta_x.saturating_add(delta_x);
                self.inputs.mouse_delta_y = self.inputs.mouse_delta_y.saturating_add(delta_y);
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => {
                self.inputs.dirty = true;

                // See the send batch now function
                debug_assert_ne!(
                    reference_width, 0,
                    "non null values as reference size will have weird results"
                );
                debug_assert_ne!(
                    reference_height, 0,
                    "non null values as reference size will have weird results"
                );

                self.inputs.mouse_absolute_x = x;
                self.inputs.mouse_absolute_y = y;
                self.inputs.mouse_absolute_reference_width = reference_width;
                self.inputs.mouse_absolute_reference_height = reference_height;
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                self.inputs.dirty = true;

                self.inputs.mouse_scroll_y = self.inputs.mouse_scroll_y.saturating_add(scroll_y);
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                self.inputs.dirty = true;

                self.inputs.mouse_scroll_x = self.inputs.mouse_scroll_x.saturating_add(scroll_x);
            }
            input => {
                self.send_input_now(input)?;
            }
        }

        Ok(())
    }

    /// Will send all batched inputs now.
    ///
    /// This is automatically done if you're following the default event loop of this struct.
    pub fn send_batched_inputs_now(&mut self) -> Result<(), ControlError> {
        trace!(batched_inputs = ?self.inputs, "sending batched inputs");

        self.inputs.last_send = self.last_now;

        if !self.inputs.dirty {
            // early return
            return Ok(());
        }
        self.inputs.dirty = false;

        // mouse relative
        if self.inputs.mouse_delta_x != 0 || self.inputs.mouse_delta_y != 0 {
            self.send_input_now(ClientInputEvent::MouseMoveRelative {
                delta_x: self.inputs.mouse_delta_x,
                delta_y: self.inputs.mouse_delta_y,
            })?;

            self.inputs.mouse_delta_x = 0;
            self.inputs.mouse_delta_y = 0;
        }

        // mouse absolute
        if self.inputs.mouse_absolute_reference_width != 0
            || self.inputs.mouse_absolute_reference_height != 0
        {
            self.send_input_now(ClientInputEvent::MouseMoveAbsolute {
                x: self.inputs.mouse_absolute_x,
                y: self.inputs.mouse_absolute_y,
                reference_width: self.inputs.mouse_absolute_reference_width,
                reference_height: self.inputs.mouse_absolute_reference_height,
            })?;
        }

        // mouse scroll
        if self.inputs.mouse_scroll_x != 0 {
            self.send_input_now(ClientInputEvent::MouseScrollHorizontal {
                scroll_x: self.inputs.mouse_scroll_x,
            })?;

            self.inputs.mouse_scroll_x = 0;
        }
        if self.inputs.mouse_scroll_y != 0 {
            self.send_input_now(ClientInputEvent::MouseScrollVertical {
                scroll_y: self.inputs.mouse_scroll_y,
            })?;

            self.inputs.mouse_scroll_y = 0;
        }

        Ok(())
    }

    fn check_input_supported(&self, input: &ClientInputEvent) -> Result<(), ControlError> {
        // TODO: add other things: e.g. controller stuff and so on

        match input {
            ClientInputEvent::MouseScrollVertical { .. }
                if !self.server_version.is_sunshine_like() =>
            {
                Err(ControlError::PacketNotSupported(ControlPacketNotSupported))
            }
            _ => Ok(()),
        }
    }

    /// You should prefer to call [Self::batch_input] to avoid spamming the server.
    pub fn send_input_now(&mut self, input: ClientInputEvent) -> Result<(), ControlError> {
        self.check_input_supported(&input)?;

        match input {
            ClientInputEvent::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
            } => {
                let is_pressed = matches!(action, KeyAction::Down);
                let was_pressed = self.inputs.pressed_keys.contains(&key_code);

                // wolf hates it when you send multiple key press / key release events because some keys can get stuck
                // -> only send on changes
                if is_pressed != was_pressed {
                    self.send_raw(ControlPacket::Keyboard {
                        action,
                        flags,
                        key_code,
                        modifiers,
                        zero: 0,
                    })?;

                    // update map
                    if is_pressed {
                        self.inputs.pressed_keys.remove(&key_code);
                    } else {
                        self.inputs.pressed_keys.insert(key_code);
                    }
                }

                debug!(is_pressed = is_pressed, was_pressed = was_pressed, key_code = ?key_code, modifiers = ?modifiers, "dropping key packet because the key is already in that state");
            }
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                self.send_raw(ControlPacket::MouseMoveRelative { delta_x, delta_y })?;
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => {
                self.send_raw(ControlPacket::MouseMoveAbsolute {
                    x,
                    y,
                    unused: 0,
                    reference_width,
                    reference_height,
                })?;
            }
            ClientInputEvent::MouseButton { action, button } => {
                self.send_raw(ControlPacket::MouseButton { action, button })?;
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                self.send_raw(ControlPacket::MouseScroll {
                    scroll_amount_1: scroll_y,
                    scroll_amount_2: scroll_y,
                    zero: 0,
                })?;
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                // we already checked if this is allowed

                self.send_raw(ControlPacket::MouseHorizontalScroll {
                    scroll_amount: scroll_x,
                })?;
            }
            ClientInputEvent::ControllerConnect {
                controller_number,
                ty,
                capabilities,
                supported_buttons,
            } => {
                let Some(controller) = ActiveGamepads::from_id(controller_number) else {
                    warn!(
                        controller_number = controller_number,
                        "received a controller event for a controller that is out of range (controller_number too high)! dropping the packet."
                    );
                    return Ok(());
                };

                if self.inputs.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "received controller connect event for a controller that was already connected! dropping the packet."
                    );
                    return Ok(());
                }

                // add to gamepads
                self.inputs.gamepads |= controller;

                self.send_raw(ControlPacket::ControllerArrival {
                    controller_number,
                    ty,
                    capabilities,
                    supported_buttons,
                })?;
            }
            ClientInputEvent::ControllerState {
                controller_number,
                pressed_buttons,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            } => {
                let Some(controller) = ActiveGamepads::from_id(controller_number) else {
                    warn!(
                        controller_number = controller_number,
                        "received a controller event for a controller that is out of range (controller_number too high)! dropping the packet."
                    );
                    return Ok(());
                };

                if !self.inputs.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "cannot send state for a non connected controller!"
                    );
                    return Ok(());
                }

                self.send_raw(ControlPacket::controller_state(
                    self.inputs.gamepads,
                    controller_number as i16,
                    pressed_buttons,
                    left_trigger,
                    right_trigger,
                    left_stick_x,
                    left_stick_y,
                    right_stick_x,
                    right_stick_y,
                ))?;
            }
            ClientInputEvent::ControllerDisconnect { controller_number } => {
                let Some(controller) = ActiveGamepads::from_id(controller_number) else {
                    warn!(
                        controller_number = controller_number,
                        "received a controller event for a controller that is out of range (controller_number too high)! dropping the packet."
                    );
                    return Ok(());
                };

                if self.inputs.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "received controller disconnect event for a controller that was not connected! dropping the packet."
                    );
                    return Ok(());
                }

                self.inputs.gamepads.remove(controller);

                // sending an empty event with the controller not in the mask will disconnect the controller
                self.send_raw(ControlPacket::controller_state(
                    self.inputs.gamepads,
                    controller_number as i16,
                    ControllerButtons::empty(),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ))?;
            }
        }

        Ok(())
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
        // TODO: we should only allow RequestIdr and StartB for starting in that order
        // this current approach is more hacky
        if matches!(packet, ControlPacket::StartB) {
            self.allow_packets = true;
        }

        if !force_packet && !self.allow_packets {
            return Err(ControlError::NotConnected);
        } else if force_packet && !self.peer_connected {
            trace!(force_packet = force_packet, packet = ?packet, "buffering forced packet");

            self.buffered_packets.push(packet);
            return Ok(());
        }

        // TODO: other packets?
        let (channel, kind) = if self.server_version.is_sunshine_like() {
            match packet {
                // request idr: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1522-L1528
                // ltr ack: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1569-L1575
                // invalidate ref frames: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1509-L1515
                ControlPacket::RequestIdr
                | ControlPacket::StartB
                | ControlPacket::LongTermReferenceFrameAcknowledgement { .. } => {
                    (CHANNEL_URGENT, PacketKind::Reliable)
                }
                // loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1469-L1475
                // frame fec: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1407-L1413
                ControlPacket::LossStats { .. } | ControlPacket::FrameFec { .. } => {
                    (CHANNEL_GENERIC, PacketKind::Unreliable { sequenced: false })
                }
                // See: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1424-L1429
                // Send the message (and don't expect a response)
                //
                // NB: We send this periodic message as reliable to ensure the RTT is recomputed
                // regularly. This only happens when an ACK is received to a reliable packet.
                // Since the other traffic on this channel is unsequenced, it doesn't really
                // cause any negative HOL blocking side-effects.
                ControlPacket::PeriodicPing => (CHANNEL_GENERIC, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L738-L742
                ControlPacket::MouseMoveRelative { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L803-L806
                ControlPacket::MouseMoveAbsolute { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L865-L866
                ControlPacket::MouseButton { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L899-L900
                ControlPacket::Keyboard { .. } => (CHANNEL_KEYBOARD, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L980-L981
                ControlPacket::Text { .. } => (CHANNEL_UTF8, PacketKind::Reliable),
                _ => todo!("{:?}", packet),
            }
        } else {
            // https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L763-L767
            // Always use channel 0 and reliable for GFE
            (0, PacketKind::Reliable)
        };

        self.host.send(self.peer, channel, kind, packet)?;

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<ControlStreamOutput, ControlError> {
        if self.peer_connected {
            debug_assert_eq!(self.buffered_packets.len(), 0);
        }
        if self.host.can_discard() {
            trace!("erroring with NotConnected because the host can be discarded");
            // This only happens when there's no peer in the connection
            // -> we must've disconnected somehow -> this object is not useable anymore
            return Err(ControlError::NotConnected);
        }

        let mut timeout = loop {
            let output = self.host.poll_output()?;

            match output {
                ControlHostOutput::Action(ControlHostAction::Timeout(timeout)) => {
                    break timeout;
                }
                ControlHostOutput::Action(action) => {
                    return Ok(ControlStreamOutput::Action(action));
                }
                ControlHostOutput::Event(ControlHostEvent::Connected {
                    id,
                    sunshine_connect_data: _,
                }) => {
                    if id != self.peer {
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

                    return Ok(ControlStreamOutput::Event(ControlStreamEvent::Connect));
                }
                ControlHostOutput::Event(ControlHostEvent::Receive {
                    id,
                    // TODO: is channel_id important?
                    channel_id: _,
                    packet,
                }) => {
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

                    return Ok(ControlStreamOutput::Event(ControlStreamEvent::Packet(
                        packet,
                    )));
                }
                ControlHostOutput::Event(ControlHostEvent::Disconnected { id }) => {
                    if id != self.peer {
                        // ignore other peers
                        continue;
                    }

                    self.peer_connected = false;

                    info!("disconnected control stream");

                    return Ok(ControlStreamOutput::Event(ControlStreamEvent::Disconnect));
                }
            }
        };

        // Handle batching
        if let Some(new_timeout) = self.do_batching()? {
            timeout = timeout.min(new_timeout);
        }

        // Handle periodic ping
        if let Some(new_timeout) = self.do_ping()? {
            timeout = timeout.min(new_timeout);
        }

        Ok(ControlStreamOutput::Action(ControlHostAction::Timeout(
            timeout,
        )))
    }

    pub fn handle_input(&mut self, input: ControlStreamInput) -> Result<(), ControlError> {
        match input {
            ControlStreamInput::Timeout(now) => {
                self.last_now = now;

                self.host.handle_input(ControlHostInput::Timeout(now))?;
            }
            ControlStreamInput::Receive { now, addr, data } => {
                self.last_now = now;

                self.host
                    .handle_input(ControlHostInput::Receive { now, addr, data })?;
            }
            ControlStreamInput::Message { now, message } => {
                self.last_now = now;

                self.host.handle_input(ControlHostInput::Timeout(now))?;

                self.handle_control_message(message)?;
            }
        }

        Ok(())
    }

    fn handle_control_message(&mut self, message: ControlMessage) -> Result<(), ControlError> {
        match message.0 {
            ControlMessageInner::SendPacket { packet, force } => {
                trace!(now = ?self.last_now, packet = ?packet, "received control message with packet");

                if force {
                    self.send_inner(packet, true)?;
                } else {
                    if let Err(err) = self.send_raw(packet) {
                        trace!(error = ?err, "failed to send packet from control message");
                    }
                }
            }
        }

        Ok(())
    }

    fn do_batching(&mut self) -> Result<Option<Instant>, ControlError> {
        if !self.inputs.dirty {
            return Ok(None);
        }

        if self.inputs.last_send + BATCH_INTERVAL_MS <= self.last_now {
            self.send_batched_inputs_now()?;
        }

        Ok(Some(self.inputs.last_send + BATCH_INTERVAL_MS))
    }

    /// Returns the time when the next ping must be sent
    fn do_ping(&mut self) -> Result<Option<Instant>, ControlError> {
        // If this server doesn't support the periodic ping
        let Some(last_ping) = self.last_ping else {
            trace!("server doesn't support periodic ping, not sending periodic ping");
            return Ok(None);
        };

        if self.last_now >= last_ping + PERIODIC_PING_INTERVAL {
            match self.send_raw(ControlPacket::PeriodicPing) {
                Ok(()) => {}
                Err(ControlError::Enet(EnetError::PeerSendError(PeerSendError::NotConnected)))
                | Err(ControlError::NotConnected) => {
                    debug!(
                        self = ?self,
                        "not sending periodic ping because the control stream (via enet) is not connected yet."
                    );
                    // We are not connected yet -> we cannot send a ping
                    return Ok(None);
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

        Ok(Some(last_ping + PERIODIC_PING_INTERVAL))
    }
}

impl<Crypto> Drop for ControlStream<Crypto> {
    fn drop(&mut self) {
        info!("terminated control stream");
    }
}

impl<Crypto> Debug for ControlStream<Crypto> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[ControlStream]")
    }
}
