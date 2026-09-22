use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;

pub struct MavenCleaner;

impl Cleaner for MavenCleaner {
    fn id(&self) -> &'static str {
        "maven"
    }

    fn display_name(&self) -> &'static str {
        "Maven repository"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("mvn")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path(".m2/repository") else {
            return Ok(Vec::new());
        };
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        Ok(vec![CleanTarget {
            size_bytes: dir_size(&path)?,
            path,
            description: "Maven downloaded artifacts".to_owned(),
            method: CleanMethod::TrashPath,
        }])
    }
}
