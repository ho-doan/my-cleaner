use super::{command_available, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;

pub struct CargoCleaner;

impl Cleaner for CargoCleaner {
    fn id(&self) -> &'static str {
        "cargo"
    }

    fn display_name(&self) -> &'static str {
        "Cargo registry cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        command_available("cargo")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path(".cargo/registry/cache") else {
            return Ok(Vec::new());
        };
        let Some(target) = non_empty_target(
            path,
            "downloaded Cargo crate archives",
            CleanMethod::TrashPath,
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
