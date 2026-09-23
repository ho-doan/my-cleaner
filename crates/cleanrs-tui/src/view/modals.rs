//! Confirmation and informational overlays.

use humansize::{format_size, DECIMAL};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::model::App;

pub(crate) fn render_confirmation_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(78, 48, frame.area());
    let destructive = app.pending_empty_trash
        || app.pending_explorer_delete.is_some()
        || app.pending_global_uninstall.is_some()
        || app.pending_standalone_uninstall.is_some();
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
    } else if let Some(tool) = &app.pending_standalone_uninstall {
        lines.push(Line::from(Span::styled(
            "Remove standalone binary",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "{} · {}",
            tool.name,
            tool.path.display()
        )));
        lines.push(Line::from(format!(
            "Estimated size: {}",
            format_size(tool.size_bytes, DECIMAL)
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Only this exact file or symlink will move to Trash.",
        ));
        lines.push(Line::from(
            "The scan cannot prove installer provenance; review the path before continuing.",
        ));
        lines.push(Line::from("Press [y] to confirm or [n]/[esc] to cancel."));
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

pub(crate) fn render_info_modal(frame: &mut Frame, app: &App) {
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

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
