//! Platform-independent core for cleanrs.
//!
//! The core deliberately keeps scanning and cleaning separate. A scan only reads
//! metadata; a clean operation is the only place where a command or a delete can
//! be executed.

pub mod allowlist;
pub mod cleaner;
pub mod executor;
pub mod global;
pub mod history;
pub mod model;
pub mod platform;
pub mod report;
pub mod rules;
pub mod scanner;
pub mod standalone;
pub mod update;

pub use allowlist::{load_delete_allowlist, toggle_delete_allowlist};
pub use cleaner::{all_cleaners, scan_all, scan_cleaner, Cleaner, CleanerScan};
pub use global::{
    scan_global_tools, uninstall_global_tool, GlobalTool, GlobalToolManager, GlobalToolResult,
    GlobalToolScan,
};
pub use model::{Category, CleanMethod, CleanOptions, CleanResult, CleanTarget, RiskLevel};
pub use platform::config::default_scan_root;
pub use report::{all_reporters, scan_all_reports, ReadOnlyScan, ReadOnlyTarget, ReportOnly};
pub use scanner::{
    can_delete_path, dir_size, full_disk_scan, is_protected_path, scan_directory, DirectoryScan,
    DirectoryScanEntry, DirectorySuggestion, DiskScanEntry, FullDiskScan,
};
pub use standalone::{
    remove_standalone_tool, scan_standalone_tools, StandaloneTool, StandaloneToolResult,
    StandaloneToolScan,
};
pub use update::{check_latest_release, perform_upgrade, UpdateInfo};
