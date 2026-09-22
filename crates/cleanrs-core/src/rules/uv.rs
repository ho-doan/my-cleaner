use super::{command_available, command_output, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct UvCleaner;

impl Cleaner for UvCleaner {
    fn id(&self) -> &'static str {
        "uv"
    }

    fn display_name(&self) -> &'static str {
        "uv cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("uv")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let cache = command_output("uv", &["cache", "dir"])?;
        if cache.is_empty() {
            return Ok(Vec::new());
        }
        let path = PathBuf::from(cache);
        let Some(target) = non_empty_target(
            path,
            "uv package and wheel cache",
            CleanMethod::RunCommand(vec![
                "uv".to_owned(),
                "cache".to_owned(),
                "clean".to_owned(),
            ]),
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
