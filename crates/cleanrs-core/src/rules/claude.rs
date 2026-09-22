use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct ClaudeCleaner;

impl Cleaner for ClaudeCleaner {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code cache and old runtimes"
    }

    fn category(&self) -> Category {
        Category::AiAgent
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("claude")
            || home_path(".claude").is_some_and(|path| path.exists())
            || home_path(".local/share/claude").is_some_and(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        if let Some(cache) = home_path(".claude/cache") {
            append_path_target(&mut targets, cache, "Claude Code cache")?;
        }
        if let Some(cache) = home_path(".cache/claude") {
            append_path_target(&mut targets, cache, "Claude CLI cache")?;
        }
        append_old_versions(&mut targets)?;
        Ok(targets)
    }
}

fn append_old_versions(targets: &mut Vec<CleanTarget>) -> Result<()> {
    let Some(versions) = home_path(".local/share/claude/versions") else {
        return Ok(());
    };
    if !versions.is_dir() {
        return Ok(());
    }

    let active = home_path(".local/bin/claude").and_then(|path| std::fs::canonicalize(path).ok());
    for entry in
        std::fs::read_dir(&versions).with_context(|| format!("read {}", versions.display()))?
    {
        let path = entry?.path();
        if active.as_ref().is_some_and(|current| {
            std::fs::canonicalize(&path)
                .map(|candidate| candidate == *current)
                .unwrap_or(false)
        }) {
            continue;
        }
        append_path_target(&mut *targets, path, "old Claude Code runtime")?;
    }
    Ok(())
}

fn append_path_target(
    targets: &mut Vec<CleanTarget>,
    path: PathBuf,
    description: &str,
) -> Result<()> {
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
