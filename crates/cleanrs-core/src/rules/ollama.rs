use super::{command_available, command_output, home_path};
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

pub struct OllamaCleaner;

impl Cleaner for OllamaCleaner {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn display_name(&self) -> &'static str {
        "Ollama cache and models"
    }

    fn category(&self) -> Category {
        Category::AiAgent
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        command_available("ollama")
            || home_path(".ollama").is_some_and(|path| path.exists())
            || home_path("Library/Caches/ollama").is_some_and(|path| path.exists())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let mut targets = Vec::new();
        for path in cache_paths() {
            append_path_target(&mut targets, path, "Ollama update/cache files")?;
        }

        let running = running_models();
        if let Ok(output) = command_output("ollama", &["ls"]) {
            for (name, size_bytes) in parse_model_rows(&output) {
                if running.contains(&name) {
                    continue;
                }
                targets.push(CleanTarget {
                    path: PathBuf::from(format!("ollama://model/{name}")),
                    size_bytes,
                    description: format!("Ollama model {name}"),
                    method: CleanMethod::RunCommand(vec![
                        "ollama".to_owned(),
                        "rm".to_owned(),
                        name,
                    ]),
                });
            }
        }
        Ok(targets)
    }
}

fn cache_paths() -> Vec<PathBuf> {
    [".ollama/cache", ".ollama/logs", "Library/Caches/ollama"]
        .into_iter()
        .filter_map(home_path)
        .collect()
}

fn running_models() -> HashSet<String> {
    command_output("ollama", &["ps"])
        .map(|output| {
            output
                .lines()
                .skip(1)
                .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn append_path_target(
    targets: &mut Vec<CleanTarget>,
    path: PathBuf,
    description: &str,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let size_bytes = dir_size(&path)?;
    if size_bytes == 0 {
        return Ok(());
    }
    targets.push(CleanTarget {
        path,
        size_bytes,
        description: description.to_owned(),
        method: CleanMethod::TrashPath,
    });
    Ok(())
}

fn parse_model_rows(output: &str) -> Vec<(String, u64)> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 {
                return None;
            }
            let name = fields[0].to_owned();
            let size = parse_size(fields[2], fields.get(3).copied())?;
            Some((name, size))
        })
        .collect()
}

fn parse_size(value: &str, unit: Option<&str>) -> Option<u64> {
    let split_at = value
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let number = value[..split_at].parse::<f64>().ok()?;
    let unit = if split_at < value.len() {
        &value[split_at..]
    } else {
        unit?
    };
    let multiplier = match unit.to_ascii_lowercase().as_str() {
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
    use super::parse_model_rows;

    #[test]
    fn parses_ollama_list_rows() {
        let output = "NAME ID SIZE MODIFIED\nqwen3:8b abc 4.7 GB 2 days ago\n";
        assert_eq!(
            parse_model_rows(output),
            vec![("qwen3:8b".to_owned(), 4_700_000_000)]
        );
    }
}
