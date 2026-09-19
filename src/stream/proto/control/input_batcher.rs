use std::collections::HashSet;

use smallvec::SmallVec;
use tracing::{debug, trace, warn};

use crate::stream::{
    bindings::{LI_ROT_UNKNOWN, LI_TILT_UNKNOWN},
    control::{
        ActiveGamepads, CompactKeyStates, ControllerButtons, ControllerCapabilities,
        ControllerType, KeyAction, KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction,
        PenButtons, ToolType, TouchEventType,
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
    /// Sunshine Extension
    ///
    /// See also:
    /// - <https://github.com/moonlight-stream/moonlight-android/blob/b48494cb96bff23d8886c4775cc4f39a1075495d/app/src/main/java/com/limelight/Game.java#L1746-L1754>
    Touch {
        event_type: TouchEventType,
        /// Rotation is in degrees from vertical in Y dimension (parallel to screen, 0..360). If rotation is
        /// unknown, pass None.
        rotation: Option<u16>,
        /// Pointer ID is an opaque ID that must uniquely identify each active touch on screen. It must
        /// remain constant through any down/up/move/cancel events involved in a single touch interaction.
        pointer_id: u32,
        /// The x and y values are normalized device coordinates stretching top-left corner (0.0, 0.0) to bottom-right corner (1.0, 1.0) of the video area.
        x: f32,
        /// See [x](ClientInputEvent::Touch::x).
        y: f32,
        /// Pressure is a 0.0 to 1.0 range value from min to max pressure. Sending a down/move event with
        /// a pressure of 0.0 indicates the actual pressure is unknown.
        ///
        /// For hover events, the pressure value is treated as a 1.0 to 0.0 range of distance from the touch
        /// surface where 1.0 is the farthest measurable distance and 0.0 is actually touching the display
        /// (which is invalid for a hover event). Reporting distance 0.0 for a hover event indicates the
        /// actual distance is unknown.
        pressure_or_distance: f32,
        /// Contact area is modelled as an ellipse with major and minor axis values in normalized device
        /// coordinates. If contact area is unknown, report 0.0 for both contact area axis parameters.
        /// For circular contact areas or if a minor axis value is not available, pass the same value
        /// for major and minor axes. For APIs or devices, that don't report contact area as an ellipse,
        /// approximations can be used such as: https://docs.kernel.org/input/multi-touch-protocol.html#event-computation
        ///
        /// For hover events, the "contact area" is the size of the hovering finger/tool. If unavailable,
        /// pass 0.0 for both contact area parameters.
        contact_area_minor: f32,
        /// See [contact_area_minor](ClientInputEvent::Touch::contact_area_minor)
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
    key_states: CompactKeyStates,
    key_states_map: HashSet<KeyCode>,
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
    fn set_key_down(&mut self, key_code: KeyCode, action: KeyAction) -> KeyAction {
        if let Some(old_is_down) = self.key_states.set_pressed(key_code, action) {
            old_is_down
        } else {
            // get old key code
            let old = if self.key_states_map.contains(&key_code) {
                KeyAction::Down
            } else {
                KeyAction::Up
            };

            // set key code
            match action {
                KeyAction::Up => self.key_states_map.remove(&key_code),
                KeyAction::Down => self.key_states_map.insert(key_code),
            };

            old
        }
    }

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
                action: is_pressed,
                flags,
                key_code,
                modifiers,
            } => {
                let was_pressed = self.set_key_down(key_code, is_pressed);

                // wolf hates it when you send multiple key press / key release events because some keys can get stuck
                // -> only send on changes
                if is_pressed != was_pressed {
                    packet = Some(ControlPacket::Keyboard {
                        action: is_pressed,
                        flags,
                        key_code,
                        modifiers,
                        zero: 0,
                    });
                } else {
                    debug!(is_pressed = ?is_pressed, was_pressed = ?was_pressed, key_code = ?key_code, modifiers = ?modifiers, "dropping key packet because the key is already in that state");
                }
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

                if !self.gamepads.contains(controller) {
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
        let mut packets = SmallVec::<[ControlPacket; 5]>::new();

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

            self.mouse_absolute_reference_width = 0;
            self.mouse_absolute_reference_height = 0;
        }

        // mouse scroll
        if self.mouse_scroll_x != 0 {
            packets.push(ControlPacket::MouseHorizontalScroll {
                scroll_amount: self.mouse_scroll_x,
            });

            self.mouse_scroll_x = 0;
        }
        if self.mouse_scroll_y != 0 {
            packets.push(ControlPacket::MouseScroll {
                scroll_amount_1: self.mouse_scroll_y,
                scroll_amount_2: 0,
                zero: 0,
            });

            self.mouse_scroll_y = 0;
        }

        packets.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::stream::{
        control::{
            ActiveGamepads, ControllerButtons, ControllerCapabilities, ControllerType, KeyAction,
            KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction,
        },
        proto::control::{
            input_batcher::{ClientInputEvent, InputBatcher},
            packet::ControlPacket,
        },
    };

    #[test]
    fn mouse_relative() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::MouseMoveRelative {
            delta_x: 1,
            delta_y: 2,
        });
        assert_eq!(iter.next(), None);
        assert!(batcher.is_dirty());

        let mut iter = batcher.batch_input(ClientInputEvent::MouseMoveRelative {
            delta_x: 1,
            delta_y: 2,
        });
        assert_eq!(iter.next(), None);
        assert!(batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseMoveRelative {
                delta_x: 2,
                delta_y: 4
            })
        );
        assert_eq!(iter.next(), None);
        assert!(!batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn mouse_absolute() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::MouseMoveAbsolute {
            x: 1,
            y: 2,
            reference_width: 3,
            reference_height: 4,
        });
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::MouseMoveAbsolute {
            x: 5,
            y: 6,
            reference_width: 7,
            reference_height: 8,
        });
        assert_eq!(iter.next(), None);
        assert!(batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseMoveAbsolute {
                x: 5,
                y: 6,
                unused: 0,
                reference_width: 7,
                reference_height: 8,
            })
        );
        assert_eq!(iter.next(), None);
        assert!(!batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn mouse_scroll_x() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::MouseScrollVertical { scroll_y: 1 });
        assert!(batcher.is_dirty());
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::MouseScrollVertical { scroll_y: 1 });
        assert!(batcher.is_dirty());
        assert_eq!(iter.next(), None);

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseScroll {
                scroll_amount_1: 2,
                scroll_amount_2: 0,
                zero: 0
            })
        );
        assert_eq!(iter.next(), None);
        assert!(!batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn mouse_scroll_y() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::MouseScrollHorizontal { scroll_x: 1 });
        assert!(batcher.is_dirty());
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::MouseScrollHorizontal { scroll_x: 1 });
        assert!(batcher.is_dirty());
        assert_eq!(iter.next(), None);

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseHorizontalScroll { scroll_amount: 2 })
        );
        assert_eq!(iter.next(), None);
        assert!(!batcher.is_dirty());

        let mut iter = batcher.remove_batched_inputs();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn mouse_button_press_and_release() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::MouseButton {
            action: MouseButtonAction::Press,
            button: MouseButton::Left,
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseButton {
                action: MouseButtonAction::Press,
                button: MouseButton::Left
            })
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::MouseButton {
            action: MouseButtonAction::Release,
            button: MouseButton::Left,
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::MouseButton {
                action: MouseButtonAction::Release,
                button: MouseButton::Left
            })
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn keyboard_press_and_release() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Down,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::Keyboard {
                action: KeyAction::Down,
                flags: KeyFlags::empty(),
                key_code: KeyCode::VK_KEY_A,
                modifiers: KeyModifiers::empty(),
                zero: 0,
            })
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Up,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::Keyboard {
                action: KeyAction::Up,
                flags: KeyFlags::empty(),
                key_code: KeyCode::VK_KEY_A,
                modifiers: KeyModifiers::empty(),
                zero: 0,
            })
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn keyboard_double_press_and_release() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Down,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::Keyboard {
                action: KeyAction::Down,
                flags: KeyFlags::empty(),
                key_code: KeyCode::VK_KEY_A,
                modifiers: KeyModifiers::empty(),
                zero: 0,
            })
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Down,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Up,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::Keyboard {
                action: KeyAction::Up,
                flags: KeyFlags::empty(),
                key_code: KeyCode::VK_KEY_A,
                modifiers: KeyModifiers::empty(),
                zero: 0,
            })
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::Keyboard {
            action: KeyAction::Up,
            flags: KeyFlags::empty(),
            key_code: KeyCode::VK_KEY_A,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn controller_lifecycle() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::ControllerConnect {
            controller_number: 0,
            ty: ControllerType::Unknown,
            capabilities: ControllerCapabilities::empty(),
            supported_buttons: ControllerButtons::all(),
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::ControllerArrival {
                controller_number: 0,
                ty: ControllerType::Unknown,
                capabilities: ControllerCapabilities::empty(),
                supported_buttons: ControllerButtons::all(),
            })
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::ControllerState {
            controller_number: 0,
            pressed_buttons: ControllerButtons::A,
            left_trigger: 1.0,
            right_trigger: 0.0,
            left_stick_x: 0.5,
            left_stick_y: 0.0,
            right_stick_x: -1.0,
            right_stick_y: 0.0,
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::controller_state(
                ActiveGamepads::GAMEPAD_1,
                0,
                ControllerButtons::A,
                1.0,
                0.0,
                0.5,
                0.0,
                -1.0,
                0.0
            ))
        );
        assert_eq!(iter.next(), None);

        let mut iter = batcher.batch_input(ClientInputEvent::ControllerDisconnect {
            controller_number: 0,
        });
        assert_eq!(
            iter.next(),
            Some(ControlPacket::controller_state(
                ActiveGamepads::empty(),
                0,
                ControllerButtons::empty(),
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0
            ))
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn controller_cannot_disconnect_without_connect() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::ControllerDisconnect {
            controller_number: 0,
        });
        assert_eq!(iter.next(), None);
    }
    #[test]
    fn controller_cannot_send_state_without_connect() {
        let mut batcher = InputBatcher::default();

        let mut iter = batcher.batch_input(ClientInputEvent::ControllerState {
            controller_number: 0,
            pressed_buttons: ControllerButtons::A,
            left_trigger: 0.0,
            right_trigger: 0.0,
            left_stick_x: 0.0,
            left_stick_y: 0.0,
            right_stick_x: 0.0,
            right_stick_y: 0.0,
        });
        assert_eq!(iter.next(), None);
    }
}
