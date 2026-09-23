//! Standalone/user-installed binary inventory panel.

use humansize::{format_size, DECIMAL};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use super::footer::shorten;
use crate::model::App;

pub(crate) fn render_standalone_tools(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items = vec![ListItem::new(
        "Standalone/user-installed · x/Enter review · exact file → Trash",
    )];
    let mut selected_index = None;

    if app.standalone_tools_scanning {
        items.push(ListItem::new(
            "Scanning user binary roots (direct entries only)…",
        ));
    } else if let Some(error) = &app.standalone_tools_error {
        items.push(ListItem::new(format!("Scan error: {error}")));
    } else if let Some(report) = &app.standalone_tools {
        if report.tools.is_empty() && report.errors.is_empty() {
            items.push(ListItem::new(
                "No standalone binaries found in the approved user roots.",
            ));
        }
        for error in &report.errors {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("[WARN] ", Style::default().fg(Color::Yellow)),
                Span::styled(error.as_str(), Style::default().fg(Color::Gray)),
            ])));
        }

        let tools_start = items.len();
        items.extend(report.tools.iter().map(|tool| {
            let (marker, color) = if tool.can_remove {
                ("REMOVE", Color::Green)
            } else {
                ("KEEP", Color::Red)
            };
            let item_kind = if tool.is_directory { "DIR" } else { "FILE" };
            let path = shorten(&tool.path.display().to_string(), 86);
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("[{marker}]"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {:<15}  ", tool.source_root),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        tool.name.as_str(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  ·  {item_kind}"),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        format!("  ·  {}", format_size(tool.size_bytes, DECIMAL)),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  path: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(path, Style::default().fg(Color::Gray)),
                ]),
            ])
        }));
        if !report.tools.is_empty() {
            selected_index = Some(tools_start + app.standalone_cursor);
        }
    } else {
        items.push(ListItem::new("Press [s] to scan standalone installs."));
    }

    let mut state = ListState::default();
    state.select(selected_index);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Standalone installs / remove "),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(38, 46, 56))
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, area, &mut state);
}
