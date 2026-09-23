use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;

pub struct DockerDesktopCleaner;

impl Cleaner for DockerDesktopCleaner {
    fn id(&self) -> &'static str {
        "docker-desktop"
    }

    fn display_name(&self) -> &'static str {
        "Docker Desktop VM disk image"
    }

    fn category(&self) -> Category {
        Category::Container
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        docker_raw_path().is_some_and(|path| path.is_file())
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        let Some(path) = docker_raw_path() else {
            return Ok(Vec::new());
        };
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let size_bytes = dir_size(&path)?;
        if size_bytes == 0 {
            return Ok(Vec::new());
        }
        Ok(vec![CleanTarget {
            path,
            size_bytes,
            description: "Docker Desktop VM image; deleting it removes Docker data".to_owned(),
            method: CleanMethod::TrashPath,
        }])
    }
}

fn docker_raw_path() -> Option<std::path::PathBuf> {
    home_path("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw")
}
