mod app;
mod brew;
mod catalog;
mod input;
mod ui;

use anyhow::Result;
use app::{App, CmdEvent, Panel, spawn_load_thread};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use input::{KeyFlow, handle_key};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;
use ui::render;

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let load_rx = spawn_load_thread();

    let mut app = App {
        packages: Vec::new(),
        filtered: Vec::new(),
        section_idx: 0,
        list_idx: 0,
        panel: Panel::Sidebar,
        leaves: Vec::new(),
        catalog: Vec::new(),
        search: String::new(),
        searching: false,
        installing: false,
        install_input: String::new(),
        output: Vec::new(),
        cmd_rx: None,
        modal: None,
        menu: None,
        frame: 0,
        load_rx: Some(load_rx),
        help: false,
    };
    app.apply_section();

    loop {
        app.frame = app.frame.wrapping_add(1);
        terminal.draw(|f| render(f, &app))?;

        let mut loaded = None;
        if let Some(rx) = &app.load_rx
            && let Ok(res) = rx.try_recv()
        {
            loaded = Some(res);
        }
        if let Some((pkgs, leaves, catalog)) = loaded {
            app.packages = pkgs;
            app.leaves = leaves;
            app.catalog = catalog;
            app.apply_section();
            app.load_rx = None;
        }

        // Drain background command events
        let mut finished = false;
        if let Some(rx) = &app.cmd_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    CmdEvent::Line(l) => app.output.push(l),
                    CmdEvent::Done(ok) => {
                        app.output.push(if ok {
                            "== done ==".into()
                        } else {
                            "== FAILED ==".into()
                        });
                        finished = true;
                    }
                }
            }
        }
        if finished {
            app.cmd_rx = None;
            app.load_rx = Some(spawn_load_thread());
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match handle_key(&mut app, key) {
                KeyFlow::Quit => return Ok(()),
                KeyFlow::Continue => {}
            }
        }
    }
}
