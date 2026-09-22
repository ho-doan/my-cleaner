use super::{command_available, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
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
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        Ok(vec![CleanTarget {
            size_bytes: dir_size(&path)?,
            path,
            description: "downloaded Cargo crate archives".to_owned(),
            method: CleanMethod::TrashPath,
        }])
    }
}
