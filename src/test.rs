use tracing::Level;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub fn init_test() {
    // init tracing
    let _ = FmtSubscriber::builder()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(Level::TRACE.into())
                .from_env_lossy(),
        )
        .with_test_writer()
        .try_init();
}
