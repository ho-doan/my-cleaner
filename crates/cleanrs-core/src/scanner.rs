use anyhow::{Context, Result};
use jwalk::WalkDir;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct DiskScanEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct FullDiskScan {
    pub root: PathBuf,
    pub root_entries: Vec<DiskScanEntry>,
    pub home_entries: Vec<DiskScanEntry>,
    pub excluded_paths: Vec<PathBuf>,
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
    if !root.is_dir() {
        anyhow::bail!("full-disk scan root is not a directory: {}", root.display());
    }

    let excluded_paths = excluded_root_paths(root);
    let (mut root_entries, root_inaccessible) =
        scan_children(root, &excluded_paths).with_context(|| format!("scan {}", root.display()))?;
    root_entries.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    root_entries.truncate(limit);

    let (mut home_entries, home_inaccessible) = match std::env::var_os("HOME") {
        Some(home) => scan_children(Path::new(&home), &[])?,
        None => (Vec::new(), 0),
    };
    home_entries.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    home_entries.truncate(limit);

    Ok(FullDiskScan {
        root: root.to_path_buf(),
        root_entries,
        home_entries,
        excluded_paths,
        inaccessible_paths: root_inaccessible + home_inaccessible,
    })
}

fn excluded_root_paths(root: &Path) -> Vec<PathBuf> {
    [
        "System", "Volumes", "private", "dev", "cores", "usr", "bin", "sbin",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .filter(|path| path.exists())
    .collect()
}

fn scan_children(parent: &Path, excluded: &[PathBuf]) -> Result<(Vec<DiskScanEntry>, usize)> {
    let children = std::fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();

    let results = children
        .into_par_iter()
        .map(|path| {
            if excluded.contains(&path) {
                return (None, 0usize);
            }

            let (size_bytes, inaccessible) = tolerant_dir_size(&path);
            let entry = (size_bytes > 0).then_some(DiskScanEntry { path, size_bytes });
            (entry, inaccessible)
        })
        .collect::<Vec<_>>();

    let inaccessible = results.iter().map(|(_, count)| count).sum();
    let entries = results.into_iter().filter_map(|(entry, _)| entry).collect();
    Ok((entries, inaccessible))
}

fn tolerant_dir_size(path: &Path) -> (u64, usize) {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_file() {
            return (metadata.len(), 0);
        }
    }

    let entries = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
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

    (size, metadata_errors)
}

#[cfg(test)]
mod tests {
    use super::dir_size;
    use std::fs;
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
}
