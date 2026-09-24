//! Full-disk inventory and directory explorer panels.

use humansize::{format_size, DECIMAL};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use super::targets::wrap_text;
use crate::model::App;

pub(crate) fn render_full_disk(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = Vec::new();
    let mut selected_index = None;
    items.push(ListItem::new(
        "Inventory only — › selected; Enter opens folder or file parent",
    ));

    if app.full_disk_scanning {
        items.push(ListItem::new(format!(
            "Scanning {} ({}/{}) — partial results update below…",
            app.full_disk_phase, app.full_disk_completed, app.full_disk_total
        )));
    }
    if let Some(error) = &app.full_disk_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    }
    if let Some(report) = &app.full_disk {
        let render_entry = |entry: &cleanrs_core::DiskScanEntry| {
            let (marker, color) = if entry.inaccessible_paths > 0 {
                ("PARTIAL", Color::Yellow)
            } else if entry.read_only {
                ("READONLY", Color::Red)
            } else if entry.path.is_dir() {
                ("DIR", Color::Cyan)
            } else {
                ("FILE", Color::Gray)
            };
            let volume = entry
                .volume_usage
                .as_ref()
                .map(|usage| {
                    format!(
                        "  · volume used {} / {}",
                        format_size(usage.used_bytes(), DECIMAL),
                        format_size(usage.total_bytes, DECIMAL)
                    )
                })
                .unwrap_or_default();
            let blocked = if entry.inaccessible_paths > 0 {
                format!("  · blocked {}", entry.inaccessible_paths)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}  ", format_size(entry.size_bytes, DECIMAL)),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("[{marker}]"), Style::default().fg(color)),
                Span::raw(format!("  {}{}{}", entry.path.display(), volume, blocked)),
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
        if report.inaccessible_paths > 0 {
            items.push(ListItem::new(Span::styled(
                "Blocked paths · press [p] to open permission settings, then [f] to rescan",
                Style::default().fg(Color::Yellow),
            )));
        }
    } else if !app.full_disk_scanning {
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
            let width = area.width.saturating_sub(6) as usize;
            items.extend(
                report
                    .entries
                    .iter()
                    .map(|entry| directory_list_item(entry, width)),
            );
            selected_index = Some(entry_start + app.directory_cursor);
        }
        if let Some(usage) = &report.volume_usage {
            items.push(ListItem::new(format!(
                "Volume used: {} / {} · visible child bytes may be lower when snapshots or blocked paths exist",
                format_size(usage.used_bytes(), DECIMAL),
                format_size(usage.total_bytes, DECIMAL)
            )));
        }
        items.push(ListItem::new(format!(
            "Blocked during recursive scan: {}",
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

fn directory_list_item(
    entry: &cleanrs_core::DirectoryScanEntry,
    width: usize,
) -> ListItem<'static> {
    let (marker, color) = if entry.inaccessible_paths > 0 {
        ("PARTIAL", Color::Yellow)
    } else if entry.read_only {
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
    let size = format_size(entry.size_bytes, DECIMAL);
    let path = entry.path.display().to_string();
    let reason = entry
        .suggestion
        .as_ref()
        .map(|suggestion| format!(" — {}", suggestion.reason))
        .unwrap_or_default();
    let blocked = if entry.inaccessible_paths > 0 {
        format!(" — blocked paths: {}", entry.inaccessible_paths)
    } else {
        String::new()
    };
    let full_text = format!("[{marker}]  {size}  {path}{reason}{blocked}");

    if full_text.chars().count() <= width {
        return ListItem::new(Line::from(vec![
            Span::styled(
                format!("[{marker}]"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {size}  "), Style::default().fg(Color::Gray)),
            Span::raw(path),
            Span::styled(reason, Style::default().fg(Color::Gray)),
        ]));
    }

    let header = format!("[{marker}]  {size}");
    let detail_indent = "  ↳ ";
    let detail_width = width.saturating_sub(detail_indent.chars().count()).max(1);
    let detail = format!("{path}{reason}");
    let mut lines = wrap_text(&header, width)
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))
        })
        .collect::<Vec<_>>();
    lines.extend(wrap_text(&detail, detail_width).into_iter().map(|line| {
        Line::from(vec![
            Span::styled(detail_indent, Style::default().fg(Color::DarkGray)),
            Span::raw(line),
        ])
    }));
    ListItem::new(lines)
}
