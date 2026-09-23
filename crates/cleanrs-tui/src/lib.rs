//! Ratatui shell for cleanrs, organized as a small MVVM-style loop:
//! `model` owns state, `update` maps input to intents, `runtime` runs effects,
//! and `view` renders the current state.

mod model;
mod runtime;
mod update;
mod view;

use anyhow::Result;
use cleanrs_core::UpdateInfo;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;

pub enum RunOutcome {
    Exit,
    Upgrade(UpdateInfo),
}

pub fn run(current_version: &str) -> Result<RunOutcome> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let result = runtime::run_loop(&mut stdout, current_version);

    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;
    result
}
