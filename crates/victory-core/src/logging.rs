//! One tracing setup for every binary.
//!
//! Filter with `VICTORY_LOG` (falls back to `RUST_LOG`), e.g.
//! `VICTORY_LOG=victory_loader=trace,info`. Debug and trace output exist
//! only in builds with the `diagnostics` feature; a plain release build
//! logs info and above whatever the filter asks for.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The most verbose level this build can log.
const CAP: LevelFilter = if cfg!(feature = "diagnostics") {
    LevelFilter::TRACE
} else {
    LevelFilter::INFO
};

pub fn init(default_level: &str) {
    let filter = std::env::var("VICTORY_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| EnvFilter::new(default_level));
    let _ = tracing_subscriber::registry()
        .with(CAP)
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_writer(writer),
        )
        .try_init();
}

/// Where events go, and whether each one reaches the file as it is logged.
///
/// A run that is killed, or whose machine goes away mid-session, used to leave
/// the log cut at whatever the writer still held — the last operations before
/// a device removal are exactly the ones that were lost. `VICTORY_LOG_FLUSH=1`
/// flushes after every event, so a truncated log is truncated at the real last
/// event rather than at a buffer boundary.
fn writer() -> FlushWriter {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    FlushWriter(*ON.get_or_init(|| std::env::var_os("VICTORY_LOG_FLUSH").is_some()))
}

struct FlushWriter(bool);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FlushWriter {
    type Writer = FlushWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FlushWriter(self.0)
    }
}

impl std::io::Write for FlushWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut out = std::io::stderr();
        let n = out.write(buf)?;
        if self.0 {
            out.flush()?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}
