use crate::model::{CleanMethod, CleanResult, CleanTarget};
use anyhow::{bail, Context, Result};
use std::process::Command;

pub fn clean_target(cleaner_id: &str, target: &CleanTarget, dry_run: bool) -> Result<CleanResult> {
    if dry_run {
        return Ok(CleanResult {
            cleaner_id: cleaner_id.to_owned(),
            target: target.path.display().to_string(),
            dry_run: true,
            executed: false,
            success: true,
            expected_freed_bytes: target.size_bytes,
            message: "dry-run: no command executed".to_owned(),
        });
    }

    let message = match &target.method {
        CleanMethod::RunCommand(arguments) => {
            let (program, args) = arguments
                .split_first()
                .context("clean command cannot be empty")?;
            let status = Command::new(program)
                .args(args)
                .status()
                .with_context(|| format!("failed to start clean command {program}"))?;

            if !status.success() {
                bail!(
                    "clean command {} exited with status {}",
                    arguments.join(" "),
                    status
                );
            }

            format!("executed: {}", arguments.join(" "))
        }
        CleanMethod::TrashPath => {
            trash::delete(&target.path)
                .with_context(|| format!("failed to move {} to Trash", target.path.display()))?;
            format!("moved {} to Trash", target.path.display())
        }
    };

    Ok(CleanResult {
        cleaner_id: cleaner_id.to_owned(),
        target: target.path.display().to_string(),
        dry_run: false,
        executed: true,
        success: true,
        expected_freed_bytes: target.size_bytes,
        message,
    })
}
