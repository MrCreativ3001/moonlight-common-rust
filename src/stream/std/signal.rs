use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tracing::debug;


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
    pub fn stop(&self) {
        debug!("sending stop signal");

        self.inner.store(true, Ordering::SeqCst);
    }

    /// Check whether the signal has been notified
    pub fn is_notified(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}
