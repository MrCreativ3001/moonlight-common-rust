use moonlight_common::stream::{
    control::{
        ActiveGamepads as ActiveGamepads2, BatteryState, ControllerButtons as ControllerButtons2,
        ControllerCapabilities as ControllerCapabilities2, ControllerType, KeyAction, KeyCode,
        KeyFlags as KeyFlags2, KeyModifiers as KeyModifiers2, MotionType, MouseButton,
        MouseButtonAction, PenButtons as PenButtons2, ToolType, TouchEventType,
    },
    proto::control::packet::{
        ControlPacket as ControlPacket2, TerminationReason, UTF8_TEXT_MAX_COUNT,
    },
    video::{Primary, SunshineHdrMetadata as SunshineHdrMetadata2},
};
use uniffi::{Enum, Record, custom_type, remote};

#[remote(Enum)]
pub enum MotionType {
    Acceleration,
    Gyroscope,
}

#[remote(Record)]
pub struct Primary {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Record)]
pub struct SunshineHdrMetadata {
    pub display_primary_1: Primary,
    pub display_primary_2: Primary,
    pub display_primary_3: Primary,
    pub white_point: Primary,
    pub max_display_luminance: u16,
    pub min_display_luminance: u16,
    pub max_content_light_level: u16,
    pub max_frame_average_light_level: u16,
    pub max_full_frame_luminance: u16,
}

impl From<SunshineHdrMetadata> for SunshineHdrMetadata2 {
    fn from(value: SunshineHdrMetadata) -> Self {
        Self {
            display_primaries: [
                value.display_primary_1,
                value.display_primary_2,
                value.display_primary_3,
            ],
            white_point: value.white_point,
            max_display_luminance: value.max_display_luminance,
            min_display_luminance: value.min_display_luminance,
            max_content_light_level: value.max_content_light_level,
            max_frame_average_light_level: value.max_frame_average_light_level,
            max_full_frame_luminance: value.max_full_frame_luminance,
        }
    }
}

impl From<SunshineHdrMetadata2> for SunshineHdrMetadata {
    fn from(value: SunshineHdrMetadata2) -> Self {
        let [display_primary_1, display_primary_2, display_primary_3] = value.display_primaries;

        Self {
            display_primary_1,
            display_primary_2,
            display_primary_3,
            white_point: value.white_point,
            max_display_luminance: value.max_display_luminance,
            min_display_luminance: value.min_display_luminance,
            max_content_light_level: value.max_content_light_level,
            max_frame_average_light_level: value.max_frame_average_light_level,
            max_full_frame_luminance: value.max_full_frame_luminance,
        }
    }
}

#[remote(Enum)]
pub enum MouseButtonAction {
    Press,
    Release,
}

#[remote(Enum)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    X1,
    X2,
}

#[remote(Enum)]
pub enum KeyAction {
    Up,
    Down,
}

custom_type!(KeyCode, i16, {
    remote,
    lower: |code| code.0,
    try_lift: |num| Ok(KeyCode(num)),
});

#[derive(Debug, Record, Default)]
pub struct KeyFlags {
    pub sunshine_non_normalized: bool,
}

impl From<KeyFlags> for KeyFlags2 {
    fn from(value: KeyFlags) -> Self {
        let mut bits = Self::empty();
        if value.sunshine_non_normalized {
            bits |= Self::SUNSHINE_NON_NORMALIZED;
        }
        bits
    }
}
impl From<KeyFlags2> for KeyFlags {
    fn from(value: KeyFlags2) -> Self {
        let mut bools = Self::default();
        if value.contains(KeyFlags2::SUNSHINE_NON_NORMALIZED) {
            bools.sunshine_non_normalized = true;
        }
        bools
    }
}

#[derive(Debug, Record, Default)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl From<KeyModifiers> for KeyModifiers2 {
    fn from(value: KeyModifiers) -> Self {
        let mut bits = Self::empty();

        if value.shift {
            bits |= Self::SHIFT;
        }
        if value.ctrl {
            bits |= Self::CTRL;
        }
        if value.alt {
            bits |= Self::ALT;
        }
        if value.meta {
            bits |= Self::META;
        }

        bits
    }
}

impl From<KeyModifiers2> for KeyModifiers {
    fn from(value: KeyModifiers2) -> Self {
        let mut bools = Self::default();

        if value.contains(KeyModifiers2::SHIFT) {
            bools.shift = true;
        }
        if value.contains(KeyModifiers2::CTRL) {
            bools.ctrl = true;
        }
        if value.contains(KeyModifiers2::ALT) {
            bools.alt = true;
        }
        if value.contains(KeyModifiers2::META) {
            bools.meta = true;
        }

        bools
    }
}

#[remote(Enum)]
pub enum TouchEventType {
    Hover,
    Down,
    Up,
    Move,
    Cancel,
    ButtonOnly,
    HoverLeave,
    CancelAll,
}

#[remote(Enum)]
pub enum ToolType {
    Unknown,
    Pen,
    Eraser,
}

#[derive(Debug, Record, Default)]
pub struct PenButtons {
    pub primary: bool,
    pub secondary: bool,
    pub tertiary: bool,
}

impl From<PenButtons> for PenButtons2 {
    fn from(value: PenButtons) -> Self {
        let mut bits = Self::empty();

        if value.primary {
            bits |= Self::PRIMARY;
        }
        if value.secondary {
            bits |= Self::SECONDARY;
        }
        if value.tertiary {
            bits |= Self::TERTIARY;
        }

        bits
    }
}

impl From<PenButtons2> for PenButtons {
    fn from(value: PenButtons2) -> Self {
        let mut bools = Self::default();

        if value.contains(PenButtons2::PRIMARY) {
            bools.primary = true;
        }
        if value.contains(PenButtons2::SECONDARY) {
            bools.secondary = true;
        }
        if value.contains(PenButtons2::TERTIARY) {
            bools.tertiary = true;
        }

        bools
    }
}

#[remote(Enum)]
pub enum ControllerType {
    Unknown,
    Xbox,
    PlayStation,
    Nintendo,
}

#[derive(Debug, Record, Default)]
pub struct ActiveGamepads {
    pub gamepad_1: bool,
    pub gamepad_2: bool,
    pub gamepad_3: bool,
    pub gamepad_4: bool,
    pub gamepad_5: bool,
    pub gamepad_6: bool,
    pub gamepad_7: bool,
    pub gamepad_8: bool,
    pub gamepad_9: bool,
    pub gamepad_10: bool,
    pub gamepad_11: bool,
    pub gamepad_12: bool,
    pub gamepad_13: bool,
    pub gamepad_14: bool,
    pub gamepad_15: bool,
    pub gamepad_16: bool,
}

impl From<ActiveGamepads> for ActiveGamepads2 {
    fn from(value: ActiveGamepads) -> Self {
        let mut bits = Self::empty();

        if value.gamepad_1 {
            bits |= Self::GAMEPAD_1;
        }
        if value.gamepad_2 {
            bits |= Self::GAMEPAD_2;
        }
        if value.gamepad_3 {
            bits |= Self::GAMEPAD_3;
        }
        if value.gamepad_4 {
            bits |= Self::GAMEPAD_4;
        }
        if value.gamepad_5 {
            bits |= Self::GAMEPAD_5;
        }
        if value.gamepad_6 {
            bits |= Self::GAMEPAD_6;
        }
        if value.gamepad_7 {
            bits |= Self::GAMEPAD_7;
        }
        if value.gamepad_8 {
            bits |= Self::GAMEPAD_8;
        }
        if value.gamepad_9 {
            bits |= Self::GAMEPAD_9;
        }
        if value.gamepad_10 {
            bits |= Self::GAMEPAD_10;
        }
        if value.gamepad_11 {
            bits |= Self::GAMEPAD_11;
        }
        if value.gamepad_12 {
            bits |= Self::GAMEPAD_12;
        }
        if value.gamepad_13 {
            bits |= Self::GAMEPAD_13;
        }
        if value.gamepad_14 {
            bits |= Self::GAMEPAD_14;
        }
        if value.gamepad_15 {
            bits |= Self::GAMEPAD_15;
        }
        if value.gamepad_16 {
            bits |= Self::GAMEPAD_16;
        }

        bits
    }
}

impl From<ActiveGamepads2> for ActiveGamepads {
    fn from(value: ActiveGamepads2) -> Self {
        let mut bools = Self::default();

        if value.contains(ActiveGamepads2::GAMEPAD_1) {
            bools.gamepad_1 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_2) {
            bools.gamepad_2 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_3) {
            bools.gamepad_3 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_4) {
            bools.gamepad_4 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_5) {
            bools.gamepad_5 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_6) {
            bools.gamepad_6 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_7) {
            bools.gamepad_7 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_8) {
            bools.gamepad_8 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_9) {
            bools.gamepad_9 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_10) {
            bools.gamepad_10 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_11) {
            bools.gamepad_11 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_12) {
            bools.gamepad_12 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_13) {
            bools.gamepad_13 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_14) {
            bools.gamepad_14 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_15) {
            bools.gamepad_15 = true;
        }
        if value.contains(ActiveGamepads2::GAMEPAD_16) {
            bools.gamepad_16 = true;
        }

        bools
    }
}

#[derive(Debug, Record, Default)]
pub struct ControllerCapabilities {
    pub analog_triggers: bool,
    pub rumble: bool,
    pub trigger_rumble: bool,
    pub touchpad: bool,
    pub accel: bool,
    pub gyro: bool,
    pub battery_state: bool,
    pub rgb_led: bool,
}

impl From<ControllerCapabilities> for ControllerCapabilities2 {
    fn from(value: ControllerCapabilities) -> Self {
        let mut bits = Self::empty();

        if value.analog_triggers {
            bits |= Self::ANALOG_TRIGGERS;
        }
        if value.rumble {
            bits |= Self::RUMBLE;
        }
        if value.trigger_rumble {
            bits |= Self::TRIGGER_RUMBLE;
        }
        if value.touchpad {
            bits |= Self::TOUCHPAD;
        }
        if value.accel {
            bits |= Self::ACCEL;
        }
        if value.gyro {
            bits |= Self::GYRO;
        }
        if value.battery_state {
            bits |= Self::BATTERY_STATE;
        }
        if value.rgb_led {
            bits |= Self::RGB_LED;
        }

        bits
    }
}

impl From<ControllerCapabilities2> for ControllerCapabilities {
    fn from(value: ControllerCapabilities2) -> Self {
        let mut bools = Self::default();

        if value.contains(ControllerCapabilities2::ANALOG_TRIGGERS) {
            bools.analog_triggers = true;
        }
        if value.contains(ControllerCapabilities2::RUMBLE) {
            bools.rumble = true;
        }
        if value.contains(ControllerCapabilities2::TRIGGER_RUMBLE) {
            bools.trigger_rumble = true;
        }
        if value.contains(ControllerCapabilities2::TOUCHPAD) {
            bools.touchpad = true;
        }
        if value.contains(ControllerCapabilities2::ACCEL) {
            bools.accel = true;
        }
        if value.contains(ControllerCapabilities2::GYRO) {
            bools.gyro = true;
        }
        if value.contains(ControllerCapabilities2::BATTERY_STATE) {
            bools.battery_state = true;
        }
        if value.contains(ControllerCapabilities2::RGB_LED) {
            bools.rgb_led = true;
        }

        bools
    }
}

#[derive(Debug, Record, Default)]
pub struct ControllerButtons {
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub lb: bool,
    pub rb: bool,
    pub play: bool,
    pub back: bool,
    pub ls_clk: bool,
    pub rs_clk: bool,
    pub special: bool,
    pub paddle1: bool,
    pub paddle2: bool,
    pub paddle3: bool,
    pub paddle4: bool,
    pub touchpad: bool,
    pub misc: bool,
}

impl From<ControllerButtons> for ControllerButtons2 {
    fn from(value: ControllerButtons) -> Self {
        let mut bits = Self::empty();

        if value.a {
            bits |= Self::A;
        }
        if value.b {
            bits |= Self::B;
        }
        if value.x {
            bits |= Self::X;
        }
        if value.y {
            bits |= Self::Y;
        }
        if value.up {
            bits |= Self::UP;
        }
        if value.down {
            bits |= Self::DOWN;
        }
        if value.left {
            bits |= Self::LEFT;
        }
        if value.right {
            bits |= Self::RIGHT;
        }
        if value.lb {
            bits |= Self::LB;
        }
        if value.rb {
            bits |= Self::RB;
        }
        if value.play {
            bits |= Self::PLAY;
        }
        if value.back {
            bits |= Self::BACK;
        }
        if value.ls_clk {
            bits |= Self::LS_CLK;
        }
        if value.rs_clk {
            bits |= Self::RS_CLK;
        }
        if value.special {
            bits |= Self::SPECIAL;
        }
        if value.paddle1 {
            bits |= Self::PADDLE1;
        }
        if value.paddle2 {
            bits |= Self::PADDLE2;
        }
        if value.paddle3 {
            bits |= Self::PADDLE3;
        }
        if value.paddle4 {
            bits |= Self::PADDLE4;
        }
        if value.touchpad {
            bits |= Self::TOUCHPAD;
        }
        if value.misc {
            bits |= Self::MISC;
        }

        bits
    }
}

impl From<ControllerButtons2> for ControllerButtons {
    fn from(value: ControllerButtons2) -> Self {
        let mut bools = Self::default();

        if value.contains(ControllerButtons2::A) {
            bools.a = true;
        }
        if value.contains(ControllerButtons2::B) {
            bools.b = true;
        }
        if value.contains(ControllerButtons2::X) {
            bools.x = true;
        }
        if value.contains(ControllerButtons2::Y) {
            bools.y = true;
        }
        if value.contains(ControllerButtons2::UP) {
            bools.up = true;
        }
        if value.contains(ControllerButtons2::DOWN) {
            bools.down = true;
        }
        if value.contains(ControllerButtons2::LEFT) {
            bools.left = true;
        }
        if value.contains(ControllerButtons2::RIGHT) {
            bools.right = true;
        }
        if value.contains(ControllerButtons2::LB) {
            bools.lb = true;
        }
        if value.contains(ControllerButtons2::RB) {
            bools.rb = true;
        }
        if value.contains(ControllerButtons2::PLAY) {
            bools.play = true;
        }
        if value.contains(ControllerButtons2::BACK) {
            bools.back = true;
        }
        if value.contains(ControllerButtons2::LS_CLK) {
            bools.ls_clk = true;
        }
        if value.contains(ControllerButtons2::RS_CLK) {
            bools.rs_clk = true;
        }
        if value.contains(ControllerButtons2::SPECIAL) {
            bools.special = true;
        }
        if value.contains(ControllerButtons2::PADDLE1) {
            bools.paddle1 = true;
        }
        if value.contains(ControllerButtons2::PADDLE2) {
            bools.paddle2 = true;
        }
        if value.contains(ControllerButtons2::PADDLE3) {
            bools.paddle3 = true;
        }
        if value.contains(ControllerButtons2::PADDLE4) {
            bools.paddle4 = true;
        }
        if value.contains(ControllerButtons2::TOUCHPAD) {
            bools.touchpad = true;
        }
        if value.contains(ControllerButtons2::MISC) {
            bools.misc = true;
        }

        bools
    }
}

#[remote(Enum)]
pub enum BatteryState {
    Unknown,
    NotPresent,
    Discharging,
    Charging,
    NotCharging,
    Full,
}

#[remote(Enum)]
pub enum TerminationReason {
    Long(u32),
    /// Prefer Long over Short
    Short(u16),
}

#[derive(Debug, Enum)]
pub enum ControlPacket {
    ControllerRumbleData {
        unused: u32,
        controller_number: u16,
        low_frequency: u16,
        high_frequency: u16,
    },
    ControllerRumbleTriggers {
        controller_number: u16,
    },
    ControllerSetMotion {
        controller_number: u16,
        rate: u16,
        motion_type: MotionType,
    },
    ControllerSetLed {
        controller_number: u16,
        r: u8,
        g: u8,
        b: u8,
    },
    RequestIdr,
    StartB,
    PeriodicPing,
    HdrMode {
        enabled: bool,
        sunshine: Option<SunshineHdrMetadata>,
    },
    LossStats {
        unknown1: u32,
        loss_report_interval_ms: u32,
        unknown2: u32,
        last_good_frame: u64,
        unknown3: u32,
        unknown4: u32,
        unknown5: u32,
    },
    FrameStats {},
    FrameFec {
        frame_index: u32,
        highest_received_sequence_number: u16,
        next_contiguous_sequence_number: u16,
        missing_packets_before_highest_received: u16,
        total_data_packets: u16,
        total_parity_packets: u16,
        received_data_packets: u16,
        received_parity_packets: u16,
        fec_percentage: u8,
        multi_fec_block_index: u8,
        multi_fec_block_count: u8,
    },
    MouseMoveRelative {
        delta_x: i16,
        delta_y: i16,
    },
    MouseMoveAbsolute {
        x: i16,
        y: i16,
        unused: i16,
        reference_width: i16,
        reference_height: i16,
    },
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
    },
    Keyboard {
        action: KeyAction,
        flags: KeyFlags,
        key_code: KeyCode,
        modifiers: KeyModifiers,
        zero: i16,
    },
    Text {
        text: Vec<u8>,
    },
    MouseScroll {
        scroll_amount_1: i16,
        scroll_amount_2: i16,
        zero: i16,
    },
    MouseHorizontalScroll {
        scroll_amount: i16,
    },
    Touch {
        event_type: TouchEventType,
        reserved: u8,
        rotation: u16,
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
        zero: u8,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        rotation: u16,
        tilt: u8,
        zero2: u8,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
    ControllerState {
        header_b: i16,
        controller_number: i16,
        active_gamepad_mask: ActiveGamepads,
        mid_b: i16,
        button_flags: i16,
        left_trigger: u8,
        right_trigger: u8,
        left_stick_x: i16,
        left_stick_y: i16,
        right_stick_x: i16,
        right_stick_y: i16,
        tail_a: i16,
        button_flags_2: i16,
        tail_b: i16,
    },
    ControllerArrival {
        controller_number: u8,
        ty: ControllerType,
        capabilities: ControllerCapabilities,
        supported_buttons: ControllerButtons,
    },
    ControllerMotion {
        controller_number: u8,
        motion_type: MotionType,
        reserved1: u8,
        reserved2: u8,
        x: f32,
        y: f32,
        z: f32,
    },
    ControllerBattery {
        controller_number: u8,
        battery_state: BatteryState,
        battery_percentage: u8,
        reserved: u8,
    },
    InvalidateReferenceFrames {
        first_frame_index: u32,
        reserved1: u32,
        last_frame_index: u32,
        reserved2: u32,
        reserved3: u32,
        reserved4: u32,
    },
    LongTermReferenceFrameAcknowledgement {
        frame_index: u32,
        reserved: u32,
    },
    ServerTermination {
        reason: TerminationReason,
    },
}

impl From<ControlPacket> for ControlPacket2 {
    fn from(value: ControlPacket) -> Self {
        match value {
            ControlPacket::ControllerRumbleData {
                unused,
                controller_number,
                low_frequency,
                high_frequency,
            } => Self::ControllerRumbleData {
                unused,
                controller_number,
                low_frequency,
                high_frequency,
            },

            ControlPacket::ControllerRumbleTriggers { controller_number } => {
                Self::ControllerRumbleTriggers { controller_number }
            }

            ControlPacket::ControllerSetMotion {
                controller_number,
                rate,
                motion_type,
            } => Self::ControllerSetMotion {
                controller_number,
                rate,
                motion_type,
            },

            ControlPacket::ControllerSetLed {
                controller_number,
                r,
                g,
                b,
            } => Self::ControllerSetLed {
                controller_number,
                r,
                g,
                b,
            },

            ControlPacket::RequestIdr => Self::RequestIdr,
            ControlPacket::StartB => Self::StartB,
            ControlPacket::PeriodicPing => Self::PeriodicPing,

            ControlPacket::HdrMode { enabled, sunshine } => Self::HdrMode {
                enabled,
                sunshine: sunshine.map(Into::into),
            },

            ControlPacket::LossStats {
                unknown1,
                loss_report_interval_ms,
                unknown2,
                last_good_frame,
                unknown3,
                unknown4,
                unknown5,
            } => Self::LossStats {
                unknown1,
                loss_report_interval_ms,
                unknown2,
                last_good_frame,
                unknown3,
                unknown4,
                unknown5,
            },

            ControlPacket::FrameStats {} => Self::FrameStats {},

            ControlPacket::FrameFec {
                frame_index,
                highest_received_sequence_number,
                next_contiguous_sequence_number,
                missing_packets_before_highest_received,
                total_data_packets,
                total_parity_packets,
                received_data_packets,
                received_parity_packets,
                fec_percentage,
                multi_fec_block_index,
                multi_fec_block_count,
            } => Self::FrameFec {
                frame_index,
                highest_received_sequence_number,
                next_contiguous_sequence_number,
                missing_packets_before_highest_received,
                total_data_packets,
                total_parity_packets,
                received_data_packets,
                received_parity_packets,
                fec_percentage,
                multi_fec_block_index,
                multi_fec_block_count,
            },

            ControlPacket::MouseMoveRelative { delta_x, delta_y } => {
                Self::MouseMoveRelative { delta_x, delta_y }
            }

            ControlPacket::MouseMoveAbsolute {
                x,
                y,
                unused,
                reference_width,
                reference_height,
            } => Self::MouseMoveAbsolute {
                x,
                y,
                unused,
                reference_width,
                reference_height,
            },

            ControlPacket::MouseButton { action, button } => Self::MouseButton { action, button },

            ControlPacket::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
                zero,
            } => Self::Keyboard {
                action,
                flags: flags.into(),
                key_code,
                modifiers: modifiers.into(),
                zero,
            },

            ControlPacket::Text { text } => {
                let mut buf = [0u8; UTF8_TEXT_MAX_COUNT];
                let text_len = text.len().min(UTF8_TEXT_MAX_COUNT);

                buf[..text_len].copy_from_slice(&text[..text_len]);

                Self::Text {
                    text: buf,
                    text_len,
                }
            }

            ControlPacket::MouseScroll {
                scroll_amount_1,
                scroll_amount_2,
                zero,
            } => Self::MouseScroll {
                scroll_amount_1,
                scroll_amount_2,
                zero,
            },

            ControlPacket::MouseHorizontalScroll { scroll_amount } => {
                Self::MouseHorizontalScroll { scroll_amount }
            }

            ControlPacket::Touch {
                event_type,
                reserved,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            } => Self::Touch {
                event_type,
                reserved,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            },

            ControlPacket::Pen {
                event_type,
                tool_type,
                buttons,
                zero,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                zero2,
                contact_area_minor,
                contact_area_major,
            } => Self::Pen {
                event_type,
                tool_type,
                buttons: buttons.into(),
                zero,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                zero2,
                contact_area_minor,
                contact_area_major,
            },

            ControlPacket::ControllerState {
                header_b,
                controller_number,
                active_gamepad_mask,
                mid_b,
                button_flags,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
                tail_a,
                button_flags_2,
                tail_b,
            } => Self::ControllerState {
                header_b,
                controller_number,
                active_gamepad_mask: active_gamepad_mask.into(),
                mid_b,
                button_flags,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
                tail_a,
                button_flags_2,
                tail_b,
            },

            ControlPacket::ControllerArrival {
                controller_number,
                ty,
                capabilities,
                supported_buttons,
            } => Self::ControllerArrival {
                controller_number,
                ty,
                capabilities: capabilities.into(),
                supported_buttons: supported_buttons.into(),
            },

            ControlPacket::ControllerMotion {
                controller_number,
                motion_type,
                reserved1,
                reserved2,
                x,
                y,
                z,
            } => Self::ControllerMotion {
                controller_number,
                motion_type,
                reserved: [reserved1, reserved2],
                x,
                y,
                z,
            },

            ControlPacket::ControllerBattery {
                controller_number,
                battery_state,
                battery_percentage,
                reserved,
            } => Self::ControllerBattery {
                controller_number,
                battery_state,
                battery_percentage,
                reserved,
            },

            ControlPacket::InvalidateReferenceFrames {
                first_frame_index,
                reserved1,
                last_frame_index,
                reserved2,
                reserved3,
                reserved4,
            } => Self::InvalidateReferenceFrames {
                first_frame_index,
                reserved1,
                last_frame_index,
                reserved2: [reserved2, reserved3, reserved4],
            },

            ControlPacket::LongTermReferenceFrameAcknowledgement {
                frame_index,
                reserved,
            } => Self::LongTermReferenceFrameAcknowledgement {
                frame_index,
                reserved,
            },

            ControlPacket::ServerTermination { reason } => Self::ServerTermination { reason },
        }
    }
}

impl From<ControlPacket2> for ControlPacket {
    fn from(value: ControlPacket2) -> Self {
        match value {
            ControlPacket2::ControllerRumbleData {
                unused,
                controller_number,
                low_frequency,
                high_frequency,
            } => Self::ControllerRumbleData {
                unused,
                controller_number,
                low_frequency,
                high_frequency,
            },

            ControlPacket2::ControllerRumbleTriggers { controller_number } => {
                Self::ControllerRumbleTriggers { controller_number }
            }

            ControlPacket2::ControllerSetMotion {
                controller_number,
                rate,
                motion_type,
            } => Self::ControllerSetMotion {
                controller_number,
                rate,
                motion_type,
            },

            ControlPacket2::ControllerSetLed {
                controller_number,
                r,
                g,
                b,
            } => Self::ControllerSetLed {
                controller_number,
                r,
                g,
                b,
            },

            ControlPacket2::RequestIdr => Self::RequestIdr,
            ControlPacket2::StartB => Self::StartB,
            ControlPacket2::PeriodicPing => Self::PeriodicPing,

            ControlPacket2::HdrMode { enabled, sunshine } => Self::HdrMode {
                enabled,
                sunshine: sunshine.map(Into::into),
            },

            ControlPacket2::LossStats {
                unknown1,
                loss_report_interval_ms,
                unknown2,
                last_good_frame,
                unknown3,
                unknown4,
                unknown5,
            } => Self::LossStats {
                unknown1,
                loss_report_interval_ms,
                unknown2,
                last_good_frame,
                unknown3,
                unknown4,
                unknown5,
            },

            ControlPacket2::FrameStats {} => Self::FrameStats {},

            ControlPacket2::FrameFec {
                frame_index,
                highest_received_sequence_number,
                next_contiguous_sequence_number,
                missing_packets_before_highest_received,
                total_data_packets,
                total_parity_packets,
                received_data_packets,
                received_parity_packets,
                fec_percentage,
                multi_fec_block_index,
                multi_fec_block_count,
            } => Self::FrameFec {
                frame_index,
                highest_received_sequence_number,
                next_contiguous_sequence_number,
                missing_packets_before_highest_received,
                total_data_packets,
                total_parity_packets,
                received_data_packets,
                received_parity_packets,
                fec_percentage,
                multi_fec_block_index,
                multi_fec_block_count,
            },

            ControlPacket2::MouseMoveRelative { delta_x, delta_y } => {
                Self::MouseMoveRelative { delta_x, delta_y }
            }

            ControlPacket2::MouseMoveAbsolute {
                x,
                y,
                unused,
                reference_width,
                reference_height,
            } => Self::MouseMoveAbsolute {
                x,
                y,
                unused,
                reference_width,
                reference_height,
            },

            ControlPacket2::MouseButton { action, button } => Self::MouseButton { action, button },

            ControlPacket2::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
                zero,
            } => Self::Keyboard {
                action,
                flags: flags.into(),
                key_code,
                modifiers: modifiers.into(),
                zero,
            },

            ControlPacket2::Text { text, text_len } => Self::Text {
                text: text[..text_len].to_vec(),
            },

            ControlPacket2::MouseScroll {
                scroll_amount_1,
                scroll_amount_2,
                zero,
            } => Self::MouseScroll {
                scroll_amount_1,
                scroll_amount_2,
                zero,
            },

            ControlPacket2::MouseHorizontalScroll { scroll_amount } => {
                Self::MouseHorizontalScroll { scroll_amount }
            }

            ControlPacket2::Touch {
                event_type,
                reserved,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            } => Self::Touch {
                event_type,
                reserved,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            },

            ControlPacket2::Pen {
                event_type,
                tool_type,
                buttons,
                zero,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                zero2,
                contact_area_minor,
                contact_area_major,
            } => Self::Pen {
                event_type,
                tool_type,
                buttons: buttons.into(),
                zero,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                zero2,
                contact_area_minor,
                contact_area_major,
            },

            ControlPacket2::ControllerState {
                header_b,
                controller_number,
                active_gamepad_mask,
                mid_b,
                button_flags,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
                tail_a,
                button_flags_2,
                tail_b,
            } => Self::ControllerState {
                header_b,
                controller_number,
                active_gamepad_mask: active_gamepad_mask.into(),
                mid_b,
                button_flags,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
                tail_a,
                button_flags_2,
                tail_b,
            },

            ControlPacket2::ControllerArrival {
                controller_number,
                ty,
                capabilities,
                supported_buttons,
            } => Self::ControllerArrival {
                controller_number,
                ty,
                capabilities: capabilities.into(),
                supported_buttons: supported_buttons.into(),
            },

            ControlPacket2::ControllerMotion {
                controller_number,
                motion_type,
                reserved,
                x,
                y,
                z,
            } => Self::ControllerMotion {
                controller_number,
                motion_type,
                reserved1: reserved[0],
                reserved2: reserved[1],
                x,
                y,
                z,
            },

            ControlPacket2::ControllerBattery {
                controller_number,
                battery_state,
                battery_percentage,
                reserved,
            } => Self::ControllerBattery {
                controller_number,
                battery_state,
                battery_percentage,
                reserved,
            },

            ControlPacket2::InvalidateReferenceFrames {
                first_frame_index,
                reserved1,
                last_frame_index,
                reserved2,
            } => Self::InvalidateReferenceFrames {
                first_frame_index,
                reserved1,
                last_frame_index,
                reserved2: reserved2[0],
                reserved3: reserved2[1],
                reserved4: reserved2[2],
            },

            ControlPacket2::LongTermReferenceFrameAcknowledgement {
                frame_index,
                reserved,
            } => Self::LongTermReferenceFrameAcknowledgement {
                frame_index,
                reserved,
            },

            ControlPacket2::ServerTermination { reason } => Self::ServerTermination { reason },
        }
    }
}
