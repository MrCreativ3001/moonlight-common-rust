use std::sync::{Arc, Mutex};

use moonlight_common::stream::{
    control::{
        ControllerType, KeyAction, KeyCode, MouseButton, MouseButtonAction, ToolType,
        TouchEventType,
    },
    proto::control::input_batcher::{
        ClientInputEvent as ClientInputEvent2, InputBatcher as InputBatcher2,
    },
};
use uniffi::{Enum, Object, export};

use crate::control_packet::{
    ControlPacket, ControllerButtons, ControllerCapabilities, KeyFlags, KeyModifiers, PenButtons,
};

#[derive(Debug, Enum)]
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
        scroll_y: i16,
    },
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

impl From<ClientInputEvent> for ClientInputEvent2 {
    fn from(value: ClientInputEvent) -> Self {
        match value {
            ClientInputEvent::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
            } => ClientInputEvent2::Keyboard {
                action,
                flags: flags.into(),
                key_code,
                modifiers: modifiers.into(),
            },
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                ClientInputEvent2::MouseMoveRelative { delta_x, delta_y }
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => ClientInputEvent2::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            },
            ClientInputEvent::MouseButton { action, button } => {
                ClientInputEvent2::MouseButton { action, button }
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                ClientInputEvent2::MouseScrollVertical { scroll_y }
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                ClientInputEvent2::MouseScrollHorizontal { scroll_x }
            }
            ClientInputEvent::ControllerConnect {
                controller_number,
                ty,
                capabilities,
                supported_buttons,
            } => ClientInputEvent2::ControllerConnect {
                controller_number,
                ty,
                capabilities: capabilities.into(),
                supported_buttons: supported_buttons.into(),
            },
            ClientInputEvent::ControllerState {
                controller_number,
                pressed_buttons,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            } => ClientInputEvent2::ControllerState {
                controller_number,
                pressed_buttons: pressed_buttons.into(),
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            },
            ClientInputEvent::ControllerDisconnect { controller_number } => {
                ClientInputEvent2::ControllerDisconnect { controller_number }
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
            } => ClientInputEvent2::Touch {
                event_type,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            },
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
            } => ClientInputEvent2::Pen {
                event_type,
                tool_type,
                buttons: buttons.into(),
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                contact_area_minor,
                contact_area_major,
            },
        }
    }
}

#[derive(Debug, Default, Object)]
pub struct InputBatcher {
    inner: Mutex<InputBatcher2>,
}

#[export]
impl InputBatcher {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn batch_input(&self, input: ClientInputEvent) -> Vec<ControlPacket> {
        let mut inner = self.inner.lock().expect("lock InputBatcher");

        inner
            .batch_input(input.into())
            .map(ControlPacket::from)
            .collect::<Vec<_>>()
    }

    pub fn is_dirty(&self) -> bool {
        let inner = self.inner.lock().expect("lock InputBatcher");

        inner.is_dirty()
    }

    pub fn remove_batched_inputs(&self) -> Vec<ControlPacket> {
        let mut inner = self.inner.lock().expect("lock InputBatcher");

        inner
            .remove_batched_inputs()
            .map(ControlPacket::from)
            .collect::<Vec<_>>()
    }
}
