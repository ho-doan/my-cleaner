use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CleanOptions {
    pub dry_run: bool,
    pub permanent: bool,
}

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
    Destructive,
}

impl RiskLevel {
    /// Short, user-facing wording for people who should not need to know the
    /// internal risk enum names.
    pub fn user_label(self) -> &'static str {
        match self {
            Self::Safe => "SAFE · can be recreated",
            Self::Caution => "REVIEW · may affect app data",
            Self::Manual => "ASK FIRST · personal/dev data",
            Self::Destructive => "IRREVERSIBLE",
        }
    }

    /// Explain why an item is or is not selected automatically.
    pub fn user_explanation(self) -> &'static str {
        match self {
            Self::Safe => "Generated cache/build data; the owning tool can recreate it.",
            Self::Caution => {
                "Review the app or data before removing it; it is not selected automatically."
            }
            Self::Manual => "May contain personal data, backups, models, archives, or live state.",
            Self::Destructive => "This permanently changes or empties user data.",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::RiskLevel;

    #[test]
    fn risk_labels_are_plain_language() {
        assert_eq!(RiskLevel::Safe.user_label(), "SAFE · can be recreated");
        assert_eq!(
            RiskLevel::Caution.user_label(),
            "REVIEW · may affect app data"
        );
        assert_eq!(
            RiskLevel::Manual.user_label(),
            "ASK FIRST · personal/dev data"
        );
        assert_eq!(RiskLevel::Destructive.user_label(), "IRREVERSIBLE");
    }
}
