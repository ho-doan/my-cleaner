use super::home_path;
use crate::model::{Category, CleanMethod, CleanTarget, RiskLevel};
use crate::scanner::dir_size;
use crate::Cleaner;
use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const CURSOR_CACHE_PREFIX: &str = "com.todesktop.230313mzl4w4u92.";

#[derive(Clone, Copy)]
struct AppSpec {
    id: &'static str,
    display_name: &'static str,
    app_paths: &'static [&'static str],
    data_paths: &'static [(&'static str, &'static str)],
}

const SPECS: &[AppSpec] = &[
    AppSpec {
        id: "docker-desktop",
        display_name: "Docker Desktop",
        app_paths: &["/Applications/Docker.app", "~/Applications/Docker.app"],
        data_paths: &[
            (
                "Library/Containers/com.docker.docker",
                "Docker Desktop container data (includes Docker.raw)",
            ),
            (
                "Library/Group Containers/group.com.docker",
                "Docker Desktop group container data",
            ),
            (
                "Library/Application Support/Docker Desktop",
                "Docker Desktop application data",
            ),
            ("Library/Caches/com.docker.docker", "Docker Desktop cache"),
            ("Library/Logs/Docker Desktop", "Docker Desktop logs"),
            (
                "Library/Preferences/com.docker.docker.plist",
                "Docker Desktop preferences",
            ),
            (
                "Library/Saved Application State/com.electron.docker-frontend.savedState",
                "Docker Desktop saved state",
            ),
        ],
    },
    AppSpec {
        id: "visual-studio-code",
        display_name: "Visual Studio Code",
        app_paths: &[
            "/Applications/Visual Studio Code.app",
            "~/Applications/Visual Studio Code.app",
        ],
        data_paths: &[
            (
                "Library/Application Support/Code",
                "Visual Studio Code application data",
            ),
            (
                "Library/Caches/com.microsoft.VSCode",
                "Visual Studio Code cache",
            ),
            ("Library/Logs/Code", "Visual Studio Code logs"),
            (
                "Library/Preferences/com.microsoft.VSCode.plist",
                "Visual Studio Code preferences",
            ),
            (
                "Library/Saved Application State/com.microsoft.VSCode.savedState",
                "Visual Studio Code saved state",
            ),
        ],
    },
    AppSpec {
        id: "cursor",
        display_name: "Cursor",
        app_paths: &["/Applications/Cursor.app", "~/Applications/Cursor.app"],
        data_paths: &[
            (
                "Library/Application Support/Cursor",
                "Cursor application data",
            ),
            (
                "Library/Caches/com.todesktop.230313mzl4w4u92",
                "Cursor cache",
            ),
            ("Library/Logs/Cursor", "Cursor logs"),
            (
                "Library/Preferences/com.todesktop.230313mzl4w4u92.plist",
                "Cursor preferences",
            ),
            (
                "Library/Saved Application State/com.todesktop.230313mzl4w4u92.savedState",
                "Cursor saved state",
            ),
        ],
    },
    AppSpec {
        id: "postman",
        display_name: "Postman",
        app_paths: &["/Applications/Postman.app", "~/Applications/Postman.app"],
        data_paths: &[
            (
                "Library/Application Support/Postman",
                "Postman application data",
            ),
            ("Library/Caches/com.postmanlabs.mac", "Postman cache"),
            ("Library/Logs/Postman", "Postman logs"),
            (
                "Library/Preferences/com.postmanlabs.mac.plist",
                "Postman preferences",
            ),
        ],
    },
    AppSpec {
        id: "slack",
        display_name: "Slack",
        app_paths: &["/Applications/Slack.app", "~/Applications/Slack.app"],
        data_paths: &[
            (
                "Library/Application Support/Slack",
                "Slack application data",
            ),
            ("Library/Caches/com.tinyspeck.slackmacgap", "Slack cache"),
            ("Library/Logs/Slack", "Slack logs"),
            (
                "Library/Preferences/com.tinyspeck.slackmacgap.plist",
                "Slack preferences",
            ),
        ],
    },
    AppSpec {
        id: "discord",
        display_name: "Discord",
        app_paths: &["/Applications/Discord.app", "~/Applications/Discord.app"],
        data_paths: &[
            (
                "Library/Application Support/discord",
                "Discord application data",
            ),
            ("Library/Caches/com.hnc.Discord", "Discord cache"),
            ("Library/Logs/Discord", "Discord logs"),
            (
                "Library/Preferences/com.hnc.Discord.plist",
                "Discord preferences",
            ),
        ],
    },
];

pub struct AppLeftoverCleaner;

impl Cleaner for AppLeftoverCleaner {
    fn id(&self) -> &'static str {
        "app-leftovers"
    }

    fn display_name(&self) -> &'static str {
        "Uninstalled app leftovers"
    }

    fn category(&self) -> Category {
        Category::ManualReview
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Manual
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos") && SPECS.iter().any(spec_has_leftovers)
    }

    fn scan(&self) -> Result<Vec<CleanTarget>> {
        if !cfg!(target_os = "macos") {
            return Ok(Vec::new());
        }

        let mut targets = Vec::new();
        for spec in SPECS {
            if app_is_installed(spec) {
                continue;
            }
            for (path, description) in app_data_paths(spec) {
                append_target(&mut targets, spec, path, &description)?;
            }
        }
        targets.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(targets)
    }
}

fn append_target(
    targets: &mut Vec<CleanTarget>,
    spec: &AppSpec,
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
        description: format!(
            "{} leftover: {} (app not installed)",
            spec.display_name, description
        ),
        method: CleanMethod::TrashPath,
    });
    Ok(())
}

fn spec_has_leftovers(spec: &AppSpec) -> bool {
    !app_is_installed(spec) && app_data_paths(spec).iter().any(|(path, _)| path.exists())
}

fn app_is_installed(spec: &AppSpec) -> bool {
    spec.app_paths
        .iter()
        .any(|path| app_path(path).map(|path| path.is_dir()).unwrap_or(false))
}

fn app_path(path: &str) -> Option<PathBuf> {
    path.strip_prefix("~/")
        .and_then(home_path)
        .or_else(|| Some(PathBuf::from(path)))
}

fn app_data_paths(spec: &AppSpec) -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for (relative, description) in spec.data_paths {
        let Some(path) = home_path(relative) else {
            continue;
        };
        if seen.insert(path.clone()) {
            paths.push((path, (*description).to_owned()));
        }
    }

    // Electron-based apps such as Cursor leave version-independent updater
    // directories beside their main cache. Match only this known bundle-id
    // prefix instead of scanning arbitrary names from Library/Caches.
    if spec.id == "cursor" {
        if let Some(cache_root) = home_path("Library/Caches") {
            if let Ok(entries) = std::fs::read_dir(cache_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(name) = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                    else {
                        continue;
                    };
                    if name.starts_with(CURSOR_CACHE_PREFIX) && seen.insert(path.clone()) {
                        paths.push((path, format!("Cursor updater cache ({name})")));
                    }
                }
            }
        }
    }

    paths
}

/// Used by the generic macOS cache scanner to avoid duplicate rows when a
/// known app has already been uninstalled and its data is owned by this rule.
pub fn is_orphaned_path(path: &Path) -> bool {
    SPECS.iter().any(|spec| {
        !app_is_installed(spec)
            && app_data_paths(spec)
                .iter()
                .any(|(candidate, _)| path == candidate || path.starts_with(candidate))
    })
}

/// Docker Desktop keeps a large VM image under this rule when its app bundle
/// is gone; the dedicated Docker rule should only own that image while the
/// app is still installed.
pub fn is_app_installed(id: &str) -> bool {
    SPECS
        .iter()
        .find(|spec| spec.id == id)
        .is_some_and(app_is_installed)
}

#[cfg(test)]
mod tests {
    use super::SPECS;
    use std::collections::HashSet;

    #[test]
    fn registry_contains_high_confidence_app_families() {
        for id in [
            "docker-desktop",
            "visual-studio-code",
            "cursor",
            "postman",
            "slack",
            "discord",
        ] {
            assert!(SPECS.iter().any(|spec| spec.id == id), "missing {id}");
        }
    }

    #[test]
    fn registry_has_no_duplicate_data_paths() {
        let mut paths = HashSet::new();
        for spec in SPECS {
            for (path, _) in spec.data_paths {
                assert!(paths.insert(*path), "duplicate leftover path: {path}");
            }
        }
    }
}
