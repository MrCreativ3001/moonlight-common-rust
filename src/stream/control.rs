use std::{
    ops::{BitAnd, BitOr, Not},
    time::Duration,
};

use bitflags::bitflags;
use num_derive::{FromPrimitive, ToPrimitive};

use crate::stream::bindings::{
    A_FLAG, B_FLAG, BACK_FLAG, BUTTON_ACTION_PRESS, BUTTON_ACTION_RELEASE, BUTTON_LEFT,
    BUTTON_MIDDLE, BUTTON_RIGHT, BUTTON_X1, BUTTON_X2, DOWN_FLAG, DS_EFFECT_LEFT_TRIGGER,
    DS_EFFECT_PAYLOAD_SIZE, DS_EFFECT_RIGHT_TRIGGER, KEY_ACTION_DOWN, KEY_ACTION_UP, LB_FLAG,
    LEFT_FLAG, LI_BATTERY_STATE_CHARGING, LI_BATTERY_STATE_DISCHARGING, LI_BATTERY_STATE_FULL,
    LI_BATTERY_STATE_NOT_CHARGING, LI_BATTERY_STATE_NOT_PRESENT, LI_BATTERY_STATE_UNKNOWN,
    LI_CCAP_ACCEL, LI_CCAP_ANALOG_TRIGGERS, LI_CCAP_BATTERY_STATE, LI_CCAP_GYRO, LI_CCAP_RGB_LED,
    LI_CCAP_RUMBLE, LI_CCAP_TOUCHPAD, LI_CCAP_TRIGGER_RUMBLE, LI_CTYPE_NINTENDO, LI_CTYPE_PS,
    LI_CTYPE_UNKNOWN, LI_CTYPE_XBOX, LI_MOTION_TYPE_ACCEL, LI_MOTION_TYPE_GYRO,
    LI_PEN_BUTTON_PRIMARY, LI_PEN_BUTTON_SECONDARY, LI_PEN_BUTTON_TERTIARY, LI_TOOL_TYPE_ERASER,
    LI_TOOL_TYPE_PEN, LI_TOOL_TYPE_UNKNOWN, LI_TOUCH_EVENT_BUTTON_ONLY, LI_TOUCH_EVENT_CANCEL,
    LI_TOUCH_EVENT_CANCEL_ALL, LI_TOUCH_EVENT_DOWN, LI_TOUCH_EVENT_HOVER,
    LI_TOUCH_EVENT_HOVER_LEAVE, LI_TOUCH_EVENT_MOVE, LI_TOUCH_EVENT_UP, LS_CLK_FLAG, MISC_FLAG,
    MODIFIER_ALT, MODIFIER_CTRL, MODIFIER_META, MODIFIER_SHIFT, PADDLE1_FLAG, PADDLE2_FLAG,
    PADDLE3_FLAG, PADDLE4_FLAG, PLAY_FLAG, RB_FLAG, RIGHT_FLAG, RS_CLK_FLAG, SPECIAL_FLAG,
    SS_KBE_FLAG_NON_NORMALIZED, TOUCHPAD_FLAG, UP_FLAG, X_FLAG, Y_FLAG,
};

// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1299
pub const DEFAULT_CONTROL_PORT: u16 = 47999;

// --------------- Keyboard ---------------

#[repr(i8)]
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq)]
pub enum KeyAction {
    Up = KEY_ACTION_UP as i8,
    Down = KEY_ACTION_DOWN as i8,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct KeyModifiers: i8 {
        const SHIFT = MODIFIER_SHIFT as i8;
        const CTRL = MODIFIER_CTRL as i8;
        const ALT = MODIFIER_ALT as i8;
        const META = MODIFIER_META as i8;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct KeyFlags: i8 {
        /// Sunshine Extension
        const SUNSHINE_NON_NORMALIZED = SS_KBE_FLAG_NON_NORMALIZED as i8;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCode(pub i16);

impl KeyCode {
    /* Mouse buttons */
    pub const VK_LBUTTON: Self = Self(0x01);
    pub const VK_RBUTTON: Self = Self(0x02);
    pub const VK_CANCEL: Self = Self(0x03);
    pub const VK_MBUTTON: Self = Self(0x04);
    pub const VK_XBUTTON1: Self = Self(0x05);
    pub const VK_XBUTTON2: Self = Self(0x06);

    /* Keyboard */
    pub const VK_BACK: Self = Self(0x08);
    pub const VK_TAB: Self = Self(0x09);
    pub const VK_CLEAR: Self = Self(0x0C);
    pub const VK_RETURN: Self = Self(0x0D);

    pub const VK_SHIFT: Self = Self(0x10);
    pub const VK_CONTROL: Self = Self(0x11);
    pub const VK_MENU: Self = Self(0x12);
    pub const VK_PAUSE: Self = Self(0x13);
    pub const VK_CAPITAL: Self = Self(0x14);
    pub const VK_KANA: Self = Self(0x15);
    pub const VK_HANGUEL: Self = Self(0x15);
    pub const VK_HANGUL: Self = Self(0x15);
    pub const VK_JUNJA: Self = Self(0x17);
    pub const VK_FINAL: Self = Self(0x18);
    pub const VK_HANJA: Self = Self(0x19);
    pub const VK_KANJI: Self = Self(0x19);

    pub const VK_ESCAPE: Self = Self(0x1B);
    pub const VK_CONVERT: Self = Self(0x1C);
    pub const VK_NONCONVERT: Self = Self(0x1D);
    pub const VK_ACCEPT: Self = Self(0x1E);
    pub const VK_MODECHANGE: Self = Self(0x1F);

    pub const VK_SPACE: Self = Self(0x20);
    pub const VK_PRIOR: Self = Self(0x21);
    pub const VK_NEXT: Self = Self(0x22);
    pub const VK_END: Self = Self(0x23);
    pub const VK_HOME: Self = Self(0x24);
    pub const VK_LEFT: Self = Self(0x25);
    pub const VK_UP: Self = Self(0x26);
    pub const VK_RIGHT: Self = Self(0x27);
    pub const VK_DOWN: Self = Self(0x28);
    pub const VK_SELECT: Self = Self(0x29);
    pub const VK_PRINT: Self = Self(0x2A);
    pub const VK_EXECUTE: Self = Self(0x2B);
    pub const VK_SNAPSHOT: Self = Self(0x2C);
    pub const VK_INSERT: Self = Self(0x2D);
    pub const VK_DELETE: Self = Self(0x2E);
    pub const VK_HELP: Self = Self(0x2F);

    /* Digits */
    pub const VK_KEY_0: Self = Self(0x30);
    pub const VK_KEY_1: Self = Self(0x31);
    pub const VK_KEY_2: Self = Self(0x32);
    pub const VK_KEY_3: Self = Self(0x33);
    pub const VK_KEY_4: Self = Self(0x34);
    pub const VK_KEY_5: Self = Self(0x35);
    pub const VK_KEY_6: Self = Self(0x36);
    pub const VK_KEY_7: Self = Self(0x37);
    pub const VK_KEY_8: Self = Self(0x38);
    pub const VK_KEY_9: Self = Self(0x39);

    /* Alphabet */
    pub const VK_KEY_A: Self = Self(0x41);
    pub const VK_KEY_B: Self = Self(0x42);
    pub const VK_KEY_C: Self = Self(0x43);
    pub const VK_KEY_D: Self = Self(0x44);
    pub const VK_KEY_E: Self = Self(0x45);
    pub const VK_KEY_F: Self = Self(0x46);
    pub const VK_KEY_G: Self = Self(0x47);
    pub const VK_KEY_H: Self = Self(0x48);
    pub const VK_KEY_I: Self = Self(0x49);
    pub const VK_KEY_J: Self = Self(0x4A);
    pub const VK_KEY_K: Self = Self(0x4B);
    pub const VK_KEY_L: Self = Self(0x4C);
    pub const VK_KEY_M: Self = Self(0x4D);
    pub const VK_KEY_N: Self = Self(0x4E);
    pub const VK_KEY_O: Self = Self(0x4F);
    pub const VK_KEY_P: Self = Self(0x50);
    pub const VK_KEY_Q: Self = Self(0x51);
    pub const VK_KEY_R: Self = Self(0x52);
    pub const VK_KEY_S: Self = Self(0x53);
    pub const VK_KEY_T: Self = Self(0x54);
    pub const VK_KEY_U: Self = Self(0x55);
    pub const VK_KEY_V: Self = Self(0x56);
    pub const VK_KEY_W: Self = Self(0x57);
    pub const VK_KEY_X: Self = Self(0x58);
    pub const VK_KEY_Y: Self = Self(0x59);
    pub const VK_KEY_Z: Self = Self(0x5A);

    pub const VK_LWIN: Self = Self(0x5B);
    pub const VK_RWIN: Self = Self(0x5C);
    pub const VK_APPS: Self = Self(0x5D);
    pub const VK_SLEEP: Self = Self(0x5F);

    /* Numeric keypad */
    pub const VK_NUMPAD0: Self = Self(0x60);
    pub const VK_NUMPAD1: Self = Self(0x61);
    pub const VK_NUMPAD2: Self = Self(0x62);
    pub const VK_NUMPAD3: Self = Self(0x63);
    pub const VK_NUMPAD4: Self = Self(0x64);
    pub const VK_NUMPAD5: Self = Self(0x65);
    pub const VK_NUMPAD6: Self = Self(0x66);
    pub const VK_NUMPAD7: Self = Self(0x67);
    pub const VK_NUMPAD8: Self = Self(0x68);
    pub const VK_NUMPAD9: Self = Self(0x69);

    pub const VK_MULTIPLY: Self = Self(0x6A);
    pub const VK_ADD: Self = Self(0x6B);
    pub const VK_SEPARATOR: Self = Self(0x6C);
    pub const VK_SUBTRACT: Self = Self(0x6D);
    pub const VK_DECIMAL: Self = Self(0x6E);
    pub const VK_DIVIDE: Self = Self(0x6F);

    /* Function keys */
    pub const VK_F1: Self = Self(0x70);
    pub const VK_F2: Self = Self(0x71);
    pub const VK_F3: Self = Self(0x72);
    pub const VK_F4: Self = Self(0x73);
    pub const VK_F5: Self = Self(0x74);
    pub const VK_F6: Self = Self(0x75);
    pub const VK_F7: Self = Self(0x76);
    pub const VK_F8: Self = Self(0x77);
    pub const VK_F9: Self = Self(0x78);
    pub const VK_F10: Self = Self(0x79);
    pub const VK_F11: Self = Self(0x7A);
    pub const VK_F12: Self = Self(0x7B);
    pub const VK_F13: Self = Self(0x7C);
    pub const VK_F14: Self = Self(0x7D);
    pub const VK_F15: Self = Self(0x7E);
    pub const VK_F16: Self = Self(0x7F);
    pub const VK_F17: Self = Self(0x80);
    pub const VK_F18: Self = Self(0x81);
    pub const VK_F19: Self = Self(0x82);
    pub const VK_F20: Self = Self(0x83);
    pub const VK_F21: Self = Self(0x84);
    pub const VK_F22: Self = Self(0x85);
    pub const VK_F23: Self = Self(0x86);
    pub const VK_F24: Self = Self(0x87);

    pub const VK_NUMLOCK: Self = Self(0x90);
    pub const VK_SCROLL: Self = Self(0x91);

    /* Modifiers */
    pub const VK_LSHIFT: Self = Self(0xA0);
    pub const VK_RSHIFT: Self = Self(0xA1);
    pub const VK_LCONTROL: Self = Self(0xA2);
    pub const VK_RCONTROL: Self = Self(0xA3);
    pub const VK_LMENU: Self = Self(0xA4);
    pub const VK_RMENU: Self = Self(0xA5);

    /* Browser */
    pub const VK_BROWSER_BACK: Self = Self(0xA6);
    pub const VK_BROWSER_FORWARD: Self = Self(0xA7);
    pub const VK_BROWSER_REFRESH: Self = Self(0xA8);
    pub const VK_BROWSER_STOP: Self = Self(0xA9);
    pub const VK_BROWSER_SEARCH: Self = Self(0xAA);
    pub const VK_BROWSER_FAVORITES: Self = Self(0xAB);
    pub const VK_BROWSER_HOME: Self = Self(0xAC);

    /* Volume */
    pub const VK_VOLUME_MUTE: Self = Self(0xAD);
    pub const VK_VOLUME_DOWN: Self = Self(0xAE);
    pub const VK_VOLUME_UP: Self = Self(0xAF);

    /* Media */
    pub const VK_MEDIA_NEXT_TRACK: Self = Self(0xB0);
    pub const VK_MEDIA_PREV_TRACK: Self = Self(0xB1);
    pub const VK_MEDIA_STOP: Self = Self(0xB2);
    pub const VK_MEDIA_PLAY_PAUSE: Self = Self(0xB3);

    /* Application launchers */
    pub const VK_LAUNCH_MAIL: Self = Self(0xB4);
    pub const VK_MEDIA_SELECT: Self = Self(0xB5);
    pub const VK_LAUNCH_APP1: Self = Self(0xB6);
    pub const VK_LAUNCH_APP2: Self = Self(0xB7);

    /* OEM */
    pub const VK_OEM_1: Self = Self(0xBA);
    pub const VK_OEM_PLUS: Self = Self(0xBB);
    pub const VK_OEM_COMMA: Self = Self(0xBC);
    pub const VK_OEM_MINUS: Self = Self(0xBD);
    pub const VK_OEM_PERIOD: Self = Self(0xBE);
    pub const VK_OEM_2: Self = Self(0xBF);
    pub const VK_OEM_3: Self = Self(0xC0);
    pub const VK_ABNT_C1: Self = Self(0xC1);
    pub const VK_ABNT_C2: Self = Self(0xC2);
    pub const VK_OEM_4: Self = Self(0xDB);
    pub const VK_OEM_5: Self = Self(0xDC);
    pub const VK_OEM_6: Self = Self(0xDD);
    pub const VK_OEM_7: Self = Self(0xDE);
    pub const VK_OEM_8: Self = Self(0xDF);
    pub const VK_OEM_102: Self = Self(0xE2);

    /* IME / input */
    pub const VK_PROCESSKEY: Self = Self(0xE5);
    pub const VK_PACKET: Self = Self(0xE7);

    /* Miscellaneous */
    pub const VK_ATTN: Self = Self(0xF6);
    pub const VK_CRSEL: Self = Self(0xF7);
    pub const VK_EXSEL: Self = Self(0xF8);
    pub const VK_EREOF: Self = Self(0xF9);
    pub const VK_PLAY: Self = Self(0xFA);
    pub const VK_ZOOM: Self = Self(0xFB);
    pub const VK_NONAME: Self = Self(0xFC);
    pub const VK_PA1: Self = Self(0xFD);
    pub const VK_OEM_CLEAR: Self = Self(0xFE);
}

// --------------- Mouse ---------------

#[repr(i8)]
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq)]
pub enum MouseButtonAction {
    Press = BUTTON_ACTION_PRESS as i8,
    Release = BUTTON_ACTION_RELEASE as i8,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq)]
pub enum MouseButton {
    Left = BUTTON_LEFT as i32,
    Middle = BUTTON_MIDDLE as i32,
    Right = BUTTON_RIGHT as i32,
    X1 = BUTTON_X1 as i32,
    X2 = BUTTON_X2 as i32,
}

// --------------- Touch / Pen ---------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, FromPrimitive)]
pub enum TouchEventType {
    Hover = LI_TOUCH_EVENT_HOVER as u8,
    Down = LI_TOUCH_EVENT_DOWN as u8,
    Up = LI_TOUCH_EVENT_UP as u8,
    Move = LI_TOUCH_EVENT_MOVE as u8,
    Cancel = LI_TOUCH_EVENT_CANCEL as u8,
    ButtonOnly = LI_TOUCH_EVENT_BUTTON_ONLY as u8,
    HoverLeave = LI_TOUCH_EVENT_HOVER_LEAVE as u8,
    CancelAll = LI_TOUCH_EVENT_CANCEL_ALL as u8,
}

// --------------- Mouse / Keyboard helper ---------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactKeyStates {
    pressed: [u8; 32],
}
impl CompactKeyStates {
    const MIN_KEY_CODE: KeyCode = KeyCode(1);
    const MAX_KEY_CODE: KeyCode = KeyCode(256);

    fn key_to_bit(key_code: KeyCode) -> Option<u8> {
        let key = u16::try_from(key_code.0).ok()?;

        if key > 0 && key <= 256 {
            Some((key - 1) as u8)
        } else {
            None
        }
    }
    fn bit_to_index_and_shift(bit: u8) -> (usize, u8) {
        let index = bit / 8;
        let shift = bit % 8;

        (index as usize, shift)
    }

    pub fn can_store(&self, key_code: KeyCode) -> bool {
        Self::key_to_bit(key_code).is_some()
    }

    pub fn set_pressed(&mut self, key_code: KeyCode, action: KeyAction) -> Option<KeyAction> {
        let bit = Self::key_to_bit(key_code)?;
        let (index, shift) = Self::bit_to_index_and_shift(bit);

        let old_is_down = (self.pressed[index] & (1u8 << shift)) != 0;

        if matches!(action, KeyAction::Up) {
            self.pressed[index] &= !(1u8 << shift);
        } else {
            self.pressed[index] |= 1u8 << shift;
        }

        Some(if old_is_down {
            KeyAction::Down
        } else {
            KeyAction::Up
        })
    }
    pub fn is_pressed(&self, key_code: KeyCode) -> Option<KeyAction> {
        let bit = Self::key_to_bit(key_code)?;
        let (index, shift) = Self::bit_to_index_and_shift(bit);

        let is_down = (self.pressed[index] & (1u8 << shift)) != 0;
        Some(if is_down {
            KeyAction::Down
        } else {
            KeyAction::Up
        })
    }

    pub fn pressed_iter(&self) -> impl Iterator<Item = KeyCode> + '_ {
        (Self::MIN_KEY_CODE.0..=Self::MAX_KEY_CODE.0)
            .filter(|key_code| matches!(self.is_pressed(KeyCode(*key_code)).expect("failed to use the CompactKeyStates::pressed_iter because of an invalid KeyCode range"), KeyAction::Down))
            .map(KeyCode)
    }
}

impl From<[u8; 32]> for CompactKeyStates {
    fn from(value: [u8; 32]) -> Self {
        Self { pressed: value }
    }
}

impl From<CompactKeyStates> for [u8; 32] {
    fn from(value: CompactKeyStates) -> Self {
        value.pressed
    }
}

impl BitAnd for CompactKeyStates {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        let mut pressed = [0u8; 32];

        for i in 0..32 {
            pressed[i] = self.pressed[i] & rhs.pressed[i];
        }

        Self { pressed }
    }
}

impl BitOr for CompactKeyStates {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        let mut pressed = [0u8; 32];

        for i in 0..32 {
            pressed[i] = self.pressed[i] | rhs.pressed[i];
        }

        Self { pressed }
    }
}

impl Not for CompactKeyStates {
    type Output = Self;

    fn not(self) -> Self::Output {
        let mut pressed = [0u8; 32];

        for i in 0..32 {
            pressed[i] = !self.pressed[i];
        }

        Self { pressed }
    }
}

// --------------- Pen ---------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, FromPrimitive)]
pub enum ToolType {
    Unknown = LI_TOOL_TYPE_UNKNOWN as u8,
    Pen = LI_TOOL_TYPE_PEN as u8,
    Eraser = LI_TOOL_TYPE_ERASER as u8,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct PenButtons: u8 {
        const PRIMARY = LI_PEN_BUTTON_PRIMARY as u8;
        const SECONDARY= LI_PEN_BUTTON_SECONDARY as u8;
        const TERTIARY = LI_PEN_BUTTON_TERTIARY as u8;
    }
}

// --------------- Controller ---------------

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ControllerButtons: u32 {
        const A        = A_FLAG;
        const B        = B_FLAG;
        const X        = X_FLAG;
        const Y        = Y_FLAG;
        const UP       = UP_FLAG;
        const DOWN     = DOWN_FLAG;
        const LEFT     = LEFT_FLAG;
        const RIGHT    = RIGHT_FLAG;
        const LB       = LB_FLAG;
        const RB       = RB_FLAG;
        const PLAY     = PLAY_FLAG;
        const BACK     = BACK_FLAG;
        const LS_CLK   = LS_CLK_FLAG;
        const RS_CLK   = RS_CLK_FLAG;
        const SPECIAL  = SPECIAL_FLAG;

        /// Extended buttons (Sunshine only)
        const PADDLE1  = PADDLE1_FLAG;
        /// Extended buttons (Sunshine only)
        const PADDLE2  = PADDLE2_FLAG;
        /// Extended buttons (Sunshine only)
        const PADDLE3  = PADDLE3_FLAG;
        /// Extended buttons (Sunshine only)
        const PADDLE4  = PADDLE4_FLAG;
        /// Extended buttons (Sunshine only)
        /// Touchpad buttons on Sony controllers
        const TOUCHPAD = TOUCHPAD_FLAG;
        /// Extended buttons (Sunshine only)
        /// Share/Mic/Capture/Mute buttons on various controllers
        const MISC     = MISC_FLAG;
    }
}
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Default)]
    pub struct ActiveGamepads: u16 {
        const GAMEPAD_1  = 0b0000_0000_0000_0001;
        const GAMEPAD_2  = 0b0000_0000_0000_0010;
        const GAMEPAD_3  = 0b0000_0000_0000_0100;
        const GAMEPAD_4  = 0b0000_0000_0000_1000;

        /// Extended gamepads (Sunshine only)
        const GAMEPAD_5  = 0b0000_0000_0001_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_6  = 0b0000_0000_0010_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_7  = 0b0000_0000_0100_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_8  = 0b0000_0000_1000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_9  = 0b0000_0001_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_10 = 0b0000_0010_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_11 = 0b0000_0100_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_12 = 0b0000_1000_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_13 = 0b0001_0000_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_14 = 0b0010_0000_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_15 = 0b0100_0000_0000_0000;
        /// Extended gamepads (Sunshine only)
        const GAMEPAD_16 = 0b1000_0000_0000_0000;
    }
}

impl ActiveGamepads {
    pub fn from_id(id: u8) -> Option<Self> {
        if id >= 16 {
            return None;
        }
        Some(ActiveGamepads::from_bits_truncate(1 << id))
    }
}

/// Represents the type of controller.
///
/// This is used to inform the host of what type of controller has arrived,
/// which can help the host decide how to emulate it and what features to expose.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, FromPrimitive)]
pub enum ControllerType {
    /// Unknown controller type.
    Unknown = LI_CTYPE_UNKNOWN as u8,
    /// Microsoft Xbox-compatible controller.
    Xbox = LI_CTYPE_XBOX as u8,
    /// Sony PlayStation-compatible controller.
    PlayStation = LI_CTYPE_PS as u8,
    /// Nintendo-compatible controller (e.g., Switch Pro Controller).
    Nintendo = LI_CTYPE_NINTENDO as u8,
}

bitflags! {
    /// Represents the capabilities of a controller.
    ///
    /// This is typically sent along with controller arrival information so the host
    /// knows which features the controller supports.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ControllerCapabilities: u16 {
        /// Reports values between `0x00` and `0xFF` for trigger axes.
        const ANALOG_TRIGGERS  = LI_CCAP_ANALOG_TRIGGERS as u16;
        /// Can rumble in response to `ConnListenerRumble()` callback.
        const RUMBLE           = LI_CCAP_RUMBLE as u16;
        /// Can rumble triggers in response to `ConnListenerRumbleTriggers()` callback.
        const TRIGGER_RUMBLE   = LI_CCAP_TRIGGER_RUMBLE as u16;
        /// Reports touchpad events via `LiSendControllerTouchEvent()`.
        const TOUCHPAD         = LI_CCAP_TOUCHPAD as u16;
        /// Can report accelerometer events via `LiSendControllerMotionEvent()`.
        const ACCEL            = LI_CCAP_ACCEL as u16;
        /// Can report gyroscope events via `LiSendControllerMotionEvent()`.
        const GYRO             = LI_CCAP_GYRO as u16;
        /// Reports battery state via `LiSendControllerBatteryEvent()`.
        const BATTERY_STATE    = LI_CCAP_BATTERY_STATE as u16;
        /// Can set RGB LED state via `ConnListenerSetControllerLED()`.
        const RGB_LED          = LI_CCAP_RGB_LED as u16;
    }
}

/// Motion sensor types for `LiSendControllerMotionEvent`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, FromPrimitive, ToPrimitive)]
pub enum MotionType {
    /// Accelerometer data in m/s² (inclusive of gravitational acceleration).
    Acceleration = LI_MOTION_TYPE_ACCEL as u8,
    /// Gyroscope data in degrees per second.
    Gyroscope = LI_MOTION_TYPE_GYRO as u8,
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct DualSenseEffect: u32 {
        const PAYLOAD_SIZE = DS_EFFECT_PAYLOAD_SIZE;
        const RIGHT_TRIGGER = DS_EFFECT_RIGHT_TRIGGER;
        const LEFT_TRIGGER = DS_EFFECT_LEFT_TRIGGER;
    }
}

/// Battery states for `LiSendControllerBatteryEvent`.
///
/// Refernces:
/// - <https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L811-L820>
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, FromPrimitive, ToPrimitive)]
pub enum BatteryState {
    /// Unknown battery state.
    Unknown = LI_BATTERY_STATE_UNKNOWN as u8,
    /// No battery present.
    NotPresent = LI_BATTERY_STATE_NOT_PRESENT as u8,
    /// Battery is discharging.
    Discharging = LI_BATTERY_STATE_DISCHARGING as u8,
    /// Battery is charging.
    Charging = LI_BATTERY_STATE_CHARGING as u8,
    /// Connected to power but not charging.
    NotCharging = LI_BATTERY_STATE_NOT_CHARGING as u8,
    /// Battery is full.
    Full = LI_BATTERY_STATE_FULL as u8,
}

/// Enet estimated round trip time
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct EstimatedRttInfo {
    pub rtt: Duration,
    pub rtt_variance: Duration,
}
