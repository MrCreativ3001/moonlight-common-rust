use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tracing::debug;

use crate::stream::std::MoonlightStreamError;

#[derive(Clone)]
pub struct StopSignal {
    inner: Arc<AtomicBool>,
}

impl StopSignal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the signal to "true", indicating stop
    pub fn stop_graceful(&self) {
        debug!("sending stop signal");

        self.inner.store(true, Ordering::SeqCst);
    }
    pub fn stop_with_error(&self, error: MoonlightStreamError) {
        debug!(error = ?error, "sending stop signal with error");

        self.inner.store(true, Ordering::SeqCst);

        todo!();
    }

    /// Check whether the signal has been notified
    pub fn is_notified(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}
