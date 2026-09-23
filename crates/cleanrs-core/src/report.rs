use crate::platform::config;
use crate::scanner::dir_size;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

/// Read-only inventory contract. ReportOnly intentionally has no clean method
/// so system-owned paths cannot become delete targets through the registry.
pub trait ReportOnly: Send + Sync {
    fn id(&self) -> &'static str;
    fn scan(&self) -> Result<Vec<ReadOnlyTarget>>;
    fn advice(&self) -> &'static str;
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadOnlyTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct ReadOnlyScan {
    pub report_id: String,
    pub advice: String,
    pub targets: Vec<ReadOnlyTarget>,
    pub error: Option<String>,
}

pub struct ProtectedSystemReport;

impl ReportOnly for ProtectedSystemReport {
    fn id(&self) -> &'static str {
        "protected-system"
    }

    fn scan(&self) -> Result<Vec<ReadOnlyTarget>> {
        let mut targets = Vec::new();
        for (path, description) in config::read_only_report_paths() {
            if let Some(target) = read_target(path, description)? {
                targets.push(target);
            }
        }
        targets.sort_by_key(|target| std::cmp::Reverse(target.size_bytes));
        Ok(targets)
    }

    fn advice(&self) -> &'static str {
        config::read_only_advice()
    }
}

fn read_target(path: PathBuf, description: &str) -> Result<Option<ReadOnlyTarget>> {
    if !path.exists() {
        return Ok(None);
    }
    let size_bytes = dir_size(&path)?;
    Ok((size_bytes > 0).then_some(ReadOnlyTarget {
        path,
        size_bytes,
        description: description.to_owned(),
    }))
}

pub fn all_reporters() -> Vec<Box<dyn ReportOnly>> {
    vec![Box::new(ProtectedSystemReport)]
}

pub fn scan_all_reports() -> Vec<ReadOnlyScan> {
    all_reporters()
        .into_iter()
        .map(|reporter| {
            let report_id = reporter.id().to_owned();
            let advice = reporter.advice().to_owned();
            match reporter.scan() {
                Ok(targets) => ReadOnlyScan {
                    report_id,
                    advice,
                    targets,
                    error: None,
                },
                Err(error) => ReadOnlyScan {
                    report_id,
                    advice,
                    targets: Vec::new(),
                    error: Some(format!("{error:#}")),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::all_reporters;

    #[test]
    fn protected_report_is_report_only() {
        let reports = all_reporters();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].id(), "protected-system");
        assert!(reports[0].advice().contains("never delete"));
    }
}
