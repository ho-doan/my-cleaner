//! View composition and top-level layout.

mod explorer;
mod footer;
mod global;
mod modals;
mod sidebar;
mod standalone;
mod targets;

pub(crate) use explorer::{render_directory, render_full_disk};
pub(crate) use footer::footer_lines;
pub(crate) use global::render_global_tools;
pub(crate) use modals::{render_confirmation_modal, render_info_modal};
pub(crate) use sidebar::render_sidebar;
pub(crate) use standalone::render_standalone_tools;
pub(crate) use targets::render_targets;

use humansize::{format_size, DECIMAL};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::model::{App, Mode};

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
        Mode::Reviewing if app.show_standalone_tools => {
            " cleanrs — standalone installs ".to_owned()
        }
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
    } else if app.show_standalone_tools {
        render_standalone_tools(frame, app, body[1]);
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
