use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct KiroCleaner;

impl Cleaner for KiroCleaner {
    fn id(&self) -> &'static str {
        "kiro"
    }

    fn display_name(&self) -> &'static str {
        "Kiro CLI cache and sessions"
    }

    fn category(&self) -> Category {
        Category::AiAgent
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        command_available("kiro-cli")
            || home_path(".kiro").is_some_and(|path| path.exists())
            || app_support_path().is_some_and(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        if let Some(sessions) = home_path(".kiro/sessions/cli") {
            append_path_target(
                &mut targets,
                sessions,
                "Kiro CLI conversation history",
                RiskLevel::Manual,
            )?;
        }
        if let Some(feed_cache) = app_support_path().map(|path| path.join("feed-cache.json")) {
            append_path_target(
                &mut targets,
                feed_cache,
                "Kiro CLI feed cache",
                RiskLevel::Safe,
            )?;
        }
        Ok(targets)
    }
}

fn app_support_path() -> Option<PathBuf> {
    home_path("Library/Application Support/kiro-cli")
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
