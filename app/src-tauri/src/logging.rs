use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_appender::rolling;

/// Initialize logging. Must be called once at app startup.
/// Returns the WorkerGuard which must be kept alive for the program's lifetime
/// to avoid losing buffered log lines.
pub fn init(app: &tauri::AppHandle) -> Result<tracing_appender::non_blocking::WorkerGuard, anyhow::Error> {
    use tauri::Manager;
    let log_dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = rolling::daily(&log_dir, "lda.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,audiopipe=warn,llama_cpp_2=warn,hyper=warn,ort=warn"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();

    tracing::info!(
        log_dir = %log_dir.display(),
        "logging initialized"
    );

    Ok(guard)
}
