//! Pure-ish Ratatui rendering of the current view model.

use cleanrs_core::RiskLevel;
use humansize::{format_size, DECIMAL};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::model::{App, Mode, TargetRow, UpdateState};

pub(crate) fn render(frame: &mut Frame, app: &App) {
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
                "› selects · Enter opens folder/file parent",
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
        "Inventory only — › selected; Enter opens folder or file parent",
    ));

    if app.full_disk_scanning {
        items.push(ListItem::new("Scanning root disk and HOME in background…"));
    } else if let Some(error) = &app.full_disk_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.full_disk {
        let render_entry = |entry: &cleanrs_core::DiskScanEntry| {
            let (marker, color) = if entry.read_only {
                ("READONLY", Color::Red)
            } else if entry.path.is_dir() {
                ("DIR", Color::Cyan)
            } else {
                ("FILE", Color::Gray)
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
            "› selects · Enter opens folders · [w] allowlist · [x] Trash SUGGEST items",
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
                                    "Folder review · › selected   ",
                                    Style::default().fg(Color::Gray),
                                ),
                                footer_key("Enter"),
                                Span::raw(" Open   "),
                                footer_key("w"),
                                Span::raw(" Allowlist   "),
                                footer_key("x"),
                                Span::raw(" Trash SUGGEST only"),
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
                            app.last_action
                                .as_deref()
                                .map(|action| shorten(action, 58))
                                .unwrap_or(status),
                            Style::default().fg(
                                if app.last_action.is_some() || app.full_disk_scanning {
                                    Color::Yellow
                                } else {
                                    Color::Green
                                },
                            ),
                        )],
                    ),
                    footer_line(
                        "MODE",
                        vec![
                            Span::styled(
                                "Inventory only · › selected   ",
                                Style::default().fg(Color::Gray),
                            ),
                            footer_key("Enter"),
                            Span::raw(" Open/parent   "),
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
