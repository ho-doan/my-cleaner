//! Inventory and recoverable removal for standalone user-installed binaries.
//!
//! Package managers already have dedicated inventory and uninstall flows in
//! crate::global. This module is intentionally narrower: it looks for
//! executable files placed directly in common user-owned binary directories,
//! which is where many curl installers put their launcher.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::executor::move_to_trash;
use crate::{history::record_history, platform::config::home_path, scanner::dir_size};

const ROOTS: [(&str, &str); 5] = [
    (".local/bin", "~/.local/bin"),
    ("bin", "~/bin"),
    (".deno/bin", "~/.deno/bin"),
    (".bun/bin", "~/.bun/bin"),
    (".volta/bin", "~/.volta/bin"),
];

const VERSION_STORES: [(&str, &str, &str, &str); 2] = [
    (
        ".codex/packages/standalone/releases",
        "~/.codex/packages/standalone/releases",
        ".codex/packages/standalone/current",
        "Codex CLI standalone runtime",
    ),
    (
        ".local/share/claude/versions",
        "~/.local/share/claude/versions",
        ".local/bin/claude",
        "Claude Code standalone runtime",
    ),
];

#[derive(Clone, Debug, Serialize)]
pub struct StandaloneTool {
    pub name: String,
    pub path: PathBuf,
    pub source_root: String,
    pub size_bytes: u64,
    pub is_directory: bool,
    pub can_remove: bool,
    pub warning: Option<String>,
}

impl StandaloneTool {
    pub fn removal_description(&self) -> String {
        let item_kind = if self.is_directory { "folder" } else { "file" };
        format!("move {item_kind} {} to Trash", self.path.display())
    }
}

#[derive(Debug, Serialize)]
pub struct StandaloneToolScan {
    pub tools: Vec<StandaloneTool>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StandaloneToolResult {
    pub name: String,
    pub path: PathBuf,
    pub dry_run: bool,
    pub executed: bool,
    pub success: bool,
    pub message: String,
}

/// Scan only direct executable-like entries in known user-owned binary roots.
/// Missing roots are normal and do not produce warnings.
pub fn scan_standalone_tools() -> StandaloneToolScan {
    let running_executable = std::env::current_exe()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok());
    let mut tools = Vec::new();
    let mut errors = Vec::new();

    for (relative_root, display_root) in ROOTS {
        let Some(root) = home_path(relative_root) else {
            continue;
        };
        match scan_root(&root, display_root, running_executable.as_deref()) {
            Ok(mut found) => tools.append(&mut found),
            Err(error) => errors.push(format!("{display_root}: {error:#}")),
        }
    }
    for (relative_root, display_root, active_relative_path, description) in VERSION_STORES {
        let Some(root) = home_path(relative_root) else {
            continue;
        };
        let active_path =
            home_path(active_relative_path).and_then(|path| fs::canonicalize(path).ok());
        match scan_version_store(&root, display_root, active_path.as_deref(), description) {
            Ok(mut found) => tools.append(&mut found),
            Err(error) => errors.push(format!("{display_root}: {error:#}")),
        }
    }

    tools.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    StandaloneToolScan { tools, errors }
}

#[tracing::instrument(skip(tool), fields(tool = %tool.name, path = %tool.path.display(), dry_run))]
pub fn remove_standalone_tool(
    tool: &StandaloneTool,
    dry_run: bool,
) -> Result<StandaloneToolResult> {
    let result = remove_standalone_tool_impl(tool, dry_run);
    match &result {
        Ok(outcome) if !outcome.dry_run => record_history(
            "uninstall",
            "Standalone/user-installed",
            &tool.path.display().to_string(),
            "success",
            &outcome.message,
        ),
        Err(error) if !dry_run => record_history(
            "uninstall",
            "Standalone/user-installed",
            &tool.path.display().to_string(),
            "failed",
            &error.to_string(),
        ),
        _ => {}
    }
    result
}

fn remove_standalone_tool_impl(
    tool: &StandaloneTool,
    dry_run: bool,
) -> Result<StandaloneToolResult> {
    if !tool.can_remove {
        bail!(
            "{} cannot be removed by cleanrs: {}",
            tool.name,
            tool.warning
                .as_deref()
                .unwrap_or("protected standalone binary")
        );
    }
    if !is_supported_path(&tool.path) {
        bail!(
            "refusing to remove standalone path outside approved user binary roots: {}",
            tool.path.display()
        );
    }

    if dry_run {
        return Ok(StandaloneToolResult {
            name: tool.name.clone(),
            path: tool.path.clone(),
            dry_run: true,
            executed: false,
            success: true,
            message: format!("dry-run: would {}", tool.removal_description()),
        });
    }

    move_to_trash(&tool.path).with_context(|| {
        format!(
            "failed to move {} to Trash; check file ownership, parent-directory write permission, or macOS Full Disk Access",
            tool.path.display()
        )
    })?;
    Ok(StandaloneToolResult {
        name: tool.name.clone(),
        path: tool.path.clone(),
        dry_run: false,
        executed: true,
        success: true,
        message: format!("moved {} to Trash", tool.path.display()),
    })
}

fn scan_root(
    root: &Path,
    display_root: &str,
    running_executable: Option<&Path>,
) -> Result<Vec<StandaloneTool>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };

    let mut tools = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        if !file_type.is_symlink() && !is_executable(&entry.path()) {
            continue;
        }

        let path = entry.path();
        let Some(size_bytes) = fs::symlink_metadata(&path)
            .ok()
            .map(|metadata| metadata.len())
        else {
            continue;
        };
        let running = running_executable.is_some_and(|running| same_path(&path, running));
        let warning = running.then(|| {
            "This is the currently running cleanrs binary; use the documented uninstall command instead"
                .to_owned()
        });
        tools.push(StandaloneTool {
            name: name.to_string_lossy().into_owned(),
            size_bytes,
            path,
            source_root: display_root.to_owned(),
            is_directory: false,
            can_remove: !running,
            warning,
        });
    }
    Ok(tools)
}

fn scan_version_store(
    root: &Path,
    display_root: &str,
    active_path: Option<&Path>,
    description: &str,
) -> Result<Vec<StandaloneTool>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };

    let mut tools = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() && !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        let is_directory = file_type.is_dir();
        let size_bytes = if is_directory {
            dir_size(&path)?
        } else {
            fs::symlink_metadata(&path)
                .with_context(|| format!("inspect {}", path.display()))?
                .len()
        };
        if size_bytes == 0 {
            continue;
        }

        let active = active_path.is_some_and(|active| same_path(&path, active));
        let warning = active.then(|| {
            format!("{description} is active; update or uninstall it through its own installer")
        });
        tools.push(StandaloneTool {
            name: name.to_string_lossy().into_owned(),
            path,
            source_root: display_root.to_owned(),
            size_bytes,
            is_directory,
            can_remove: !active,
            warning,
        });
    }
    Ok(tools)
}

fn is_supported_path(path: &Path) -> bool {
    ROOTS
        .iter()
        .filter_map(|(relative, _)| home_path(relative))
        .chain(
            VERSION_STORES
                .iter()
                .filter_map(|(relative, _, _, _)| home_path(relative)),
        )
        .any(|root| path.parent().is_some_and(|parent| parent == root))
}

fn same_path(left: &Path, right: &Path) -> bool {
    fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "exe" | "cmd" | "bat" | "ps1"
        )
    })
}

#[cfg(not(any(unix, windows)))]
fn is_executable(_path: &Path) -> bool {
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::{scan_root, scan_version_store};
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn scans_only_direct_executable_entries_and_protects_running_binary() {
        let root = tempfile::tempdir().expect("tempdir");
        let executable = root.path().join("curl-tool");
        fs::write(&executable, b"binary").expect("write executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("make executable");
        fs::write(root.path().join("notes.txt"), b"not a binary").expect("write note");
        fs::create_dir(root.path().join("nested")).expect("create nested directory");

        let tools = scan_root(root.path(), "~/bin", Some(&executable)).expect("scan root");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "curl-tool");
        assert!(!tools[0].can_remove);
        assert!(tools[0]
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("currently running")));
    }

    #[test]
    fn protects_active_version_and_reports_old_directory_size() {
        let root = tempfile::tempdir().expect("tempdir");
        let active = root.path().join("active-version");
        let old = root.path().join("old-version");
        fs::create_dir(&active).expect("create active version");
        fs::create_dir(&old).expect("create old version");
        fs::write(active.join("runtime"), b"active").expect("write active runtime");
        fs::write(old.join("runtime"), b"old runtime").expect("write old runtime");

        let tools = scan_version_store(
            root.path(),
            "~/.codex/packages/standalone/releases",
            Some(&active),
            "Codex CLI standalone runtime",
        )
        .expect("scan version store");
        let active_tool = tools
            .iter()
            .find(|tool| tool.path == active)
            .expect("active version");
        let old_tool = tools
            .iter()
            .find(|tool| tool.path == old)
            .expect("old version");
        assert!(!active_tool.can_remove);
        assert!(old_tool.can_remove);
        assert!(old_tool.is_directory);
        assert!(old_tool.size_bytes > 0);
    }
}
