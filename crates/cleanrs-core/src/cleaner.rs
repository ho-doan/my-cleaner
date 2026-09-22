use crate::executor::clean_target;
use crate::model::{Category, CleanResult, CleanTarget, RiskLevel};
use crate::rules;
use anyhow::Result;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashSet;

pub trait Cleaner: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn category(&self) -> Category;
    fn risk_level(&self) -> RiskLevel;
    fn is_available(&self) -> bool;
    fn scan(&self) -> Result<Vec<CleanTarget>>;

    fn clean(&self, target: &CleanTarget, dry_run: bool) -> Result<CleanResult> {
        clean_target(self.id(), target, dry_run)
    }
}

#[derive(Debug, Serialize)]
pub struct CleanerScan {
    pub cleaner_id: String,
    pub display_name: String,
    pub category: Category,
    pub risk_level: RiskLevel,
    pub available: bool,
    pub targets: Vec<CleanTarget>,
    pub error: Option<String>,
}

pub fn all_cleaners() -> Vec<Box<dyn Cleaner>> {
    vec![
        Box::new(rules::agent_cache::AgentCacheCleaner),
        Box::new(rules::cargo::CargoCleaner),
        Box::new(rules::codex::CodexCleaner),
        Box::new(rules::claude::ClaudeCleaner),
        Box::new(rules::dart::DartCleaner),
        Box::new(rules::npm::NpmCleaner),
        Box::new(rules::brew::BrewCleaner),
        Box::new(rules::pip::PipCleaner),
        Box::new(rules::pnpm::PnpmCleaner),
        Box::new(rules::uv::UvCleaner),
        Box::new(rules::yarn::YarnCleaner),
        Box::new(rules::gradle::GradleCleaner),
        Box::new(rules::maven::MavenCleaner),
        Box::new(rules::macos_system::MacosSystemCleaner),
        Box::new(rules::kiro::KiroCleaner),
        Box::new(rules::ollama::OllamaCleaner),
        Box::new(rules::xcode::XcodeCleaner),
        Box::new(rules::docker::DockerCleaner),
    ]
}

pub fn scan_cleaner(cleaner: &dyn Cleaner) -> CleanerScan {
    let available = cleaner.is_available();
    let (targets, error) = if !available {
        (Vec::new(), None)
    } else {
        match cleaner.scan() {
            Ok(targets) => (targets, None),
            Err(error) => (Vec::new(), Some(format!("{error:#}"))),
        }
    };

    CleanerScan {
        cleaner_id: cleaner.id().to_owned(),
        display_name: cleaner.display_name().to_owned(),
        category: cleaner.category(),
        risk_level: cleaner.risk_level(),
        available,
        targets,
        error,
    }
}

pub fn scan_all(only: Option<&[String]>) -> Vec<CleanerScan> {
    let only = only.map(|ids| {
        ids.iter()
            .map(|id| id.to_ascii_lowercase())
            .collect::<HashSet<_>>()
    });

    let mut scans = all_cleaners()
        .into_par_iter()
        .filter(|cleaner| {
            only.as_ref()
                .map(|ids| ids.contains(cleaner.id()))
                .unwrap_or(true)
        })
        .map(|cleaner| scan_cleaner(cleaner.as_ref()))
        .collect::<Vec<_>>();
    scans.sort_by(|left, right| left.cleaner_id.cmp(&right.cleaner_id));
    scans
}
