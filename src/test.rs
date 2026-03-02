use tracing::{Level, Span, info_span};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use venator::Venator;

#[macro_export]
macro_rules! init_test {
    () => {
        let __guard = $crate::test::init_test_priv(module_path!(), line!());
        let __guard = __guard.enter();
    };
}

pub fn init_test_priv(module: &str, line: u32) -> Span {
    let venator = Venator::default();

    // init tracing
    let _ = tracing_subscriber::registry()
        .with(venator)
        .with(fmt::layer().with_test_writer())
        .with(
            EnvFilter::builder()
                .with_default_directive(Level::TRACE.into())
                .from_env_lossy(),
        )
        .try_init();

    info_span!("test", module = module, line = line)
}

pub fn init_test() -> Span {
    init_test_priv("test", 0)
}
