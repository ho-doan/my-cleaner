use anyhow::{Context, Result};
use fs2::{available_space, total_space};
use jwalk::WalkDir;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::config;

const STANDARD_HOME_DIRECTORIES: [&str; 3] = config::STANDARD_HOME_DIRECTORIES;

#[derive(Clone, Debug, Serialize)]
pub struct VolumeUsage {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

impl VolumeUsage {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DiskScanEntry {
    pub path: PathBuf,
    /// Bytes visible through recursive directory enumeration.
    pub size_bytes: u64,
    /// Filesystem-level usage, including metadata/snapshots and inaccessible
    /// files. Present for mounted volume roots.
    pub volume_usage: Option<VolumeUsage>,
    /// Number of traversal or metadata failures below this entry.
    pub inaccessible_paths: usize,
    /// The path is protected by the system-safety policy.
    ///
    /// A `false` value does not make an inventory row directly deletable: the
    /// full-disk scan is still informational and cleanup must go through an
    /// approved `CleanTarget`.
    pub read_only: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FullDiskScan {
    pub root: PathBuf,
    pub root_entries: Vec<DiskScanEntry>,
    pub home_entries: Vec<DiskScanEntry>,
    /// Protected system/mount roots with recursively calculated sizes.
    pub readonly_entries: Vec<DiskScanEntry>,
    /// System and mount paths intentionally not traversed by the inventory.
    /// They are surfaced as read-only instead of becoming delete targets.
    pub readonly_paths: Vec<PathBuf>,
    pub inaccessible_paths: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct FullDiskScanProgress {
    pub phase: String,
    pub completed: usize,
    pub total: usize,
    pub report: FullDiskScan,
    pub finished: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectorySuggestion {
    pub reason: String,
    /// Whether the explorer may move this whole directory to Trash after an
    /// explicit confirmation. Sensitive data remains suggestion-only.
    pub can_delete: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectoryScanEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub inaccessible_paths: usize,
    pub is_dir: bool,
    pub read_only: bool,
    pub allowlisted: bool,
    pub suggestion: Option<DirectorySuggestion>,
}

#[derive(Debug, Serialize)]
pub struct DirectoryScan {
    pub path: PathBuf,
    pub entries: Vec<DirectoryScanEntry>,
    pub volume_usage: Option<VolumeUsage>,
    pub is_git_repo: bool,
    pub git_dirty: bool,
    pub inaccessible_paths: usize,
}

/// Calculate the size of a file or directory.
///
/// jwalk parallelizes traversal and rayon parallelizes the metadata pass. A
/// symlink is never followed into another tree.
pub fn dir_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        return Ok(std::fs::symlink_metadata(path)?.len());
    }

    let entries = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let size = entries
        .par_iter()
        .filter_map(|entry| std::fs::symlink_metadata(entry.path()).ok())
        .filter(|metadata| metadata.file_type().is_file())
        .map(|metadata| metadata.len())
        .sum();

    Ok(size)
}

/// Scan the largest immediate directories under the root filesystem and the
/// current user's home directory.
///
/// This is intentionally an inventory scan, not a delete plan. System-managed
/// mount points and virtual filesystems are excluded to avoid traversing areas
/// that are not useful cleanup targets or may block indefinitely.
pub fn full_disk_scan(root: &Path, limit: usize) -> Result<FullDiskScan> {
    full_disk_scan_with_progress(root, limit, |_| {})
}

/// Scan the full inventory while publishing a cloned partial report after
/// each root, protected/mounted, or HOME child completes.
pub fn full_disk_scan_with_progress<F>(
    root: &Path,
    limit: usize,
    mut on_progress: F,
) -> Result<FullDiskScan>
where
    F: FnMut(FullDiskScanProgress),
{
    if !root.is_dir() {
        anyhow::bail!("full-disk scan root is not a directory: {}", root.display());
    }

    let excluded_root_paths = config::excluded_root_paths(root);
    let readonly_paths = config::readonly_inventory_paths(root);
    let mut report = FullDiskScan {
        root: root.to_path_buf(),
        root_entries: Vec::new(),
        home_entries: Vec::new(),
        readonly_entries: Vec::new(),
        readonly_paths,
        inaccessible_paths: 0,
    };

    let (mut root_entries, _) = scan_children_with_progress(
        root,
        &excluded_root_paths,
        |entry, inaccessible, completed, total| {
            if let Some(entry) = entry {
                report.root_entries.push(entry);
                trim_entries(&mut report.root_entries, limit);
            }
            report.inaccessible_paths += inaccessible;
            emit_progress(&mut on_progress, &report, "root", completed, total, false);
        },
    )
    .with_context(|| format!("scan {}", root.display()))?;
    trim_entries(&mut root_entries, limit);
    report.root_entries = root_entries;

    let readonly_paths = report.readonly_paths.clone();
    let (mut readonly_entries, _) = scan_readonly_paths_with_progress(
        &readonly_paths,
        |entry, inaccessible, completed, total| {
            if let Some(entry) = entry {
                report.readonly_entries.push(entry);
                report
                    .readonly_entries
                    .sort_by_key(|entry| entry.path.clone());
            }
            report.inaccessible_paths += inaccessible;
            emit_progress(
                &mut on_progress,
                &report,
                "mounted volumes and protected paths",
                completed,
                total,
                false,
            );
        },
    );
    readonly_entries.sort_by_key(|entry| entry.path.clone());
    report.readonly_entries = readonly_entries;

    if let Some(home) = config::home_dir() {
        let (home_entries, _) = scan_home_entries_with_progress(
            &home,
            limit,
            |entry, inaccessible, completed, total| {
                if let Some(entry) = entry {
                    report.home_entries.push(entry);
                    trim_entries(&mut report.home_entries, limit);
                }
                report.inaccessible_paths += inaccessible;
                emit_progress(&mut on_progress, &report, "HOME", completed, total, false);
            },
        )?;
        report.home_entries = home_entries;
        emit_progress(&mut on_progress, &report, "HOME", 1, 1, false);
    }

    emit_progress(&mut on_progress, &report, "complete", 1, 1, true);
    Ok(report)
}

fn emit_progress<F>(
    on_progress: &mut F,
    report: &FullDiskScan,
    phase: &str,
    completed: usize,
    total: usize,
    finished: bool,
) where
    F: FnMut(FullDiskScanProgress),
{
    on_progress(FullDiskScanProgress {
        phase: phase.to_owned(),
        completed,
        total,
        report: report.clone(),
        finished,
    });
}

/// Returns whether a path belongs to a protected system or mount tree.
///
/// User-owned paths such as `~/Library/Developer/CoreSimulator` are not
/// protected by this policy. They can be inspected and may receive a cleanup
/// suggestion, while still requiring an approved cleaner before deletion.
pub fn is_protected_path(path: &Path) -> bool {
    config::is_protected_path(path)
}

/// Returns whether an explorer-selected path is safe enough for an explicit
/// move to Trash. Protected trees only allow individually selected transient
/// files with a specific suggestion; protected directories remain blocked.
pub fn can_delete_path(path: &Path) -> bool {
    if !is_protected_path(path) {
        return true;
    }

    let is_file = std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false);
    is_file && file_suggestion(path).is_some_and(|suggestion| suggestion.can_delete)
}

fn volume_usage(path: &Path) -> Option<VolumeUsage> {
    if !config::is_volume_root(path) {
        return None;
    }
    let total_bytes = total_space(path).ok()?;
    let available_bytes = available_space(path).ok()?.min(total_bytes);
    Some(VolumeUsage {
        total_bytes,
        available_bytes,
    })
}

/// Scan one directory level for an interactive, read-only explorer.
///
/// Directory sizes are calculated recursively, but only immediate children are
/// returned. Known regenerable folders receive a deletion suggestion; no
/// suggestion is a delete authorization or a `CleanTarget`.
pub fn scan_directory(path: &Path, limit: usize) -> Result<DirectoryScan> {
    if !path.is_dir() {
        anyhow::bail!("directory scan path is not a directory: {}", path.display());
    }

    let readonly_paths = if config::is_default_scan_root(path) {
        config::excluded_root_paths(path)
    } else {
        Vec::new()
    };
    let delete_allowlist = crate::allowlist::load_delete_allowlist()?;
    let children = std::fs::read_dir(path)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    let (is_git_repo, git_dirty) = git_context(path);

    let results = children
        .into_par_iter()
        .map(|child| {
            if readonly_paths.contains(&child) {
                let (entry, inaccessible) = readonly_directory_entry(child);
                return (Some(entry), inaccessible);
            }

            let Ok(metadata) = std::fs::symlink_metadata(&child) else {
                return (None, 1usize);
            };
            let is_dir = metadata.is_dir();
            let (size_bytes, inaccessible) = if is_dir {
                tolerant_dir_size(&child)
            } else if metadata.file_type().is_file() {
                (metadata.len(), 0)
            } else {
                (0, 0)
            };
            let allowlisted = is_dir && delete_allowlist.contains(&child);
            let suggestion = if is_dir {
                directory_suggestion(&child, allowlisted)
            } else {
                file_suggestion(&child)
            };
            let protected_file_is_deletable = !is_dir
                && is_protected_path(&child)
                && suggestion
                    .as_ref()
                    .is_some_and(|suggestion| suggestion.can_delete);
            let read_only = child.file_name().is_some_and(|name| name == ".git")
                || (is_protected_path(&child) && !protected_file_is_deletable);
            (
                Some(DirectoryScanEntry {
                    path: child,
                    size_bytes,
                    inaccessible_paths: inaccessible,
                    is_dir,
                    read_only,
                    allowlisted,
                    suggestion,
                }),
                inaccessible,
            )
        })
        .collect::<Vec<_>>();

    let inaccessible_paths = results.iter().map(|(_, count)| count).sum();
    let mut entries = results
        .into_iter()
        .filter_map(|(entry, _)| entry)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (!entry.read_only, std::cmp::Reverse(entry.size_bytes)));

    let readonly_entries = entries
        .iter()
        .filter(|entry| entry.read_only)
        .cloned()
        .collect::<Vec<_>>();
    entries.retain(|entry| !entry.read_only);
    entries.truncate(limit);
    entries.extend(readonly_entries);

    Ok(DirectoryScan {
        path: path.to_path_buf(),
        entries,
        volume_usage: volume_usage(path),
        is_git_repo,
        git_dirty,
        inaccessible_paths,
    })
}

fn git_context(path: &Path) -> (bool, bool) {
    let Ok(output) = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
    else {
        return (false, false);
    };
    if !output.status.success() {
        return (false, false);
    }
    (
        true,
        !String::from_utf8_lossy(&output.stdout).trim().is_empty(),
    )
}

fn directory_suggestion(path: &Path, allowlisted: bool) -> Option<DirectorySuggestion> {
    let name = path.file_name()?.to_string_lossy();
    if allowlisted {
        return Some(DirectorySuggestion {
            reason: "user-approved delete allowlist entry; review contents before Trash".to_owned(),
            can_delete: true,
        });
    }
    if let Some(area) = user_file_area(path) {
        return Some(DirectorySuggestion {
            reason: format!("item inside {area}; move to Trash after confirmation"),
            can_delete: true,
        });
    }

    let (reason, can_delete) = match name.as_ref() {
        "node_modules" => ("regenerable Node dependencies", true),
        "target" => ("Rust build artifacts", true),
        ".dart_tool" => ("Dart/Flutter tool cache", true),
        "build" => ("generated build output", true),
        "coverage" => ("generated test coverage", true),
        ".next" => ("Next.js build output", true),
        ".nuxt" => ("Nuxt build output", true),
        ".turbo" => ("Turborepo cache", true),
        ".gradle" => ("Gradle project cache", true),
        "Pods" => ("CocoaPods dependencies", true),
        ".venv" => ("Python virtual environment", true),
        "__pycache__" => ("Python bytecode cache", true),
        ".pytest_cache" => ("pytest cache", true),
        "DerivedData" => ("Xcode build intermediates", true),
        "CoreSimulator" => (
            "iOS Simulator data; review devices and runtimes before deleting",
            false,
        ),
        _ => return None,
    };
    Some(DirectorySuggestion {
        reason: reason.to_owned(),
        can_delete,
    })
}

fn file_suggestion(path: &Path) -> Option<DirectorySuggestion> {
    let name = path.file_name()?.to_string_lossy();
    if let Some(area) = user_file_area(path) {
        return Some(DirectorySuggestion {
            reason: format!("file inside {area}; move to Trash after confirmation"),
            can_delete: true,
        });
    }
    if path.starts_with(Path::new("/cores")) && (name == "core" || name.starts_with("core.")) {
        return Some(DirectorySuggestion {
            reason: "crash dump; keep only if it is still needed for debugging".to_owned(),
            can_delete: true,
        });
    }
    if path.starts_with(Path::new("/private/tmp"))
        || path.starts_with(Path::new("/private/var/tmp"))
    {
        return Some(DirectorySuggestion {
            reason: "system temporary file; verify it is stale and not in use".to_owned(),
            can_delete: true,
        });
    }
    let reason = match name.as_ref() {
        ".claude.json" | ".claude.json.backup" => {
            "Claude config; verify the CLI/app is no longer used"
        }
        _ => return None,
    };
    Some(DirectorySuggestion {
        reason: reason.to_owned(),
        can_delete: false,
    })
}

fn user_file_area(path: &Path) -> Option<&'static str> {
    let home = config::home_dir()?;
    ["Downloads", "Desktop", "Documents"]
        .into_iter()
        .find(|area| {
            let root = home.join(area);
            path.starts_with(&root) && path != root
        })
}

fn scan_children_with_progress<F>(
    parent: &Path,
    excluded: &[PathBuf],
    mut on_item: F,
) -> Result<(Vec<DiskScanEntry>, usize)>
where
    F: FnMut(Option<DiskScanEntry>, usize, usize, usize),
{
    let children = std::fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    let total = children.len();
    let (sender, receiver) = std::sync::mpsc::channel();

    Ok(std::thread::scope(|scope| {
        scope.spawn(move || {
            children
                .into_par_iter()
                .for_each_with(sender, |sender, path| {
                    if excluded.contains(&path) {
                        let _ = sender.send((None, 0usize));
                        return;
                    }

                    let (size_bytes, inaccessible) = tolerant_dir_size(&path);
                    let read_only = is_protected_path(&path);
                    // Keep zero-byte and inaccessible children in the inventory. A
                    // permission or mount error must be visible as PARTIAL instead
                    // of making the folder disappear between rescans.
                    let entry = Some(DiskScanEntry {
                        path: path.clone(),
                        size_bytes,
                        volume_usage: volume_usage(&path),
                        inaccessible_paths: inaccessible,
                        read_only,
                    });
                    let _ = sender.send((entry, inaccessible));
                });
        });

        let mut entries = Vec::new();
        let mut inaccessible = 0;
        let mut completed = 0;
        for (entry, blocked) in receiver {
            completed += 1;
            inaccessible += blocked;
            if let Some(item) = entry.as_ref() {
                entries.push(item.clone());
            }
            on_item(entry, blocked, completed, total);
        }
        (entries, inaccessible)
    }))
}

fn trim_entries(entries: &mut Vec<DiskScanEntry>, limit: usize) {
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    entries.truncate(limit);
}

/// Keep the largest HOME entries while always retaining the standard user
/// folders. A small Documents folder should remain inspectable even when the
/// HOME directory contains more than `limit` larger caches or toolchains.
#[cfg(test)]
fn scan_home_entries(home: &Path, limit: usize) -> Result<(Vec<DiskScanEntry>, usize)> {
    scan_home_entries_with_progress(home, limit, |_, _, _, _| {})
}

fn scan_home_entries_with_progress<F>(
    home: &Path,
    limit: usize,
    mut on_item: F,
) -> Result<(Vec<DiskScanEntry>, usize)>
where
    F: FnMut(Option<DiskScanEntry>, usize, usize, usize),
{
    let (mut entries, mut inaccessible) =
        scan_children_with_progress(home, &[], |entry, blocked, completed, total| {
            on_item(entry, blocked, completed, total);
        })?;
    let standard_paths = STANDARD_HOME_DIRECTORIES
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    for path in &standard_paths {
        if entries.iter().any(|entry| entry.path == *path) {
            continue;
        }
        let (size_bytes, inaccessible_paths) = tolerant_dir_size(path);
        inaccessible += inaccessible_paths;
        entries.push(DiskScanEntry {
            path: path.clone(),
            size_bytes,
            volume_usage: volume_usage(path),
            inaccessible_paths,
            read_only: false,
        });
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    let mut selected = entries
        .iter()
        .filter(|entry| standard_paths.contains(&entry.path))
        .cloned()
        .collect::<Vec<_>>();
    selected.extend(
        entries
            .into_iter()
            .filter(|entry| !standard_paths.contains(&entry.path))
            .take(limit),
    );
    selected.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    Ok((selected, inaccessible))
}

fn readonly_directory_entry(path: PathBuf) -> (DirectoryScanEntry, usize) {
    let (size_bytes, inaccessible) = tolerant_dir_size(&path);
    (
        DirectoryScanEntry {
            path,
            size_bytes,
            inaccessible_paths: inaccessible,
            is_dir: true,
            read_only: true,
            allowlisted: false,
            suggestion: None,
        },
        inaccessible,
    )
}

#[cfg(test)]
fn scan_readonly_paths(paths: &[PathBuf]) -> (Vec<DiskScanEntry>, usize) {
    scan_readonly_paths_with_progress(paths, |_, _, _, _| {})
}

fn scan_readonly_paths_with_progress<F>(
    paths: &[PathBuf],
    mut on_item: F,
) -> (Vec<DiskScanEntry>, usize)
where
    F: FnMut(Option<DiskScanEntry>, usize, usize, usize),
{
    let total = paths.len();
    let (sender, receiver) = std::sync::mpsc::channel();
    let paths = paths.to_vec();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            paths.into_par_iter().for_each_with(sender, |sender, path| {
                let (size_bytes, inaccessible) = tolerant_dir_size(&path);
                let entry = DiskScanEntry {
                    path: path.clone(),
                    size_bytes,
                    volume_usage: volume_usage(&path),
                    inaccessible_paths: inaccessible,
                    read_only: true,
                };
                let _ = sender.send((Some(entry), inaccessible));
            });
        });
        let mut entries = Vec::new();
        let mut inaccessible = 0;
        let mut completed = 0;
        for (entry, blocked) in receiver {
            completed += 1;
            inaccessible += blocked;
            if let Some(item) = entry.as_ref() {
                entries.push(item.clone());
            }
            on_item(entry, blocked, completed, total);
        }
        (entries, inaccessible)
    })
}

fn tolerant_dir_size(path: &Path) -> (u64, usize) {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_file() {
            return (metadata.len(), 0);
        }
    }

    let mut traversal_errors = 0usize;
    let entries = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|result| match result {
            Ok(entry) => Some(entry),
            Err(_) => {
                traversal_errors += 1;
                None
            }
        })
        .collect::<Vec<_>>();

    let (size, metadata_errors) = entries
        .par_iter()
        .map(|entry| match std::fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.file_type().is_file() => (metadata.len(), 0usize),
            Ok(_) => (0, 0),
            Err(_) => (0, 1),
        })
        .reduce(
            || (0, 0),
            |left, right| (left.0 + right.0, left.1 + right.1),
        );

    (size, traversal_errors + metadata_errors)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::is_protected_path;
    use super::{
        config, dir_size, directory_suggestion, file_suggestion, readonly_directory_entry,
        scan_children_with_progress, scan_directory, scan_home_entries, scan_readonly_paths,
        STANDARD_HOME_DIRECTORIES,
    };
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn calculates_nested_file_sizes() {
        let root = tempdir().expect("temp directory");
        fs::write(root.path().join("one.txt"), b"12345").expect("write first file");
        fs::create_dir(root.path().join("nested")).expect("create nested directory");
        fs::write(root.path().join("nested/two.txt"), b"1234567").expect("write second file");

        assert_eq!(dir_size(root.path()).expect("directory size"), 12);
    }

    #[test]
    fn missing_paths_have_zero_size() {
        let root = tempdir().expect("temp directory");
        assert_eq!(
            dir_size(&root.path().join("missing")).expect("directory size"),
            0
        );
    }

    #[test]
    fn child_scan_reports_items_as_they_finish() {
        let root = tempdir().expect("temporary directory");
        for name in ["first", "second"] {
            let directory = root.path().join(name);
            fs::create_dir(&directory).expect("child directory");
            fs::write(directory.join("data.bin"), vec![0_u8; 8]).expect("child data");
        }
        fs::create_dir(root.path().join("empty")).expect("empty child directory");

        let mut progress = Vec::new();
        let (entries, inaccessible) =
            scan_children_with_progress(root.path(), &[], |entry, blocked, completed, total| {
                progress.push((entry.is_some(), blocked, completed, total));
            })
            .expect("child scan");

        assert_eq!(inaccessible, 0);
        assert_eq!(entries.len(), 3);
        assert_eq!(progress.len(), 3);
        assert!(progress.iter().all(|(_, _, _, total)| *total == 3));
        assert_eq!(progress.last().map(|item| item.2), Some(3));
        assert!(entries
            .iter()
            .any(|entry| entry.path.ends_with("empty") && entry.size_bytes == 0));
    }

    #[test]
    fn directory_scan_suggests_regenerable_folders() {
        let root = tempdir().expect("temporary directory");
        let node_modules = root.path().join("node_modules");
        fs::create_dir(&node_modules).expect("node_modules directory");
        fs::write(node_modules.join("package.json"), b"{}").expect("dependency marker");

        let report = scan_directory(root.path(), 10).expect("directory scan");
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.path == node_modules)
            .expect("node_modules entry");

        assert!(entry.is_dir);
        assert_eq!(
            entry
                .suggestion
                .as_ref()
                .map(|suggestion| suggestion.reason.as_str()),
            Some("regenerable Node dependencies")
        );
        assert!(entry
            .suggestion
            .as_ref()
            .is_some_and(|suggestion| suggestion.can_delete));
        assert!(!report.is_git_repo);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn home_data_is_reviewable_not_system_read_only() {
        assert!(is_protected_path(std::path::Path::new("/System/Library")));
        assert!(is_protected_path(std::path::Path::new("/private/var")));
        assert!(is_protected_path(std::path::Path::new("/Library/Updates")));
        assert!(!is_protected_path(std::path::Path::new(
            "/Users/test/Library/Developer/CoreSimulator"
        )));
    }

    #[test]
    fn directory_scan_suggests_simulator_data_and_stale_claude_config() {
        let root = tempdir().expect("temporary directory");
        let simulator = root.path().join("CoreSimulator");
        fs::create_dir(&simulator).expect("CoreSimulator directory");
        fs::write(simulator.join("device.db"), b"simulator data").expect("device data");
        fs::write(root.path().join(".claude.json"), b"{}").expect("Claude config");

        let report = scan_directory(root.path(), 10).expect("directory scan");
        let simulator_entry = report
            .entries
            .iter()
            .find(|entry| entry.path == simulator)
            .expect("CoreSimulator entry");
        let claude_entry = report
            .entries
            .iter()
            .find(|entry| entry.path == root.path().join(".claude.json"))
            .expect("Claude config entry");

        assert!(!simulator_entry.read_only);
        assert!(simulator_entry
            .suggestion
            .as_ref()
            .is_some_and(|suggestion| !suggestion.can_delete));
        assert!(!claude_entry.read_only);
        assert!(claude_entry
            .suggestion
            .as_ref()
            .is_some_and(|suggestion| !suggestion.can_delete));
    }

    #[test]
    fn directory_scan_approves_allowlisted_folder() {
        let path = Path::new("/Users/test/.Cocos/profiles");
        let suggestion = directory_suggestion(path, true).expect("allowlisted folder");

        assert!(suggestion.can_delete);
        assert!(suggestion.reason.contains("allowlist"));
        assert!(directory_suggestion(path, false).is_none());
    }

    #[test]
    fn protected_inventory_keeps_size_without_authorizing_root_deletion() {
        let root = tempdir().expect("temporary directory");
        let file = root.path().join("large.cache");
        fs::write(&file, vec![0_u8; 32]).expect("write protected fixture");

        let (entries, inaccessible) = scan_readonly_paths(&[file]);

        assert_eq!(inaccessible, 0);
        assert_eq!(entries[0].size_bytes, 32);
        assert!(entries[0].read_only);
    }

    #[test]
    fn readonly_directory_entries_keep_recursive_size() {
        let root = tempdir().expect("temporary directory");
        fs::create_dir(root.path().join("nested")).expect("create nested directory");
        fs::write(root.path().join("nested/data.bin"), vec![0_u8; 17]).expect("write nested data");

        let (entry, inaccessible) = readonly_directory_entry(root.path().to_path_buf());

        assert_eq!(inaccessible, 0);
        assert_eq!(entry.size_bytes, 17);
        assert!(entry.read_only);
    }

    #[test]
    fn user_file_area_items_are_explicitly_deletable() {
        let Some(home) = config::home_dir() else {
            return;
        };
        for area in ["Downloads", "Desktop", "Documents"] {
            let root = home.join(area);
            let file = file_suggestion(&root.join("old.zip")).expect("user-area file");
            let folder =
                directory_suggestion(&root.join("old-folder"), false).expect("user-area folder");

            assert!(file.can_delete);
            assert!(folder.can_delete);
            assert!(directory_suggestion(&root, false).is_none());
        }
    }

    #[test]
    fn home_scan_keeps_standard_directories_outside_top_limit() {
        let root = tempdir().expect("temporary home directory");
        for name in STANDARD_HOME_DIRECTORIES {
            fs::create_dir(root.path().join(name)).expect("standard home directory");
        }
        for index in 0..4 {
            fs::create_dir(root.path().join(format!("cache-{index}"))).expect("cache directory");
            fs::write(
                root.path().join(format!("cache-{index}/data.bin")),
                vec![0_u8; 8],
            )
            .expect("cache data");
        }

        let (entries, inaccessible) = scan_home_entries(root.path(), 1).expect("home scan");
        assert_eq!(inaccessible, 0);
        for name in STANDARD_HOME_DIRECTORIES {
            assert!(entries
                .iter()
                .any(|entry| entry.path == root.path().join(name)));
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn protected_transient_files_get_explicit_review_suggestions() {
        let core_dump =
            file_suggestion(Path::new("/cores/core.123")).expect("core dump should be recognized");
        assert!(core_dump.can_delete);

        let temp_file = file_suggestion(Path::new("/private/tmp/my-cleaner.tmp"))
            .expect("system temp file should be recognized");
        assert!(temp_file.can_delete);

        assert!(file_suggestion(Path::new("/private/var/db/system.db")).is_none());
    }
}
