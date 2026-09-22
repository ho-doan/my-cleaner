use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    PackageManager,
    SystemCache,
    Container,
    Ide,
    AiAgent,
    ManualReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Caution,
    Manual,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "value")]
pub enum CleanMethod {
    /// Run an executable directly. Arguments are kept separate so no shell is
    /// involved and rule definitions cannot accidentally expand shell syntax.
    RunCommand(Vec<String>),
    /// Run an executable directly while providing explicit stdin. This is for
    /// trusted commands whose own confirmation prompt is part of the rule.
    RunCommandWithInput {
        arguments: Vec<String>,
        stdin: String,
    },
    /// Move a path to the operating system trash.
    TrashPath,
}

#[derive(Clone, Debug, Serialize)]
pub struct CleanTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub description: String,
    pub method: CleanMethod,
}

#[derive(Clone, Debug, Serialize)]
pub struct CleanResult {
    pub cleaner_id: String,
    pub target: String,
    pub dry_run: bool,
    pub executed: bool,
    pub success: bool,
    pub expected_freed_bytes: u64,
    pub message: String,
}
