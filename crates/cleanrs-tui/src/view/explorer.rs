//! Full-disk inventory and directory explorer panels.

use humansize::{format_size, DECIMAL};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::App;

pub(crate) fn render_full_disk(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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

pub(crate) fn render_directory(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
