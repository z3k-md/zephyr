use flexi_logger::{Cleanup, Criterion, FileSpec, Logger, LoggerHandle, Naming, WriteMode};
use std::path::PathBuf;
use std::sync::OnceLock;

static LOGGER: OnceLock<LoggerHandle> = OnceLock::new();

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const KEPT_LOG_FILES: usize = 4;

pub fn init() {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);

    let mut logger = Logger::try_with_str("info")
        .unwrap_or_else(|err| panic!("logger config failed: {err}"))
        .log_to_file(
            FileSpec::default()
                .directory(dir)
                .basename("zephyr")
                .suppress_timestamp(),
        )
        .rotate(
            Criterion::Size(MAX_LOG_BYTES),
            Naming::Numbers,
            Cleanup::KeepLogFiles(KEPT_LOG_FILES),
        )
        .append()
        .write_mode(WriteMode::Direct);

    if cfg!(debug_assertions) {
        logger = logger.duplicate_to_stderr(flexi_logger::Duplicate::Info);
    }

    match logger.start() {
        Ok(handle) => {
            let _ = LOGGER.set(handle);
        }
        Err(err) => eprintln!("Zephyr logging failed: {err}"),
    }

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
}

/// Logging starts before Tauri does, so this can't use Tauri's path resolver.
pub fn log_dir() -> PathBuf {
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("Zephyr");
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join("Zephyr")
        .join("logs")
}
