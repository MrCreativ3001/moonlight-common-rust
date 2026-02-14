use bitflags::bitflags;
use num_derive::FromPrimitive;

use crate::stream::bindings::{
    A_FLAG, B_FLAG, BACK_FLAG, BUTTON_ACTION_PRESS, BUTTON_ACTION_RELEASE, BUTTON_LEFT,
    BUTTON_MIDDLE, BUTTON_RIGHT, BUTTON_X1, BUTTON_X2, DOWN_FLAG, DS_EFFECT_LEFT_TRIGGER,
    DS_EFFECT_PAYLOAD_SIZE, DS_EFFECT_RIGHT_TRIGGER, KEY_ACTION_DOWN, KEY_ACTION_UP, LB_FLAG,
    LEFT_FLAG, LI_BATTERY_STATE_CHARGING, LI_BATTERY_STATE_DISCHARGING, LI_BATTERY_STATE_FULL,
    LI_BATTERY_STATE_NOT_CHARGING, LI_BATTERY_STATE_NOT_PRESENT, LI_BATTERY_STATE_UNKNOWN,
    LI_CCAP_ACCEL, LI_CCAP_ANALOG_TRIGGERS, LI_CCAP_BATTERY_STATE, LI_CCAP_GYRO, LI_CCAP_RGB_LED,
    LI_CCAP_RUMBLE, LI_CCAP_TOUCHPAD, LI_CCAP_TRIGGER_RUMBLE, LI_CTYPE_NINTENDO, LI_CTYPE_PS,
    LI_CTYPE_UNKNOWN, LI_CTYPE_XBOX, LI_MOTION_TYPE_ACCEL, LI_MOTION_TYPE_GYRO,
    LI_TOUCH_EVENT_BUTTON_ONLY, LI_TOUCH_EVENT_CANCEL, LI_TOUCH_EVENT_CANCEL_ALL,
    LI_TOUCH_EVENT_DOWN, LI_TOUCH_EVENT_HOVER, LI_TOUCH_EVENT_HOVER_LEAVE, LI_TOUCH_EVENT_MOVE,
    LI_TOUCH_EVENT_UP, LS_CLK_FLAG, MISC_FLAG, MODIFIER_ALT, MODIFIER_CTRL, MODIFIER_META,
    MODIFIER_SHIFT, PADDLE1_FLAG, PADDLE2_FLAG, PADDLE3_FLAG, PADDLE4_FLAG, PLAY_FLAG, RB_FLAG,
    RIGHT_FLAG, RS_CLK_FLAG, SPECIAL_FLAG, SS_KBE_FLAG_NON_NORMALIZED, TOUCHPAD_FLAG, UP_FLAG,
    X_FLAG, Y_FLAG,
};

// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1299
pub const DEFAULT_CONTROL_PORT: u16 = 47999;

// --------------- Keyboard ---------------

#[repr(i8)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum KeyAction {
    Up = KEY_ACTION_UP as i8,
    Down = KEY_ACTION_DOWN as i8,
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct KeyModifiers: i8 {
        const SHIFT = MODIFIER_SHIFT as i8;
        const CTRL = MODIFIER_CTRL as i8;
        const ALT = MODIFIER_ALT as i8;
        const META = MODIFIER_META as i8;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct KeyFlags: i8 {
        const NON_NORMALIZED = SS_KBE_FLAG_NON_NORMALIZED as i8;
    }
}

// --------------- Mouse ---------------

#[repr(i8)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum MouseButtonAction {
    Press = BUTTON_ACTION_PRESS as i8,
    Release = BUTTON_ACTION_RELEASE as i8,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum MouseButton {
    Left = BUTTON_LEFT as i32,
    Middle = BUTTON_MIDDLE as i32,
    Right = BUTTON_RIGHT as i32,
    X1 = BUTTON_X1 as i32,
    X2 = BUTTON_X2 as i32,
}

// --------------- Touch ---------------

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum TouchEventType {
    Hover = LI_TOUCH_EVENT_HOVER,
    Down = LI_TOUCH_EVENT_DOWN,
    Up = LI_TOUCH_EVENT_UP,
    Move = LI_TOUCH_EVENT_MOVE,
    Cancel = LI_TOUCH_EVENT_CANCEL,
    ButtonOnly = LI_TOUCH_EVENT_BUTTON_ONLY,
    HoverLeave = LI_TOUCH_EVENT_HOVER_LEAVE,
    CancelAll = LI_TOUCH_EVENT_CANCEL_ALL,
}

// --------------- Controller ---------------

bitflags! {
    #[derive(Debug, Clone, Copy)]
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
    #[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone, Copy)]
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
    #[derive(Debug, Clone, Copy)]
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

bitflags! {
    /// Motion sensor types for [`LiSendControllerMotionEvent`].
    #[derive(Debug, Clone, Copy)]
    pub struct MotionType: u8 {
        /// Accelerometer data in m/s² (inclusive of gravitational acceleration).
        const ACCEL = LI_MOTION_TYPE_ACCEL as u8;
        /// Gyroscope data in degrees per second.
        const GYRO  = LI_MOTION_TYPE_GYRO as u8;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct DualSenseEffect: u32 {
        const PAYLOAD_SIZE = DS_EFFECT_PAYLOAD_SIZE;
        const RIGHT_TRIGGER = DS_EFFECT_RIGHT_TRIGGER;
        const LEFT_TRIGGER = DS_EFFECT_LEFT_TRIGGER;
    }
}

bitflags! {
    /// Battery states for [`LiSendControllerBatteryEvent`].
    #[derive(Debug, Clone, Copy)]
    pub struct BatteryState: u8 {
        /// Unknown battery state.
        const UNKNOWN       = LI_BATTERY_STATE_UNKNOWN as u8;
        /// No battery present.
        const NOT_PRESENT   = LI_BATTERY_STATE_NOT_PRESENT as u8;
        /// Battery is discharging.
        const DISCHARGING   = LI_BATTERY_STATE_DISCHARGING as u8;
        /// Battery is charging.
        const CHARGING      = LI_BATTERY_STATE_CHARGING as u8;
        /// Connected to power but not charging.
        const NOT_CHARGING  = LI_BATTERY_STATE_NOT_CHARGING as u8;
        /// Battery is full.
        const FULL          = LI_BATTERY_STATE_FULL as u8;
    }
}
