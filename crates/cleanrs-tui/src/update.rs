//! Input-to-intent mapping and state transitions.

use crossterm::event::{KeyCode, KeyEvent};
use humansize::{format_size, DECIMAL};

use super::{
    model::{App, Mode},
    runtime::{
        start_cleaning, start_explorer_delete, start_global_uninstall, start_standalone_uninstall,
    },
};

pub(crate) enum KeyAction {
    ToggleAllowlist,
    BackDirectory,
    BackGlobalTools,
    BackStandaloneTools,
    Continue,
    DeleteFile,
    FullDisk,
    GlobalTools,
    StandaloneTools,
    OpenDirectory,
    Quit,
    Rescan,
    UninstallGlobal,
    UninstallStandalone,
    Upgrade,
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> KeyAction {
    if key.code == KeyCode::Char('q') {
        return KeyAction::Quit;
    }

    if app.info_overlay.is_some() {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('i')) {
            app.info_overlay = None;
        }
        return KeyAction::Continue;
    }

    if matches!(app.mode, Mode::Reviewing)
        && key.code == KeyCode::Char('u')
        && app.available_update().is_some()
    {
        return KeyAction::Upgrade;
    }

    match app.mode {
        Mode::Scanning => {}
        Mode::Reviewing => {
            if app.show_global_tools {
                if app.global_tools_scanning {
                    match key.code {
                        KeyCode::Char('b') => return KeyAction::BackGlobalTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => app.move_global_cursor(1),
                        KeyCode::Up | KeyCode::Char('k') => app.move_global_cursor(-1),
                        KeyCode::Enter | KeyCode::Char('x') => return KeyAction::UninstallGlobal,
                        KeyCode::Char('r') => return KeyAction::GlobalTools,
                        KeyCode::Char('s') => return KeyAction::StandaloneTools,
                        KeyCode::Char('b') => return KeyAction::BackGlobalTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                }
            } else if app.show_standalone_tools {
                if app.standalone_tools_scanning {
                    match key.code {
                        KeyCode::Char('b') => return KeyAction::BackStandaloneTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => app.move_standalone_cursor(1),
                        KeyCode::Up | KeyCode::Char('k') => app.move_standalone_cursor(-1),
                        KeyCode::Enter | KeyCode::Char('x') => {
                            return KeyAction::UninstallStandalone
                        }
                        KeyCode::Char('g') => return KeyAction::GlobalTools,
                        KeyCode::Char('r') => return KeyAction::StandaloneTools,
                        KeyCode::Char('b') => return KeyAction::BackStandaloneTools,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                }
            } else if app.show_full_disk {
                if app.directory_scanning {
                    match key.code {
                        KeyCode::Char('b') => return KeyAction::BackDirectory,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => app.move_directory_cursor(1),
                        KeyCode::Up | KeyCode::Char('k') => app.move_directory_cursor(-1),
                        KeyCode::Enter => return KeyAction::OpenDirectory,
                        KeyCode::Char('w') => return KeyAction::ToggleAllowlist,
                        KeyCode::Char('x') => return KeyAction::DeleteFile,
                        KeyCode::Char('b') => return KeyAction::BackDirectory,
                        KeyCode::Char('f') => return KeyAction::FullDisk,
                        _ => {}
                    }
                }
            } else {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
                    KeyCode::Char(' ') => app.toggle_current(),
                    KeyCode::Char('a') => app.select_all_safe(),
                    KeyCode::Char('t') => app.toggle_trash(),
                    KeyCode::Char('i') => {
                        if let Some(report) = &app.readonly_report {
                            let paths = report
                                .targets
                                .iter()
                                .map(|target| {
                                    format!(
                                        "{}  ·  {}",
                                        target.path.display(),
                                        format_size(target.size_bytes, DECIMAL)
                                    )
                                })
                                .collect::<Vec<_>>();
                            app.info_overlay = Some(format!(
                                "{}\n\n{}",
                                report.advice,
                                if paths.is_empty() {
                                    "No current read-only items reported.".to_owned()
                                } else {
                                    paths.join("\n")
                                }
                            ));
                        } else {
                            app.last_action =
                                Some("Read-only inventory is still scanning".to_owned());
                        }
                    }
                    KeyCode::Char('d') => app.dry_run = !app.dry_run,
                    KeyCode::Char('f') => return KeyAction::FullDisk,
                    KeyCode::Char('g') => return KeyAction::GlobalTools,
                    KeyCode::Char('s') => return KeyAction::StandaloneTools,
                    KeyCode::Char('r') => return KeyAction::Rescan,
                    KeyCode::Enter if app.trash_selected => {
                        app.confirm_text.clear();
                        app.pending_empty_trash = true;
                        app.mode = Mode::Confirming;
                    }
                    KeyCode::Enter if app.selected_count() > 0 => {
                        app.confirm_text.clear();
                        app.mode = Mode::Confirming;
                    }
                    _ => {}
                }
            }
        }
        Mode::Confirming => handle_confirmation(app, key),
        Mode::Cleaning => {}
    }
    KeyAction::Continue
}

pub(crate) fn handle_confirmation(app: &mut App, key: KeyEvent) {
    if app.pending_empty_trash {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_empty_trash = false;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Enter if app.confirm_text == "EMPTY TRASH" => {
                if app.has_manual_selection() {
                    app.pending_empty_trash = false;
                    app.confirm_text.clear();
                    app.mode = Mode::Reviewing;
                    app.last_action =
                        Some("Deselect Manual targets before confirming Empty Trash".to_owned());
                } else {
                    app.pending_empty_trash = false;
                    app.confirm_text.clear();
                    start_cleaning(app);
                }
            }
            KeyCode::Char(character) => app.confirm_text.push(character),
            KeyCode::Backspace => {
                app.confirm_text.pop();
            }
            _ => {}
        }
        return;
    }

    if app.pending_global_uninstall.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_global_uninstall = None;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Char('y') => start_global_uninstall(app),
            _ => {}
        }
        return;
    }

    if app.pending_standalone_uninstall.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_standalone_uninstall = None;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Char('y') => start_standalone_uninstall(app),
            _ => {}
        }
        return;
    }

    if app.pending_explorer_delete.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                app.pending_explorer_delete = None;
                app.confirm_text.clear();
                app.mode = Mode::Reviewing;
            }
            KeyCode::Char('y') => start_explorer_delete(app),
            _ => {}
        }
        return;
    }

    let manual = app.has_manual_selection();
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.confirm_text.clear();
            app.mode = Mode::Reviewing;
        }
        KeyCode::Char('y') if !manual => start_cleaning(app),
        KeyCode::Enter if manual && app.confirm_text == "FORCE" => start_cleaning(app),
        KeyCode::Char(character) if manual && !app.dry_run => app.confirm_text.push(character),
        KeyCode::Backspace if manual && !app.dry_run => {
            app.confirm_text.pop();
        }
        _ => {}
    }
}
