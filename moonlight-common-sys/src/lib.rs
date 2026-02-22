#![feature(c_variadic)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::c_char;

use printf_compat::{format, output};

pub mod limelight {
    include!(concat!(env!("OUT_DIR"), "/limelight.rs"));
}

pub trait LogMessageCallback {
    fn log_message(text: String);
}

/// Wraps the log_message function
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn log_message_wrapper<C>(message: *const c_char, args: ...)
where
    C: LogMessageCallback,
{
    unsafe {
        let mut text = String::new();
        format(message, args, output::fmt_write(&mut text));

        C::log_message(text);
    }
}
