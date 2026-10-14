//! Opt-in local traces. No paths, source contents, or network telemetry are recorded.
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() -> Option<tracing_chrome::FlushGuard> {
    let path = std::env::var_os("DIFFY_TRACE_PATH")?;
    let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file(path)
        .include_args(false)
        .build();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "diffy=info,gpui_component::diff=info".into());
    if tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init()
        .is_err()
    {
        return None;
    }
    Some(guard)
}
