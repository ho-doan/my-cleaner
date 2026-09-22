use super::{command_available, command_output, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
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
        let Some(target) = non_empty_target(
            path,
            "Homebrew downloads and old formula archives",
            CleanMethod::RunCommand(vec![
                "brew".to_owned(),
                "cleanup".to_owned(),
                "-s".to_owned(),
            ]),
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
