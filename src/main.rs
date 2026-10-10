mod action;
mod brew;
mod brewfile;
mod catalog;
mod effect;
mod exec;
mod input;
mod registry;
mod runtime;
mod self_update;
mod state;
mod theme;
mod ui;
mod update;

#[cfg(test)]
mod tests;

use anyhow::Result;
use crossterm::{
    event, execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use runtime::run_app;
use std::io;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut brewfile_path: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--file" => {
                brewfile_path = args.get(i + 1).cloned();
                i += 1;
            }
            "-v" | "--version" => {
                println!("lazybrew {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("lazybrew - a lazygit-style TUI for Homebrew");
                println!("Usage: lazybrew [-v] [-f <brewfile-path-or-url>]");
                return Ok(());
            }
            other => {
                eprintln!("unknown flag: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    execute!(stdout, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, brewfile_path);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    execute!(terminal.backend_mut(), event::DisableMouseCapture)?;
    terminal.show_cursor()?;
    res
}
