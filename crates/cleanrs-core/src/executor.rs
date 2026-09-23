use crate::{
    can_delete_path,
    history::record_history,
    model::{CleanMethod, CleanOptions, CleanResult, CleanTarget},
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

pub fn clean_target(cleaner_id: &str, target: &CleanTarget, dry_run: bool) -> Result<CleanResult> {
    clean_target_with_options(
        cleaner_id,
        target,
        CleanOptions {
            dry_run,
            permanent: false,
        },
    )
}

#[tracing::instrument(
    skip(target),
    fields(
        cleaner_id,
        target = %target.path.display(),
        dry_run = options.dry_run,
        permanent = options.permanent
    )
)]
pub fn clean_target_with_options(
    cleaner_id: &str,
    target: &CleanTarget,
    options: CleanOptions,
) -> Result<CleanResult> {
    clean_target_with_progress(cleaner_id, target, options, &mut |_| {})
}

#[tracing::instrument(
    skip(target, progress),
    fields(
        cleaner_id,
        target = %target.path.display(),
        dry_run = options.dry_run,
        permanent = options.permanent
    )
)]
pub fn clean_target_with_progress(
    cleaner_id: &str,
    target: &CleanTarget,
    options: CleanOptions,
    progress: &mut dyn FnMut(String),
) -> Result<CleanResult> {
    if !can_delete_path(&target.path) {
        let error = anyhow!(
            "refusing to clean protected or unsafe path: {}",
            target.path.display()
        );
        if !options.dry_run {
            record_history(
                "clean",
                cleaner_id,
                &target.path.display().to_string(),
                "rejected",
                &error.to_string(),
            );
        }
        return Err(error);
    }

    let result: Result<CleanResult> = if options.dry_run {
        Ok(CleanResult {
            cleaner_id: cleaner_id.to_owned(),
            target: target.path.display().to_string(),
            dry_run: true,
            executed: false,
            success: true,
            expected_freed_bytes: target.size_bytes,
            message: if options.permanent {
                "dry-run: would permanently delete".to_owned()
            } else {
                "dry-run: would move to Trash".to_owned()
            },
        })
    } else {
        let message = match &target.method {
            CleanMethod::RunCommand(arguments) => execute_command(arguments, None, progress)?,
            CleanMethod::RunCommandWithInput { arguments, stdin } => {
                execute_command(arguments, Some(stdin.as_bytes()), progress)?
            }
            CleanMethod::TrashPath if options.permanent => {
                delete_permanently(&target.path)?;
                format!("permanently deleted {}", target.path.display())
            }
            CleanMethod::TrashPath => {
                move_to_trash(&target.path).with_context(|| {
                    format!(
                        "failed to move {} to Trash; check file ownership, parent-directory write permission, or macOS Full Disk Access",
                        target.path.display()
                    )
                })?;
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
    };

    match &result {
        Ok(clean_result) if !clean_result.dry_run => record_history(
            "clean",
            cleaner_id,
            &clean_result.target,
            "success",
            &clean_result.message,
        ),
        Err(error) if !options.dry_run => record_history(
            "clean",
            cleaner_id,
            &target.path.display().to_string(),
            "failed",
            &error.to_string(),
        ),
        _ => {}
    }
    result
}

pub(crate) fn move_to_trash(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        let mut context = trash::TrashContext::new();
        context.set_delete_method(DeleteMethod::NsFileManager);
        context.delete(path)?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path)?;
        Ok(())
    }
}

fn delete_permanently(path: &std::path::Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .with_context(|| format!("permanently delete {}", path.display()))
}

fn execute_command(
    arguments: &[String],
    stdin: Option<&[u8]>,
    progress: &mut dyn FnMut(String),
) -> Result<String> {
    let (program, args) = arguments
        .split_first()
        .context("clean command cannot be empty")?;

    let (status, output_lines) = if let Some(input) = stdin {
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
        stream_command_output(child, arguments, progress)?
    } else {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start clean command {program}"))?;
        stream_command_output(child, arguments, progress)?
    };

    if !status.success() {
        let detail = output_lines
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "clean command {} exited with status {}{}",
            arguments.join(" "),
            status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        );
    }

    Ok(format!("executed: {}", arguments.join(" ")))
}

fn stream_command_output(
    mut child: std::process::Child,
    arguments: &[String],
    progress: &mut dyn FnMut(String),
) -> Result<(std::process::ExitStatus, Vec<String>)> {
    let stdout = child
        .stdout
        .take()
        .context("clean command stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("clean command stderr was not captured")?;
    let (sender, receiver) = mpsc::channel::<String>();

    spawn_output_reader(stdout, sender.clone());
    spawn_output_reader(stderr, sender.clone());
    drop(sender);

    progress(format!("running {}", arguments.join(" ")));
    let mut output_lines = Vec::new();
    while let Ok(line) = receiver.recv() {
        if !line.trim().is_empty() {
            progress(line.clone());
            output_lines.push(line);
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for clean command {}", arguments[0]))?;
    Ok((status, output_lines))
}

fn spawn_output_reader<R>(stream: R, sender: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut stream = stream;
        let mut buffer = [0_u8; 1024];
        let mut pending = Vec::new();
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            for byte in &buffer[..read] {
                if matches!(byte, b'\n' | b'\r') {
                    if !pending.is_empty() {
                        let line = String::from_utf8_lossy(&pending).into_owned();
                        let _ = sender.send(line);
                        pending.clear();
                    }
                } else {
                    pending.push(*byte);
                }
            }
        }
        if !pending.is_empty() {
            let _ = sender.send(String::from_utf8_lossy(&pending).into_owned());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{clean_target, clean_target_with_progress};
    use crate::model::{CleanMethod, CleanTarget};
    use std::path::PathBuf;

    #[test]
    fn run_command_with_input_supplies_confirmation_without_inheriting_tui_stdin() {
        let arguments = if cfg!(target_os = "windows") {
            vec![
                "cmd".to_owned(),
                "/C".to_owned(),
                "set /p answer= && if \"%answer%\"==\"y\" exit /b 0".to_owned(),
            ]
        } else {
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "read answer && test \"$answer\" = y".to_owned(),
            ]
        };
        let target = CleanTarget {
            path: PathBuf::from("test://interactive-command"),
            size_bytes: 1,
            description: "test command".to_owned(),
            method: CleanMethod::RunCommandWithInput {
                arguments,
                stdin: "y\n".to_owned(),
            },
        };

        let result = clean_target("test", &target, false).expect("command should succeed");

        assert!(result.executed);
        assert!(result.success);
    }

    #[test]
    fn streams_command_output_to_progress_callback() {
        let arguments = if cfg!(target_os = "windows") {
            vec![
                "cmd".to_owned(),
                "/C".to_owned(),
                "echo step-one && echo step-two".to_owned(),
            ]
        } else {
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "printf 'step-one\\n'; printf 'step-two\\r' >&2".to_owned(),
            ]
        };
        let target = CleanTarget {
            path: PathBuf::from("test://streaming-command"),
            size_bytes: 1,
            description: "test command".to_owned(),
            method: CleanMethod::RunCommand(arguments),
        };
        let mut updates = Vec::new();

        let result = clean_target_with_progress(
            "test",
            &target,
            crate::model::CleanOptions::default(),
            &mut |update| updates.push(update),
        )
        .expect("command should succeed");

        assert!(result.executed);
        assert!(updates.iter().any(|update| update == "step-one"));
        assert!(updates.iter().any(|update| update == "step-two"));
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn refuses_protected_system_paths() {
        let protected_path = if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\Windows\System32")
        } else {
            PathBuf::from("/System/Library")
        };
        let target = CleanTarget {
            path: protected_path,
            size_bytes: 1,
            description: "protected path".to_owned(),
            method: CleanMethod::TrashPath,
        };

        let error = clean_target("test", &target, false).expect_err("path must be rejected");

        assert!(error.to_string().contains("protected or unsafe path"));
    }

    #[test]
    fn permanent_delete_requires_an_explicit_option_but_is_supported() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("generated");
        std::fs::write(&path, b"temporary").expect("fixture");
        let target = CleanTarget {
            path: path.clone(),
            size_bytes: 9,
            description: "generated fixture".to_owned(),
            method: CleanMethod::TrashPath,
        };

        let preview = super::clean_target_with_options(
            "test",
            &target,
            crate::model::CleanOptions {
                dry_run: true,
                permanent: true,
            },
        )
        .expect("preview should succeed");
        assert!(preview.dry_run);
        assert!(path.exists());

        super::clean_target_with_options(
            "test",
            &target,
            crate::model::CleanOptions {
                dry_run: false,
                permanent: true,
            },
        )
        .expect("permanent delete should succeed");
        assert!(!path.exists());
    }
}
