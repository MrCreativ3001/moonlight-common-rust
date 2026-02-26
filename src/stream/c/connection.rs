use std::{
    os::raw::{c_int, c_uchar, c_ushort},
    sync::Mutex,
};

use moonlight_common_sys::{
    LogMessageCallback, limelight::_CONNECTION_LISTENER_CALLBACKS, log_message_wrapper,
};
use num::FromPrimitive;

use crate::stream::{
    c::bindings::{ConnectionStatus, Stage},
    connection::ConnectionListener,
};

static GLOBAL_CONNECTION_LISTENER: Mutex<
    Option<(
        Box<dyn ConnectionListener + Send + 'static>,
        Box<dyn ConnectionListenerC + Send + 'static>,
    )>,
> = Mutex::new(None);

fn global_listener<R>(
    f: impl FnOnce((&mut dyn ConnectionListener, &mut dyn ConnectionListenerC)) -> R,
) -> R {
    let lock = GLOBAL_CONNECTION_LISTENER.lock();
    let mut lock = lock.expect("global connection listener");

    let (listener, listener_c) = lock.as_mut().expect("global connection listener");
    f((listener.as_mut(), listener_c.as_mut()))
}

pub(crate) fn set_global(
    listener: impl ConnectionListener + Send + 'static,
    listener_c: impl ConnectionListenerC + Send + 'static,
) {
    let mut global_listener = GLOBAL_CONNECTION_LISTENER
        .lock()
        .expect("global connection lock");

    *global_listener = Some((Box::new(listener), Box::new(listener_c)));
}
pub(crate) fn clear_global() {
    let mut decoder = GLOBAL_CONNECTION_LISTENER
        .lock()
        .expect("global video decoder");

    *decoder = None;
}

unsafe extern "C" fn stage_starting(stage: c_int) {
    global_listener(|(_, listener)| {
        listener.stage_starting(Stage::from_i32(stage).expect("valid stage"));
    });
}
unsafe extern "C" fn stage_complete(stage: c_int) {
    global_listener(|(_, listener)| {
        listener.stage_complete(Stage::from_i32(stage).expect("valid stage"));
    });
}
unsafe extern "C" fn stage_failed(stage: c_int, error_code: c_int) {
    global_listener(|(_, listener)| {
        listener.stage_failed(Stage::from_i32(stage).expect("valid stage"), error_code);
    });
}
unsafe extern "C" fn connection_started() {
    global_listener(|(_, listener)| {
        listener.connection_started();
    });
}
unsafe extern "C" fn connection_terminated(error_code: c_int) {
    global_listener(|(_, listener)| {
        listener.connection_terminated(error_code);
    });
}
unsafe extern "C" fn connection_status_update(status: c_int) {
    global_listener(|(_, listener)| {
        listener.connection_status_update(
            ConnectionStatus::from_i32(status).expect("valid connection status"),
        );
    });
}

struct LogMessage;

impl LogMessageCallback for LogMessage {
    fn log_message(text: String) {
        global_listener(|(_, listener)| unsafe {
            listener.log_message(&text);
        });
    }
}

unsafe extern "C" fn set_hdr_mode(hdr_enabled: bool) {
    global_listener(|(listener, _)| {
        listener.set_hdr_mode(hdr_enabled);
    })
}

unsafe extern "C" fn controller_rumble(
    controller_number: c_ushort,
    low_frequency_motor: c_ushort,
    high_frequency_motor: c_ushort,
) {
    global_listener(|(listener, _)| {
        listener.controller_rumble(controller_number, low_frequency_motor, high_frequency_motor);
    });
}
unsafe extern "C" fn controller_rumble_triggers(
    controller_number: c_ushort,
    left_trigger_motor: c_ushort,
    right_trigger_motor: c_ushort,
) {
    global_listener(|(listener, _)| {
        listener.controller_rumble_triggers(
            controller_number,
            left_trigger_motor,
            right_trigger_motor,
        );
    });
}
unsafe extern "C" fn controller_set_motion_event_state(
    controller_number: c_ushort,
    motion_type: c_uchar,
    report_rate_hz: c_ushort,
) {
    global_listener(|(listener, _)| {
        listener.controller_set_motion_event_state(controller_number, motion_type, report_rate_hz);
    })
}
unsafe extern "C" fn controller_set_led(controller_number: u16, r: u8, g: u8, b: u8) {
    global_listener(|(listener, _)| {
        listener.controller_set_led(controller_number, r, g, b);
    })
}
unsafe extern "C" fn controller_set_adaptive_triggers(
    controller_number: c_ushort,
    event_flags: c_uchar,
    type_left: c_uchar,
    type_right: c_uchar,
    left: *mut c_uchar,
    right: *mut c_uchar,
) {
    global_listener(|(listener, _)| {
        let (left, right) = unsafe { (&mut *left, &mut *right) };

        listener.controller_set_adaptive_triggers(
            controller_number,
            event_flags,
            type_left,
            type_right,
            left,
            right,
        );
    })
}

pub(crate) unsafe fn raw_callbacks() -> _CONNECTION_LISTENER_CALLBACKS {
    _CONNECTION_LISTENER_CALLBACKS {
        stageStarting: Some(stage_starting),
        stageComplete: Some(stage_complete),
        stageFailed: Some(stage_failed),
        connectionStarted: Some(connection_started),
        connectionTerminated: Some(connection_terminated),
        logMessage: Some(log_message_wrapper::<LogMessage>),
        rumble: Some(controller_rumble),
        connectionStatusUpdate: Some(connection_status_update),
        setHdrMode: Some(set_hdr_mode),
        rumbleTriggers: Some(controller_rumble_triggers),
        setMotionEventState: Some(controller_set_motion_event_state),
        setControllerLED: Some(controller_set_led),
        setAdaptiveTriggers: Some(controller_set_adaptive_triggers),
    }
}

/// This adds special connection events specifically for moonlight common c
pub trait ConnectionListenerC {
    /// This callback is invoked to indicate that a stage of initialization is about to begin
    fn stage_starting(&mut self, stage: Stage);
    /// This callback is invoked to indicate that a stage of initialization has completed
    fn stage_complete(&mut self, stage: Stage);

    /// This callback is invoked to indicate that a stage of initialization has failed.
    /// ConnListenerConnectionTerminated() will not be invoked because the connection was
    /// not yet fully established. LiInterruptConnection() and LiStopConnection() may
    /// result in this callback being invoked, but it is not guaranteed.
    fn stage_failed(&mut self, stage: Stage, error_code: i32);

    /// This callback is invoked after the connection is successfully established
    fn connection_started(&mut self);

    /// This callback is invoked when a connection is terminated after establishment.
    /// The errorCode will be 0 if the termination was reported to be intentional
    /// from the server (for example, the user closed the game). If errorCode is
    /// non-zero, it means the termination was probably unexpected (loss of network,
    /// crash, or similar conditions). This will not be invoked as a result of a call
    /// to LiStopConnection() or LiInterruptConnection().
    /// HINT: Use TerminationError
    fn connection_terminated(&mut self, error_code: i32);

    /// This callback is invoked to log debug message
    fn log_message(&mut self, message: &str);

    /// This callback is used to notify the client of a connection status change.
    /// Consider displaying an overlay for the user to notify them why their stream
    /// is not performing as expected.
    fn connection_status_update(&mut self, status: ConnectionStatus);
}
