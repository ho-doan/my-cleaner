use super::{command_available, command_output, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct PnpmCleaner;

impl Cleaner for PnpmCleaner {
    fn id(&self) -> &'static str {
        "pnpm"
    }

    fn display_name(&self) -> &'static str {
        "pnpm store"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("pnpm")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let store = command_output("pnpm", &["store", "path"])?;
        if store.is_empty() {
            return Ok(Vec::new());
        }
        let path = PathBuf::from(store);
        let Some(target) = non_empty_target(
            path,
            "pnpm content-addressable store",
            CleanMethod::RunCommand(vec![
                "pnpm".to_owned(),
                "store".to_owned(),
                "prune".to_owned(),
            ]),
        )?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
