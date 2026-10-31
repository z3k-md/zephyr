use flexi_logger::{Cleanup, Criterion, FileSpec, Logger, LoggerHandle, Naming, WriteMode};
use std::path::PathBuf;
use std::sync::OnceLock;

static LOGGER: OnceLock<LoggerHandle> = OnceLock::new();

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const KEPT_LOG_FILES: usize = 4;
const BASENAME: &str = "zephyr";

/// Dev builds also keep Zephyr's own debug lines. The updater plugin logs every failed
/// check as an error of its own; updater.rs already logs it, as the warning a network blip is.
const LEVELS: &str = if cfg!(debug_assertions) {
    "info, zephyr_lib=debug, tauri_plugin_updater=off"
} else {
    "info, tauri_plugin_updater=off"
};

pub fn init() {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);

    let mut logger = Logger::try_with_str(LEVELS)
        .unwrap_or_else(|err| panic!("logger config failed: {err}"))
        .log_to_file(
            FileSpec::default()
                .directory(dir)
                .basename(BASENAME)
                .suppress_timestamp(),
        )
        .rotate(
            Criterion::Size(MAX_LOG_BYTES),
            Naming::Numbers,
            Cleanup::KeepLogFiles(KEPT_LOG_FILES),
        )
        .append()
        .format_for_files(timestamped)
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
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        // Release builds are stripped, so their frames are addresses; dev builds name them.
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic on thread {name}: {info}\n{backtrace}");
        previous(info);
    }));
}

/// The first line of each run, so a log read later says which build wrote what follows.
pub fn session_started(version: &str) {
    log::info!(
        "Zephyr {version} ({}) starting on {} {}, pid {}",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id()
    );
}

/// `2026-10-19 09:36:57.120 INFO [zephyr_lib] Zephyr is running`: the time shows how long
/// startup took after login. The target is the module, or `webview` for a page's lines.
fn timestamped(
    out: &mut dyn std::io::Write,
    now: &mut flexi_logger::DeferredNow,
    record: &log::Record,
) -> std::io::Result<()> {
    write!(
        out,
        "{} {} [{}] {}",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        record.level(),
        record.target(),
        record.args()
    )
}

/// Logging starts before Tauri does, so this can't use Tauri's path resolver. Dev builds log
/// to a folder of their own, so a dev run's lines never mix with the installed app's.
pub fn log_dir() -> PathBuf {
    let dir = if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("Zephyr")
    } else {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir)
            .join("Zephyr")
            .join("logs")
    };
    if cfg!(debug_assertions) {
        dir.join("dev")
    } else {
        dir
    }
}

/// The file the log is writing to now; earlier runs' lines may come before this one's.
pub fn current_file() -> PathBuf {
    log_dir().join(format!("{BASENAME}_rCURRENT.log"))
}

/// The last `lines` lines of the current log file (fewer when it's shorter, as just after a
/// rotation).
pub fn tail(lines: usize) -> String {
    let text = std::fs::read_to_string(current_file()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// A line from a page's console (src/log.ts): warnings and errors, uncaught exceptions and
/// failed commands. `window` is the window's label, `page` its path.
pub fn webview(level: &str, window: &str, page: &str, text: &str) {
    const MAX_CHARS: usize = 4000;
    let level = match level {
        "info" => log::Level::Info,
        "warn" => log::Level::Warn,
        _ => log::Level::Error,
    };
    let text: String = text.chars().take(MAX_CHARS).collect();
    log::log!(target: "webview", level, "{window} {page}: {text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_builds_log_to_their_own_folder() {
        assert_eq!(log_dir().ends_with("dev"), cfg!(debug_assertions));
        assert_eq!(current_file().parent(), Some(log_dir().as_path()));
    }
}
