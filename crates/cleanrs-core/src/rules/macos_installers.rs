//! Inventory of old macOS installer applications.

use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct MacosInstallerCleaner;

impl Cleaner for MacosInstallerCleaner {
    fn id(&self) -> &'static str {
        "macos-installers"
    }

    fn display_name(&self) -> &'static str {
        "macOS installer applications"
    }

    fn category(&self) -> Category {
        Category::SystemCache
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        installer_roots().iter().any(|root| {
            root.is_dir()
                && std::fs::read_dir(root)
                    .map(|entries| {
                        entries
                            .flatten()
                            .any(|entry| is_macos_installer(&entry.path()))
                    })
                    .unwrap_or(false)
        })
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        for root in installer_roots() {
            append_installers(&mut targets, &root)?;
        }
        Ok(targets)
    }
}

fn installer_roots() -> Vec<PathBuf> {
    [
        Some(PathBuf::from("/Applications")),
        home_path("Applications"),
        home_path("Downloads"),
        home_path("Desktop"),
        home_path("Documents"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn append_installers(targets: &mut Vec<CleanTarget>, root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !is_macos_installer(&path) {
            continue;
        }
        let size_bytes = dir_size(&path)?;
        if size_bytes == 0 {
            continue;
        }
        targets.push(CleanTarget {
            path: path.clone(),
            size_bytes,
            description: format!("old macOS installer: {}", path.display()),
            method: CleanMethod::TrashPath,
        });
    }
    Ok(())
}

fn is_macos_installer(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("Install macOS "))
        && path.extension().is_some_and(|extension| extension == "app")
}

#[cfg(test)]
mod tests {
    use super::append_installers;
    use std::fs;

    #[test]
    fn finds_installer_apps_in_user_file_roots() {
        let root = tempfile::tempdir().expect("tempdir");
        let installer = root.path().join("Install macOS Test.app");
        fs::create_dir(&installer).expect("installer app");
        fs::write(installer.join("Contents"), b"installer").expect("installer content");
        fs::create_dir(root.path().join("Not an installer.app")).expect("other app");

        let mut targets = Vec::new();
        append_installers(&mut targets, root.path()).expect("scan installers");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, installer);
        assert!(targets[0].size_bytes > 0);
    }
}
