use std::collections::HashSet;

use smallvec::SmallVec;
use tracing::{debug, trace, warn};

use crate::stream::{
    bindings::{LI_ROT_UNKNOWN, LI_TILT_UNKNOWN},
    control::{
        ActiveGamepads, ControllerButtons, ControllerCapabilities, ControllerType, KeyAction,
        KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction, PenButtons, ToolType,
        TouchEventType,
    },
    proto::control::packet::ControlPacket,
};

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
    // TODO: batch touch and pen events?
    /// Sunshine Extension
    Touch {
        event_type: TouchEventType,
        rotation: Option<u16>,
        pointer_id: u32,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
    Pen {
        event_type: TouchEventType,
        tool_type: ToolType,
        buttons: PenButtons,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        rotation: Option<u16>,
        tilt: Option<u8>,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
}

#[derive(Debug, Default)]
pub struct InputBatcher {
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

impl InputBatcher {
    pub fn batch_input(
        &mut self,
        input: ClientInputEvent,
    ) -> impl Iterator<Item = ControlPacket> + 'static {
        trace!(input = ?input, "batching input for control stream");

        let mut dispatch_now = Default::default();

        match input {
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                self.mouse_delta_x = self.mouse_delta_x.saturating_add(delta_x);
                self.mouse_delta_y = self.mouse_delta_y.saturating_add(delta_y);
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => {
                // See the send batch now function
                debug_assert_ne!(
                    reference_width, 0,
                    "non null values as reference size will have weird results"
                );
                debug_assert_ne!(
                    reference_height, 0,
                    "non null values as reference size will have weird results"
                );

                self.mouse_absolute_x = x;
                self.mouse_absolute_y = y;
                self.mouse_absolute_reference_width = reference_width;
                self.mouse_absolute_reference_height = reference_height;
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                self.mouse_scroll_y = self.mouse_scroll_y.saturating_add(scroll_y);
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                self.mouse_scroll_x = self.mouse_scroll_x.saturating_add(scroll_x);
            }
            input => dispatch_now = self.convert_input(input),
        };

        dispatch_now.into_iter()
    }
    fn convert_input(&mut self, input: ClientInputEvent) -> Option<ControlPacket> {
        let mut packet = None;

        match input {
            ClientInputEvent::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
            } => {
                let is_pressed = matches!(action, KeyAction::Down);
                let was_pressed = self.pressed_keys.contains(&key_code);

                // wolf hates it when you send multiple key press / key release events because some keys can get stuck
                // -> only send on changes
                if is_pressed != was_pressed {
                    packet = Some(ControlPacket::Keyboard {
                        action,
                        flags,
                        key_code,
                        modifiers,
                        zero: 0,
                    });

                    // update map
                    if is_pressed {
                        self.pressed_keys.remove(&key_code);
                    } else {
                        self.pressed_keys.insert(key_code);
                    }
                }

                debug!(is_pressed = is_pressed, was_pressed = was_pressed, key_code = ?key_code, modifiers = ?modifiers, "dropping key packet because the key is already in that state");
            }
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                packet = Some(ControlPacket::MouseMoveRelative { delta_x, delta_y });
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => {
                packet = Some(ControlPacket::MouseMoveAbsolute {
                    x,
                    y,
                    unused: 0,
                    reference_width,
                    reference_height,
                });
            }
            ClientInputEvent::MouseButton { action, button } => {
                packet = Some(ControlPacket::MouseButton { action, button });
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                packet = Some(ControlPacket::MouseScroll {
                    scroll_amount_1: scroll_y,
                    scroll_amount_2: scroll_y,
                    zero: 0,
                });
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                // we already checked if this is allowed

                packet = Some(ControlPacket::MouseHorizontalScroll {
                    scroll_amount: scroll_x,
                });
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
                    return Default::default();
                };

                if self.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "received controller connect event for a controller that was already connected! dropping the packet."
                    );
                    return Default::default();
                }

                // add to gamepads
                self.gamepads |= controller;

                packet = Some(ControlPacket::ControllerArrival {
                    controller_number,
                    ty,
                    capabilities,
                    supported_buttons,
                });
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
                    return Default::default();
                };

                if !self.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "cannot send state for a non connected controller!"
                    );
                    return Default::default();
                }

                packet = Some(ControlPacket::controller_state(
                    self.gamepads,
                    controller_number as i16,
                    pressed_buttons,
                    left_trigger,
                    right_trigger,
                    left_stick_x,
                    left_stick_y,
                    right_stick_x,
                    right_stick_y,
                ));
            }
            ClientInputEvent::ControllerDisconnect { controller_number } => {
                let Some(controller) = ActiveGamepads::from_id(controller_number) else {
                    warn!(
                        controller_number = controller_number,
                        "received a controller event for a controller that is out of range (controller_number too high)! dropping the packet."
                    );
                    return Default::default();
                };

                if self.gamepads.contains(controller) {
                    warn!(
                        controller_number = controller_number,
                        "received controller disconnect event for a controller that was not connected! dropping the packet."
                    );
                    return Default::default();
                }

                self.gamepads.remove(controller);

                // sending an empty event with the controller not in the mask will disconnect the controller
                packet = Some(ControlPacket::controller_state(
                    self.gamepads,
                    controller_number as i16,
                    ControllerButtons::empty(),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ));
            }
            ClientInputEvent::Touch {
                event_type,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            } => {
                // TODO: some touch events are batched
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1326-L1371

                packet = Some(ControlPacket::Touch {
                    event_type,
                    reserved: 0,
                    rotation: rotation.unwrap_or(LI_ROT_UNKNOWN as u16),
                    pointer_id,
                    x,
                    y,
                    pressure_or_distance,
                    contact_area_minor,
                    contact_area_major,
                });
            }
            ClientInputEvent::Pen {
                event_type,
                tool_type,
                buttons,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                contact_area_minor,
                contact_area_major,
            } => {
                // TODO: some pen events are batched
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1326-L1371

                packet = Some(ControlPacket::Pen {
                    event_type,
                    tool_type,
                    buttons,
                    zero: 0,
                    x,
                    y,
                    pressure_or_distance,
                    rotation: rotation.unwrap_or(LI_ROT_UNKNOWN as u16),
                    tilt: tilt.unwrap_or(LI_TILT_UNKNOWN as u8),
                    zero2: 0,
                    contact_area_minor,
                    contact_area_major,
                });
            }
        };

        packet
    }

    pub fn is_dirty(&self) -> bool {
        let mut is_dirty = false;

        // mouse relative
        if self.mouse_delta_x != 0 || self.mouse_delta_y != 0 {
            is_dirty = true;
        }
        // mouse absolute
        if self.mouse_absolute_reference_width != 0 || self.mouse_absolute_reference_height != 0 {
            is_dirty = true;
        }
        // mouse scroll
        if self.mouse_scroll_x != 0 || self.mouse_scroll_y != 0 {
            is_dirty = true;
        }

        is_dirty
    }

    pub fn remove_batched_inputs(&mut self) -> impl Iterator<Item = ControlPacket> + 'static {
        let mut packets = SmallVec::<[ControlPacket; 4]>::new();

        // mouse relative
        if self.mouse_delta_x != 0 || self.mouse_delta_y != 0 {
            packets.push(ControlPacket::MouseMoveRelative {
                delta_x: self.mouse_delta_x,
                delta_y: self.mouse_delta_y,
            });

            self.mouse_delta_x = 0;
            self.mouse_delta_y = 0;
        }

        // mouse absolute
        if self.mouse_absolute_reference_width != 0 || self.mouse_absolute_reference_height != 0 {
            packets.push(ControlPacket::MouseMoveAbsolute {
                x: self.mouse_absolute_x,
                y: self.mouse_absolute_y,
                unused: 0,
                reference_width: self.mouse_absolute_reference_width,
                reference_height: self.mouse_absolute_reference_height,
            });
        }

        // mouse scroll
        if self.mouse_scroll_x != 0 {
            packets.push(ControlPacket::MouseScroll {
                scroll_amount_1: self.mouse_scroll_x,
                scroll_amount_2: 0,
                zero: 0,
            });

            self.mouse_scroll_x = 0;
        }
        if self.mouse_scroll_y != 0 {
            packets.push(ControlPacket::MouseHorizontalScroll {
                scroll_amount: self.mouse_scroll_y,
            });

            self.mouse_scroll_y = 0;
        }

        packets.into_iter()
    }
}
