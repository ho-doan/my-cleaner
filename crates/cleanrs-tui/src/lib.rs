use anyhow::Result;
use cleanrs_core::{
    all_cleaners, scan_cleaner, Category, CleanResult, CleanTarget, CleanerScan, RiskLevel,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use humansize::{format_size, DECIMAL};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    io::{self, Stdout},
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
    Done,
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
        }
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

fn run_loop(stdout: &mut Stdout) -> Result<()> {
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

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(expected_scans);
    let result = loop {
        receive_scans(&mut app, &receiver);
        terminal.draw(|frame| render(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && handle_key(&mut app, key) {
                    break Ok(());
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
                    app.mode = Mode::Reviewing;
                }
                break;
            }
        }
    }
    if app.received_scans == app.expected_scans && matches!(app.mode, Mode::Scanning) {
        app.mode = Mode::Reviewing;
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::Char('q') {
        return true;
    }

    match app.mode {
        Mode::Scanning => {}
        Mode::Reviewing => match key.code {
            KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
            KeyCode::Char(' ') => app.toggle_current(),
            KeyCode::Char('a') => app.select_all_safe(),
            KeyCode::Char('d') => app.dry_run = !app.dry_run,
            KeyCode::Enter if app.selected_count() > 0 => {
                app.confirm_text.clear();
                app.mode = Mode::Confirming;
            }
            _ => {}
        },
        Mode::Confirming => handle_confirmation(app, key),
        Mode::Cleaning => {}
        Mode::Done => {}
    }
    false
}

fn handle_confirmation(app: &mut App, key: KeyEvent) {
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
}

fn execute_selected(app: &mut App) {
    app.mode = Mode::Cleaning;
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
        match cleaner.clean(&row.target, app.dry_run) {
            Ok(result) => app.results.push(result),
            Err(error) => app.errors.push(format!("{}: {error:#}", row.cleaner_id)),
        }
    }
    app.mode = Mode::Done;
}

fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
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
        Mode::Done => " cleanrs — done ".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        vertical[0],
    );

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(24), Constraint::Percentage(76)])
        .split(vertical[1]);
    render_sidebar(frame, app, body[0]);
    render_targets(frame, app, body[1]);

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
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_targets(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items = if app.rows.is_empty() {
        vec![ListItem::new("No reclaimable targets found.")]
    } else {
        app.rows
            .iter()
            .map(|row| {
                let checkbox = if row.selected { "[x]" } else { "[ ]" };
                let label = format!(
                    "{checkbox} {} — {} ({}, {:?})",
                    row.cleaner_name,
                    row.target.description,
                    format_size(row.target.size_bytes, DECIMAL),
                    row.risk
                );
                ListItem::new(label)
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
            " [↑/↓] move  [space] toggle  [a] select Safe  [d] dry-run  [enter] continue  [q] quit"
                .to_owned()
        }
        Mode::Confirming if app.has_manual_selection() && !app.dry_run => format!(
            "Docker/manual selected. Type FORCE then [enter] to execute, [n] cancel. Current: {}",
            app.confirm_text
        ),
        Mode::Confirming => format!(
            "Execute {} selected target(s) in {} mode? [y] yes  [n] cancel",
            app.selected_count(),
            if app.dry_run { "dry-run" } else { "execute" }
        ),
        Mode::Cleaning => "Working…".to_owned(),
        Mode::Done => format!(
            "Completed {} operation(s), {} error(s). [q] quit",
            app.results.len(),
            app.errors.len()
        ),
    }
}
