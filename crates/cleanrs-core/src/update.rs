use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

const RELEASES_API: &str = "https://api.github.com/repos/ho-doan/my-cleaner/releases/latest";
const INSTALL_SCRIPT: &str =
    "https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.sh";
const BREW_FORMULA: &str = "ho-doan/my-cleaner/cleanrs";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// Check GitHub's latest release without blocking the TUI thread.
pub fn check_latest_release(current_version: &str) -> Result<Option<UpdateInfo>> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "5",
            "--proto",
            "=https",
            "--tlsv1.2",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: cleanrs-version-check",
            RELEASES_API,
        ])
        .output()
        .context("start update check with curl")?;
    if !output.status.success() {
        anyhow::bail!("GitHub update check failed with status {}", output.status);
    }

    let release: GithubRelease =
        serde_json::from_slice(&output.stdout).context("parse GitHub latest release")?;
    let latest_version = normalize_version(&release.tag_name)
        .context("GitHub latest release has an invalid version tag")?;
    let current_version =
        normalize_version(current_version).context("cleanrs has an invalid current version")?;

    if compare_versions(&latest_version, &current_version).is_gt() {
        Ok(Some(UpdateInfo {
            current_version,
            latest_version,
            release_url: release.html_url,
        }))
    } else {
        Ok(None)
    }
}

/// Run the official installer after the TUI has been closed.
pub fn perform_upgrade(update: &UpdateInfo) -> Result<()> {
    let executable =
        std::env::current_exe().context("locate the running cleanrs executable for upgrade")?;
    let executable = executable.canonicalize().unwrap_or(executable);

    if is_homebrew_path(&executable) {
        run_command("brew", &["update"])?;
        run_command("brew", &["upgrade", BREW_FORMULA])?;
        return Ok(());
    }

    if executable
        .components()
        .any(|component| component.as_os_str() == "target")
    {
        bail!(
            "cannot upgrade a development build; install cleanrs with Homebrew or the curl installer"
        );
    }

    let install_dir = executable
        .parent()
        .context("running cleanrs executable has no parent directory")?;
    let installer_command = format!("curl -fsSL --proto '=https' --tlsv1.2 {INSTALL_SCRIPT} | sh");
    let status = Command::new("sh")
        .arg("-c")
        .arg(installer_command)
        .env("CLEANRS_VERSION", &update.latest_version)
        .env("CLEANRS_INSTALL_DIR", install_dir)
        .status()
        .context("start curl installer")?;
    if !status.success() {
        anyhow::bail!("curl installer exited with status {status}");
    }
    Ok(())
}

fn run_command(program: &str, arguments: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .with_context(|| format!("start {program}"))?;
    if !status.success() {
        anyhow::bail!(
            "command {} exited with status {status}",
            std::iter::once(program)
                .chain(arguments.iter().copied())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(())
}

fn is_homebrew_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "Cellar")
}

fn normalize_version(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|part| part.parse::<u64>().is_err()) {
        return None;
    }
    Some(parts.join("."))
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect::<Vec<_>>();
    let right = right
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect::<Vec<_>>();
    let length = left.len().max(right.len());
    (0..length)
        .map(|index| {
            (
                left.get(index).copied().unwrap_or_default(),
                right.get(index).copied().unwrap_or_default(),
            )
        })
        .find_map(|(left, right)| (left != right).then_some(left.cmp(&right)))
        .unwrap_or(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::{compare_versions, normalize_version};
    use std::cmp::Ordering;

    #[test]
    fn normalizes_release_versions() {
        assert_eq!(normalize_version("v0.1.3").as_deref(), Some("0.1.3"));
        assert!(normalize_version("latest").is_none());
    }

    #[test]
    fn compares_release_versions_without_semver_dependency() {
        assert_eq!(compare_versions("0.1.4", "0.1.3"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.3", "0.1.3"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "0.99.9"), Ordering::Greater);
    }
}
