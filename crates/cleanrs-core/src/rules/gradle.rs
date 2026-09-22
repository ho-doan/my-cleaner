use super::{command_available, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;

pub struct GradleCleaner;

impl Cleaner for GradleCleaner {
    fn id(&self) -> &'static str {
        "gradle"
    }

    fn display_name(&self) -> &'static str {
        "Gradle cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("gradle")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path(".gradle/caches") else {
            return Ok(Vec::new());
        };
        let Some(target) = non_empty_target(
            path,
            "Gradle dependency and build cache",
            CleanMethod::TrashPath,
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
