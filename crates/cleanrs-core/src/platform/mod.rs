//! Platform boundaries for paths, storage roots, and system-owned locations.
//!
//! Shared scanning and cleaning code talks to this facade instead of encoding
//! operating-system paths directly. Platform-specific rules can build on the
//! same boundary without duplicating the TUI or cleanup model.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub mod unsupported;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub use macos::config;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use unsupported::config;
#[cfg(target_os = "windows")]
pub use windows::config;
