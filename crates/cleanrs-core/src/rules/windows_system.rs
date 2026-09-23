//! Windows-specific cleanup targets.

use super::non_empty_target;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::platform::config;
use crate::Cleaner;
use anyhow::Result;

pub struct WindowsTempCleaner;

impl Cleaner for WindowsTempCleaner {
    fn id(&self) -> &'static str {
        "windows-temp"
    }

    fn display_name(&self) -> &'static str {
        "Windows temporary files"
    }

    fn category(&self) -> Category {
        Category::SystemCache
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        config::temporary_paths().iter().any(|path| path.is_dir())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        config::temporary_paths()
            .into_iter()
            .map(|path| {
                non_empty_target(
                    path,
                    "Windows temporary files; active files may be skipped",
                    CleanMethod::TrashPath,
                )
            })
            .collect::<Result<Vec<_>>>()
            .map(|targets| targets.into_iter().flatten().collect())
    }
}
