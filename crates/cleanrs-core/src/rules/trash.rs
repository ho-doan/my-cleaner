use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::cmp::Reverse;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct TrashCleaner;

impl Cleaner for TrashCleaner {
    fn id(&self) -> &'static str {
        "trash"
    }

    fn display_name(&self) -> &'static str {
        "macOS Trash"
    }

    fn category(&self) -> Category {
        Category::ManualReview
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Destructive
    }

    fn is_available(&self) -> bool {
        command_available("osascript") && trash_path().is_some_and(|path| path.is_dir())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = trash_path() else {
            return Ok(Vec::new());
        };
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&path).with_context(|| format!("read {}", path.display()))? {
            let entry = entry?;
            let entry_path = entry.path();
            let metadata = fs::symlink_metadata(&entry_path)?;
            let size_bytes = dir_size(&entry_path)?;
            entries.push(TrashEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                size_bytes,
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let total_size = entries.iter().map(|entry| entry.size_bytes).sum();
        let largest = names_of(&mut entries.clone(), |entry| Reverse(entry.size_bytes));
        let newest = names_of(&mut entries, |entry| Reverse(entry.modified));
        let description = format!(
            "{} item(s); largest: {}; newest: {}",
            largest.0, largest.1, newest.1
        );

        Ok(vec![CleanTarget {
            path,
            size_bytes: total_size,
            description,
            method: CleanMethod::RunCommand(vec![
                "osascript".to_owned(),
                "-e".to_owned(),
                "tell application \"Finder\" to empty trash".to_owned(),
            ]),
        }])
    }
}

#[derive(Clone)]
struct TrashEntry {
    name: String,
    size_bytes: u64,
    modified: SystemTime,
}

fn trash_path() -> Option<PathBuf> {
    home_path(".Trash")
}

fn names_of<K, F>(entries: &mut [TrashEntry], key: F) -> (usize, String)
where
    K: Ord,
    F: FnMut(&TrashEntry) -> K,
{
    entries.sort_by_key(key);
    let count = entries.len();
    let names = entries
        .iter()
        .take(3)
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    (
        count,
        if names.is_empty() {
            "-".to_owned()
        } else {
            names
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{names_of, TrashEntry};
    use std::time::{Duration, SystemTime};

    #[test]
    fn orders_trash_items_for_summary() {
        let mut entries = vec![
            TrashEntry {
                name: "small".to_owned(),
                size_bytes: 1,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
            },
            TrashEntry {
                name: "large".to_owned(),
                size_bytes: 20,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(2),
            },
        ];
        let summary = names_of(&mut entries, |entry| std::cmp::Reverse(entry.size_bytes));
        assert_eq!(summary.0, 2);
        assert_eq!(summary.1, "large, small");
    }
}
