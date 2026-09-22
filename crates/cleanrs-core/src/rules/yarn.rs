use super::{command_available, command_output};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct YarnCleaner;

impl Cleaner for YarnCleaner {
    fn id(&self) -> &'static str {
        "yarn"
    }

    fn display_name(&self) -> &'static str {
        "Yarn cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("yarn")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let cache = command_output("yarn", &["cache", "dir"])?;
        if cache.is_empty() {
            return Ok(Vec::new());
        }
        let path = PathBuf::from(cache);
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        Ok(vec![CleanTarget {
            size_bytes: dir_size(&path)?,
            path,
            description: "Yarn package cache".to_owned(),
            method: CleanMethod::RunCommand(vec![
                "yarn".to_owned(),
                "cache".to_owned(),
                "clean".to_owned(),
            ]),
        }])
    }
}
