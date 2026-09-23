use crate::executor::{clean_target, clean_target_with_options, clean_target_with_progress};
use crate::model::{Category, CleanOptions, CleanResult, CleanTarget, RiskLevel};
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

    fn clean_with_progress(
        &self,
        target: &CleanTarget,
        dry_run: bool,
        progress: &mut dyn FnMut(String),
    ) -> Result<CleanResult> {
        clean_target_with_progress(
            self.id(),
            target,
            CleanOptions {
                dry_run,
                permanent: false,
            },
            progress,
        )
    }

    #[tracing::instrument(
        skip(self, target),
        fields(
            cleaner_id = self.id(),
            target = %target.path.display(),
            dry_run = options.dry_run,
            permanent = options.permanent
        )
    )]
    fn clean_with_options(
        &self,
        target: &CleanTarget,
        options: CleanOptions,
    ) -> Result<CleanResult> {
        clean_target_with_options(self.id(), target, options)
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
    let mut cleaners: Vec<Box<dyn Cleaner>> = vec![
        Box::new(rules::agent_cache::AgentCacheCleaner),
        Box::new(rules::configured::ConfiguredCleaner),
        Box::new(rules::cargo::CargoCleaner),
        Box::new(rules::codex::CodexCleaner),
        Box::new(rules::claude::ClaudeCleaner),
        Box::new(rules::dart::DartCleaner),
        Box::new(rules::npm::NpmCleaner),
        Box::new(rules::pip::PipCleaner),
        Box::new(rules::pnpm::PnpmCleaner),
        Box::new(rules::uv::UvCleaner),
        Box::new(rules::yarn::YarnCleaner),
        Box::new(rules::gradle::GradleCleaner),
        Box::new(rules::maven::MavenCleaner),
        Box::new(rules::kiro::KiroCleaner),
        Box::new(rules::ollama::OllamaCleaner),
        Box::new(rules::docker::DockerCleaner),
    ];

    #[cfg(target_os = "macos")]
    {
        cleaners.extend([
            Box::new(rules::macos_system::MacosSystemCleaner) as Box<dyn Cleaner>,
            Box::new(rules::brew::BrewCleaner),
            Box::new(rules::docker_desktop::DockerDesktopCleaner),
            Box::new(rules::app_leftovers::AppLeftoverCleaner),
            Box::new(rules::personal::PersonalDataCleaner),
            Box::new(rules::simulator::SimulatorUnavailableCleaner),
            Box::new(rules::xcode::XcodeCleaner),
            Box::new(rules::xcode::XcodeArchivesCleaner),
            Box::new(rules::xcode::SimulatorCacheCleaner),
            Box::new(rules::trash::TrashCleaner),
        ]);
    }

    #[cfg(target_os = "windows")]
    cleaners.push(Box::new(rules::windows_system::WindowsTempCleaner));

    cleaners
}

#[tracing::instrument(skip(cleaner), fields(cleaner_id = cleaner.id()))]
pub fn scan_cleaner(cleaner: &dyn Cleaner) -> CleanerScan {
    let available = cleaner.is_available();
    let (targets, error) = if !available {
        (Vec::new(), None)
    } else {
        match cleaner.scan() {
            Ok(targets) => (
                targets
                    .into_iter()
                    .filter(|target| target.size_bytes > 0)
                    .collect(),
                None,
            ),
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

#[cfg(test)]
mod tests {
    use super::{scan_cleaner, Cleaner};
    use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
    use anyhow::Result;
    use std::path::PathBuf;

    struct EmptyTargetCleaner;

    impl Cleaner for EmptyTargetCleaner {
        fn id(&self) -> &'static str {
            "empty-target-test"
        }

        fn display_name(&self) -> &'static str {
            "Empty target test"
        }

        fn category(&self) -> Category {
            Category::ManualReview
        }

        fn risk_level(&self) -> RiskLevel {
            RiskLevel::Caution
        }

        fn is_available(&self) -> bool {
            true
        }

        fn scan(&self) -> Result<Vec<CleanTarget>> {
            Ok(vec![CleanTarget {
                path: PathBuf::from("cache://empty"),
                size_bytes: 0,
                description: "already empty".to_owned(),
                method: CleanMethod::TrashPath,
            }])
        }
    }

    #[test]
    fn scan_cleaner_drops_zero_byte_targets() {
        let report = scan_cleaner(&EmptyTargetCleaner);

        assert!(report.error.is_none());
        assert!(report.targets.is_empty());
    }
}
