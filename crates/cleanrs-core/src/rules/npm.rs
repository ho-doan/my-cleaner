use super::{command_available, command_output, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct NpmCleaner;

impl NpmCleaner {
    fn cache_path() -> Result<PathBuf> {
        if command_available("npm") {
            if let Ok(path) = command_output("npm", &["config", "get", "cache"]) {
                if !path.is_empty() && path != "undefined" {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        Ok(home_path(".npm").unwrap_or_else(|| PathBuf::from("~/.npm")))
    }
}

impl Cleaner for NpmCleaner {
    fn id(&self) -> &'static str {
        "npm"
    }

    fn display_name(&self) -> &'static str {
        "npm cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("npm")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let path = Self::cache_path()?;
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        Ok(vec![CleanTarget {
            size_bytes: dir_size(&path)?,
            path,
            description: "npm global cache".to_owned(),
            method: CleanMethod::RunCommand(vec![
                "npm".to_owned(),
                "cache".to_owned(),
                "clean".to_owned(),
                "--force".to_owned(),
            ]),
        }])
    }
}
