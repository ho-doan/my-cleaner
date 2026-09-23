//! Fallback platform implementation for compilation on unsupported targets.

pub mod config {
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

    pub fn excluded_root_paths(_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn is_protected_path(_path: &Path) -> bool {
        false
    }

    pub fn read_only_report_paths() -> Vec<(PathBuf, &'static str)> {
        Vec::new()
    }

    pub fn read_only_advice() -> &'static str {
        "No platform-specific read-only system paths are configured."
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
}
