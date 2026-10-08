//! The event loop driver.
//!
//! Owns the terminal drawing and the background receivers; converts their
//! events into [`Action`]s, folds them with [`crate::update::update`], and
//! dispatches the resulting [`Effect`]s back to new background threads.

use crate::action::Action;
use crate::effect::Effect;
use crate::exec::{self, CmdEvent};
use crate::state::{AppState, CatalogData, LoadResult, Section};
use crate::ui::render;
use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::prelude::*;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

pub fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    brewfile_path: Option<String>,
) -> Result<()> {
    let mut state = boot_state(&brewfile_path);
    let mut background = Background::new();

    loop {
        state.frame = state.frame.wrapping_add(1);
        terminal.draw(|f| render(f, &state))?;

        let mut actions = background.pump();
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => actions.push(Action::Key(key)),
                Event::Mouse(me) => actions.push(Action::Mouse(me)),
                _ => {}
            }
        }

        for action in actions {
            let effects = crate::update::update(&mut state, action);
            let quitting = effects.contains(&Effect::Quit);
            for effect in effects {
                background.dispatch(effect, &state);
            }
            if quitting {
                return Ok(());
            }
        }
    }
}

/// Seed the model: section list (Brewfile first with -f), Brewfile entries,
/// saved theme, and the initial apply.
fn boot_state(brewfile_path: &Option<String>) -> AppState {
    let mut sections = vec![
        Section::Installed,
        Section::Outdated,
        Section::Casks,
        Section::Leaves,
        Section::Catalog,
        Section::Services,
        Section::Taps,
    ];
    let mut entries = Vec::new();
    let mut have_entries = false;
    if let Some(path) = brewfile_path {
        match crate::brewfile::load(path) {
            Ok(e) => {
                sections.insert(0, Section::Brewfile);
                entries = e;
                have_entries = true;
            }
            Err(e) => eprintln!("warning: Brewfile not loaded: {e}"),
        }
    }
    let mut state = AppState {
        sections,
        brewfile_path: brewfile_path.clone(),
        theme: crate::theme::load(),
        ..AppState::default()
    };
    if have_entries {
        state.brewfile_entries = entries;
        state.refresh_brewfile();
    }
    state.apply_section();
    state
}

/// Owns the live receivers for every background worker. Effects spawn new
/// workers here; `pump` converts whatever arrived into `Action`s.
struct Background {
    load_rx: Option<mpsc::Receiver<LoadResult>>,
    catalog_rx: Option<mpsc::Receiver<CatalogData>>,
    cmd_rx: Option<mpsc::Receiver<CmdEvent>>,
}

impl Background {
    fn new() -> Self {
        Self {
            load_rx: Some(exec::spawn_load_thread()),
            catalog_rx: None,
            cmd_rx: None,
        }
    }

    /// Run one effect: spawn the matching background thread (stashing its
    /// receiver) or perform the tiny synchronous side effect (theme save).
    fn dispatch(&mut self, effect: Effect, state: &AppState) {
        match effect {
            Effect::RunBrew(commands) => self.cmd_rx = Some(exec::spawn_brew_thread(commands)),
            Effect::RunVulnScan(name) => self.cmd_rx = Some(exec::spawn_vuln_thread(name)),
            Effect::RunSelfUpdate { url, target, exe } => {
                self.cmd_rx = Some(exec::spawn_self_update_thread(url, target, exe));
            }
            Effect::SaveTheme(theme) => crate::theme::save(&theme),
            Effect::FetchCatalog => {
                self.catalog_rx = Some(exec::spawn_catalog_thread(state.packages.clone()));
            }
            Effect::Reload => self.load_rx = Some(exec::spawn_load_thread()),
            Effect::Quit => {}
        }
    }

    /// Drain every ready receiver into a batch of `Action`s.
    fn pump(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(rx) = &self.load_rx
            && let Ok(res) = rx.try_recv()
        {
            self.load_rx = None;
            actions.push(Action::InstalledLoaded(res));
        }
        if let Some(rx) = &self.catalog_rx
            && let Ok(res) = rx.try_recv()
        {
            self.catalog_rx = None;
            actions.push(Action::CatalogLoaded(res));
        }
        if let Some(rx) = &self.cmd_rx {
            let mut done = false;
            for ev in rx.try_iter() {
                match ev {
                    CmdEvent::Line(l) => actions.push(Action::CmdLine(l)),
                    CmdEvent::Vulns(name, list) => actions.push(Action::VulnsScanned(name, list)),
                    CmdEvent::VulnsMissing => actions.push(Action::VulnsMissing),
                    CmdEvent::Done(ok) => {
                        done = true;
                        actions.push(Action::CmdDone(ok));
                    }
                }
            }
            if done {
                self.cmd_rx = None;
            }
        }
        actions
    }
}
