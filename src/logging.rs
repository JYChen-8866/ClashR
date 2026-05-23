use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

pub fn log_dir() -> PathBuf {
    crate::core::paths::log_dir()
}

/// Initialize the global logger.
///
/// Returns a guard that must be kept alive for the duration of the program;
/// dropping it flushes pending log messages to disk.
pub fn init() -> WorkerGuard {
    let dir = log_dir();
    std::fs::create_dir_all(&dir).ok();

    // Daily rotation, keep up to 7 most recent files
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("clashr")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&dir)
        .expect("failed to init file appender");

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("clashr=info,warn"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();

    tracing::info!(log_dir = %dir.display(), "logger initialized");

    guard
}
