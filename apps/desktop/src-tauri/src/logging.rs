//! `tracing` setup: daily-rotated files in `%LOCALAPPDATA%\Granary\logs` (7 kept) plus stderr in debug builds,
//! every line passed through [`crate::redact`]. Filter via `GRANARY_LOG` (EnvFilter syntax), default `info`.
use crate::redact::RedactingWriter;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

/// Install the global subscriber. Keep the returned guard alive for the process lifetime (it flushes on drop).
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("GRANARY_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file = std::fs::create_dir_all(dir).ok().and_then(|()| {
        Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix("granary")
            .filename_suffix("log")
            .max_log_files(7)
            .build(dir)
            .ok()
    });
    let (file_layer, guard) = match file {
        Some(appender) => {
            let (nb, guard) = tracing_appender::non_blocking(appender);
            let make = move || RedactingWriter(nb.clone());
            (
                Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(make)),
                Some(guard),
            )
        }
        None => (None, None),
    };
    let stderr_layer = cfg!(debug_assertions)
        .then(|| tracing_subscriber::fmt::layer().with_writer(|| RedactingWriter(std::io::stderr())));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
    guard
}
