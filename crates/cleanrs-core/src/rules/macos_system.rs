use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct MacosSystemCleaner;

impl Cleaner for MacosSystemCleaner {
    fn id(&self) -> &'static str {
        "macos-system"
    }

    fn display_name(&self) -> &'static str {
        "macOS app caches and logs"
    }

    fn category(&self) -> Category {
        Category::SystemCache
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos") && home_path("Library").is_some()
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        if !self.is_available() {
            return Ok(Vec::new());
        }

        let mut targets = Vec::new();
        append_children(
            &mut targets,
            &home_path("Library/Caches").context("HOME is not set")?,
            "app cache",
            &[
                "Homebrew",
                "pip",
                "Yarn",
                "Codex",
                "com.openai.codex",
                "ollama",
                "aider",
                "amp",
                "cursor-agent",
                "gemini",
                "goose",
                "openclaw",
                "opencode",
            ],
        )?;
        append_children(
            &mut targets,
            &home_path("Library/Logs").context("HOME is not set")?,
            "app logs",
            &[],
        )?;

        let crash_reporter =
            home_path("Library/Application Support/CrashReporter").context("HOME is not set")?;
        append_target(&mut targets, crash_reporter, "CrashReporter data")?;

        targets.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(targets)
    }
}

fn append_children(
    targets: &mut Vec<CleanTarget>,
    root: &Path,
    description: &str,
    excluded_names: &[&str],
) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if excluded_names
            .iter()
            .any(|excluded| name.to_string_lossy() == *excluded)
        {
            continue;
        }
        append_target(
            targets,
            entry.path(),
            &format!("{description}: {}", name.to_string_lossy()),
        )?;
    }
    Ok(())
}

fn append_target(targets: &mut Vec<CleanTarget>, path: PathBuf, description: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let size_bytes = dir_size(&path)?;
    if size_bytes == 0 {
        return Ok(());
    }

    targets.push(CleanTarget {
        path,
        size_bytes,
        description: description.to_owned(),
        method: CleanMethod::TrashPath,
    });
    Ok(())
}
