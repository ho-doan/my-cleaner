use anyhow::Result;
use cleanrs_core::{
    all_cleaners, full_disk_scan, scan_cleaner, Category, CleanResult, CleanTarget, CleanerScan,
    FullDiskScan, RiskLevel,
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
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    io::{self, Stdout},
    path::Path,
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
    Continue,
    FullDisk,
    Quit,
    Rescan,
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

fn run_loop(stdout: &mut Stdout) -> Result<()> {
    let (expected_scans, mut receiver) = start_scan();
    let mut full_disk_receiver: Option<Receiver<Result<FullDiskScan, String>>> = None;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(expected_scans);
    let result = loop {
        receive_scans(&mut app, &receiver);
        receive_full_disk_scan(&mut app, &mut full_disk_receiver);
        terminal.draw(|frame| render(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match handle_key(&mut app, key) {
                        KeyAction::Quit => break Ok(()),
                        KeyAction::FullDisk => {
                            if !app.full_disk_scanning {
                                app.full_disk_scanning = true;
                                app.full_disk_error = None;
                                app.show_full_disk = true;
                                full_disk_receiver = Some(start_full_disk_scan());
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

fn handle_key(app: &mut App, key: KeyEvent) -> KeyAction {
    if key.code == KeyCode::Char('q') {
        return KeyAction::Quit;
    }

    match app.mode {
        Mode::Scanning => {}
        Mode::Reviewing => match key.code {
            KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
            KeyCode::Char(' ') => app.toggle_current(),
            KeyCode::Char('a') => app.select_all_safe(),
            KeyCode::Char('d') => app.dry_run = !app.dry_run,
            KeyCode::Char('f') => return KeyAction::FullDisk,
            KeyCode::Char('b') if app.show_full_disk => app.show_full_disk = false,
            KeyCode::Char('r') => return KeyAction::Rescan,
            KeyCode::Enter if app.selected_count() > 0 => {
                app.confirm_text.clear();
                app.mode = Mode::Confirming;
            }
            _ => {}
        },
        Mode::Confirming => {
            if handle_confirmation(app, key) {
                return KeyAction::Rescan;
            }
        }
        Mode::Cleaning => {}
    }
    KeyAction::Continue
}

fn handle_confirmation(app: &mut App, key: KeyEvent) -> bool {
    let manual = app.has_manual_selection();
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.confirm_text.clear();
            app.mode = Mode::Reviewing;
        }
        KeyCode::Char('y') if !manual => execute_selected(app),
        KeyCode::Enter if manual && app.confirm_text == "FORCE" => execute_selected(app),
        KeyCode::Char(character) if manual && !app.dry_run => app.confirm_text.push(character),
        KeyCode::Backspace if manual && !app.dry_run => {
            app.confirm_text.pop();
        }
        _ => {}
    }
    matches!(app.mode, Mode::Cleaning)
}

fn execute_selected(app: &mut App) {
    let dry_run = app.dry_run;
    app.mode = if dry_run {
        Mode::Reviewing
    } else {
        Mode::Cleaning
    };
    app.results.clear();
    app.errors.clear();
    let disk_before = app.disk.or_else(read_disk_usage);
    let cleaners = all_cleaners();
    for row in app.rows.iter().filter(|row| row.selected) {
        let Some(cleaner) = cleaners
            .iter()
            .find(|cleaner| cleaner.id() == row.cleaner_id)
        else {
            app.errors
                .push(format!("unknown cleaner {}", row.cleaner_id));
            continue;
        };
        match cleaner.clean(&row.target, dry_run) {
            Ok(result) => app.results.push(result),
            Err(error) => app.errors.push(format!("{}: {error:#}", row.cleaner_id)),
        }
    }
    let action = if dry_run { "Previewed" } else { "Cleaned" };
    let estimated_freed = app
        .results
        .iter()
        .map(|result| result.expected_freed_bytes)
        .sum::<u64>();
    let disk_after = read_disk_usage().or(disk_before);
    let actual_free_change = match (disk_before, disk_after) {
        (Some(before), Some(after)) => after.free_bytes.saturating_sub(before.free_bytes),
        _ => 0,
    };
    app.disk = disk_after;
    let follow_up = if dry_run {
        "no files changed; targets kept"
    } else {
        "scan refreshed"
    };
    app.last_action = Some(format!(
        "{action} {} target(s), estimated {} freed, actual free change {}; {} error(s); {follow_up}",
        app.results.len(),
        format_size(estimated_freed, DECIMAL),
        format_size(actual_free_change, DECIMAL),
        app.errors.len()
    ));
}

fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(4),
        ])
        .split(frame.area());

    let title = match app.mode {
        Mode::Scanning => format!(
            " cleanrs — scanning {}/{} cleaners ",
            app.received_scans, app.expected_scans
        ),
        Mode::Reviewing => " cleanrs — review targets ".to_owned(),
        Mode::Confirming => " cleanrs — confirm ".to_owned(),
        Mode::Cleaning => " cleanrs — cleaning ".to_owned(),
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
    frame.render_widget(
        Paragraph::new(vec![Line::from(title), Line::from(disk_line)])
            .block(Block::default().borders(Borders::ALL)),
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
        render_full_disk(frame, app, body[1]);
    } else {
        render_targets(frame, app, body[1]);
    }

    let footer = footer_text(app);
    frame.render_widget(
        Paragraph::new(footer)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL)),
        vertical[2],
    );
}

fn render_sidebar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut categories = std::collections::BTreeMap::new();
    for row in &app.rows {
        *categories
            .entry(format!("{:?}", row.category))
            .or_insert(0usize) += 1;
    }
    let mut lines = vec![Line::from(Span::styled(
        "Categories",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    for (category, count) in categories {
        lines.push(Line::from(format!("{category}: {count}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!("Selected: {}", app.selected_count())));
    lines.push(Line::from(format!(
        "Size: {}",
        format_size(app.selected_size(), DECIMAL)
    )));
    lines.push(Line::from(format!(
        "Mode: {}",
        if app.dry_run { "dry-run" } else { "execute" }
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Full disk",
        Style::default().add_modifier(Modifier::BOLD),
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
        lines.push(Line::from(format!(
            "Top root: {}",
            format_size(root_total, DECIMAL)
        )));
        lines.push(Line::from(format!(
            "Top HOME: {}",
            format_size(home_total, DECIMAL)
        )));
        lines.push(Line::from(format!(
            "Blocked: {}",
            report.inaccessible_paths
        )));
    } else {
        lines.push(Line::from("Press [f] to scan"));
    }
    if let Some(action) = &app.last_action {
        lines.push(Line::from(""));
        lines.push(Line::from(action.as_str()));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_full_disk(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = Vec::new();
    items.push(ListItem::new("Read-only inventory — no delete targets"));

    if app.full_disk_scanning {
        items.push(ListItem::new("Scanning root disk and HOME in background…"));
    } else if let Some(error) = &app.full_disk_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.full_disk {
        items.push(ListItem::new("Largest root entries:"));
        items.extend(report.root_entries.iter().map(|entry| {
            ListItem::new(format!(
                "{}  [{}] {}",
                format_size(entry.size_bytes, DECIMAL),
                if entry.read_only {
                    "READONLY"
                } else {
                    "deletable"
                },
                entry.path.display()
            ))
        }));
        items.push(ListItem::new(""));
        items.push(ListItem::new("Largest HOME entries:"));
        items.extend(report.home_entries.iter().map(|entry| {
            ListItem::new(format!(
                "{}  [{}] {}",
                format_size(entry.size_bytes, DECIMAL),
                if entry.read_only {
                    "READONLY"
                } else {
                    "deletable"
                },
                entry.path.display()
            ))
        }));
        items.push(ListItem::new("Read-only system/mount paths:"));
        items.extend(
            report
                .readonly_paths
                .iter()
                .map(|path| ListItem::new(format!("[READONLY] {}", path.display()))),
        );
        items.push(ListItem::new(format!(
            "Read-only paths: {}  |  Inaccessible: {}",
            report.readonly_paths.len(),
            report.inaccessible_paths
        )));
    } else {
        items.push(ListItem::new("Press [f] to start a full-disk scan."));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Full disk"))
        .highlight_style(Style::default().bg(Color::DarkGray));
    frame.render_widget(list, area);
}

fn render_targets(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items = if app.rows.is_empty() {
        vec![ListItem::new("No reclaimable targets found.")]
    } else {
        app.rows
            .iter()
            .map(|row| {
                let checkbox = if row.selected { "[x]" } else { "[ ]" };
                let risk_style = match row.risk {
                    RiskLevel::Safe => Style::default().fg(Color::Green),
                    RiskLevel::Caution => Style::default().fg(Color::Yellow),
                    RiskLevel::Manual => Style::default().fg(Color::Red),
                };
                ListItem::new(Line::from(vec![
                    Span::raw(format!(
                        "{checkbox} {} — {} ({}, ",
                        row.cleaner_name,
                        row.target.description,
                        format_size(row.target.size_bytes, DECIMAL),
                    )),
                    Span::styled(format!("{:?}", row.risk), risk_style),
                    Span::raw(")"),
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
        .highlight_style(Style::default().bg(Color::DarkGray));
    frame.render_stateful_widget(list, area, &mut state);
}

fn footer_text(app: &App) -> String {
    match app.mode {
        Mode::Scanning => "Scanning in background…  [q] quit".to_owned(),
        Mode::Reviewing => {
            if app.show_full_disk {
                return "Full-disk inventory is read-only. [f] rescan  [b] back to targets  [q] quit"
                    .to_owned();
            }
            let action = app.last_action.as_deref().unwrap_or("Ready for review");
            format!(
                "{action}\n[↑/↓] move  [space] toggle  [a] select Safe  [d] dry-run  [r] reload  [f] full disk  [enter] continue  [q] quit"
            )
        }
        Mode::Confirming if app.has_manual_selection() && !app.dry_run => format!(
            "Docker/manual selected. Type FORCE then [enter] to execute, [n] cancel. Current: {}",
            app.confirm_text
        ),
        Mode::Confirming => format!(
            "{} {} selected target(s)? [y] {}  [n] cancel",
            if app.dry_run { "Preview" } else { "Execute" },
            app.selected_count(),
            if app.dry_run {
                "preview only; no files changed"
            } else {
                "yes"
            }
        ),
        Mode::Cleaning => "Working…".to_owned(),
    }
}
