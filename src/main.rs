mod app;
mod brew;
mod brewfile;
mod catalog;
mod input;
mod ui;

use anyhow::Result;
use app::{
    App, CmdEvent, Modal, ModalAction, Panel, Section, spawn_catalog_thread, spawn_load_thread,
};
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
    let args: Vec<String> = std::env::args().collect();
    let mut brewfile_path: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--file" => {
                brewfile_path = args.get(i + 1).cloned();
                i += 1;
            }
            "-h" | "--help" => {
                println!("lazybrew - a lazygit-style TUI for Homebrew");
                println!("Usage: lazybrew [-f <brewfile-path-or-url>]");
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
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, brewfile_path);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    brewfile_path: Option<String>,
) -> Result<()> {
    let load_rx = spawn_load_thread();

    let mut sections = vec![
        Section::Installed,
        Section::Outdated,
        Section::Casks,
        Section::Leaves,
        Section::Catalog,
        Section::Services,
        Section::Taps,
    ];
    if let Some(path) = &brewfile_path {
        match brewfile::load(path) {
            Ok(_) => sections.insert(0, Section::Brewfile),
            Err(e) => eprintln!("warning: Brewfile not loaded: {e}"),
        }
    }

    let mut app = App {
        sections,
        brewfile_entries: Vec::new(),
        brewfile: Vec::new(),
        packages: Vec::new(),
        filtered: Vec::new(),
        section_idx: 0,
        list_idx: 0,
        panel: Panel::Sidebar,
        leaves: Vec::new(),
        catalog_rx: None,
        services: Vec::new(),
        taps: Vec::new(),
        vulns: Default::default(),
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
    if let Some(path) = &brewfile_path
        && let Ok(entries) = brewfile::load(path)
    {
        app.brewfile_entries = entries;
        app.refresh_brewfile();
    }
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
        if let Some((pkgs, leaves, services, taps)) = loaded {
            app.packages = pkgs;
            app.leaves = leaves;
            app.services = services;
            app.taps = taps;
            app.load_rx = None;
            app.refresh_brewfile();
            // Refresh the catalog (installed status) whenever installed data refreshes
            app.catalog_rx = Some(spawn_catalog_thread(app.packages.clone()));
            app.apply_section();
        }

        let mut catalog_loaded = None;
        if let Some(rx) = &app.catalog_rx
            && let Ok(cat) = rx.try_recv()
        {
            catalog_loaded = Some(cat);
        }
        if let Some(cat) = catalog_loaded {
            app.catalog = cat;
            app.catalog_rx = None;
            app.refresh_brewfile();
            app.apply_section();
        }

        // Drain background command events
        let mut finished = false;
        if let Some(rx) = &app.cmd_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    CmdEvent::Line(l) => app.output.push(l),
                    CmdEvent::Vulns(name, list) => {
                        if list.is_empty() {
                            app.output.push(format!("{name}: no known vulnerabilities"));
                        } else {
                            app.output
                                .push(format!("{name}: {} vulnerabilities", list.len()));
                        }
                        app.vulns.insert(name, list);
                    }
                    CmdEvent::VulnsMissing => {
                        app.modal = Some(Modal {
                            text: "brew vulns is not installed.\n\nInstall it now? (y/n)".into(),
                            confirm: ModalAction::InstallVulns,
                        });
                    }
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
