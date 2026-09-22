//! Platform-independent core for cleanrs.
//!
//! The core deliberately keeps scanning and cleaning separate. A scan only reads
//! metadata; a clean operation is the only place where a command or a delete can
//! be executed.

pub mod cleaner;
pub mod executor;
pub mod model;
pub mod rules;
pub mod scanner;

pub use cleaner::{all_cleaners, scan_all, scan_cleaner, Cleaner, CleanerScan};
pub use model::{Category, CleanMethod, CleanResult, CleanTarget, RiskLevel};
pub use scanner::{dir_size, full_disk_scan, DiskScanEntry, FullDiskScan};
