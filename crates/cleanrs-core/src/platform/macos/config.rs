//! macOS paths and system-safety policy.

use std::path::{Path, PathBuf};

pub const STANDARD_HOME_DIRECTORIES: [&str; 3] = ["Desktop", "Documents", "Downloads"];

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn home_path(relative: &str) -> Option<PathBuf> {
    home_dir().map(|home| home.join(relative))
}

pub fn default_scan_root() -> PathBuf {
    PathBuf::from("/")
}

pub fn is_default_scan_root(path: &Path) -> bool {
    path == Path::new("/")
}

pub fn excluded_root_paths(root: &Path) -> Vec<PathBuf> {
    [
        "System", "Library", "Volumes", "private", "dev", "cores", "usr", "bin", "sbin",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .filter(|path| path.exists())
    .collect()
}

pub fn readonly_inventory_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = excluded_root_paths(root);
    let volumes = root.join("Volumes");
    if root == Path::new("/") {
        if let Ok(entries) = std::fs::read_dir(&volumes) {
            let mounted_volumes = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            if !mounted_volumes.is_empty() {
                paths.retain(|path| path != &volumes);
                paths.extend(mounted_volumes);
            }
        }
    }
    paths
}

pub fn is_volume_root(path: &Path) -> bool {
    path.parent() == Some(Path::new("/Volumes")) && path != Path::new("/Volumes")
}

pub fn is_protected_path(path: &Path) -> bool {
    [
        "/System", "/Library", "/Volumes", "/private", "/dev", "/cores", "/usr", "/bin", "/sbin",
    ]
    .into_iter()
    .map(Path::new)
    .any(|protected| path == protected || path.starts_with(protected))
}

pub fn read_only_report_paths() -> Vec<(PathBuf, &'static str)> {
    let mut paths = vec![
        (PathBuf::from("/Library/Updates"), "macOS update leftovers"),
        (PathBuf::from("/macOS Install Data"), "macOS installer data"),
    ];
    let system_volumes = PathBuf::from("/System/Volumes");
    if let Ok(entries) = std::fs::read_dir(&system_volumes) {
        paths.extend(entries.filter_map(|entry| {
            entry
                .ok()
                .map(|entry| (entry.path(), "System volume (read-only)"))
        }));
    }
    paths
}

pub fn read_only_advice() -> &'static str {
    "Read-only macOS system report. Use macOS system tools; cleanrs will never delete these paths."
}

pub fn command_available(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).is_file();
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(command))
        .any(|path| path.is_file())
}
