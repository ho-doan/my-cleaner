use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/ho-doan/my-cleaner/releases/latest";
#[cfg(target_os = "windows")]
const INSTALL_SCRIPT: &str =
    "https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.ps1";
#[cfg(not(target_os = "windows"))]
const INSTALL_SCRIPT: &str =
    "https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.sh";
#[cfg(target_os = "windows")]
const RELEASE_CHECKSUMS: &str =
    "https://github.com/ho-doan/my-cleaner/releases/download/v{version}/checksums.txt";
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

/// Check GitHub's latest release and expose it only after the installer CDN is ready.
pub fn check_latest_release(current_version: &str) -> Result<Option<UpdateInfo>> {
    let release: GithubRelease =
        serde_json::from_str(&fetch_url(RELEASES_API).context("fetch GitHub latest release")?)
            .context("parse GitHub latest release")?;
    let latest_version = normalize_version(&release.tag_name)
        .context("GitHub latest release has an invalid version tag")?;
    let current_version =
        normalize_version(current_version).context("cleanrs has an invalid current version")?;

    if compare_versions(&latest_version, &current_version).is_gt() {
        wait_for_installer_script(&latest_version).with_context(|| {
            format!("update v{latest_version} is published, but its installer CDN is not ready yet")
        })?;
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
            Ok(script) => match installer_is_ready(version, &script, target, attempt) {
                Ok(true) => {
                    if attempt > 1 {
                        eprintln!("Installer CDN is ready for cleanrs {version}.");
                    }
                    return Ok(script);
                }
                Ok(false) => eprintln!(
                    "Waiting for installer CDN to publish the cleanrs {version} checksum ({attempt}/{INSTALLER_READY_ATTEMPTS})…"
                ),
                Err(error) => eprintln!(
                    "Installer CDN checksum check is not ready ({attempt}/{INSTALLER_READY_ATTEMPTS}): {error}"
                ),
            },
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
    fetch_url(&cache_busted_url).context("fetch installer CDN script")
}

#[cfg(target_os = "windows")]
fn fetch_url(url: &str) -> Result<String> {
    let escaped_url = url.replace('\'', "''");
    let command = format!(
        "$ProgressPreference='SilentlyContinue'; (Invoke-WebRequest -UseBasicParsing -Headers @{{'Cache-Control'='no-cache'; Pragma='no-cache'}} -Uri '{escaped_url}').Content"
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command.as_str(),
        ])
        .output()
        .context("start PowerShell network request")?;
    if !output.status.success() {
        bail!(
            "PowerShell network request exited with status {}",
            output.status
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(target_os = "windows"))]
fn fetch_url(url: &str) -> Result<String> {
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
            url,
        ])
        .output()
        .context("start curl network request")?;
    if !output.status.success() {
        bail!("curl network request exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("network response was not UTF-8")
}

fn current_release_target() -> Result<&'static str> {
    current_release_target_for_platform()
}

#[cfg(target_os = "macos")]
fn current_release_target_for_platform() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("aarch64-apple-darwin"),
        "x86_64" => Ok("x86_64-apple-darwin"),
        architecture => bail!("unsupported macOS architecture for upgrade: {architecture}"),
    }
}

#[cfg(target_os = "windows")]
fn current_release_target_for_platform() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64-pc-windows-msvc"),
        architecture => bail!("unsupported Windows architecture for upgrade: {architecture}"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn current_release_target_for_platform() -> Result<&'static str> {
    bail!("unsupported platform for upgrade")
}

fn installer_is_ready(version: &str, script: &str, target: &str, attempt: usize) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let _ = attempt;
        Ok(installer_script_has_checksum(script, version, target))
    }

    #[cfg(target_os = "windows")]
    {
        let _ = script;
        let archive = format!("cleanrs-v{version}-{target}.zip");
        let manifest_url = format!(
            "{}?version={version}&attempt={attempt}",
            RELEASE_CHECKSUMS.replace("{version}", version)
        );
        let manifest = fetch_url(&manifest_url)?;
        Ok(checksum_manifest_has_archive(&manifest, &archive))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (version, script, target, attempt);
        Ok(false)
    }
}

#[cfg(any(target_os = "windows", test))]
fn checksum_manifest_has_archive(manifest: &str, archive: &str) -> bool {
    manifest.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let Some(checksum) = parts.next() else {
            return false;
        };
        let Some(name) = parts.next_back() else {
            return false;
        };
        checksum.len() == 64
            && checksum
                .chars()
                .all(|character| character.is_ascii_hexdigit())
            && name.trim_start_matches('*') == archive
    })
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_installer_script(_script: &str, _version: &str, _install_dir: &Path) -> Result<()> {
    bail!("automatic upgrade is not supported on this platform")
}

#[cfg(target_os = "windows")]
fn run_installer_script(script: &str, version: &str, install_dir: &Path) -> Result<()> {
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .env("CLEANRS_VERSION", version)
        .env("CLEANRS_INSTALL_DIR", install_dir)
        .spawn()
        .context("start verified PowerShell installer")?;

    let mut stdin = child
        .stdin
        .take()
        .context("open stdin for verified PowerShell installer")?;
    stdin
        .write_all(script.as_bytes())
        .context("send verified PowerShell installer")?;
    drop(stdin);

    let status = child.wait().context("wait for PowerShell installer")?;
    if !status.success() {
        bail!("PowerShell installer exited with status {status}");
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
    use super::{checksum_manifest_has_archive, compare_versions, normalize_version};
    use std::cmp::Ordering;

    #[cfg(target_os = "macos")]
    use super::installer_script_has_checksum;

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
    #[cfg(target_os = "macos")]
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

    #[test]
    fn checksum_manifest_requires_a_verified_archive_entry() {
        let manifest = "abc\n1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef  cleanrs-v0.1.6-x86_64-pc-windows-msvc.zip\n";
        assert!(checksum_manifest_has_archive(
            manifest,
            "cleanrs-v0.1.6-x86_64-pc-windows-msvc.zip"
        ));
        assert!(!checksum_manifest_has_archive(
            manifest,
            "cleanrs-v0.1.6-aarch64-pc-windows-msvc.zip"
        ));
    }
}
