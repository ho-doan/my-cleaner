use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Return the audit-log path. Tests and controlled integrations may override it
/// without changing the user's normal history location.
pub fn history_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLEANRS_HISTORY_PATH") {
        return Some(PathBuf::from(path));
    }
    // Unit tests exercise the file writer with explicit temp paths. Avoid
    // appending test operations to a developer's real history file.
    if cfg!(test) {
        return None;
    }
    crate::platform::config::home_path(".cleanrs/history.log")
}

#[derive(Serialize)]
struct HistoryEntry {
    timestamp: u64,
    operation: String,
    actor: String,
    target: String,
    status: String,
    detail: String,
}

/// Append one JSON-lines history entry. Newlines are normalized so one
/// operation always occupies exactly one audit-log row.
pub fn append_history_at(
    path: &Path,
    operation: &str,
    actor: &str,
    target: &str,
    status: &str,
    detail: &str,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = HistoryEntry {
        timestamp,
        operation: sanitize(operation),
        actor: sanitize(actor),
        target: sanitize(target),
        status: sanitize(status),
        detail: sanitize(detail),
    };
    serde_json::to_writer(&mut file, &entry)?;
    writeln!(file)?;
    Ok(())
}

/// Record an event best-effort. Logging must never turn a successful cleanup
/// into a failed cleanup when the home directory is temporarily unavailable.
pub fn record_history(operation: &str, actor: &str, target: &str, status: &str, detail: &str) {
    let Some(path) = history_path() else {
        return;
    };
    let _ = append_history_at(&path, operation, actor, target, status, detail);
}

fn sanitize(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::append_history_at;
    use std::fs;

    #[test]
    fn appends_sanitized_history_entries() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("nested/history.log");

        append_history_at(
            &path,
            "clean",
            "npm",
            "/tmp/cache\nforged",
            "success",
            "moved\tto trash",
        )
        .expect("history should be written");

        let contents = fs::read_to_string(path).expect("history should be readable");
        assert_eq!(contents.lines().count(), 1);
        let entry: serde_json::Value = serde_json::from_str(contents.trim()).expect("JSONL");
        assert_eq!(entry["target"], "/tmp/cache forged");
        assert_eq!(entry["detail"], "moved to trash");
    }
}
