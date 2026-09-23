//! Inventory and safe command-backed removal for globally installed tools.
//!
//! This module deliberately uses each package manager's own uninstall command.
//! It never removes a package directory directly, and callers must still pass
//! `dry_run = false` explicitly before a command is executed.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::{process::Command, string::String};

use crate::rules::command_available;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalToolManager {
    Cargo,
    Dart,
    Npm,
    Pnpm,
    Yarn,
    Uv,
    Pipx,
    BrewFormula,
    BrewCask,
}

impl GlobalToolManager {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo installed",
            Self::Dart => "Dart global",
            Self::Npm => "npm global",
            Self::Pnpm => "pnpm global",
            Self::Yarn => "Yarn global",
            Self::Uv => "uv tools",
            Self::Pipx => "pipx",
            Self::BrewFormula => "Homebrew formula",
            Self::BrewCask => "Homebrew cask",
        }
    }

    fn binary(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Dart => "dart",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Uv => "uv",
            Self::Pipx => "pipx",
            Self::BrewFormula | Self::BrewCask => "brew",
        }
    }

    fn uninstall_arguments(self, name: &str) -> Vec<String> {
        match self {
            Self::Cargo => vec!["uninstall".to_owned(), name.to_owned()],
            Self::Dart => vec![
                "pub".to_owned(),
                "global".to_owned(),
                "deactivate".to_owned(),
                name.to_owned(),
            ],
            Self::Npm => vec![
                "uninstall".to_owned(),
                "--global".to_owned(),
                name.to_owned(),
            ],
            Self::Pnpm => vec!["remove".to_owned(), "--global".to_owned(), name.to_owned()],
            Self::Yarn => vec!["global".to_owned(), "remove".to_owned(), name.to_owned()],
            Self::Uv => vec!["tool".to_owned(), "uninstall".to_owned(), name.to_owned()],
            Self::Pipx => vec!["uninstall".to_owned(), name.to_owned()],
            Self::BrewFormula => vec![
                "uninstall".to_owned(),
                "--formula".to_owned(),
                name.to_owned(),
            ],
            Self::BrewCask => vec!["uninstall".to_owned(), "--cask".to_owned(), name.to_owned()],
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct GlobalTool {
    pub manager: GlobalToolManager,
    pub name: String,
    pub version: Option<String>,
    pub can_uninstall: bool,
    pub warning: Option<String>,
}

impl GlobalTool {
    fn new(manager: GlobalToolManager, name: impl Into<String>, version: Option<String>) -> Self {
        let name = name.into();
        let can_uninstall = !(manager == GlobalToolManager::Npm && name == "npm");
        let warning = (!can_uninstall).then(|| {
            "npm manages itself; use a Node version manager or npm upgrade instead".to_owned()
        });
        Self {
            manager,
            name,
            version,
            can_uninstall,
            warning,
        }
    }

    pub fn uninstall_command(&self) -> String {
        std::iter::once(self.manager.binary().to_owned())
            .chain(self.manager.uninstall_arguments(&self.name))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Serialize)]
pub struct GlobalToolScan {
    pub tools: Vec<GlobalTool>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GlobalToolResult {
    pub manager: GlobalToolManager,
    pub name: String,
    pub dry_run: bool,
    pub executed: bool,
    pub success: bool,
    pub message: String,
}

pub fn scan_global_tools() -> GlobalToolScan {
    let mut tools = Vec::new();
    let mut errors = Vec::new();

    collect_provider(
        GlobalToolManager::Cargo,
        scan_cargo,
        &mut tools,
        &mut errors,
    );
    collect_provider(GlobalToolManager::Dart, scan_dart, &mut tools, &mut errors);
    collect_provider(GlobalToolManager::Npm, scan_npm, &mut tools, &mut errors);
    collect_provider(GlobalToolManager::Pnpm, scan_pnpm, &mut tools, &mut errors);
    collect_provider(GlobalToolManager::Yarn, scan_yarn, &mut tools, &mut errors);
    collect_provider(GlobalToolManager::Uv, scan_uv, &mut tools, &mut errors);
    collect_provider(GlobalToolManager::Pipx, scan_pipx, &mut tools, &mut errors);
    collect_provider(
        GlobalToolManager::BrewFormula,
        scan_brew_formula,
        &mut tools,
        &mut errors,
    );
    collect_provider(
        GlobalToolManager::BrewCask,
        scan_brew_cask,
        &mut tools,
        &mut errors,
    );

    tools.sort_by(|left, right| {
        left.manager
            .cmp(&right.manager)
            .then_with(|| left.name.cmp(&right.name))
    });

    GlobalToolScan { tools, errors }
}

pub fn uninstall_global_tool(tool: &GlobalTool, dry_run: bool) -> Result<GlobalToolResult> {
    if !tool.can_uninstall {
        bail!(
            "{} cannot be uninstalled by cleanrs: {}",
            tool.name,
            tool.warning.as_deref().unwrap_or("protected global tool")
        );
    }

    let arguments = tool.manager.uninstall_arguments(&tool.name);
    let command_text = tool.uninstall_command();
    if dry_run {
        return Ok(GlobalToolResult {
            manager: tool.manager,
            name: tool.name.clone(),
            dry_run: true,
            executed: false,
            success: true,
            message: format!("dry-run: would run {command_text}"),
        });
    }

    let output = Command::new(tool.manager.binary())
        .args(&arguments)
        .output()
        .with_context(|| format!("failed to start {}", tool.manager.binary()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        bail!(
            "{command_text} exited with status {}{detail}",
            output.status
        );
    }

    Ok(GlobalToolResult {
        manager: tool.manager,
        name: tool.name.clone(),
        dry_run: false,
        executed: true,
        success: true,
        message: format!("executed: {command_text}"),
    })
}

fn collect_provider(
    manager: GlobalToolManager,
    scan: fn() -> Result<Vec<GlobalTool>>,
    tools: &mut Vec<GlobalTool>,
    errors: &mut Vec<String>,
) {
    if !command_available(manager.binary()) {
        return;
    }
    match scan() {
        Ok(mut found) => tools.append(&mut found),
        Err(error) => errors.push(format!("{}: {error:#}", manager.label())),
    }
}

fn command_output(program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("failed to start {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        bail!("{program} exited with status {}{detail}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn scan_dart() -> Result<Vec<GlobalTool>> {
    let output = command_output("dart", &["pub", "global", "list"])?;
    Ok(parse_dart_list(&output))
}

fn scan_cargo() -> Result<Vec<GlobalTool>> {
    let output = command_output("cargo", &["install", "--list"])?;
    Ok(parse_cargo_list(&output))
}

fn scan_npm() -> Result<Vec<GlobalTool>> {
    let output = command_output("npm", &["ls", "--global", "--depth=0", "--json"])?;
    let value: Value = serde_json::from_str(&output).context("invalid npm global JSON")?;
    Ok(parse_dependency_map(&value, GlobalToolManager::Npm))
}

fn scan_pnpm() -> Result<Vec<GlobalTool>> {
    let output = command_output("pnpm", &["list", "--global", "--depth=0", "--json"])?;
    let value: Value = serde_json::from_str(&output).context("invalid pnpm global JSON")?;
    Ok(parse_json_packages(&value, GlobalToolManager::Pnpm))
}

fn scan_yarn() -> Result<Vec<GlobalTool>> {
    let output = command_output("yarn", &["global", "list", "--json"])?;
    Ok(parse_yarn_list(&output))
}

fn scan_uv() -> Result<Vec<GlobalTool>> {
    let output = command_output("uv", &["tool", "list"])?;
    Ok(parse_uv_list(&output))
}

fn scan_pipx() -> Result<Vec<GlobalTool>> {
    let output = command_output("pipx", &["list", "--json"])?;
    let value: Value = serde_json::from_str(&output).context("invalid pipx JSON")?;
    Ok(parse_pipx_list(&value))
}

fn scan_brew_formula() -> Result<Vec<GlobalTool>> {
    let output = command_output("brew", &["list", "--formula", "--versions"])?;
    Ok(parse_brew_list(&output, GlobalToolManager::BrewFormula))
}

fn scan_brew_cask() -> Result<Vec<GlobalTool>> {
    let output = command_output("brew", &["list", "--cask", "--versions"])?;
    Ok(parse_brew_list(&output, GlobalToolManager::BrewCask))
}

fn parse_dart_list(output: &str) -> Vec<GlobalTool> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            if name == "No" || name == "active" {
                return None;
            }
            let version = parts.next().map(str::to_owned);
            Some(GlobalTool::new(GlobalToolManager::Dart, name, version))
        })
        .collect()
}

fn parse_cargo_list(output: &str) -> Vec<GlobalTool> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_suffix(':')?;
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version_token = parts.next()?;
            let version = version_token.strip_prefix('v').unwrap_or(version_token);
            if !version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(GlobalTool::new(
                GlobalToolManager::Cargo,
                name,
                Some(version.to_owned()),
            ))
        })
        .collect()
}

fn parse_dependency_map(value: &Value, manager: GlobalToolManager) -> Vec<GlobalTool> {
    value
        .get("dependencies")
        .and_then(Value::as_object)
        .map(|dependencies| {
            dependencies
                .iter()
                .map(|(name, metadata)| {
                    GlobalTool::new(
                        manager,
                        name,
                        metadata
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_json_packages(value: &Value, manager: GlobalToolManager) -> Vec<GlobalTool> {
    if let Some(packages) = value.as_array() {
        return packages
            .iter()
            .filter_map(|package| {
                Some(GlobalTool::new(
                    manager,
                    package.get("name")?.as_str()?,
                    package
                        .get("version")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ))
            })
            .collect();
    }
    parse_dependency_map(value, manager)
}

fn parse_yarn_list(output: &str) -> Vec<GlobalTool> {
    output
        .lines()
        .filter_map(|line| {
            let start = line.find('"')? + 1;
            let end = start + line[start..].find('"')?;
            let package = &line[start..end];
            let at = package.rfind('@')?;
            let name = &package[..at];
            let version = &package[at + 1..];
            if name.is_empty()
                || version.is_empty()
                || !version.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                return None;
            }
            Some(GlobalTool::new(
                GlobalToolManager::Yarn,
                name,
                Some(version.to_owned()),
            ))
        })
        .collect()
}

fn parse_uv_list(output: &str) -> Vec<GlobalTool> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.next()?.strip_prefix('v')?;
            if !version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(GlobalTool::new(
                GlobalToolManager::Uv,
                name,
                Some(version.to_owned()),
            ))
        })
        .collect()
}

fn parse_pipx_list(value: &Value) -> Vec<GlobalTool> {
    value
        .get("venvs")
        .and_then(Value::as_object)
        .map(|venvs| {
            venvs
                .iter()
                .map(|(name, metadata)| {
                    GlobalTool::new(
                        GlobalToolManager::Pipx,
                        name,
                        metadata
                            .get("metadata")
                            .and_then(|metadata| metadata.get("package_version"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_brew_list(output: &str, manager: GlobalToolManager) -> Vec<GlobalTool> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let versions = parts.collect::<Vec<_>>();
            Some(GlobalTool::new(
                manager,
                name,
                (!versions.is_empty()).then(|| versions.join(", ")),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        parse_brew_list, parse_cargo_list, parse_dart_list, parse_pipx_list, parse_uv_list,
        parse_yarn_list, GlobalTool, GlobalToolManager,
    };
    use serde_json::json;

    fn parse_npm_global(output: &str) -> Vec<super::GlobalTool> {
        let value: serde_json::Value = serde_json::from_str(output).expect("valid JSON");
        super::parse_dependency_map(&value, GlobalToolManager::Npm)
    }

    #[test]
    fn parses_dart_global_packages() {
        let tools = parse_dart_list("melos 6.3.0\nvery_good_cli 0.24.0\n");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "melos");
        assert_eq!(tools[0].version.as_deref(), Some("6.3.0"));
    }

    #[test]
    fn parses_cargo_installed_packages() {
        let tools = parse_cargo_list("cargo-edit v0.13.0:\n    cargo-add\n");
        assert_eq!(tools[0].name, "cargo-edit");
        assert_eq!(tools[0].version.as_deref(), Some("0.13.0"));
    }

    #[test]
    fn parses_npm_global_json_and_protects_npm_itself() {
        let tools = parse_npm_global(
            r#"{"dependencies":{"npm":{"version":"11.5.1"},"typescript":{"version":"5.9.2"}}}"#,
        );
        assert_eq!(tools.len(), 2);
        let npm = tools.iter().find(|tool| tool.name == "npm").expect("npm");
        assert!(!npm.can_uninstall);
        assert!(tools
            .iter()
            .find(|tool| tool.name == "typescript")
            .is_some_and(|tool| tool.can_uninstall));
    }

    #[test]
    fn parses_uv_and_yarn_versions() {
        let uv = parse_uv_list("ruff v0.9.2\n- ruff\n");
        assert_eq!(uv[0].version.as_deref(), Some("0.9.2"));

        let yarn = parse_yarn_list("info \"@scope/tool@1.2.3\" has binaries:\n");
        assert_eq!(yarn[0].name, "@scope/tool");
        assert_eq!(yarn[0].version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn parses_pipx_and_brew_inventory() {
        let pipx = parse_pipx_list(&json!({
            "venvs": {
                "ruff": {"metadata": {"package_version": "0.9.2"}}
            }
        }));
        assert_eq!(pipx[0].name, "ruff");
        assert_eq!(pipx[0].version.as_deref(), Some("0.9.2"));

        let brew = parse_brew_list(
            "node 24.5.0\nvisual-studio-code 1.99.0\n",
            GlobalToolManager::BrewFormula,
        );
        assert_eq!(brew[0].version.as_deref(), Some("24.5.0"));
        assert_eq!(brew[1].manager, GlobalToolManager::BrewFormula);
    }

    #[test]
    fn builds_manager_owned_uninstall_commands() {
        let args = GlobalToolManager::Dart.uninstall_arguments("melos");
        assert_eq!(args, ["pub", "global", "deactivate", "melos"]);
        let args = GlobalToolManager::BrewCask.uninstall_arguments("cursor");
        assert_eq!(args, ["uninstall", "--cask", "cursor"]);
        let tool = GlobalTool::new(GlobalToolManager::Uv, "ruff", Some("0.9.2".to_owned()));
        assert_eq!(tool.uninstall_command(), "uv tool uninstall ruff");
    }
}
