use super::{command_available, command_output};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::Cleaner;
use anyhow::Result;
use std::path::PathBuf;

pub struct DockerCleaner;

impl Cleaner for DockerCleaner {
    fn id(&self) -> &'static str {
        "docker"
    }

    fn display_name(&self) -> &'static str {
        "Docker reclaimable data"
    }

    fn category(&self) -> Category {
        Category::Container
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        command_available("docker")
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let output = command_output(
            "docker",
            &["system", "df", "--format", "{{.Type}}\t{{.Reclaimable}}"],
        )?;
        if output.trim().is_empty() {
            return Ok(Vec::new());
        }

        let reclaimable_bytes = output
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .filter_map(|(_, value)| parse_size(value))
            .sum();

        Ok(vec![CleanTarget {
            path: PathBuf::from("docker://system"),
            size_bytes: reclaimable_bytes,
            description: "images, containers, networks and volumes".to_owned(),
            method: CleanMethod::RunCommand(vec![
                "docker".to_owned(),
                "system".to_owned(),
                "prune".to_owned(),
                "-af".to_owned(),
                "--volumes".to_owned(),
            ]),
        }])
    }
}

fn parse_size(value: &str) -> Option<u64> {
    let token = value.split_whitespace().next()?;
    let unit_start = token.find(|character: char| character.is_ascii_alphabetic())?;
    let number = token[..unit_start].parse::<f64>().ok()?;
    let multiplier = match token[unit_start..].to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1_000.0,
        "mb" | "mib" => 1_000_000.0,
        "gb" | "gib" => 1_000_000_000.0,
        "tb" | "tib" => 1_000_000_000_000.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn parses_docker_reclaimable_sizes() {
        assert_eq!(parse_size("1.5GB (20%)"), Some(1_500_000_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("unknown"), None);
    }
}
