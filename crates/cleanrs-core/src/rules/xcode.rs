use super::{command_available, command_output, home_path, non_empty_target};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct XcodeCleaner;
pub struct XcodeArchivesCleaner;
pub struct SimulatorCacheCleaner;
pub struct XcodeInstallationsCleaner;

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

impl Cleaner for XcodeArchivesCleaner {
    fn id(&self) -> &'static str {
        "xcode-archives"
    }

    fn display_name(&self) -> &'static str {
        "Xcode Archives"
    }

    fn category(&self) -> Category {
        Category::Ide
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        home_path("Library/Developer/Xcode/Archives").is_some_and(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path("Library/Developer/Xcode/Archives") else {
            return Ok(Vec::new());
        };
        let Some(target) = non_empty_target(path, "Xcode archived builds", CleanMethod::TrashPath)?
        else {
            return Ok(Vec::new());
        };
        Ok(vec![target])
    }
}

impl Cleaner for SimulatorCacheCleaner {
    fn id(&self) -> &'static str {
        "xcode-simulator"
    }

    fn display_name(&self) -> &'static str {
        "CoreSimulator caches"
    }

    fn category(&self) -> Category {
        Category::Ide
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        home_path("Library/Developer/CoreSimulator/Caches").is_some_and(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = home_path("Library/Developer/CoreSimulator/Caches") else {
            return Ok(Vec::new());
        };
        let Some(target) = non_empty_target(
            path,
            "CoreSimulator generated caches",
            CleanMethod::TrashPath,
        )?
        else {
            return Ok(Vec::new());
        };
        Ok(vec![target])
    }
}

impl Cleaner for XcodeInstallationsCleaner {
    fn id(&self) -> &'static str {
        "xcode-installations"
    }

    fn display_name(&self) -> &'static str {
        "Old Xcode installations"
    }

    fn category(&self) -> Category {
        Category::Ide
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Caution
    }

    fn is_available(&self) -> bool {
        xcode_roots().iter().any(|root| {
            root.is_dir()
                && std::fs::read_dir(root)
                    .map(|entries| entries.flatten().any(|entry| is_xcode_app(&entry.path())))
                    .unwrap_or(false)
        })
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let active = active_xcode_path()?;
        let mut targets = Vec::new();
        for root in xcode_roots() {
            append_old_xcode_apps(&mut targets, &root, &active)?;
        }
        Ok(targets)
    }
}

fn xcode_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(path) = home_path("Applications") {
        roots.push(path);
    }
    roots
}

fn active_xcode_path() -> Result<PathBuf> {
    let developer_dir =
        command_output("xcode-select", &["-p"]).context("read active xcode-select path")?;
    let developer_dir = PathBuf::from(developer_dir);
    let contents = developer_dir
        .parent()
        .context("active xcode-select path has no Contents parent")?;
    let app = contents
        .parent()
        .context("active xcode-select path has no app parent")?;
    std::fs::canonicalize(app).with_context(|| format!("resolve active Xcode {}", app.display()))
}

fn append_old_xcode_apps(targets: &mut Vec<CleanTarget>, root: &Path, active: &Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !is_xcode_app(&path) || same_path(&path, active) {
            continue;
        }
        let size_bytes = dir_size(&path)?;
        if size_bytes == 0 {
            continue;
        }
        targets.push(CleanTarget {
            path: path.clone(),
            size_bytes,
            description: format!("old Xcode installation: {}", path.display()),
            method: CleanMethod::TrashPath,
        });
    }
    Ok(())
}

fn is_xcode_app(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("Xcode"))
        && path.extension().is_some_and(|extension| extension == "app")
}

fn same_path(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::append_old_xcode_apps;
    use std::fs;

    #[test]
    fn finds_old_xcode_apps_but_excludes_active_app() {
        let root = tempfile::tempdir().expect("tempdir");
        let active = root.path().join("Xcode.app");
        let old = root.path().join("Xcode-15.4.app");
        fs::create_dir(&active).expect("active app");
        fs::create_dir(&old).expect("old app");
        fs::write(old.join("Contents"), b"old").expect("old app content");

        let mut targets = Vec::new();
        append_old_xcode_apps(&mut targets, root.path(), &active).expect("scan apps");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, old);
        assert!(targets[0].size_bytes > 0);
    }
}
