use super::{command_available, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct PipCleaner;

impl PipCleaner {
    fn command() -> Option<Vec<String>> {
        if command_available("pip3") {
            Some(vec!["pip3".to_owned()])
        } else if command_available("pip") {
            Some(vec!["pip".to_owned()])
        } else if command_available("python3") {
            Some(vec![
                "python3".to_owned(),
                "-m".to_owned(),
                "pip".to_owned(),
            ])
        } else {
            None
        }
    }

    fn cache_path(command: &[String]) -> Result<PathBuf> {
        let program = &command[0];
        let args = command[1..]
            .iter()
            .map(String::as_str)
            .chain(["cache", "dir"])
            .collect::<Vec<_>>();
        let output = std::process::Command::new(program).args(&args).output()?;
        if !output.status.success() {
            anyhow::bail!("pip cache dir command failed with status {}", output.status);
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if path.is_empty() {
            anyhow::bail!("pip cache dir command returned an empty path");
        }
        Ok(PathBuf::from(path))
    }
}

impl Cleaner for PipCleaner {
    fn id(&self) -> &'static str {
        "pip"
    }

    fn display_name(&self) -> &'static str {
        "pip cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        Self::command().is_some()
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let command = Self::command().ok_or_else(|| anyhow::anyhow!("pip is unavailable"))?;
        let path = Self::cache_path(&command).or_else(|_| {
            ["Library/Caches/pip", ".cache/pip"]
                .into_iter()
                .filter_map(home_path)
                .find(|candidate| candidate.is_dir())
                .ok_or_else(|| anyhow::anyhow!("pip cache directory was not found"))
        })?;
        let mut clean_command = command;
        clean_command.extend(["cache".to_owned(), "purge".to_owned()]);

        let Some(target) = non_empty_target(
            path,
            "pip download and wheel cache",
            CleanMethod::RunCommand(clean_command),
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
