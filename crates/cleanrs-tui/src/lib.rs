use anyhow::Result;
use cleanrs_core::{
    all_cleaners, full_disk_scan, scan_cleaner, scan_directory, Category, CleanMethod, CleanResult,
    CleanTarget, CleanerScan, DirectoryScan, DirectoryScanEntry, FullDiskScan, RiskLevel,
};
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
use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

pub fn run() -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let result = run_loop(&mut stdout);

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
    rows: Vec<TargetRow>,
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
    last_action: Option<String>,
}

impl App {
    fn new(expected_scans: usize) -> Self {
        Self {
            mode: Mode::Scanning,
            rows: Vec::new(),
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
            last_action: None,
        }
    }

    fn reset_for_scan(&mut self, expected_scans: usize) {
        self.mode = Mode::Scanning;
        self.rows.clear();
        self.cursor = 0;
        self.received_scans = 0;
        self.expected_scans = expected_scans;
        self.confirm_text.clear();
        self.show_full_disk = false;
        self.full_disk_cursor = 0;
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
        self.disk = read_disk_usage().or(self.disk);
    }

    fn add_scan(&mut self, report: CleanerScan) {
        self.received_scans += 1;
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
    }

    fn selected_size(&self) -> u64 {
        self.rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.target.size_bytes)
            .sum()
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
    BackDirectory,
    Continue,
    DeleteFile,
    FullDisk,
    OpenDirectory,
    Quit,
    Rescan,
}

enum CleanMessage {
    Target(Result<CleanResult, String>),
    Finished,
}

fn start_scan() -> (usize, Receiver<CleanerScan>) {
    let cleaners = all_cleaners();
    let expected_scans = cleaners.len();
    let (sender, receiver) = mpsc::channel();
    for cleaner in cleaners {
        let sender = sender.clone();
        thread::spawn(move || {
            let report = scan_cleaner(cleaner.as_ref());
            let _ = sender.send(report);
        });
    }
    drop(sender);
    (expected_scans, receiver)
}

fn start_full_disk_scan() -> Receiver<Result<FullDiskScan, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = full_disk_scan(Path::new("/"), 12).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

fn start_directory_scan(path: PathBuf) -> Receiver<Result<DirectoryScan, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
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
    if entry.is_dir
        && !entry
            .suggestion
            .as_ref()
            .is_some_and(|suggestion| suggestion.can_delete)
    {
        return None;
    }

    Some(CleanTarget {
        path: entry.path.clone(),
        size_bytes: entry.size_bytes,
        description: if entry.is_dir {
            "Safe suggested folder selected from the directory explorer"
        } else {
            "File selected from the directory explorer"
        }
        .to_owned(),
        method: CleanMethod::TrashPath,
    })
}

fn start_explorer_delete(app: &mut App) {
    let Some(target) = app.pending_explorer_delete.take() else {
        app.mode = Mode::Reviewing;
        return;
    };
    let (sender, receiver) = mpsc::channel();
    app.mode = Mode::Cleaning;
    app.explorer_deleting = true;
    app.cleaning_completed = 0;
    app.cleaning_total = 1;
    app.cleaning_disk_before = app.disk.or_else(read_disk_usage);
    app.explorer_delete_receiver = Some(receiver);

    thread::spawn(move || {
        let result = cleanrs_core::executor::clean_target("explorer", &target, false)
            .map_err(|error| format!("{}: {error:#}", target.path.display()));
        let _ = sender.send(result);
    });
}

fn start_cleaning(app: &mut App) {
    let jobs = app
        .rows
        .iter()
        .filter(|row| row.selected)
        .map(|row| (row.cleaner_id.clone(), row.target.clone()))
        .collect::<Vec<_>>();
    let total = jobs.len();
    let dry_run = app.dry_run;
    let disk_before = app.disk.or_else(read_disk_usage);
    let (sender, receiver) = mpsc::channel();

    app.mode = Mode::Cleaning;
    app.results.clear();
    app.errors.clear();
    app.cleaning_completed = 0;
    app.cleaning_total = total;
    app.cleaning_disk_before = disk_before;
    app.clean_receiver = Some(receiver);

    thread::spawn(move || {
        let cleaners = all_cleaners();
        for (cleaner_id, target) in jobs {
            let outcome = match cleaners.iter().find(|cleaner| cleaner.id() == cleaner_id) {
                Some(cleaner) => cleaner
                    .clean(&target, dry_run)
                    .map_err(|error| format!("{cleaner_id}: {error:#}")),
                None => Err(format!("unknown cleaner {cleaner_id}")),
            };
            if sender.send(CleanMessage::Target(outcome)).is_err() {
                return;
            }
        }
        let _ = sender.send(CleanMessage::Finished);
    });
}

fn run_loop(stdout: &mut Stdout) -> Result<()> {
    let (expected_scans, mut receiver) = start_scan();
    let mut full_disk_receiver: Option<Receiver<Result<FullDiskScan, String>>> = None;
    let mut directory_receiver: Option<Receiver<Result<DirectoryScan, String>>> = None;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(expected_scans);
    let result = loop {
        receive_scans(&mut app, &receiver);
        receive_full_disk_scan(&mut app, &mut full_disk_receiver);
        receive_directory_scan(&mut app, &mut directory_receiver);
        receive_explorer_delete(&mut app, &mut directory_receiver);
        if receive_cleaning(&mut app) {
            let (expected, next_receiver) = start_scan();
            app.reset_for_scan(expected);
            receiver = next_receiver;
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
                        KeyAction::Quit => break Ok(()),
                        KeyAction::DeleteFile => {
                            if let Some(target) = selected_explorer_target(&app) {
                                app.pending_explorer_delete = Some(target);
                                app.confirm_text.clear();
                                app.mode = Mode::Confirming;
                            } else {
                                app.last_action = Some(
                                    "Only approved files or safe suggested folders can be moved to Trash"
                                        .to_owned(),
                                );
                            }
                        }
                        KeyAction::FullDisk => {
                            if !app.full_disk_scanning {
                                app.full_disk_scanning = true;
                                app.full_disk_error = None;
                                app.show_full_disk = true;
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
                            let (expected, next_receiver) = start_scan();
                            app.reset_for_scan(expected);
                            receiver = next_receiver;
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
            app.mode = Mode::Reviewing;
            app.last_action = Some("File delete worker stopped unexpectedly".to_owned());
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

    match app.mode {
        Mode::Scanning => {}
        Mode::Reviewing => {
            if app.show_full_disk {
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
                    KeyCode::Char('d') => app.dry_run = !app.dry_run,
                    KeyCode::Char('f') => return KeyAction::FullDisk,
                    KeyCode::Char('r') => return KeyAction::Rescan,
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
                .title(" MY CLEANER ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        ),
        header[0],
    );
    if let Some(disk) = app.disk {
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
    if app.show_full_disk {
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
    }
}

fn render_confirmation_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(78, 48, frame.area());
    let destructive = app.pending_explorer_delete.is_some();
    let border_color = if destructive {
        Color::Red
    } else {
        Color::Yellow
    };
    let mut lines = Vec::new();

    if let Some(target) = &app.pending_explorer_delete {
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
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "STORAGE",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    if app.full_disk_scanning {
        lines.push(Line::from("Scanning…"));
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
            "Entries — Enter opens folders; [x] moves a file/safe suggested folder to Trash",
        ));
        let entry_start = items.len();
        if report.entries.is_empty() {
            items.push(ListItem::new("No readable children."));
        } else {
            items.extend(report.entries.iter().map(|entry| {
                let (marker, color) = if entry.read_only {
                    ("READONLY", Color::Red)
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
    let items = if app.rows.is_empty() {
        vec![ListItem::new("No reclaimable targets found.")]
    } else {
        app.rows
            .iter()
            .map(|row| {
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
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{checkbox} "), Style::default().fg(checkbox_color)),
                    Span::styled(
                        row.cleaner_name.as_str(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  ·  "),
                    Span::raw(row.target.description.as_str()),
                    Span::styled(
                        format!("  ·  {}", format_size(row.target.size_bytes, DECIMAL)),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(format!("  {:?}", row.risk), risk_style),
                ]))
            })
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
    frame.render_stateful_widget(list, area, &mut state);
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
                    return vec![
                        footer_line(
                            "STATUS",
                            vec![Span::styled(
                                format!("Explorer · {} entries", app.directory_entries().len()),
                                Style::default().fg(Color::Green),
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
                footer_line(
                    "KEYS",
                    vec![
                        footer_key("↑↓"),
                        Span::raw(" Move "),
                        footer_key("Space"),
                        Span::raw(" Sel "),
                        footer_key("a"),
                        Span::raw(" Safe "),
                        footer_key("r"),
                        Span::raw(" Reload "),
                        footer_key("f"),
                        Span::raw(" Full "),
                        footer_key("q"),
                        Span::raw(" Quit"),
                    ],
                ),
            ]
        }
        Mode::Confirming => {
            if let Some(target) = &app.pending_explorer_delete {
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
            vec![
                footer_line(
                    "STATUS",
                    vec![Span::styled(
                        if app.explorer_deleting {
                            "Moving selected item to Trash…".to_owned()
                        } else {
                            format!("Cleaning {completed}/{total} targets")
                        },
                        Style::default().fg(Color::Magenta),
                    )],
                ),
                footer_line(
                    "PROGRESS",
                    vec![Span::styled(
                        format!("{percent}% · background operation"),
                        Style::default().fg(Color::Gray),
                    )],
                ),
                footer_line("KEYS", vec![footer_key("q"), Span::raw(" Quit")]),
            ]
        }
    }
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
