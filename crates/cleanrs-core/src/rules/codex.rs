use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct CodexCleaner;

impl Cleaner for CodexCleaner {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex CLI cache and old runtimes"
    }

    fn category(&self) -> Category {
        Category::AiAgent
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("codex")
            || codex_home().is_some_and(|path| path.exists())
            || app_cache_paths().iter().any(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();

        if let Some(root) = codex_home() {
            append_path_target(
                &mut targets,
                root.join("cache"),
                "Codex CLI cache",
                RiskLevel::Safe,
            )?;
            append_path_target(
                &mut targets,
                root.join(".tmp"),
                "Codex CLI temporary files",
                RiskLevel::Caution,
            )?;
            append_old_releases(&mut targets, &root)?;
        }

        for path in app_cache_paths() {
            append_path_target(
                &mut targets,
                path,
                "Codex desktop app cache",
                RiskLevel::Safe,
            )?;
        }

        Ok(targets)
    }
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_path(".codex"))
}

fn app_cache_paths() -> Vec<PathBuf> {
    ["Library/Caches/Codex", "Library/Caches/com.openai.codex"]
        .into_iter()
        .filter_map(home_path)
        .collect()
}

fn append_old_releases(targets: &mut Vec<CleanTarget>, root: &Path) -> Result<()> {
    let releases = root.join("packages/standalone/releases");
    if !releases.is_dir() {
        return Ok(());
    }

    let active = std::fs::canonicalize(root.join("packages/standalone/current")).ok();
    for entry in
        std::fs::read_dir(&releases).with_context(|| format!("read {}", releases.display()))?
    {
        let path = entry?.path();
        if active.as_ref().is_some_and(|current| {
            std::fs::canonicalize(&path)
                .map(|candidate| candidate == *current)
                .unwrap_or(false)
        }) {
            continue;
        }
        append_path_target(
            targets,
            path,
            "old Codex standalone runtime",
            RiskLevel::Caution,
        )?;
    }
    Ok(())
}

fn append_path_target(
    targets: &mut Vec<CleanTarget>,
    path: PathBuf,
    description: &str,
    _risk: RiskLevel,
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
