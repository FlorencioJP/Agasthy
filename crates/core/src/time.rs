use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch. All timestamps in AGASTHY use this unit.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Convert a [`SystemTime`] (for example a file mtime) to epoch milliseconds.
pub fn system_time_ms(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}
