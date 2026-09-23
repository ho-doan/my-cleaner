use crate::scanner::is_protected_path;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ALLOWLIST_FILE: &str = ".config/cleanrs/whitelist.toml";

#[derive(Debug, Default, Deserialize, Serialize)]
struct AllowlistFile {
    #[serde(default)]
    paths: Vec<String>,
}

/// Load exact directory paths approved by the user for Explorer cleanup.
///
/// This is intentionally separate from cleanup rules: an allowlisted path is
/// still only an Explorer suggestion and still requires an explicit `x`/`y`
/// action before it is moved to Trash.
pub fn load_delete_allowlist() -> Result<BTreeSet<PathBuf>> {
    let Some(path) = allowlist_path() else {
        return Ok(BTreeSet::new());
    };
    if !path.is_file() {
        return Ok(BTreeSet::new());
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("read delete allowlist from {}", path.display()))?;
    let config: AllowlistFile = toml::from_str(&contents).context("parse delete allowlist")?;
    Ok(config.paths.into_iter().map(PathBuf::from).collect())
}

/// Add or remove one exact directory from the Explorer delete allowlist.
/// Returns `true` when the path is now allowlisted and `false` when it was
/// removed.
pub fn toggle_delete_allowlist(path: &Path) -> Result<bool> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect allowlist path {}", path.display()))?;
    if !metadata.is_dir() {
        anyhow::bail!("only real directories can be added to the delete allowlist");
    }
    if !path.is_absolute() {
        anyhow::bail!("delete allowlist path must be absolute: {}", path.display());
    }
    if is_protected_path(path) {
        anyhow::bail!("protected system paths cannot be added to the delete allowlist");
    }
    if std::env::var_os("HOME").is_some_and(|home| path == Path::new(&home)) {
        anyhow::bail!("the home directory cannot be added to the delete allowlist");
    }

    let mut paths = load_delete_allowlist()?;
    let now_allowlisted = if paths.contains(path) {
        paths.remove(path);
        false
    } else {
        paths.insert(path.to_path_buf());
        true
    };
    write_allowlist(&paths)?;
    Ok(now_allowlisted)
}

fn allowlist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(ALLOWLIST_FILE))
}

fn write_allowlist(paths: &BTreeSet<PathBuf>) -> Result<()> {
    let path = allowlist_path().context("HOME is not set for delete allowlist")?;
    let parent = path
        .parent()
        .context("delete allowlist has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create delete allowlist directory {}", parent.display()))?;

    let config = AllowlistFile {
        paths: paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
    };
    let contents = toml::to_string_pretty(&config).context("serialize delete allowlist")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("write delete allowlist to {}", path.display()))?;
    Ok(())
}
