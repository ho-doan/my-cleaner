use super::{command_available, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;

pub struct XcodeCleaner;

impl Cleaner for XcodeCleaner {
    fn id(&self) -> &'static str {
        "xcode"
    }

    fn display_name(&self) -> &'static str {
        "Xcode DerivedData"
    }

    fn category(&self) -> Category {
        Category::Ide
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Safe
    }

    fn is_available(&self) -> bool {
        command_available("xcrun")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path("Library/Developer/Xcode/DerivedData") else {
            return Ok(Vec::new());
        };
        let Some(target) =
            non_empty_target(path, "Xcode build intermediates", CleanMethod::TrashPath)?
        else {
            return Ok(Vec::new());
        };

        Ok(vec![target])
    }
}
