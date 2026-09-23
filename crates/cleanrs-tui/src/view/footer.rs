//! Status, progress, and keyboard shortcut footer.

use humansize::{format_size, DECIMAL};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::model::{App, Mode};

pub(crate) fn footer_lines(app: &App) -> Vec<Line<'static>> {
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

pub(crate) fn shorten(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let prefix = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{prefix}…")
}
