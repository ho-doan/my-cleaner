use anyhow::Result;
use cleanrs_core::{
    all_cleaners, check_latest_release, full_disk_scan, scan_all_reports, scan_cleaner,
    scan_directory, scan_global_tools, toggle_delete_allowlist, uninstall_global_tool, Category,
    CleanMethod, CleanResult, CleanTarget, CleanerScan, DirectoryScan, DirectoryScanEntry,
    FullDiskScan, GlobalTool, GlobalToolResult, GlobalToolScan, ReadOnlyScan, RiskLevel,
    UpdateInfo,
};
use crossbeam_channel::{unbounded, Receiver, TryRecvError};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fs2::{available_space, total_space};
use humansize::{format_size, DECIMAL};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use rayon::prelude::*;
use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub enum RunOutcome {
    Exit,
    Upgrade(UpdateInfo),
}

pub fn run(current_version: &str) -> Result<RunOutcome> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let result = run_loop(&mut stdout, current_version);

    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;
    result
}

struct TargetRow {
    cleaner_id: String,
    cleaner_name: String,
    category: Category,
    risk: RiskLevel,
    target: CleanTarget,
    selected: bool,
}

enum Mode {
    Scanning,
    Reviewing,
    Confirming,
    Cleaning,
}

enum UpdateState {
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Unavailable(String),
}

#[derive(Clone, Copy)]
struct DiskUsage {
    total_bytes: u64,
    free_bytes: u64,
}

impl DiskUsage {
    fn used_bytes(self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    fn used_ratio(self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes() as f64 / self.total_bytes as f64
        }
    }
}

struct App {
    mode: Mode,
    current_version: String,
    update_state: UpdateState,
    rows: Vec<TargetRow>,
    trash_target: Option<CleanTarget>,
    trash_selected: bool,
    readonly_report: Option<ReadOnlyScan>,
    info_overlay: Option<String>,
    pending_empty_trash: bool,
    cursor: usize,
    dry_run: bool,
    confirm_text: String,
    received_scans: usize,
    expected_scans: usize,
    results: Vec<CleanResult>,
    errors: Vec<String>,
    disk: Option<DiskUsage>,
    full_disk: Option<FullDiskScan>,
    full_disk_scanning: bool,
    full_disk_error: Option<String>,
    show_full_disk: bool,
    full_disk_cursor: usize,
    show_global_tools: bool,
    global_tools: Option<GlobalToolScan>,
    global_tools_scanning: bool,
    global_tools_error: Option<String>,
    global_cursor: usize,
    pending_global_uninstall: Option<GlobalTool>,
    global_uninstall_receiver: Option<Receiver<Result<GlobalToolResult, String>>>,
    global_uninstalling: bool,
    directory_scan: Option<DirectoryScan>,
    directory_scanning: bool,
    directory_error: Option<String>,
    directory_history: Vec<std::path::PathBuf>,
    directory_cursor: usize,
    pending_explorer_delete: Option<CleanTarget>,
    explorer_delete_receiver: Option<Receiver<Result<CleanResult, String>>>,
    explorer_deleting: bool,
    clean_receiver: Option<Receiver<CleanMessage>>,
    cleaning_completed: usize,
    cleaning_total: usize,
    cleaning_disk_before: Option<DiskUsage>,
    cleaning_started_at: Option<Instant>,
    active_cleaner: Option<String>,
    active_target: Option<String>,
    active_target_size: u64,
    active_detail: Option<String>,
    last_action: Option<String>,
}

impl App {
    fn new(expected_scans: usize, current_version: &str) -> Self {
        Self {
            mode: Mode::Scanning,
            current_version: current_version.to_owned(),
            update_state: UpdateState::Checking,
            rows: Vec::new(),
            trash_target: None,
            trash_selected: false,
            readonly_report: None,
            info_overlay: None,
            pending_empty_trash: false,
            cursor: 0,
            dry_run: true,
            confirm_text: String::new(),
            received_scans: 0,
            expected_scans,
            results: Vec::new(),
            errors: Vec::new(),
            disk: read_disk_usage(),
            full_disk: None,
            full_disk_scanning: false,
            full_disk_error: None,
            show_full_disk: false,
            full_disk_cursor: 0,
            show_global_tools: false,
            global_tools: None,
            global_tools_scanning: false,
            global_tools_error: None,
            global_cursor: 0,
            pending_global_uninstall: None,
            global_uninstall_receiver: None,
            global_uninstalling: false,
            directory_scan: None,
            directory_scanning: false,
            directory_error: None,
            directory_history: Vec::new(),
            directory_cursor: 0,
            pending_explorer_delete: None,
            explorer_delete_receiver: None,
            explorer_deleting: false,
            clean_receiver: None,
            cleaning_completed: 0,
            cleaning_total: 0,
            cleaning_disk_before: None,
            cleaning_started_at: None,
            active_cleaner: None,
            active_target: None,
            active_target_size: 0,
            active_detail: None,
            last_action: None,
        }
    }

    fn reset_for_scan(&mut self, expected_scans: usize) {
        self.mode = Mode::Scanning;
        self.rows.clear();
        self.trash_target = None;
        self.trash_selected = false;
        self.readonly_report = None;
        self.info_overlay = None;
        self.pending_empty_trash = false;
        self.cursor = 0;
        self.received_scans = 0;
        self.expected_scans = expected_scans;
        self.confirm_text.clear();
        self.show_full_disk = false;
        self.full_disk_cursor = 0;
        self.show_global_tools = false;
        self.global_tools = None;
        self.global_tools_scanning = false;
        self.global_tools_error = None;
        self.global_cursor = 0;
        self.pending_global_uninstall = None;
        self.global_uninstall_receiver = None;
        self.global_uninstalling = false;
        self.directory_scan = None;
        self.directory_scanning = false;
        self.directory_error = None;
        self.directory_history.clear();
        self.directory_cursor = 0;
        self.pending_explorer_delete = None;
        self.explorer_delete_receiver = None;
        self.explorer_deleting = false;
        self.clean_receiver = None;
        self.cleaning_completed = 0;
        self.cleaning_total = 0;
        self.cleaning_disk_before = None;
        self.cleaning_started_at = None;
        self.active_cleaner = None;
        self.active_target = None;
        self.active_target_size = 0;
        self.active_detail = None;
        self.disk = read_disk_usage().or(self.disk);
    }

    fn available_update(&self) -> Option<UpdateInfo> {
        match &self.update_state {
            UpdateState::Available(update) => Some(update.clone()),
            _ => None,
        }
    }

    fn add_scan(&mut self, report: CleanerScan) {
        self.received_scans += 1;
        if report.risk_level == RiskLevel::Destructive {
            self.trash_target = report.targets.into_iter().next();
            self.trash_selected = false;
            return;
        }
        self.rows
            .extend(report.targets.into_iter().map(|target| TargetRow {
                cleaner_id: report.cleaner_id.clone(),
                cleaner_name: report.display_name.clone(),
                category: report.category,
                risk: report.risk_level,
                target,
                selected: report.risk_level == RiskLevel::Safe,
            }));
    }

    fn selected_count(&self) -> usize {
        self.rows.iter().filter(|row| row.selected).count()
            + usize::from(self.trash_selected && self.trash_target.is_some())
    }

    fn selected_size(&self) -> u64 {
        let regular = self
            .rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.target.size_bytes)
            .sum::<u64>();
        regular
            + self
                .trash_target
                .as_ref()
                .filter(|_| self.trash_selected)
                .map(|target| target.size_bytes)
                .unwrap_or_default()
    }

    fn has_manual_selection(&self) -> bool {
        !self.dry_run
            && self
                .rows
                .iter()
                .any(|row| row.selected && row.risk == RiskLevel::Manual)
    }

    fn toggle_current(&mut self) {
        if let Some(row) = self.rows.get_mut(self.cursor) {
            row.selected = !row.selected;
        }
    }

    fn toggle_trash(&mut self) {
        if self.trash_target.is_some() {
            self.trash_selected = !self.trash_selected;
        }
    }

    fn select_all_safe(&mut self) {
        for row in &mut self.rows {
            if row.risk == RiskLevel::Safe {
                row.selected = true;
            }
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let max = self.rows.len() - 1;
        self.cursor = if delta.is_negative() {
            self.cursor.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.cursor + delta as usize).min(max)
        };
    }

    fn full_disk_entries(&self) -> Vec<cleanrs_core::DiskScanEntry> {
        self.full_disk
            .as_ref()
            .map(|report| {
                report
                    .root_entries
                    .iter()
                    .chain(report.home_entries.iter())
                    .chain(report.readonly_entries.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn directory_entries(&self) -> &[DirectoryScanEntry] {
        self.directory_scan
            .as_ref()
            .map(|scan| scan.entries.as_slice())
            .unwrap_or(&[])
    }

    fn global_tools(&self) -> &[GlobalTool] {
        self.global_tools
            .as_ref()
            .map(|scan| scan.tools.as_slice())
            .unwrap_or(&[])
    }

    fn move_global_cursor(&mut self, delta: i32) {
        let len = self.global_tools().len();
        if len == 0 {
            return;
        }
        let max = len - 1;
        self.global_cursor = if delta.is_negative() {
            self.global_cursor
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.global_cursor + delta as usize).min(max)
        };
    }

    fn move_directory_cursor(&mut self, delta: i32) {
        let len = if self.directory_scan.is_some() {
            self.directory_entries().len()
        } else {
            self.full_disk_entries().len()
        };
        if len == 0 {
            return;
        }
        let max = len - 1;
        self.directory_cursor = if delta.is_negative() {
            self.directory_cursor
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.directory_cursor + delta as usize).min(max)
        };
        self.full_disk_cursor = self.directory_cursor;
    }
}

fn read_disk_usage() -> Option<DiskUsage> {
    let total_bytes = total_space("/").ok()?;
    let free_bytes = available_space("/").ok()?;
    Some(DiskUsage {
        total_bytes,
        free_bytes,
    })
}

enum KeyAction {
    ToggleAllowlist,
    BackDirectory,
    BackGlobalTools,
    Continue,
    DeleteFile,
    FullDisk,
    GlobalTools,
    OpenDirectory,
    Quit,
    Rescan,
    UninstallGlobal,
    Upgrade,
}

enum CleanMessage {
    Started {
        cleaner_id: String,
        target: CleanTarget,
    },
    Progress(String),
    Target(Result<CleanResult, String>),
    Finished,
}

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
        let result = full_disk_scan(Path::new("/"), 12).map_err(|error| format!("{error:#}"));
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

fn start_explorer_delete(app: &mut App) {
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

fn start_global_uninstall(app: &mut App) {
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

fn start_cleaning(app: &mut App) {
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

fn run_loop(stdout: &mut Stdout, current_version: &str) -> Result<RunOutcome> {
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
                                if previous == Path::new("/") {
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
                                    .filter(|entry| entry.path.is_dir())
                                    .map(|entry| entry.path.clone())
                            };
                            if let Some(path) = selected_path {
                                let current = app
                                    .directory_scan
                                    .as_ref()
                                    .map(|scan| scan.path.clone())
                                    .unwrap_or_else(|| PathBuf::from("/"));
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

fn handle_key(app: &mut App, key: KeyEvent) -> KeyAction {
    if key.code == KeyCode::Char('q') {
        return KeyAction::Quit;
    }

    if app.info_overlay.is_some() {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('i')) {
            app.info_overlay = None;
        }
        return KeyAction::Continue;
    }

    if matches!(app.mode, Mode::Reviewing)
        && key.code == KeyCode::Char('u')
        && app.available_update().is_some()
    {
        return KeyAction::Upgrade;
    }

    match app.mode {
        Mode::Scanning => {}
        Mode::Reviewing => {
            if app.show_global_tools {
                if app.global_tools_scanning {
                    match key.code {
                        KeyCode::Char('b') => return KeyAction::BackGlobalTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => app.move_global_cursor(1),
                        KeyCode::Up | KeyCode::Char('k') => app.move_global_cursor(-1),
                        KeyCode::Enter | KeyCode::Char('x') => return KeyAction::UninstallGlobal,
                        KeyCode::Char('r') => return KeyAction::GlobalTools,
                        KeyCode::Char('b') => return KeyAction::BackGlobalTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                }
            } else if app.show_full_disk {
                if app.directory_scanning {
                    match key.code {
                        KeyCode::Char('b') => return KeyAction::BackDirectory,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => app.move_directory_cursor(1),
                        KeyCode::Up | KeyCode::Char('k') => app.move_directory_cursor(-1),
                        KeyCode::Enter => return KeyAction::OpenDirectory,
                        KeyCode::Char('w') => return KeyAction::ToggleAllowlist,
                        KeyCode::Char('x') => return KeyAction::DeleteFile,
                        KeyCode::Char('b') => return KeyAction::BackDirectory,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                }
            } else {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
                    KeyCode::Char(' ') => app.toggle_current(),
                    KeyCode::Char('a') => app.select_all_safe(),
                    KeyCode::Char('t') => app.toggle_trash(),
                    KeyCode::Char('i') => {
                        if let Some(report) = &app.readonly_report {
                            let paths = report
                                .targets
                                .iter()
                                .map(|target| {
                                    format!(
                                        "{}  ·  {}",
                                        target.path.display(),
                                        format_size(target.size_bytes, DECIMAL)
                                    )
                                })
                                .collect::<Vec<_>>();
                            app.info_overlay = Some(format!(
                                "{}\n\n{}",
                                report.advice,
                                if paths.is_empty() {
                                    "No current read-only items reported.".to_owned()
                                } else {
                                    paths.join("\n")
                                }
                            ));
                        } else {
                            app.last_action =
                                Some("Read-only inventory is still scanning".to_owned());
                        }
                    }
                    KeyCode::Char('d') => app.dry_run = !app.dry_run,
                    KeyCode::Char('f') => return KeyAction::FullDisk,
                    KeyCode::Char('g') => return KeyAction::GlobalTools,
                    KeyCode::Char('r') => return KeyAction::Rescan,
                    KeyCode::Enter if app.trash_selected => {
                        app.confirm_text.clear();
                        app.pending_empty_trash = true;
                        app.mode = Mode::Confirming;
                    }
                    KeyCode::Enter if app.selected_count() > 0 => {
                        app.confirm_text.clear();
                        app.mode = Mode::Confirming;
                    }
                    _ => {}
                }
            }
        }
        Mode::Confirming => handle_confirmation(app, key),
        Mode::Cleaning => {}
    }
    KeyAction::Continue
}

fn handle_confirmation(app: &mut App, key: KeyEvent) {
    if app.pending_empty_trash {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_empty_trash = false;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Enter if app.confirm_text == "EMPTY TRASH" => {
                if app.has_manual_selection() {
                    app.pending_empty_trash = false;
                    app.confirm_text.clear();
                    app.mode = Mode::Reviewing;
                    app.last_action =
                        Some("Deselect Manual targets before confirming Empty Trash".to_owned());
                } else {
                    app.pending_empty_trash = false;
                    app.confirm_text.clear();
                    start_cleaning(app);
                }
            }
            KeyCode::Char(character) => app.confirm_text.push(character),
            KeyCode::Backspace => {
                app.confirm_text.pop();
            }
            _ => {}
        }
        return;
    }

    if app.pending_global_uninstall.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_global_uninstall = None;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Char('y') => start_global_uninstall(app),
            _ => {}
        }
        return;
    }

    if app.pending_explorer_delete.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_explorer_delete = None;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Char('y') => start_explorer_delete(app),
            _ => {}
        }
        return;
    }

    let manual = app.has_manual_selection();
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.confirm_text.clear();
            app.mode = Mode::Reviewing;
        }
        KeyCode::Char('y') if !manual => start_cleaning(app),
        KeyCode::Enter if manual && app.confirm_text == "FORCE" => start_cleaning(app),
        KeyCode::Char(character) if manual && !app.dry_run => app.confirm_text.push(character),
        KeyCode::Backspace if manual && !app.dry_run => {
            app.confirm_text.pop();
        }
        _ => {}
    }
}

fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(frame.area());

    let title = match app.mode {
        Mode::Scanning => format!(
            " cleanrs — scanning {}/{} cleaners ",
            app.received_scans, app.expected_scans
        ),
        Mode::Reviewing if app.show_global_tools => " cleanrs — global tools ".to_owned(),
        Mode::Reviewing if app.show_full_disk => " cleanrs — full-disk explorer ".to_owned(),
        Mode::Reviewing => " cleanrs — review targets ".to_owned(),
        Mode::Confirming => " cleanrs — confirm ".to_owned(),
        Mode::Cleaning if app.explorer_deleting => " cleanrs — moving item to Trash ".to_owned(),
        Mode::Cleaning => format!(
            " cleanrs — cleaning {}/{} targets ",
            app.cleaning_completed, app.cleaning_total
        ),
    };
    let header = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1)])
        .split(vertical[0]);
    let disk_line = match app.disk {
        Some(disk) => format!(
            "Disk /  Used {}  |  Free {}  |  Total {}",
            format_size(disk.used_bytes(), DECIMAL),
            format_size(disk.free_bytes, DECIMAL),
            format_size(disk.total_bytes, DECIMAL)
        ),
        None => "Disk /  usage unavailable".to_owned(),
    };
    let mode_color = match app.mode {
        Mode::Scanning => Color::Cyan,
        Mode::Reviewing => Color::Green,
        Mode::Confirming => Color::Yellow,
        Mode::Cleaning => Color::Magenta,
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                title,
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(disk_line, Style::default().fg(Color::Gray))),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" MY CLEANER v{} ", app.current_version))
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        ),
        header[0],
    );
    if matches!(app.mode, Mode::Cleaning) {
        let ratio = if app.cleaning_total == 0 {
            0.0
        } else {
            app.cleaning_completed as f64 / app.cleaning_total as f64
        };
        frame.render_widget(
            Gauge::default()
                .ratio(ratio.min(1.0))
                .gauge_style(Style::default().fg(Color::Magenta))
                .label(format!(
                    "{}% · {}/{} targets",
                    (ratio * 100.0).round() as u16,
                    app.cleaning_completed,
                    app.cleaning_total
                )),
            header[1],
        );
    } else if let Some(disk) = app.disk {
        let gauge_color = if disk.used_ratio() >= 0.9 {
            Color::Red
        } else if disk.used_ratio() >= 0.75 {
            Color::Yellow
        } else {
            Color::Green
        };
        frame.render_widget(
            Gauge::default()
                .ratio(disk.used_ratio())
                .gauge_style(Style::default().fg(gauge_color))
                .label(format!("{:.1}% used", disk.used_ratio() * 100.0)),
            header[1],
        );
    }

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(24), Constraint::Percentage(76)])
        .split(vertical[1]);
    render_sidebar(frame, app, body[0]);
    if app.show_global_tools {
        render_global_tools(frame, app, body[1]);
    } else if app.show_full_disk {
        if app.directory_scan.is_some() || app.directory_scanning || app.directory_error.is_some() {
            render_directory(frame, app, body[1]);
        } else {
            render_full_disk(frame, app, body[1]);
        }
    } else {
        render_targets(frame, app, body[1]);
    }

    let footer_color = match app.mode {
        Mode::Scanning => Color::Cyan,
        Mode::Reviewing => Color::Green,
        Mode::Confirming => Color::Yellow,
        Mode::Cleaning => Color::Magenta,
    };
    frame.render_widget(
        Paragraph::new(footer_lines(app))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Status / shortcuts ")
                    .title_style(
                        Style::default()
                            .fg(footer_color)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(footer_color)),
            ),
        vertical[2],
    );
    if matches!(app.mode, Mode::Confirming) {
        render_confirmation_modal(frame, app);
    } else if app.info_overlay.is_some() {
        render_info_modal(frame, app);
    }
}

fn render_confirmation_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(78, 48, frame.area());
    let destructive = app.pending_empty_trash
        || app.pending_explorer_delete.is_some()
        || app.pending_global_uninstall.is_some();
    let border_color = if destructive {
        Color::Red
    } else {
        Color::Yellow
    };
    let mut lines = Vec::new();

    if app.pending_empty_trash {
        lines.push(Line::from(Span::styled(
            "EMPTY TRASH",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "This will remove everything inside ~/.Trash ({}).",
            app.trash_target
                .as_ref()
                .map(|target| format_size(target.size_bytes, DECIMAL))
                .unwrap_or_else(|| "size unavailable".to_owned())
        )));
        lines.push(Line::from("This is irreversible from cleanrs."));
        lines.push(Line::from("Type EMPTY TRASH exactly, then press [enter]."));
        lines.push(Line::from(format!(
            "Input: [{}]",
            if app.confirm_text.is_empty() {
                "_____"
            } else {
                app.confirm_text.as_str()
            }
        )));
    } else if let Some(tool) = &app.pending_global_uninstall {
        lines.push(Line::from(Span::styled(
            "Uninstall global tool",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "{} · {}{}",
            tool.manager.label(),
            tool.name,
            tool.version
                .as_deref()
                .map(|version| format!(" · {version}"))
                .unwrap_or_default()
        )));
        lines.push(Line::from(format!("Command: {}", tool.uninstall_command())));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "This uses the package manager's official uninstall flow.",
        ));
        if tool.requires_admin_authentication() {
            lines.push(Line::from(Span::styled(
                "Administrator password may be required.",
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(
                "Run sudo -v in another Terminal first; cleanrs never captures passwords.",
            ));
        }
        lines.push(Line::from(
            "Check dependencies first; Homebrew may remove linked dependents.",
        ));
        lines.push(Line::from("Press [y] to execute or [n]/[esc] to cancel."));
    } else if let Some(target) = &app.pending_explorer_delete {
        let item_kind = if target.path.is_dir() {
            "folder and all contents"
        } else {
            "file"
        };
        lines.push(Line::from(Span::styled(
            "Move to Trash",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "{item_kind}: {}",
            target.path.display()
        )));
        lines.push(Line::from(format!(
            "Estimated size: {}",
            format_size(target.size_bytes, DECIMAL)
        )));
        if cleanrs_core::is_protected_path(&target.path) {
            lines.push(Line::from(Span::styled(
                "Protected-area item: verify it is stale and not in use.",
                Style::default().fg(Color::Yellow),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("This is recoverable from the macOS Trash."));
        lines.push(Line::from("Press [y] to confirm or [n]/[esc] to cancel."));
    } else if app.has_manual_selection() && !app.dry_run {
        lines.push(Line::from(Span::styled(
            "Manual-risk cleanup",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("Type FORCE in the footer, then press [enter]."));
        lines.push(Line::from(format!(
            "Selected: {} target(s) · {}",
            app.selected_count(),
            format_size(app.selected_size(), DECIMAL)
        )));
    } else {
        lines.push(Line::from(Span::styled(
            if app.dry_run {
                "Preview cleanup"
            } else {
                "Execute cleanup"
            },
            Style::default()
                .fg(if app.dry_run {
                    Color::Cyan
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "{} selected target(s) · {}",
            app.selected_count(),
            format_size(app.selected_size(), DECIMAL)
        )));
        lines.push(Line::from(if app.dry_run {
            "No files will change in preview mode."
        } else {
            "Selected targets will be moved/processed."
        }));
        lines.push(Line::from("Press [y] to confirm or [n]/[esc] to cancel."));
    }

    let modal = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(if destructive {
                " Confirm destructive action "
            } else {
                " Confirm "
            })
            .title_style(
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(modal, area);
}

fn render_info_modal(frame: &mut Frame, app: &App) {
    let Some(info) = &app.info_overlay else {
        return;
    };
    let area = centered_rect(78, 48, frame.area());
    let modal = Paragraph::new(info.as_str())
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(" Read-only info ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    frame.render_widget(Clear, area);
    frame.render_widget(modal, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn render_sidebar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut categories = std::collections::BTreeMap::new();
    for row in &app.rows {
        *categories
            .entry(format!("{:?}", row.category))
            .or_insert(0usize) += 1;
    }
    let mut lines = vec![Line::from(Span::styled(
        "CLEANUP",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))];
    for (category, count) in categories {
        lines.push(Line::from(vec![
            Span::styled(format!("{category:<14}"), Style::default().fg(Color::Gray)),
            Span::styled(
                count.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Selected  ", Style::default().fg(Color::Gray)),
        Span::styled(
            app.selected_count().to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Size      ", Style::default().fg(Color::Gray)),
        Span::styled(
            format_size(app.selected_size(), DECIMAL),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Mode      ", Style::default().fg(Color::Gray)),
        Span::styled(
            if app.dry_run { "DRY-RUN" } else { "EXECUTE" },
            Style::default()
                .fg(if app.dry_run {
                    Color::Cyan
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if !matches!(&app.update_state, UpdateState::UpToDate) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "VERSION",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!("Current: v{}", app.current_version)));
        let (update_label, update_color) = match &app.update_state {
            UpdateState::Checking => ("Checking for updates…".to_owned(), Color::Yellow),
            UpdateState::Available(update) => (
                format!("New v{} · press [u]", update.latest_version),
                Color::Green,
            ),
            UpdateState::Unavailable(error) => (
                format!("Check unavailable · {}", shorten(error, 24)),
                Color::DarkGray,
            ),
            UpdateState::UpToDate => unreachable!("up-to-date version panel is hidden"),
        };
        lines.push(Line::from(Span::styled(
            update_label,
            Style::default().fg(update_color),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "GLOBAL TOOLS",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    if app.global_tools_scanning {
        lines.push(Line::from(Span::styled(
            "Scanning global tools…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(report) = &app.global_tools {
        let removable = report
            .tools
            .iter()
            .filter(|tool| tool.can_uninstall)
            .count();
        lines.push(Line::from(format!("Installed: {}", report.tools.len())));
        lines.push(Line::from(format!("Removable: {removable}")));
        if !report.errors.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Provider warnings: {}", report.errors.len()),
                Style::default().fg(Color::Yellow),
            )));
        }
    } else {
        lines.push(Line::from("Press [g] to inspect"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "STORAGE",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    if app.full_disk_scanning {
        if matches!(app.mode, Mode::Cleaning) {
            lines.push(Line::from(Span::styled(
                "Background scan · read-only",
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(Span::styled(
                "Cleanup independent · snapshot may be stale",
                Style::default().fg(Color::Gray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "Scanning root + HOME…",
                Style::default().fg(Color::Yellow),
            )));
        }
    } else if let Some(error) = &app.full_disk_error {
        lines.push(Line::from(format!("Error: {error}")));
    } else if let Some(report) = &app.full_disk {
        let root_total = report
            .root_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        let home_total = report
            .home_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        let protected_total = report
            .readonly_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        lines.push(Line::from(format!(
            "Top root: {}",
            format_size(root_total, DECIMAL)
        )));
        lines.push(Line::from(format!(
            "Top HOME: {}",
            format_size(home_total, DECIMAL)
        )));
        lines.push(Line::from(format!(
            "Protected: {}",
            format_size(protected_total, DECIMAL)
        )));
        lines.push(Line::from(format!(
            "Blocked: {}",
            report.inaccessible_paths
        )));
    } else {
        lines.push(Line::from("Press [f] to scan"));
    }
    if app.show_full_disk {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "EXPLORER",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        if app.directory_scanning {
            lines.push(Line::from(Span::styled(
                "Scanning folder…",
                Style::default().fg(Color::Yellow),
            )));
        } else if let Some(report) = &app.directory_scan {
            lines.push(Line::from(Span::styled(
                report.path.display().to_string(),
                Style::default().fg(Color::Gray),
            )));
            let git_status = if !report.is_git_repo {
                ("Git  NOT A REPO", Color::Gray)
            } else if report.git_dirty {
                ("Git  DIRTY · preserve changes", Color::Yellow)
            } else {
                ("Git  CLEAN", Color::Green)
            };
            lines.push(Line::from(Span::styled(
                git_status.0,
                Style::default().fg(git_status.1),
            )));
            let suggestions = report
                .entries
                .iter()
                .filter(|entry| entry.suggestion.is_some())
                .count();
            lines.push(Line::from(vec![
                Span::styled("Suggestions  ", Style::default().fg(Color::Gray)),
                Span::styled(
                    suggestions.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "Enter opens selected folder",
                Style::default().fg(Color::Gray),
            )));
        }
    }
    if let Some(action) = &app.last_action {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            action.as_str(),
            Style::default().fg(Color::Yellow),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Status ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        ),
        area,
    );
}

fn render_global_tools(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = vec![ListItem::new(
        "Global installs · x/Enter review · manager-owned uninstall",
    )];
    let mut selected_index = None;

    if app.global_tools_scanning {
        items.push(ListItem::new("Scanning Cargo and other providers…"));
    } else if let Some(error) = &app.global_tools_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.global_tools {
        if report.tools.is_empty() && report.errors.is_empty() {
            items.push(ListItem::new("No global tools found."));
        }
        for error in &report.errors {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("[WARN] ", Style::default().fg(Color::Yellow)),
                Span::styled(error.as_str(), Style::default().fg(Color::Gray)),
            ])));
        }

        let tools_start = items.len();
        items.extend(report.tools.iter().map(|tool| {
            let (marker, color) = if tool.can_uninstall {
                ("UNINSTALL", Color::Green)
            } else {
                ("KEEP", Color::Red)
            };
            let version = tool
                .version
                .as_deref()
                .map(|version| format!("  ·  {version}"))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{marker}]"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {:<19}  ", tool.manager.label()),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    tool.name.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(version, Style::default().fg(Color::Gray)),
            ]))
        }));
        if !report.tools.is_empty() {
            selected_index = Some(tools_start + app.global_cursor);
        }
    } else {
        items.push(ListItem::new("Press [g] to scan global tools."));
    }

    let mut state = ListState::default();
    state.select(selected_index);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Global tools / uninstall "),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(38, 46, 56))
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_full_disk(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = Vec::new();
    let mut selected_index = None;
    items.push(ListItem::new(
        "Inventory — Enter opens folders; x moves only approved items to Trash",
    ));

    if app.full_disk_scanning {
        items.push(ListItem::new("Scanning root disk and HOME in background…"));
    } else if let Some(error) = &app.full_disk_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.full_disk {
        let render_entry = |entry: &cleanrs_core::DiskScanEntry| {
            let (marker, color) = if entry.read_only {
                ("READONLY", Color::Red)
            } else {
                ("REVIEW", Color::Yellow)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}  ", format_size(entry.size_bytes, DECIMAL)),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("[{marker}]"), Style::default().fg(color)),
                Span::raw(format!("  {}", entry.path.display())),
            ]))
        };
        items.push(ListItem::new(Span::styled(
            "Largest root entries",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        let root_start = items.len();
        items.extend(report.root_entries.iter().map(&render_entry));
        items.push(ListItem::new(""));
        items.push(ListItem::new(Span::styled(
            "Largest HOME entries",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        let home_start = items.len();
        items.extend(report.home_entries.iter().map(&render_entry));
        items.push(ListItem::new(""));
        items.push(ListItem::new(Span::styled(
            "Protected system/mount paths — size only; Enter to inspect",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        let readonly_start = items.len();
        items.extend(report.readonly_entries.iter().map(&render_entry));
        let root_len = report.root_entries.len();
        let home_len = report.home_entries.len();
        let readonly_len = report.readonly_entries.len();
        if app.full_disk_cursor < root_len {
            selected_index = Some(root_start + app.full_disk_cursor);
        } else if app.full_disk_cursor < root_len + home_len {
            selected_index = Some(home_start + app.full_disk_cursor - root_len);
        } else if app.full_disk_cursor < root_len + home_len + readonly_len {
            selected_index = Some(readonly_start + app.full_disk_cursor - root_len - home_len);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled("Protected  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(
                    "{}  ·  {}",
                    report.readonly_entries.len(),
                    format_size(
                        report
                            .readonly_entries
                            .iter()
                            .map(|entry| entry.size_bytes)
                            .sum::<u64>(),
                        DECIMAL
                    )
                ),
                Style::default().fg(Color::Red),
            ),
            Span::styled("  ·  Inaccessible  ", Style::default().fg(Color::Gray)),
            Span::styled(
                report.inaccessible_paths.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ])));
    } else {
        items.push(ListItem::new("Press [f] to start a full-disk scan."));
    }

    let mut state = ListState::default();
    state.select(selected_index);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Full disk / inventory "),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(38, 46, 56))
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_directory(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = Vec::new();
    let mut selected_index = None;
    let path = app
        .directory_scan
        .as_ref()
        .map(|scan| scan.path.display().to_string())
        .unwrap_or_else(|| "loading…".to_owned());
    items.push(ListItem::new(format!("Path: {path}")));

    if app.explorer_deleting {
        items.push(ListItem::new(
            "Moving the selected item to Trash in background…",
        ));
    } else if app.directory_scanning {
        items.push(ListItem::new(
            "Git/status and children are scanning in background…",
        ));
    } else if let Some(error) = &app.directory_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.directory_scan {
        let git_status = if !report.is_git_repo {
            "Git: not a repository"
        } else if report.git_dirty {
            "Git: DIRTY — preserve source changes"
        } else {
            "Git: clean — generated folders may be regenerated"
        };
        items.push(ListItem::new(git_status));
        items.push(ListItem::new(
            "Entries — Enter opens folders; [w] toggle delete allowlist; [x] move to Trash",
        ));
        let entry_start = items.len();
        if report.entries.is_empty() {
            items.push(ListItem::new("No readable children."));
        } else {
            items.extend(report.entries.iter().map(|entry| {
                let (marker, color) = if entry.read_only {
                    ("READONLY", Color::Red)
                } else if entry.allowlisted {
                    ("ALLOW", Color::Green)
                } else if entry
                    .suggestion
                    .as_ref()
                    .is_some_and(|suggestion| suggestion.can_delete)
                {
                    ("SUGGEST", Color::Green)
                } else if entry.suggestion.is_some() {
                    ("REVIEW", Color::Yellow)
                } else if entry.is_dir {
                    ("DIR", Color::Cyan)
                } else {
                    ("FILE", Color::Gray)
                };
                let reason = entry
                    .suggestion
                    .as_ref()
                    .map(|suggestion| format!(" — {}", suggestion.reason))
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{marker}]"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}  ", format_size(entry.size_bytes, DECIMAL)),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::raw(entry.path.display().to_string()),
                    Span::styled(reason, Style::default().fg(Color::Gray)),
                ]))
            }));
            selected_index = Some(entry_start + app.directory_cursor);
        }
        items.push(ListItem::new(format!(
            "Inaccessible: {}",
            report.inaccessible_paths
        )));
    } else {
        items.push(ListItem::new("Press [enter] to open a selected folder."));
    }

    let mut state = ListState::default();
    state.select(selected_index);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Explorer / folder "),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(38, 46, 56))
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_targets(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(7)])
        .split(area);
    let items = if app.rows.is_empty() {
        vec![ListItem::new("No reclaimable targets found.")]
    } else {
        app.rows
            .iter()
            .map(|row| target_list_item(row, area.width.saturating_sub(6) as usize))
            .collect()
    };
    let mut state = ListState::default();
    if !app.rows.is_empty() {
        state.select(Some(app.cursor));
    }
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Targets"))
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(38, 46, 56))
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, sections[0], &mut state);

    let mut special_items = Vec::new();
    if let Some(target) = &app.trash_target {
        let checkbox = if app.trash_selected { "[x]" } else { "[ ]" };
        special_items.push(ListItem::new(Line::from(vec![
            Span::styled("🔥 ", Style::default().fg(Color::Red)),
            Span::styled(
                format!("{checkbox} Empty Trash"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  ·  {}  ·  DESTRUCTIVE  ·  [t] select",
                    format_size(target.size_bytes, DECIMAL)
                ),
                Style::default().fg(Color::Gray),
            ),
        ])));
    } else {
        special_items.push(ListItem::new("🔥 Empty Trash unavailable or empty."));
    }
    if let Some(report) = &app.readonly_report {
        if report.targets.is_empty() {
            special_items.push(ListItem::new(Line::from(vec![
                Span::styled("🔒 [disabled] ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "No read-only system items reported",
                    Style::default().fg(Color::Gray),
                ),
            ])));
        } else {
            for target in report.targets.iter().take(3) {
                special_items.push(ListItem::new(Line::from(vec![
                    Span::styled("🔒 [disabled] ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!(
                            "{}  ·  {}",
                            target.path.display(),
                            format_size(target.size_bytes, DECIMAL)
                        ),
                        Style::default().fg(Color::Gray),
                    ),
                ])));
            }
            special_items.push(ListItem::new(Span::styled(
                "[i] Info · read-only targets are never selectable",
                Style::default().fg(Color::Cyan),
            )));
        }
    } else {
        special_items.push(ListItem::new(Span::styled(
            "🔒 Read-only inventory scanning…",
            Style::default().fg(Color::Yellow),
        )));
    }
    let special = List::new(special_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Special review · destructive / read-only"),
    );
    frame.render_widget(special, sections[1]);
}

fn target_list_item(row: &TargetRow, width: usize) -> ListItem<'static> {
    let checkbox = if row.selected { "[x]" } else { "[ ]" };
    let checkbox_color = if row.selected {
        Color::Green
    } else {
        Color::DarkGray
    };
    let risk_style = match row.risk {
        RiskLevel::Safe => Style::default().fg(Color::Green),
        RiskLevel::Caution => Style::default().fg(Color::Yellow),
        RiskLevel::Manual => Style::default().fg(Color::Red),
        RiskLevel::Destructive => Style::default().fg(Color::Magenta),
    };
    let size = format_size(row.target.size_bytes, DECIMAL);
    let risk = format!("{:?}", row.risk);
    let full_text = format!(
        "{checkbox} {}  ·  {}  ·  {size}  {risk}",
        row.cleaner_name, row.target.description
    );

    if full_text.chars().count() <= width {
        return ListItem::new(Line::from(vec![
            Span::styled(format!("{checkbox} "), Style::default().fg(checkbox_color)),
            Span::styled(
                row.cleaner_name.to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  ·  "),
            Span::raw(row.target.description.clone()),
            Span::styled(format!("  ·  {size}"), Style::default().fg(Color::Gray)),
            Span::styled(format!("  {risk}"), risk_style),
        ]));
    }

    let header = format!("{checkbox} {}  ·  {size}  {risk}", row.cleaner_name);
    let description_indent = "  ↳ ";
    let description_width = width
        .saturating_sub(description_indent.chars().count())
        .max(1);
    let mut lines = wrap_text(&header, width)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                Line::from(vec![Span::styled(
                    line,
                    Style::default()
                        .fg(checkbox_color)
                        .add_modifier(Modifier::BOLD),
                )])
            } else {
                Line::from(line)
            }
        })
        .collect::<Vec<_>>();
    lines.extend(
        wrap_text(&row.target.description, description_width)
            .into_iter()
            .map(|line| {
                Line::from(vec![
                    Span::styled(description_indent, Style::default().fg(Color::DarkGray)),
                    Span::raw(line),
                ])
            }),
    );
    ListItem::new(lines)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if word.chars().count() <= width {
            let candidate_width =
                current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
            if candidate_width <= width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            current.push_str(word);
            continue;
        }

        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut chunk = String::new();
        for character in word.chars() {
            chunk.push(character);
            if chunk.chars().count() == width {
                lines.push(std::mem::take(&mut chunk));
            }
        }
        current = chunk;
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn footer_lines(app: &App) -> Vec<Line<'static>> {
    match app.mode {
        Mode::Scanning => vec![
            footer_line(
                "STATUS",
                vec![Span::styled(
                    format!(
                        "Scanning cleaners · {}/{}",
                        app.received_scans, app.expected_scans
                    ),
                    Style::default().fg(Color::Cyan),
                )],
            ),
            footer_line(
                "MODE",
                vec![Span::styled(
                    "Background scan · results stream in",
                    Style::default().fg(Color::Gray),
                )],
            ),
            footer_line("KEYS", vec![footer_key("q"), Span::raw(" Quit")]),
        ],
        Mode::Reviewing => {
            if app.show_global_tools {
                let status = if app.global_tools_scanning {
                    "Global tools · scanning providers".to_owned()
                } else if let Some(report) = &app.global_tools {
                    format!(
                        "Global tools · {} installed · {} provider warning(s)",
                        report.tools.len(),
                        report.errors.len()
                    )
                } else {
                    "Global tools · not scanned".to_owned()
                };
                return vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            shorten(&status, 58),
                            Style::default().fg(if app.global_tools_scanning {
                                Color::Yellow
                            } else {
                                Color::Green
                            }),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![Span::styled(
                            "Manager-owned command · confirmation required",
                            Style::default().fg(Color::Gray),
                        )],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("↑↓"),
                            Span::raw(" Move   "),
                            footer_key("x/Enter"),
                            Span::raw(" Remove   "),
                            footer_key("r"),
                            Span::raw(" Reload   "),
                            footer_key("b"),
                            Span::raw(" Back"),
                        ],
                    ),
                ];
            }
            if app.show_full_disk {
                if app.directory_scanning {
                    return vec![
                        footer_line(
                            "STATUS",
                            vec![Span::styled(
                                "Scanning folder · Git/status and sizes",
                                Style::default().fg(Color::Yellow),
                            )],
                        ),
                        footer_line(
                            "MODE",
                            vec![Span::styled(
                                "Background scan",
                                Style::default().fg(Color::Gray),
                            )],
                        ),
                        footer_line(
                            "KEYS",
                            vec![
                                footer_key("b"),
                                Span::raw(" Back   "),
                                footer_key("q"),
                                Span::raw(" Quit"),
                            ],
                        ),
                    ];
                }
                if app.directory_scan.is_some() {
                    let explorer_status = app
                        .last_action
                        .as_deref()
                        .map(|action| format!("Explorer · {}", shorten(action, 48)))
                        .unwrap_or_else(|| {
                            format!("Explorer · {} entries", app.directory_entries().len())
                        });
                    return vec![
                        footer_line(
                            "STATUS",
                            vec![Span::styled(
                                explorer_status,
                                Style::default().fg(if app.last_action.is_some() {
                                    Color::Yellow
                                } else {
                                    Color::Green
                                }),
                            )],
                        ),
                        footer_line(
                            "MODE",
                            vec![
                                Span::styled(
                                    "Folder navigation   ",
                                    Style::default().fg(Color::Gray),
                                ),
                                footer_key("Enter"),
                                Span::raw(" Open   "),
                                footer_key("w"),
                                Span::raw(" Allowlist   "),
                                footer_key("x"),
                                Span::raw(" Trash"),
                            ],
                        ),
                        footer_line(
                            "KEYS",
                            vec![
                                footer_key("↑↓"),
                                Span::raw(" Move   "),
                                footer_key("b"),
                                Span::raw(" Back   "),
                                footer_key("f"),
                                Span::raw(" Root   "),
                                footer_key("q"),
                                Span::raw(" Quit"),
                            ],
                        ),
                    ];
                }
                let status = if app.full_disk_scanning {
                    "Full-disk inventory · scanning root + HOME".to_owned()
                } else {
                    format!(
                        "Full-disk inventory · {} entries",
                        app.full_disk_entries().len()
                    )
                };
                return vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            status,
                            Style::default().fg(if app.full_disk_scanning {
                                Color::Yellow
                            } else {
                                Color::Green
                            }),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![
                            Span::styled("Inventory   ", Style::default().fg(Color::Gray)),
                            footer_key("Enter"),
                            Span::raw(" Open   "),
                            footer_key("b"),
                            Span::raw(" Targets"),
                        ],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("↑↓"),
                            Span::raw(" Choose   "),
                            footer_key("f"),
                            Span::raw(" Rescan   "),
                            footer_key("q"),
                            Span::raw(" Quit"),
                        ],
                    ),
                ];
            }
            let status = app
                .last_action
                .as_deref()
                .map(|action| shorten(action, 58))
                .unwrap_or_else(|| {
                    format!(
                        "Ready · {} selected · {}",
                        app.selected_count(),
                        format_size(app.selected_size(), DECIMAL)
                    )
                });
            let mut keys = vec![
                footer_key("↑↓"),
                Span::raw(" Move "),
                footer_key("Space"),
                Span::raw(" Sel "),
                footer_key("a"),
                Span::raw(" Safe "),
                footer_key("t"),
                Span::raw(" Trash "),
                footer_key("i"),
                Span::raw(" Info "),
                footer_key("r"),
                Span::raw(" Reload "),
                footer_key("f"),
                Span::raw(" Full "),
            ];
            if app.available_update().is_some() {
                keys.push(footer_key("u"));
                keys.push(Span::raw(" Upgrade "));
            }
            keys.push(footer_key("q"));
            keys.push(Span::raw(" Quit"));
            vec![
                footer_line(
                    "STATUS",
                    vec![Span::styled(status, Style::default().fg(Color::Green))],
                ),
                footer_line(
                    "MODE",
                    vec![
                        Span::styled(
                            if app.dry_run {
                                "DRY-RUN · no files will change   "
                            } else {
                                "EXECUTE · targets will be processed   "
                            },
                            Style::default().fg(if app.dry_run {
                                Color::Cyan
                            } else {
                                Color::Yellow
                            }),
                        ),
                        footer_key("d"),
                        Span::raw(" Mode   "),
                        footer_key("Enter"),
                        Span::raw(" Continue"),
                    ],
                ),
                footer_line("KEYS", keys),
            ]
        }
        Mode::Confirming => {
            if app.pending_empty_trash {
                let input = if app.confirm_text.is_empty() {
                    "_____".to_owned()
                } else {
                    app.confirm_text.clone()
                };
                vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            format!(
                                "Destructive · Empty Trash · {}",
                                app.trash_target
                                    .as_ref()
                                    .map(|target| format_size(target.size_bytes, DECIMAL))
                                    .unwrap_or_else(|| "size unavailable".to_owned())
                            ),
                            Style::default().fg(Color::Red),
                        )],
                    ),
                    footer_line(
                        "INPUT",
                        vec![
                            Span::styled("Type EMPTY TRASH: ", Style::default().fg(Color::Yellow)),
                            Span::styled(
                                format!("[{input}]"),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("Enter"),
                            Span::raw(" Confirm exact phrase   "),
                            footer_key("n/Esc"),
                            Span::raw(" Cancel"),
                        ],
                    ),
                ]
            } else if let Some(tool) = &app.pending_global_uninstall {
                vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            format!("Uninstall {} · {}", tool.manager.label(), tool.name),
                            Style::default().fg(Color::Red),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![Span::styled(
                            if tool.requires_admin_authentication() {
                                "Run sudo -v first if admin auth is needed"
                            } else {
                                "Manager command · check dependencies"
                            },
                            Style::default().fg(Color::Gray),
                        )],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("y"),
                            Span::raw(" Run   "),
                            footer_key("n/Esc"),
                            Span::raw(" Cancel"),
                        ],
                    ),
                ]
            } else if let Some(target) = &app.pending_explorer_delete {
                let item_kind = if target.path.is_dir() {
                    "folder and all contents"
                } else {
                    "file"
                };
                vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            format!(
                                "Move {item_kind} to Trash · {}",
                                format_size(target.size_bytes, DECIMAL)
                            ),
                            Style::default().fg(Color::Red),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![Span::styled(
                            "Recoverable via macOS Trash",
                            Style::default().fg(Color::Gray),
                        )],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("y"),
                            Span::raw(" Confirm   "),
                            footer_key("n/Esc"),
                            Span::raw(" Cancel"),
                        ],
                    ),
                ]
            } else if app.has_manual_selection() && !app.dry_run {
                let input = if app.confirm_text.is_empty() {
                    "_____".to_owned()
                } else {
                    app.confirm_text.clone()
                };
                vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            format!(
                                "Manual-risk · {} selected · {}",
                                app.selected_count(),
                                format_size(app.selected_size(), DECIMAL)
                            ),
                            Style::default().fg(Color::Red),
                        )],
                    ),
                    footer_line(
                        "INPUT",
                        vec![
                            Span::styled("Type FORCE: ", Style::default().fg(Color::Yellow)),
                            Span::styled(
                                format!("[{input}]"),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("Enter"),
                            Span::raw(" Execute   "),
                            footer_key("n/Esc"),
                            Span::raw(" Cancel"),
                        ],
                    ),
                ]
            } else {
                vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            format!(
                                "{} · {} selected · {}",
                                if app.dry_run { "Preview" } else { "Execute" },
                                app.selected_count(),
                                format_size(app.selected_size(), DECIMAL)
                            ),
                            Style::default().fg(if app.dry_run {
                                Color::Cyan
                            } else {
                                Color::Yellow
                            }),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![Span::styled(
                            if app.dry_run {
                                "No files will change"
                            } else {
                                "Selected targets will be processed"
                            },
                            Style::default().fg(Color::Gray),
                        )],
                    ),
                    footer_line(
                        "KEYS",
                        vec![
                            footer_key("y"),
                            Span::raw(" Confirm   "),
                            footer_key("n/Esc"),
                            Span::raw(" Cancel"),
                        ],
                    ),
                ]
            }
        }
        Mode::Cleaning => {
            if app.global_uninstalling {
                return vec![
                    footer_line(
                        "STATUS",
                        vec![Span::styled(
                            "Uninstalling global tool…",
                            Style::default().fg(Color::Magenta),
                        )],
                    ),
                    footer_line(
                        "PROGRESS",
                        vec![Span::styled(
                            format!(
                                "{} Background command · elapsed {}",
                                activity_marker(app),
                                elapsed_label(app)
                            ),
                            Style::default().fg(Color::Gray),
                        )],
                    ),
                    footer_line("KEYS", vec![footer_key("q"), Span::raw(" Quit")]),
                ];
            }
            let completed = app.cleaning_completed;
            let total = app.cleaning_total;
            let percent = if total == 0 {
                0
            } else {
                completed
                    .saturating_mul(100)
                    .checked_div(total)
                    .unwrap_or(0)
            };
            let active_target = app
                .active_target
                .as_deref()
                .map(|target| shorten(target, 64))
                .unwrap_or_else(|| "Waiting for cleanup worker…".to_owned());
            let active_size = if app.active_target_size == 0 {
                String::new()
            } else {
                format!(" · {}", format_size(app.active_target_size, DECIMAL))
            };
            let active_owner = app.active_cleaner.as_deref().unwrap_or("cleaner");
            let active_detail = app
                .active_detail
                .as_deref()
                .map(|detail| shorten(detail, 58))
                .unwrap_or_else(|| "worker active".to_owned());
            vec![
                footer_line(
                    "STATUS",
                    vec![Span::styled(
                        if app.explorer_deleting {
                            format!("{} Moving selected item to Trash", activity_marker(app))
                        } else {
                            format!(
                                "{} Cleaning {completed}/{total} targets · elapsed {}",
                                activity_marker(app),
                                elapsed_label(app)
                            )
                        },
                        Style::default().fg(Color::Magenta),
                    )],
                ),
                footer_line(
                    "TARGET",
                    vec![Span::styled(
                        format!("[{active_owner}] {active_target}{active_size}"),
                        Style::default().fg(Color::Gray),
                    )],
                ),
                footer_line(
                    "PROGRESS",
                    vec![Span::styled(
                        format!("{percent}% overall · {active_detail}"),
                        Style::default().fg(Color::Gray),
                    )],
                ),
                footer_line("KEYS", vec![footer_key("q"), Span::raw(" Quit")]),
            ]
        }
    }
}

fn activity_marker(app: &App) -> char {
    const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
    let elapsed_ms = app
        .cleaning_started_at
        .map(|started| started.elapsed().as_millis())
        .unwrap_or_default();
    FRAMES[((elapsed_ms / 150) as usize) % FRAMES.len()]
}

fn elapsed_label(app: &App) -> String {
    let seconds = app
        .cleaning_started_at
        .map(|started| started.elapsed().as_secs())
        .unwrap_or_default();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn footer_line(label: &str, mut content: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![footer_label(label)];
    spans.append(&mut content);
    Line::from(spans)
}

fn footer_label(label: &str) -> Span<'static> {
    Span::styled(
        format!("{label:<8}"),
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    )
}

fn footer_key(key: &str) -> Span<'static> {
    Span::styled(
        format!("[{key}]"),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

fn shorten(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let prefix = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{prefix}…")
}
