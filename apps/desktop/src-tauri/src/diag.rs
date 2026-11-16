//! A small local diagnostics log, so a problem on someone's machine can be
//! looked at without telemetry. It lives in the app data folder
//! (`logs/agasthy.log`, rotated at 2 MB, one previous file kept), is never
//! uploaded, and records only: start-up (version, OS), crashes (panics),
//! background-task failures and scan failures. The home folder is written as `~`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOG: OnceLock<Mutex<PathBuf>> = OnceLock::new();
static HOME: OnceLock<String> = OnceLock::new();

pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Create the log folder, record start-up and install the crash hook.
pub fn init(data_dir: &Path, home: Option<&Path>, version: &str, os: &str) {
    let dir = log_dir(data_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = LOG.set(Mutex::new(dir.join("agasthy.log")));
    if let Some(h) = home {
        let _ = HOME.set(h.display().to_string());
    }
    write("start", &format!("AGASTHY {version} on {os}"));
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        write("panic", &format!("thread `{thread}`: {msg}{at}"));
        default(info);
    }));
}

/// Append one line. Never fails loudly: diagnostics must not cause problems.
pub fn write(level: &str, message: &str) {
    let Some(lock) = LOG.get() else { return };
    let Ok(path) = lock.lock() else { return };
    if std::fs::metadata(&*path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&*path, path.with_extension("log.1"));
    }
    let mut msg = message.replace(['\n', '\r'], " ");
    if let Some(h) = HOME.get().filter(|h| h.len() > 1) {
        msg = msg.replace(h.as_str(), "~");
    }
    let line = format!(
        "{} {level:<5} {}\n",
        iso_now(),
        msg.chars().take(2000).collect::<String>()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn iso_now() -> String {
    let ms = agasthy_core::time::now_ms();
    let secs = ms.div_euclid(1000);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{}T{h:02}:{m:02}:{s:02}Z", crate::insights::date_of(ms))
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_and_redacts_home() {
        let d = tempfile::tempdir().unwrap();
        super::init(
            d.path(),
            Some(std::path::Path::new("/Users/someone")),
            "0.0.0",
            "test",
        );
        super::write("error", "scan failed in /Users/someone/code\nsecond line");
        let text = std::fs::read_to_string(d.path().join("logs/agasthy.log")).unwrap();
        assert!(text.contains("AGASTHY 0.0.0 on test"));
        assert!(text.contains("scan failed in ~/code second line"), "{text}");
        assert!(!text.contains("/Users/someone"));
    }
}
