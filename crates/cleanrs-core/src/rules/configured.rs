use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

pub struct ConfiguredCleaner;

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    rules: Vec<ConfigRule>,
}

#[derive(Debug, Deserialize)]
struct ConfigRule {
    path: String,
    description: Option<String>,
    risk: Option<ConfigRisk>,
    /// Commands are argv arrays, never shell strings. They are always treated
    /// as Manual unless the user explicitly supplies another risk level.
    command: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ConfigRisk {
    Safe,
    Caution,
    Manual,
}

#[derive(Debug)]
struct ResolvedRule {
    path: PathBuf,
    description: String,
    method: CleanMethod,
    risk: RiskLevel,
}

impl Cleaner for ConfiguredCleaner {
    fn id(&self) -> &'static str {
        "configured"
    }

    fn display_name(&self) -> &'static str {
        "User-configured rules"
    }

    fn category(&self) -> Category {
        Category::ManualReview
    }

    fn risk_level(&self) -> RiskLevel {
        load_rules()
            .map(|rules| {
                rules
                    .iter()
                    .map(|rule| rule.risk)
                    .max()
                    .unwrap_or(RiskLevel::Manual)
            })
            .unwrap_or(RiskLevel::Manual)
    }

    fn is_available(&self) -> bool {
        config_path().is_some_and(|path| path.is_file())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        for rule in load_rules()? {
            let size_bytes = dir_size(&rule.path)?;
            if size_bytes == 0 {
                continue;
            }
            targets.push(CleanTarget {
                path: rule.path,
                size_bytes,
                description: rule.description,
                method: rule.method,
            });
        }
        Ok(targets)
    }
}

fn config_path() -> Option<PathBuf> {
    home_path(".config/cleanrs/rules.toml")
}

fn load_rules() -> Result<Vec<ResolvedRule>> {
    let Some(path) = config_path() else {
        return Ok(Vec::new());
    };
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("read configured rules from {}", path.display()))?;
    parse_rules(&contents)
}

fn parse_rules(contents: &str) -> Result<Vec<ResolvedRule>> {
    let config: ConfigFile = toml::from_str(contents).context("parse configured rules")?;
    config
        .rules
        .into_iter()
        .map(ResolvedRule::try_from)
        .collect()
}

impl TryFrom<ConfigRule> for ResolvedRule {
    type Error = anyhow::Error;

    fn try_from(rule: ConfigRule) -> Result<Self> {
        let path = expand_path(&rule.path)?;
        let description = rule
            .description
            .unwrap_or_else(|| format!("configured rule: {}", path.display()));
        let (method, default_risk) = match rule.command {
            Some(arguments) if arguments.is_empty() => {
                bail!("configured command for {} cannot be empty", path.display())
            }
            Some(arguments) => (CleanMethod::RunCommand(arguments), RiskLevel::Manual),
            None => (CleanMethod::TrashPath, RiskLevel::Caution),
        };

        Ok(Self {
            path,
            description,
            method,
            risk: rule.risk.map(Into::into).unwrap_or(default_risk),
        })
    }
}

fn expand_path(value: &str) -> Result<PathBuf> {
    if value == "~" || value.starts_with("~/") {
        let home = home_path("").context("HOME is not set for configured rule")?;
        return Ok(if value == "~" {
            home
        } else {
            home.join(value.trim_start_matches("~/"))
        });
    }

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        bail!("configured rule path must be absolute or start with ~/: {value}");
    }
    Ok(path)
}

impl From<ConfigRisk> for RiskLevel {
    fn from(risk: ConfigRisk) -> Self {
        match risk {
            ConfigRisk::Safe => Self::Safe,
            ConfigRisk::Caution => Self::Caution,
            ConfigRisk::Manual => Self::Manual,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_rules, RiskLevel};
    use crate::model::CleanMethod;

    #[test]
    fn parses_path_and_direct_command_rules() {
        let rules = parse_rules(
            r#"
                [[rules]]
                path = "/tmp/cache"
                description = "Temporary cache"
                risk = "caution"

                [[rules]]
                path = "/tmp/tool-data"
                command = ["tool", "cache", "clean"]
            "#,
        )
        .expect("rules should parse");

        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].risk, RiskLevel::Caution);
        assert!(matches!(rules[0].method, CleanMethod::TrashPath));
        assert_eq!(rules[1].risk, RiskLevel::Manual);
        assert!(matches!(rules[1].method, CleanMethod::RunCommand(_)));
    }

    #[test]
    fn rejects_relative_paths_and_empty_commands() {
        assert!(parse_rules("[[rules]]\npath = \"cache\"").is_err());
        assert!(parse_rules("[[rules]]\npath = \"/tmp/cache\"\ncommand = []").is_err());
    }
}
