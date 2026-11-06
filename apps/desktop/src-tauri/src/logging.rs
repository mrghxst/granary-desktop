//! `tracing` setup: daily-rotated files in `%LOCALAPPDATA%\Granary\logs` (7 kept) plus stderr in debug builds,
//! every line passed through [`crate::redact`]. Filter via `GRANARY_LOG` (EnvFilter syntax), default `info`.
//!
//! Tauri ends the process with `std::process::exit`, which skips destructors, so the non-blocking writer's guard is
//! parked in a static and [`flush`] must be called on `RunEvent::Exit` and before any explicit exit (review M2).
use crate::redact::RedactingWriter;
use std::path::Path;
use std::sync::Mutex;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

static GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

fn file_layer<S>(dir: &Path) -> Option<impl tracing_subscriber::Layer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    std::fs::create_dir_all(dir).ok()?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("granary")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .ok()?;
    let (nb, guard) = tracing_appender::non_blocking(appender);
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
    Some(fmt_layer(move || RedactingWriter(nb.clone())))
}

/// The one formatting configuration used by every sink. ANSI is always off: colour codes split `key=value` and
/// would defeat redaction (review H4).
pub fn fmt_layer<S, W>(make: W) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer().with_ansi(false).with_writer(make)
}

/// Install the global subscriber.
pub fn init(dir: &Path) {
    let filter = EnvFilter::try_from_env("GRANARY_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = cfg!(debug_assertions).then(|| fmt_layer(|| RedactingWriter(std::io::stderr())));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer(dir))
        .with(stderr_layer)
        .try_init();
}

/// Flush and stop the file writer; later log lines go to stderr only. Idempotent.
pub fn flush() {
    if let Ok(mut g) = GUARD.lock() {
        drop(g.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if let Ok(mut v) = self.0.lock() {
                v.extend_from_slice(b);
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn real_layer_configuration_redacts_structured_fields() {
        let buf = Buf::default();
        let sink = buf.clone();
        let subscriber = tracing_subscriber::registry().with(fmt_layer(move || RedactingWriter(sink.clone())));
        tracing::subscriber::with_default(subscriber, || {
            let secret = "opaque-secret-value";
            tracing::info!(token = %secret, password = ?Some("pw-123"), "refreshed");
        });
        let out = buf
            .0
            .lock()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default();
        assert!(out.contains("refreshed"), "{out}");
        assert!(!out.contains("opaque-secret-value") && !out.contains("pw-123"), "{out}");
        assert!(!out.contains('\x1b'), "{out}");
    }
}
