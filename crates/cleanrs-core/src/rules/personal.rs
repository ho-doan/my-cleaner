use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct PersonalDataCleaner;

impl Cleaner for PersonalDataCleaner {
    fn id(&self) -> &'static str {
        "personal-data"
    }

    fn display_name(&self) -> &'static str {
        "Mail downloads and iOS backups"
    }

    fn category(&self) -> Category {
        Category::ManualReview
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        personal_paths().iter().any(|(path, _)| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        for (path, description) in personal_paths() {
            if !path.exists() {
                continue;
            }
            let size_bytes = dir_size(&path)?;
            if size_bytes == 0 {
                continue;
            }
            targets.push(CleanTarget {
                path,
                size_bytes,
                description: description.to_owned(),
                method: CleanMethod::TrashPath,
            });
        }
        Ok(targets)
    }
}

fn personal_paths() -> Vec<(PathBuf, &'static str)> {
    [
        (
            "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
            "Mail downloaded attachments",
        ),
        ("Library/Mail Downloads", "Mail downloaded attachments"),
        (
            "Library/Application Support/MobileSync/Backup",
            "iPhone and iPad backups",
        ),
    ]
    .into_iter()
    .filter_map(|(relative, description)| home_path(relative).map(|path| (path, description)))
    .collect()
}
