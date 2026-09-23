//! Platform-independent core for cleanrs.
//!
//! The core deliberately keeps scanning and cleaning separate. A scan only reads
//! metadata; a clean operation is the only place where a command or a delete can
//! be executed.

pub mod cleaner;
pub mod executor;
pub mod global;
pub mod history;
pub mod model;
pub mod rules;
pub mod scanner;

pub use cleaner::{all_cleaners, scan_all, scan_cleaner, Cleaner, CleanerScan};
pub use global::{
    scan_global_tools, uninstall_global_tool, GlobalTool, GlobalToolManager, GlobalToolResult,
    GlobalToolScan,
};
pub use model::{Category, CleanMethod, CleanOptions, CleanResult, CleanTarget, RiskLevel};
pub use scanner::{
    can_delete_path, dir_size, full_disk_scan, is_protected_path, scan_directory, DirectoryScan,
    DirectoryScanEntry, DirectorySuggestion, DiskScanEntry, FullDiskScan,
};
