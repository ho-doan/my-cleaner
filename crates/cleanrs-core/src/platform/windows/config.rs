//! Windows paths and system-safety policy.

use std::path::{Path, PathBuf};

pub const STANDARD_HOME_DIRECTORIES: [&str; 3] = ["Desktop", "Documents", "Downloads"];

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(
            || match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
                (Some(drive), Some(path)) => Some(PathBuf::from(drive).join(path)),
                _ => None,
            },
        )
}

pub fn home_path(relative: &str) -> Option<PathBuf> {
    home_dir().map(|home| home.join(relative))
}

pub fn default_scan_root() -> PathBuf {
    std::env::var_os("SystemDrive")
        .map(PathBuf::from)
        .map(|drive| {
            let drive_name = drive.as_os_str().to_string_lossy();
            if drive_name.ends_with('\\') || drive_name.ends_with('/') {
                drive
            } else {
                let mut root = drive;
                root.push("\\");
                root
            }
        })
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
}

pub fn is_default_scan_root(path: &Path) -> bool {
    path == default_scan_root()
}

pub fn excluded_root_paths(_root: &Path) -> Vec<PathBuf> {
    [
        std::env::var_os("SystemRoot").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        std::env::var_os("ProgramData").map(PathBuf::from),
    ]
    .into_iter()
    .flatten()
    .filter(|path| path.exists())
    .collect()
}

pub fn is_protected_path(path: &Path) -> bool {
    let protected = [
        std::env::var_os("SystemRoot").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        std::env::var_os("ProgramData").map(PathBuf::from),
    ];
    protected
        .into_iter()
        .flatten()
        .any(|root| path == root || path.starts_with(root))
}

pub fn read_only_report_paths() -> Vec<(PathBuf, &'static str)> {
    Vec::new()
}

pub fn read_only_advice() -> &'static str {
    "Read-only Windows system report. cleanrs will never delete protected system paths."
}

pub fn command_available(command: &str) -> bool {
    if command.contains('\\') || command.contains('/') {
        return Path::new(command).is_file();
    }

    let extensions = std::env::var_os("PATHEXT")
        .map(|value| {
            std::env::split_paths(&value)
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            vec![
                ".COM".to_owned(),
                ".EXE".to_owned(),
                ".BAT".to_owned(),
                ".CMD".to_owned(),
            ]
        });
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .flat_map(|path| {
            let mut candidates = vec![path.join(command)];
            candidates.extend(
                extensions
                    .iter()
                    .map(|extension| path.join(format!("{command}{extension}"))),
            );
            candidates
        })
        .any(|path| path.is_file())
}
