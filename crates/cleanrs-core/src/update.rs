use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

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
    let installer_script = wait_for_installer_script(&update.latest_version)?;
    run_installer_script(&installer_script, &update.latest_version, install_dir)
}

const INSTALLER_READY_ATTEMPTS: usize = 6;

fn wait_for_installer_script(version: &str) -> Result<String> {
    let target = current_release_target()?;
    for attempt in 1..=INSTALLER_READY_ATTEMPTS {
        match fetch_installer_script(version, attempt) {
            Ok(script) if installer_script_has_checksum(&script, version, target) => {
                if attempt > 1 {
                    eprintln!("Installer CDN is ready for cleanrs {version}.");
                }
                return Ok(script);
            }
            Ok(_) => {
                eprintln!(
                    "Waiting for installer CDN to publish the cleanrs {version} checksum ({attempt}/{INSTALLER_READY_ATTEMPTS})…"
                );
            }
            Err(error) => {
                eprintln!(
                    "Installer CDN is not ready ({attempt}/{INSTALLER_READY_ATTEMPTS}): {error}"
                );
            }
        }

        if attempt < INSTALLER_READY_ATTEMPTS {
            let delay_seconds = 2_u64.pow((attempt.saturating_sub(1).min(3)) as u32);
            sleep(Duration::from_secs(delay_seconds));
        }
    }

    bail!(
        "installer CDN did not publish a verified checksum for cleanrs {version} ({target}) after {INSTALLER_READY_ATTEMPTS} attempts"
    )
}

fn fetch_installer_script(version: &str, attempt: usize) -> Result<String> {
    let cache_busted_url = format!("{INSTALL_SCRIPT}?version={version}&attempt={attempt}");
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "10",
            "--retry",
            "2",
            "--proto",
            "=https",
            "--tlsv1.2",
            "-H",
            "Cache-Control: no-cache",
            "-H",
            "Pragma: no-cache",
            cache_busted_url.as_str(),
        ])
        .output()
        .context("start installer CDN check")?;
    if !output.status.success() {
        bail!("installer CDN request exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("installer CDN returned non-UTF-8 script")
}

fn current_release_target() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("aarch64-apple-darwin"),
        "x86_64" => Ok("x86_64-apple-darwin"),
        architecture => bail!("unsupported macOS architecture for upgrade: {architecture}"),
    }
}

fn installer_script_has_checksum(script: &str, version: &str, target: &str) -> bool {
    let case_entry = format!("{version}:{target})");
    let mut lines = script.lines();
    while let Some(line) = lines.next() {
        if line.trim() == case_entry {
            return lines.next().is_some_and(|checksum| {
                checksum.contains("EXPECTED_SHA256=") && !checksum.contains("\"\"")
            });
        }
    }
    false
}

fn run_installer_script(script: &str, version: &str, install_dir: &Path) -> Result<()> {
    let mut child = Command::new("sh")
        .arg("-s")
        .stdin(Stdio::piped())
        .env("CLEANRS_VERSION", version)
        .env("CLEANRS_INSTALL_DIR", install_dir)
        .spawn()
        .context("start verified curl installer")?;

    let mut stdin = child
        .stdin
        .take()
        .context("open stdin for verified curl installer")?;
    stdin
        .write_all(script.as_bytes())
        .context("send verified installer script")?;
    drop(stdin);

    let status = child.wait().context("wait for curl installer")?;
    if !status.success() {
        bail!("curl installer exited with status {status}");
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
    use super::{compare_versions, installer_script_has_checksum, normalize_version};
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

    #[test]
    fn installer_readiness_requires_a_non_empty_target_checksum() {
        let script = r#"
            0.1.5:aarch64-apple-darwin)
              EXPECTED_SHA256="abc123"
            0.1.5:x86_64-apple-darwin)
              EXPECTED_SHA256=""
        "#;

        assert!(installer_script_has_checksum(
            script,
            "0.1.5",
            "aarch64-apple-darwin"
        ));
        assert!(!installer_script_has_checksum(
            script,
            "0.1.5",
            "x86_64-apple-darwin"
        ));
        assert!(!installer_script_has_checksum(
            script,
            "0.1.6",
            "aarch64-apple-darwin"
        ));
    }
}
