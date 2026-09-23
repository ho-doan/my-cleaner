//! Side effects, worker channels, and the terminal event loop.

use anyhow::Result;
use cleanrs_core::{
    all_cleaners, check_latest_release, default_scan_root, full_disk_scan, scan_all_reports,
    scan_cleaner, scan_directory, scan_global_tools, toggle_delete_allowlist,
    uninstall_global_tool, CleanMethod, CleanTarget, CleanerScan, DirectoryScan, FullDiskScan,
    GlobalTool, GlobalToolScan, ReadOnlyScan, UpdateInfo,
};
use crossbeam_channel::{unbounded, Receiver, TryRecvError};
use crossterm::event::{self, Event, KeyEventKind};
use humansize::{format_size, DECIMAL};
use ratatui::{backend::CrosstermBackend, Terminal};
use rayon::prelude::*;
use std::{
    io::Stdout,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use super::{
    model::{read_disk_usage, App, CleanMessage, Mode, UpdateState},
    update::{handle_key, KeyAction},
    view::render,
    RunOutcome,
};

fn start_scan() -> (usize, Receiver<CleanerScan>, Receiver<ReadOnlyScan>) {
    let cleaners = all_cleaners();
    let expected_scans = cleaners.len();
    let (sender, receiver) = unbounded();
    thread::spawn(move || {
        cleaners
            .into_par_iter()
            .for_each_with(sender, |sender, cleaner| {
                let _ = sender.send(scan_cleaner(cleaner.as_ref()));
            });
    });
    let (readonly_sender, readonly_receiver) = unbounded();
    rayon::spawn(move || {
        for report in scan_all_reports() {
            let _ = readonly_sender.send(report);
        }
    });
    (expected_scans, receiver, readonly_receiver)
}

fn start_update_check(current_version: String) -> Receiver<Result<Option<UpdateInfo>, String>> {
    let (sender, receiver) = unbounded();
    rayon::spawn(move || {
        let result = check_latest_release(&current_version).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

fn start_full_disk_scan() -> Receiver<Result<FullDiskScan, String>> {
    let (sender, receiver) = unbounded();
    rayon::spawn(move || {
        let root = default_scan_root();
        let result = full_disk_scan(&root, 12).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

fn start_global_scan() -> Receiver<Result<GlobalToolScan, String>> {
    let (sender, receiver) = unbounded();
    rayon::spawn(move || {
        let report = scan_global_tools();
        let _ = sender.send(Ok(report));
    });
    receiver
}

fn start_directory_scan(path: PathBuf) -> Receiver<Result<DirectoryScan, String>> {
    let (sender, receiver) = unbounded();
    rayon::spawn(move || {
        let result = scan_directory(&path, 24).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

fn queue_directory_scan(
    app: &mut App,
    receiver: &mut Option<Receiver<Result<DirectoryScan, String>>>,
    path: PathBuf,
) {
    app.directory_scanning = true;
    app.directory_error = None;
    app.directory_scan = None;
    app.directory_cursor = 0;
    *receiver = Some(start_directory_scan(path));
}

fn selected_explorer_target(app: &App) -> Option<CleanTarget> {
    let scan = app.directory_scan.as_ref()?;
    let entry = scan.entries.get(app.directory_cursor)?;
    if entry.read_only {
        return None;
    }
    if !entry
        .suggestion
        .as_ref()
        .is_some_and(|suggestion| suggestion.can_delete)
    {
        return None;
    }

    let description = entry
        .suggestion
        .as_ref()
        .map(|suggestion| suggestion.reason.clone())
        .unwrap_or_else(|| {
            if entry.is_dir {
                "Approved suggested folder selected from the directory explorer".to_owned()
            } else {
                "File selected from the directory explorer".to_owned()
            }
        });

    Some(CleanTarget {
        path: entry.path.clone(),
        size_bytes: entry.size_bytes,
        description,
        method: CleanMethod::TrashPath,
    })
}

fn explorer_delete_rejection(app: &App) -> &'static str {
    if app.directory_scan.is_none() {
        return "Inventory only · Enter opens folder/file parent";
    }
    let Some(scan) = app.directory_scan.as_ref() else {
        return "No explorer item selected";
    };
    let Some(entry) = scan.entries.get(app.directory_cursor) else {
        return "No explorer item selected";
    };
    if entry.read_only {
        "Read-only item · x disabled"
    } else if entry.is_dir
        && !entry
            .suggestion
            .as_ref()
            .is_some_and(|suggestion| suggestion.can_delete)
    {
        "Folder not approved · x disabled"
    } else {
        "Review-only item · x disabled"
    }
}

fn selected_explorer_path(app: &App) -> Option<PathBuf> {
    let scan = app.directory_scan.as_ref()?;
    scan.entries
        .get(app.directory_cursor)
        .map(|entry| entry.path.clone())
}

fn selected_global_tool(app: &App) -> Option<GlobalTool> {
    app.global_tools().get(app.global_cursor).cloned()
}

pub(crate) fn start_explorer_delete(app: &mut App) {
    let Some(target) = app.pending_explorer_delete.take() else {
        app.mode = Mode::Reviewing;
        return;
    };
    let (sender, receiver) = unbounded();
    app.mode = Mode::Cleaning;
    app.explorer_deleting = true;
    app.cleaning_completed = 0;
    app.cleaning_total = 1;
    app.cleaning_disk_before = app.disk.or_else(read_disk_usage);
    app.cleaning_started_at = Some(Instant::now());
    app.active_cleaner = Some("explorer".to_owned());
    app.active_target = Some(target.path.display().to_string());
    app.active_target_size = target.size_bytes;
    app.active_detail = Some("moving selected item to Trash".to_owned());
    app.explorer_delete_receiver = Some(receiver);

    rayon::spawn(move || {
        let result = cleanrs_core::executor::clean_target("explorer", &target, false)
            .map_err(|error| format!("{}: {error:#}", target.path.display()));
        let _ = sender.send(result);
    });
}

pub(crate) fn start_global_uninstall(app: &mut App) {
    let Some(tool) = app.pending_global_uninstall.take() else {
        app.mode = Mode::Reviewing;
        return;
    };
    let (sender, receiver) = unbounded();
    app.mode = Mode::Cleaning;
    app.global_uninstalling = true;
    app.cleaning_completed = 0;
    app.cleaning_total = 1;
    app.cleaning_disk_before = app.disk.or_else(read_disk_usage);
    app.cleaning_started_at = Some(Instant::now());
    app.active_cleaner = Some(tool.manager.label().to_owned());
    app.active_target = Some(tool.name.clone());
    app.active_target_size = 0;
    app.active_detail = Some("waiting for package manager".to_owned());
    app.global_uninstall_receiver = Some(receiver);

    rayon::spawn(move || {
        let result = uninstall_global_tool(&tool, false).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
}

pub(crate) fn start_cleaning(app: &mut App) {
    let mut jobs = app
        .rows
        .iter()
        .filter(|row| row.selected)
        .map(|row| (row.cleaner_id.clone(), row.target.clone()))
        .collect::<Vec<_>>();
    if app.trash_selected {
        if let Some(target) = app.trash_target.clone() {
            jobs.push(("trash".to_owned(), target));
        }
    }
    let total = jobs.len();
    let dry_run = app.dry_run;
    let disk_before = app.disk.or_else(read_disk_usage);
    let (sender, receiver) = unbounded();

    app.mode = Mode::Cleaning;
    app.results.clear();
    app.errors.clear();
    app.cleaning_completed = 0;
    app.cleaning_total = total;
    app.cleaning_disk_before = disk_before;
    app.cleaning_started_at = Some(Instant::now());
    app.active_cleaner = None;
    app.active_target = None;
    app.active_target_size = 0;
    app.active_detail = None;
    app.clean_receiver = Some(receiver);

    rayon::spawn(move || {
        let cleaners = all_cleaners();
        for (cleaner_id, target) in jobs {
            if sender
                .send(CleanMessage::Started {
                    cleaner_id: cleaner_id.clone(),
                    target: target.clone(),
                })
                .is_err()
            {
                return;
            }
            let outcome = match cleaners.iter().find(|cleaner| cleaner.id() == cleaner_id) {
                Some(cleaner) => {
                    let progress_sender = sender.clone();
                    let mut progress = move |detail: String| {
                        let _ = progress_sender.send(CleanMessage::Progress(detail));
                    };
                    cleaner
                        .clean_with_progress(&target, dry_run, &mut progress)
                        .map_err(|error| format!("{cleaner_id}: {error:#}"))
                }
                None => Err(format!("unknown cleaner {cleaner_id}")),
            };
            if sender.send(CleanMessage::Target(outcome)).is_err() {
                return;
            }
        }
        let _ = sender.send(CleanMessage::Finished);
    });
}

pub(crate) fn run_loop(stdout: &mut Stdout, current_version: &str) -> Result<RunOutcome> {
    let (expected_scans, mut receiver, mut readonly_receiver) = start_scan();
    let mut update_receiver = Some(start_update_check(current_version.to_owned()));
    let mut full_disk_receiver: Option<Receiver<Result<FullDiskScan, String>>> = None;
    let mut global_scan_receiver: Option<Receiver<Result<GlobalToolScan, String>>> = None;
    let mut directory_receiver: Option<Receiver<Result<DirectoryScan, String>>> = None;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(expected_scans, current_version);
    let result = loop {
        receive_scans(&mut app, &receiver);
        receive_readonly_scan(&mut app, &readonly_receiver);
        receive_update_check(&mut app, &mut update_receiver);
        receive_full_disk_scan(&mut app, &mut full_disk_receiver);
        receive_global_scan(&mut app, &mut global_scan_receiver);
        receive_directory_scan(&mut app, &mut directory_receiver);
        receive_explorer_delete(&mut app, &mut directory_receiver);
        receive_global_uninstall(&mut app, &mut global_scan_receiver);
        if receive_cleaning(&mut app) {
            let (expected, next_receiver, next_readonly_receiver) = start_scan();
            app.reset_for_scan(expected);
            receiver = next_receiver;
            readonly_receiver = next_readonly_receiver;
        }
        terminal.draw(|frame| render(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match handle_key(&mut app, key) {
                        KeyAction::BackDirectory => {
                            if app.directory_scanning {
                                continue;
                            }
                            if let Some(previous) = app.directory_history.pop() {
                                if previous == default_scan_root() {
                                    app.directory_scan = None;
                                    app.directory_error = None;
                                    app.directory_cursor = 0;
                                } else {
                                    queue_directory_scan(
                                        &mut app,
                                        &mut directory_receiver,
                                        previous,
                                    );
                                }
                            } else {
                                app.show_full_disk = false;
                                app.directory_scan = None;
                                app.directory_error = None;
                                app.directory_cursor = 0;
                            }
                        }
                        KeyAction::BackGlobalTools => {
                            app.show_global_tools = false;
                            app.global_cursor = 0;
                        }
                        KeyAction::Quit => break Ok(RunOutcome::Exit),
                        KeyAction::Upgrade => {
                            if let Some(update) = app.available_update() {
                                break Ok(RunOutcome::Upgrade(update));
                            }
                        }
                        KeyAction::DeleteFile => {
                            if let Some(target) = selected_explorer_target(&app) {
                                app.pending_explorer_delete = Some(target);
                                app.confirm_text.clear();
                                app.mode = Mode::Confirming;
                            } else {
                                app.last_action = Some(explorer_delete_rejection(&app).to_owned());
                            }
                        }
                        KeyAction::ToggleAllowlist => {
                            let Some(path) = selected_explorer_path(&app) else {
                                app.last_action = Some(
                                    "Select a directory before changing the delete allowlist"
                                        .to_owned(),
                                );
                                continue;
                            };
                            match toggle_delete_allowlist(&path) {
                                Ok(true) => {
                                    app.last_action = Some(format!(
                                        "Added {} to the delete allowlist; no files were changed",
                                        path.display()
                                    ));
                                }
                                Ok(false) => {
                                    app.last_action = Some(format!(
                                        "Removed {} from the delete allowlist",
                                        path.display()
                                    ));
                                }
                                Err(error) => {
                                    app.last_action =
                                        Some(format!("Delete allowlist unchanged: {error:#}"));
                                }
                            }
                            if let Some(current) =
                                app.directory_scan.as_ref().map(|scan| scan.path.clone())
                            {
                                queue_directory_scan(&mut app, &mut directory_receiver, current);
                            }
                        }
                        KeyAction::FullDisk => {
                            if !app.full_disk_scanning {
                                app.full_disk_scanning = true;
                                app.full_disk_error = None;
                                app.show_full_disk = true;
                                app.show_global_tools = false;
                                app.full_disk_cursor = 0;
                                app.directory_scan = None;
                                app.directory_scanning = false;
                                app.directory_error = None;
                                app.directory_history.clear();
                                app.directory_cursor = 0;
                                directory_receiver = None;
                                full_disk_receiver = Some(start_full_disk_scan());
                            }
                        }
                        KeyAction::GlobalTools => {
                            app.show_global_tools = true;
                            app.show_full_disk = false;
                            app.directory_scan = None;
                            app.directory_scanning = false;
                            app.directory_error = None;
                            if !app.global_tools_scanning {
                                app.global_tools_scanning = true;
                                app.global_tools_error = None;
                                app.global_cursor = 0;
                                global_scan_receiver = Some(start_global_scan());
                            }
                        }
                        KeyAction::UninstallGlobal => {
                            if let Some(tool) = selected_global_tool(&app) {
                                if tool.can_uninstall {
                                    app.pending_global_uninstall = Some(tool);
                                    app.confirm_text.clear();
                                    app.mode = Mode::Confirming;
                                } else {
                                    app.last_action = Some(tool.warning.unwrap_or_else(|| {
                                        "This global tool is protected from uninstall".to_owned()
                                    }));
                                }
                            }
                        }
                        KeyAction::OpenDirectory => {
                            if app.directory_scanning {
                                continue;
                            }
                            let selected_path = if let Some(scan) = &app.directory_scan {
                                scan.entries
                                    .get(app.directory_cursor)
                                    .filter(|entry| entry.is_dir)
                                    .map(|entry| entry.path.clone())
                            } else {
                                app.full_disk_entries()
                                    .get(app.full_disk_cursor)
                                    .map(|entry| {
                                        if entry.path.is_dir() {
                                            entry.path.clone()
                                        } else {
                                            entry
                                                .path
                                                .parent()
                                                .map(Path::to_path_buf)
                                                .unwrap_or_else(default_scan_root)
                                        }
                                    })
                            };
                            if let Some(path) = selected_path {
                                let current = app
                                    .directory_scan
                                    .as_ref()
                                    .map(|scan| scan.path.clone())
                                    .unwrap_or_else(default_scan_root);
                                app.directory_history.push(current);
                                queue_directory_scan(&mut app, &mut directory_receiver, path);
                            }
                        }
                        KeyAction::Rescan => {
                            let (expected, next_receiver, next_readonly_receiver) = start_scan();
                            app.reset_for_scan(expected);
                            receiver = next_receiver;
                            readonly_receiver = next_readonly_receiver;
                        }
                        KeyAction::Continue => {}
                    }
                }
            }
        }
    };
    terminal.show_cursor()?;
    result
}

fn receive_scans(app: &mut App, receiver: &Receiver<CleanerScan>) {
    loop {
        match receiver.try_recv() {
            Ok(report) => app.add_scan(report),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                if matches!(app.mode, Mode::Scanning) {
                    app.disk = read_disk_usage().or(app.disk);
                    app.mode = Mode::Reviewing;
                }
                break;
            }
        }
    }
    if app.received_scans == app.expected_scans && matches!(app.mode, Mode::Scanning) {
        app.disk = read_disk_usage().or(app.disk);
        app.mode = Mode::Reviewing;
    }
}

fn receive_readonly_scan(app: &mut App, receiver: &Receiver<ReadOnlyScan>) {
    while let Ok(report) = receiver.try_recv() {
        app.readonly_report = Some(report);
    }
}

fn receive_update_check(
    app: &mut App,
    receiver: &mut Option<Receiver<Result<Option<UpdateInfo>, String>>>,
) {
    let message = receiver.as_ref().map(|channel| channel.try_recv());
    match message {
        Some(Ok(Ok(Some(update)))) => {
            app.update_state = UpdateState::Available(update);
            *receiver = None;
        }
        Some(Ok(Ok(None))) => {
            app.update_state = UpdateState::UpToDate;
            *receiver = None;
        }
        Some(Ok(Err(error))) => {
            app.update_state = UpdateState::Unavailable(error);
            *receiver = None;
        }
        Some(Err(TryRecvError::Disconnected)) => {
            app.update_state =
                UpdateState::Unavailable("update check stopped unexpectedly".to_owned());
            *receiver = None;
        }
        Some(Err(TryRecvError::Empty)) | None => {}
    }
}

fn receive_full_disk_scan(
    app: &mut App,
    receiver: &mut Option<Receiver<Result<FullDiskScan, String>>>,
) {
    let message = receiver.as_ref().map(|channel| channel.try_recv());
    match message {
        Some(Ok(Ok(report))) => {
            app.full_disk = Some(report);
            app.full_disk_scanning = false;
            app.full_disk_error = None;
            *receiver = None;
        }
        Some(Ok(Err(error))) => {
            app.full_disk_scanning = false;
            app.full_disk_error = Some(error);
            *receiver = None;
        }
        Some(Err(TryRecvError::Disconnected)) => {
            app.full_disk_scanning = false;
            app.full_disk_error = Some("full-disk scan stopped unexpectedly".to_owned());
            *receiver = None;
        }
        Some(Err(TryRecvError::Empty)) | None => {}
    }
}

fn receive_global_scan(
    app: &mut App,
    receiver: &mut Option<Receiver<Result<GlobalToolScan, String>>>,
) {
    let message = receiver.as_ref().map(|channel| channel.try_recv());
    match message {
        Some(Ok(Ok(report))) => {
            app.global_tools = Some(report);
            app.global_tools_scanning = false;
            app.global_tools_error = None;
            app.global_cursor = 0;
            *receiver = None;
        }
        Some(Ok(Err(error))) => {
            app.global_tools_scanning = false;
            app.global_tools_error = Some(error);
            *receiver = None;
        }
        Some(Err(TryRecvError::Disconnected)) => {
            app.global_tools_scanning = false;
            app.global_tools_error = Some("global tools scan stopped unexpectedly".to_owned());
            *receiver = None;
        }
        Some(Err(TryRecvError::Empty)) | None => {}
    }
}

fn receive_directory_scan(
    app: &mut App,
    receiver: &mut Option<Receiver<Result<DirectoryScan, String>>>,
) {
    let message = receiver.as_ref().map(|channel| channel.try_recv());
    match message {
        Some(Ok(Ok(report))) => {
            app.directory_scan = Some(report);
            app.directory_scanning = false;
            app.directory_error = None;
            app.directory_cursor = 0;
            *receiver = None;
        }
        Some(Ok(Err(error))) => {
            app.directory_scanning = false;
            app.directory_error = Some(error);
            *receiver = None;
        }
        Some(Err(TryRecvError::Disconnected)) => {
            app.directory_scanning = false;
            app.directory_error = Some("directory scan stopped unexpectedly".to_owned());
            *receiver = None;
        }
        Some(Err(TryRecvError::Empty)) | None => {}
    }
}

fn receive_explorer_delete(
    app: &mut App,
    directory_receiver: &mut Option<Receiver<Result<DirectoryScan, String>>>,
) {
    let Some(receiver) = app.explorer_delete_receiver.take() else {
        return;
    };

    match receiver.try_recv() {
        Ok(outcome) => {
            app.explorer_deleting = false;
            app.cleaning_completed = 1;
            app.cleaning_started_at = None;
            app.active_cleaner = None;
            app.active_target = None;
            app.active_target_size = 0;
            app.active_detail = None;
            let disk_after = read_disk_usage().or(app.cleaning_disk_before);
            app.disk = disk_after;
            match outcome {
                Ok(result) => {
                    app.results.push(result);
                    app.last_action = Some("Item moved to Trash; folder is rescanning".to_owned());
                }
                Err(error) => {
                    app.errors.push(error.clone());
                    app.last_action = Some(format!("Item was not moved to Trash: {error}"));
                }
            }
            app.cleaning_disk_before = None;
            app.mode = Mode::Reviewing;
            if let Some(path) = app.directory_scan.as_ref().map(|scan| scan.path.clone()) {
                queue_directory_scan(app, directory_receiver, path);
            }
        }
        Err(TryRecvError::Empty) => {
            app.explorer_delete_receiver = Some(receiver);
        }
        Err(TryRecvError::Disconnected) => {
            app.explorer_deleting = false;
            app.cleaning_started_at = None;
            app.active_cleaner = None;
            app.active_target = None;
            app.active_target_size = 0;
            app.active_detail = None;
            app.mode = Mode::Reviewing;
            app.last_action = Some("File delete worker stopped unexpectedly".to_owned());
        }
    }
}

fn receive_global_uninstall(
    app: &mut App,
    global_scan_receiver: &mut Option<Receiver<Result<GlobalToolScan, String>>>,
) {
    let Some(receiver) = app.global_uninstall_receiver.take() else {
        return;
    };

    match receiver.try_recv() {
        Ok(outcome) => {
            app.global_uninstalling = false;
            app.cleaning_completed = 1;
            app.cleaning_started_at = None;
            app.active_cleaner = None;
            app.active_target = None;
            app.active_target_size = 0;
            app.active_detail = None;
            app.disk = read_disk_usage().or(app.disk);
            app.cleaning_disk_before = None;
            match outcome {
                Ok(result) => {
                    app.last_action = Some(format!(
                        "Uninstalled {} {} · rescanning global tools",
                        result.manager.label(),
                        result.name
                    ));
                    app.global_tools_scanning = true;
                    app.global_tools_error = None;
                    app.global_cursor = 0;
                    *global_scan_receiver = Some(start_global_scan());
                }
                Err(error) => {
                    app.last_action = Some(format!("Global uninstall failed: {error}"));
                }
            }
            app.mode = Mode::Reviewing;
        }
        Err(TryRecvError::Empty) => {
            app.global_uninstall_receiver = Some(receiver);
        }
        Err(TryRecvError::Disconnected) => {
            app.global_uninstalling = false;
            app.cleaning_started_at = None;
            app.active_cleaner = None;
            app.active_target = None;
            app.active_target_size = 0;
            app.active_detail = None;
            app.cleaning_disk_before = None;
            app.mode = Mode::Reviewing;
            app.last_action = Some("Global uninstall worker stopped unexpectedly".to_owned());
        }
    }
}

fn receive_cleaning(app: &mut App) -> bool {
    let Some(receiver) = app.clean_receiver.take() else {
        return false;
    };

    let mut finished = false;
    loop {
        match receiver.try_recv() {
            Ok(CleanMessage::Started { cleaner_id, target }) => {
                app.cleaning_started_at = Some(Instant::now());
                app.active_cleaner = Some(cleaner_id);
                app.active_target = Some(target.path.display().to_string());
                app.active_target_size = target.size_bytes;
                app.active_detail = Some("starting cleanup command".to_owned());
            }
            Ok(CleanMessage::Progress(detail)) => {
                app.active_detail = Some(detail);
            }
            Ok(CleanMessage::Target(outcome)) => {
                app.cleaning_completed += 1;
                match outcome {
                    Ok(result) => app.results.push(result),
                    Err(error) => app.errors.push(error),
                }
            }
            Ok(CleanMessage::Finished) => {
                finished = true;
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                if app.cleaning_completed < app.cleaning_total {
                    app.errors
                        .push("cleanup worker stopped unexpectedly".to_owned());
                }
                finished = true;
                break;
            }
        }
    }

    if !finished {
        app.clean_receiver = Some(receiver);
        return false;
    }

    let dry_run = app.dry_run;
    let disk_after = read_disk_usage().or(app.cleaning_disk_before);
    let actual_free_change = match (app.cleaning_disk_before, disk_after) {
        (Some(before), Some(after)) => after.free_bytes.saturating_sub(before.free_bytes),
        _ => 0,
    };
    let estimated_freed = app
        .results
        .iter()
        .map(|result| result.expected_freed_bytes)
        .sum::<u64>();
    app.disk = disk_after;
    app.last_action = Some(format!(
        "{} {}/{} target(s), estimated {} freed, actual free change {}; {} error(s); {}",
        if dry_run { "Previewed" } else { "Cleaned" },
        app.cleaning_completed,
        app.cleaning_total,
        format_size(estimated_freed, DECIMAL),
        format_size(actual_free_change, DECIMAL),
        app.errors.len(),
        if dry_run {
            "no files changed; targets kept"
        } else {
            "scan refreshed"
        }
    ));
    app.cleaning_disk_before = None;
    app.cleaning_started_at = None;
    app.active_cleaner = None;
    app.active_target = None;
    app.active_target_size = 0;
    app.active_detail = None;

    if dry_run {
        app.mode = Mode::Reviewing;
        false
    } else {
        true
    }
}
