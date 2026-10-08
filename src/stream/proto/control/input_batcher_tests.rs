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
