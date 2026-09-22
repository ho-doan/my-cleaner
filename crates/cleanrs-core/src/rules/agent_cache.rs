use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct AgentCacheCleaner;

const COMMANDS: &[&str] = &[
    "aider",
    "amp",
    "cursor-agent",
    "gemini",
    "goose",
    "openclaw",
    "opencode",
];

const CACHE_PATHS: &[(&str, &str)] = &[
    (".cache/aider", "Aider cache"),
    (".cache/amp", "Amp CLI cache"),
    (".cache/cursor-agent", "Cursor Agent cache"),
    (".cache/gemini", "Gemini CLI cache"),
    (".cache/goose", "Goose cache"),
    (".cache/openclaw", "OpenClaw cache"),
    (".cache/opencode", "OpenCode cache"),
    ("Library/Caches/aider", "Aider cache"),
    ("Library/Caches/amp", "Amp CLI cache"),
    ("Library/Caches/cursor-agent", "Cursor Agent cache"),
    ("Library/Caches/gemini", "Gemini CLI cache"),
    ("Library/Caches/goose", "Goose cache"),
    ("Library/Caches/openclaw", "OpenClaw cache"),
    ("Library/Caches/opencode", "OpenCode cache"),
];

impl Cleaner for AgentCacheCleaner {
    fn id(&self) -> &'static str {
        "agent-caches"
    }

    fn display_name(&self) -> &'static str {
        "Other CLI agent caches"
    }

    fn category(&self) -> Category {
        Category::AiAgent
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        COMMANDS.iter().any(|command| command_available(command))
            || CACHE_PATHS
                .iter()
                .filter_map(|(relative, _)| home_path(relative))
                .any(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        for (relative, description) in CACHE_PATHS {
            let Some(path) = home_path(relative) else {
                continue;
            };
            append_path_target(&mut targets, path, description)?;
        }
        Ok(targets)
    }
}

fn append_path_target(
    targets: &mut Vec<CleanTarget>,
    path: PathBuf,
    description: &str,
) -> Result<()> {
    if !path.is_dir() {
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
