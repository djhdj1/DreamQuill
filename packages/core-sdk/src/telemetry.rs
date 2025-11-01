use std::{fs::OpenOptions, io::Write, path::PathBuf};

use anyhow::Result;
use once_cell::sync::Lazy;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

static TELEMETRY_ENABLED: Lazy<std::sync::RwLock<bool>> =
    Lazy::new(|| std::sync::RwLock::new(false));

/**
 * \brief 更新遥测开关状态。
 */
pub fn set_enabled(enabled: bool) {
    if let Ok(mut guard) = TELEMETRY_ENABLED.write() {
        *guard = enabled;
    }
}

/**
 * \brief 查询当前遥测开关状态。
 */
pub fn is_enabled() -> bool {
    TELEMETRY_ENABLED.read().map(|g| *g).unwrap_or(false)
}

/**
 * \brief 记录常规事件。
 */
pub fn log_event(category: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    if let Err(err) = write_line("INFO", category, message) {
        eprintln!("telemetry write failed: {}", err);
    }
}

/**
 * \brief 记录错误事件。
 */
pub fn log_error(category: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    if let Err(err) = write_line("ERROR", category, message) {
        eprintln!("telemetry write failed: {}", err);
    }
}

fn write_line(level: &str, category: &str, message: &str) -> Result<()> {
    let log_dir = PathBuf::from("logs");
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)?;
    }
    let timestamp = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("dreamquill.log"))?;
    writeln!(file, "{} [{}] {} - {}", timestamp, level, category, message)?;
    Ok(())
}
