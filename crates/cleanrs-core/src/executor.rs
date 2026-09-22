use crate::{
    can_delete_path,
    model::{CleanMethod, CleanResult, CleanTarget},
};
use anyhow::{bail, Context, Result};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub fn clean_target(cleaner_id: &str, target: &CleanTarget, dry_run: bool) -> Result<CleanResult> {
    if !can_delete_path(&target.path) {
        bail!(
            "refusing to clean protected or unsafe path: {}",
            target.path.display()
        );
    }

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
        CleanMethod::RunCommand(arguments) => execute_command(arguments, None)?,
        CleanMethod::RunCommandWithInput { arguments, stdin } => {
            execute_command(arguments, Some(stdin.as_bytes()))?
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

fn execute_command(arguments: &[String], stdin: Option<&[u8]>) -> Result<String> {
    let (program, args) = arguments
        .split_first()
        .context("clean command cannot be empty")?;

    let output = if let Some(input) = stdin {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start clean command {program}"))?;

        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin
                .write_all(input)
                .with_context(|| format!("failed to provide input to clean command {program}"))?;
        }
        child
            .wait_with_output()
            .with_context(|| format!("failed waiting for clean command {program}"))?
    } else {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to start clean command {program}"))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        bail!(
            "clean command {} exited with status {}{}",
            arguments.join(" "),
            output.status,
            detail
        );
    }

    Ok(format!("executed: {}", arguments.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::clean_target;
    use crate::model::{CleanMethod, CleanTarget};
    use std::path::PathBuf;

    #[test]
    fn run_command_with_input_supplies_confirmation_without_inheriting_tui_stdin() {
        let target = CleanTarget {
            path: PathBuf::from("test://interactive-command"),
            size_bytes: 1,
            description: "test command".to_owned(),
            method: CleanMethod::RunCommandWithInput {
                arguments: vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "read answer && test \"$answer\" = y".to_owned(),
                ],
                stdin: "y\n".to_owned(),
            },
        };

        let result = clean_target("test", &target, false).expect("command should succeed");

        assert!(result.executed);
        assert!(result.success);
    }

    #[test]
    fn refuses_protected_system_paths() {
        let target = CleanTarget {
            path: PathBuf::from("/System/Library"),
            size_bytes: 1,
            description: "protected path".to_owned(),
            method: CleanMethod::TrashPath,
        };

        let error = clean_target("test", &target, false).expect_err("path must be rejected");

        assert!(error.to_string().contains("protected or unsafe path"));
    }
}
