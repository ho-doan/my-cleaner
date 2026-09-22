pub mod agent_cache;
pub mod brew;
pub mod cargo;
pub mod claude;
pub mod codex;
pub mod dart;
pub mod docker;
pub mod gradle;
pub mod kiro;
pub mod macos_system;
pub mod maven;
pub mod npm;
pub mod ollama;
pub mod pip;
pub mod pnpm;
pub mod uv;
pub mod xcode;
pub mod yarn;

#[cfg(test)]
mod tests;

use crate::model::{CleanMethod, CleanTarget};
use crate::scanner::dir_size;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn command_available(command: &str) -> bool {
    if command.contains('/') {
        return std::path::Path::new(command).is_file();
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(command))
        .any(|path| path.is_file())
}

pub fn home_path(relative: &str) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(relative))
}

pub fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to start {program}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "command {} exited with status {}",
            std::iter::once(program)
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
            output.status
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Build a target only when it still contains reclaimable data.
///
/// Package-manager cleanup commands commonly leave their cache directory in
/// place but empty. Omitting zero-byte targets keeps a post-clean rescan honest.
pub fn non_empty_target(
    path: PathBuf,
    description: impl Into<String>,
    method: CleanMethod,
) -> Result<Option<CleanTarget>> {
    if !path.exists() {
        return Ok(None);
    }
    let size_bytes = dir_size(&path)?;
    if size_bytes == 0 {
        return Ok(None);
    }
    Ok(Some(CleanTarget {
        path,
        size_bytes,
        description: description.into(),
        method,
    }))
}
