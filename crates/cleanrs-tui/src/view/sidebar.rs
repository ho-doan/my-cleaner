//! Cleanup, version, storage, and explorer status sidebar.

use humansize::{format_size, DECIMAL};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::footer::shorten;
use crate::model::{App, Mode, UpdateState};

pub(crate) fn render_sidebar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
