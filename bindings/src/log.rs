use std::fmt::Debug;
use tracing::{Level, Subscriber, level_filters::LevelFilter};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};
use uniffi::{Enum, export};

use crate::MoonlightError;

#[derive(Debug, Default, Enum)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            Level::DEBUG => LogLevel::Debug,
            Level::TRACE => LogLevel::Trace,
        }
    }
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => LevelFilter::ERROR,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Trace => LevelFilter::TRACE,
        }
    }
}

#[export(callback_interface)]
pub trait Logger: Send + Sync + Debug {
    fn log(&self, level: LogLevel, message: String);
}

struct LoggerWrapper {
    inner: Box<dyn Logger>,
}

impl<S> Layer<S> for LoggerWrapper
where
    S: Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut buf = String::new();
        let mut visitor = StringVisitor(&mut buf);
        event.record(&mut visitor);

        let level = event.metadata().level();

        self.inner.log(LogLevel::from(*level), buf);
    }
}

struct StringVisitor<'a>(&'a mut String);
impl<'a> tracing::field::Visit for StringVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;

        if field.name() == "message" {
            // Write the main message text directly
            let _ = write!(self.0, "{:?} ", value);
        } else {
            // Append explicit arguments as "key=value" pairs
            let _ = write!(self.0, "{}={:?} ", field.name(), value);
        }
    }
}

#[export]
pub fn set_logger(logger: Box<dyn Logger>, filter: LogLevel) -> Result<(), MoonlightError> {
    tracing_subscriber::registry()
        .with(LevelFilter::from(filter))
        .with(LoggerWrapper { inner: logger })
        .try_init()?;
    Ok(())
}
