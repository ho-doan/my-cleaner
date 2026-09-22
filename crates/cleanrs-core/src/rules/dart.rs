use super::{command_available, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;

pub struct DartCleaner;

impl Cleaner for DartCleaner {
    fn id(&self) -> &'static str {
        "dart"
    }

    fn display_name(&self) -> &'static str {
        "Dart pub cache"
    }

    fn category(&self) -> Category {
        Category::PackageManager
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("dart")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path(".pub-cache") else {
            return Ok(Vec::new());
        };
        let Some(target) = non_empty_target(
            path,
            "Dart package cache",
            CleanMethod::RunCommand(vec![
                "dart".to_owned(),
                "pub".to_owned(),
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
