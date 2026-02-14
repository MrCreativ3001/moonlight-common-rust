pub trait ConnectionListener {
    /// This callback is invoked to notify the client of a change in HDR mode on
    /// the host. The client will probably want to update the local display mode
    /// to match the state of HDR on the host. This callback may be invoked even
    /// if the stream is not using an HDR-capable codec.
    fn set_hdr_mode(&mut self, hdr_enabled: bool);

    /// This callback is invoked to rumble a gamepad. The rumble effect values
    /// set in this callback are expected to persist until a future call sets a
    /// different haptic effect or turns off the motors by passing 0 for both
    /// motors. It is possible to receive rumble events for gamepads that aren't
    /// physically present, so your callback should handle this possibility.
    fn controller_rumble(
        &mut self,
        controller_number: u16,
        low_frequency_motor: u16,
        high_frequency_motor: u16,
    );

    /// This callback is invoked to rumble a gamepad's triggers. For more details,
    /// see the comment above on ConnListenerRumble().
    fn controller_rumble_triggers(
        &mut self,
        controller_number: u16,
        left_trigger_motor: u16,
        right_trigger_motor: u16,
    );

    /// This callback is invoked to notify the client that the host would like motion
    /// sensor reports for the specified gamepad (see LiSendControllerMotionEvent())
    /// at the specified reporting rate (or as close as possible).
    ///
    /// If reportRateHz is 0, the host is asking for motion event reporting to stop.
    fn controller_set_motion_event_state(
        &mut self,
        controller_number: u16,
        motion_type: u8,
        report_rate_hz: u16,
    );

    /// This callback is invoked to notify the client of a change in the dualsense
    /// adaptive trigger configuration.
    fn controller_set_adaptive_triggers(
        &mut self,
        controller_number: u16,
        event_flags: u8,
        type_left: u8,
        type_right: u8,
        left: &mut u8,
        right: &mut u8,
    );

    /// This callback is invoked to set a controller's RGB LED (if present).
    fn controller_set_led(&mut self, controller_number: u16, r: u8, g: u8, b: u8);
}
