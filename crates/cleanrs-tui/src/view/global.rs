//! Global package/tool inventory panel.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::model::App;

pub(crate) fn render_global_tools(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
