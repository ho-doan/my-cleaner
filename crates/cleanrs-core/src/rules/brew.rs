use super::{command_available, command_output};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct BrewCleaner;

impl Cleaner for BrewCleaner {
    fn id(&self) -> &'static str {
        "brew"
    }

    fn display_name(&self) -> &'static str {
        "Homebrew cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("brew")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let cache = command_output("brew", &["--cache"])?;
        if cache.is_empty() {
            return Ok(Vec::new());
        }
        let path = PathBuf::from(cache);
        if !path.exists() {
            return Ok(Vec::new());
        }

        Ok(vec![CleanTarget {
            size_bytes: dir_size(&path)?,
            path,
            description: "Homebrew downloads and old formula archives".to_owned(),
            method: CleanMethod::RunCommand(vec![
                "brew".to_owned(),
                "cleanup".to_owned(),
                "-s".to_owned(),
            ]),
        }])
    }
}
