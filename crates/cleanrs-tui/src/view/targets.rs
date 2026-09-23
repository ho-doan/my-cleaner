//! Cleanup target list and special review panels.

use cleanrs_core::RiskLevel;
use humansize::{format_size, DECIMAL};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::{App, TargetRow};

pub(crate) fn render_targets(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
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
