//! Structured log files: one JSON object per line, one file per process and day, the last
//! [`KEPT_DAYS`] kept. Writes are synchronous, so the lines just before a crash are on disk
//! when the watchdog restarts the agent.
//!
//! What may be logged: states, counts, durations, error kinds and codes. What may not:
//! window titles, URLs, file names, screenshot or event contents, keys and tokens. Home folder
//! paths are shortened to `~` as a safety net.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::Subscriber;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry, fmt};

use super::{HomeRedactor, ProcessKind, RedactingMakeWriter};

/// Overrides the log filter, for example `WORKLUME_LOG=debug`.
pub const LOG_FILTER_ENV: &str = "WORKLUME_LOG";

/// Daily files kept per process.
pub const KEPT_DAYS: usize = 7;

/// Worklume's crates at `info`; dependencies, such as Tauri and the webview, only at `warn`.
const DEFAULT_FILTER: &str = "warn,worklume_desktop=info,worklume_core=info,\
                              worklume_storage=info,worklume_sync=info,worklume_platform=info";

/// Where and what to log.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub dir: PathBuf,
    pub process: ProcessKind,
    pub redactor: HomeRedactor,
    /// A filter such as `debug` or `warn,worklume_core=trace`; `None` for the default.
    pub filter: Option<String>,
    /// Also print to stderr (debug builds, for `tauri dev`).
    pub echo_to_stderr: bool,
}

impl LogConfig {
    /// The configuration for this process: the app's log folder, the filter from
    /// [`LOG_FILTER_ENV`], and stderr output in debug builds.
    #[must_use]
    pub fn for_process(dir: PathBuf, process: ProcessKind) -> Self {
        Self {
            dir,
            process,
            redactor: HomeRedactor::current(),
            filter: std::env::var(LOG_FILTER_ENV).ok(),
            echo_to_stderr: cfg!(debug_assertions),
        }
    }
}

/// Why logging could not start.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("could not create the log folder: {0}")]
    Folder(#[source] io::Error),
    #[error("could not open the log file: {0}")]
    File(#[from] tracing_appender::rolling::InitError),
    #[error("a logger is already installed")]
    AlreadyInstalled,
}

/// Builds the subscriber for `config`, creating the log folder.
///
/// # Errors
///
/// Fails when the folder or the first log file cannot be created.
pub fn subscriber(config: &LogConfig) -> Result<Box<dyn Subscriber + Send + Sync>, LogError> {
    create_private_dir(&config.dir).map_err(LogError::Folder)?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(config.process.as_str())
        .filename_suffix("log")
        .max_log_files(KEPT_DAYS)
        .build(&config.dir)?;
    let files = fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .with_writer(RedactingMakeWriter::new(appender, config.redactor.clone()));
    let stderr = config.echo_to_stderr.then(|| {
        fmt::layer().with_writer(RedactingMakeWriter::new(
            io::stderr,
            config.redactor.clone(),
        ))
    });
    let filter = config
        .filter
        .as_deref()
        .and_then(|filter| EnvFilter::try_new(filter).ok())
        .unwrap_or_else(|| EnvFilter::new(DEFAULT_FILTER));
    Ok(Box::new(
        Registry::default().with(filter).with(files).with(stderr),
    ))
}

/// Installs the subscriber for the whole process, and routes the `log` records of dependencies
/// into it.
///
/// # Errors
///
/// Fails when the log file cannot be created or a logger is already installed.
pub fn init(config: &LogConfig) -> Result<(), LogError> {
    subscriber(config)?
        .try_init()
        .map_err(|_| LogError::AlreadyInstalled)
}

/// Logs every panic before the previously installed hook (the crash reporter's, then Rust's
/// default) runs.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("(no message)");
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()));
        tracing::error!(
            target: "worklume_desktop::panic",
            location = location.as_deref().unwrap_or("unknown"),
            "panic: {message}"
        );
        previous(info);
    }));
}

/// Creates a folder only the current user can enter (on Unix).
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}
