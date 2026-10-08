//! The pure update step: fold one [`Action`] into the [`AppState`] and
//! return the effects the runtime should run.
//!
//! This module is the heart of the Model–Update–View architecture. It never
//! spawns a thread, opens a channel, or touches the terminal — commands are
//! returned as [`Effect`]s and background results arrive as
//! [`Action`]s. Every transition here can be driven and asserted in tests.

use crate::action::Action;
use crate::effect::Effect;
use crate::input::completions;
use crate::state::{AppMode, AppState, Modal, ModalAction, Panel, Section, is_official_tap, modal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;

/// Fold `action` into `state` and return the effects to run. In debug/test
/// builds every transition is checked against [`AppState::check_invariants`]
/// so a structural break fails immediately at the source.
pub fn update(state: &mut AppState, action: Action) -> Vec<Effect> {
    let effects = update_inner(state, &action);
    #[cfg(debug_assertions)]
    if let Err(violation) = state.check_invariants() {
        panic!("state invariant violation after {action:?}: {violation}");
    }
    effects
}

fn update_inner(state: &mut AppState, action: &Action) -> Vec<Effect> {
    match action {
        Action::Key(key) => handle_key(state, *key),
        Action::Mouse(e) => handle_mouse(state, *e),
        Action::Tick => Vec::new(),
        Action::InstalledLoaded((pkgs, leaves, services, taps)) => {
            state.packages = pkgs.clone();
            state.leaves = leaves.clone();
            state.services = services.clone();
            state.taps = taps.clone();
            state.loading_packages = false;
            state.loading_catalog = true;
            state.refresh_brewfile();
            state.apply_section();
            // Installed status changed: refresh the (remote) catalog.
            vec![Effect::FetchCatalog]
        }
        Action::CatalogLoaded((cat, installs)) => {
            state.catalog = cat.clone();
            state.installs = installs.clone();
            state.loading_catalog = false;
            state.refresh_brewfile();
            state.apply_section();
            Vec::new()
        }
        Action::CmdLine(line) => {
            state.output.push(line.clone());
            Vec::new()
        }
        Action::CmdDone(ok) => {
            state.output.push(if *ok {
                "== done ==".into()
            } else {
                "== FAILED ==".into()
            });
            // Snap back to the tail so the result is always visible.
            state.output_offset = 0;
            state.cmd_active = false;
            // The command touched installed data: re-run the load chain.
            state.loading_packages = true;
            vec![Effect::Reload]
        }
        Action::VulnsScanned(name, list) => {
            if list.is_empty() {
                state
                    .output
                    .push(format!("{name}: no known vulnerabilities"));
            } else {
                state
                    .output
                    .push(format!("{name}: {} vulnerabilities", list.len()));
            }
            state.vulns.insert(name.clone(), list.clone());
            Vec::new()
        }
        Action::VulnsMissing => {
            state.mode = AppMode::Confirm(modal(
                "brew vulns is not installed.\n\nInstall it now? (y/n)",
                ModalAction::InstallVulns,
            ));
            Vec::new()
        }
        Action::Quit => vec![Effect::Quit],
    }
}

/// Translate a key event into state changes (and effects), by mode.
pub fn handle_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    // Exactly one mode is active; dispatch by it. Overlays handle their own
    // keys and never leak into list navigation (or vice versa).
    match state.mode {
        AppMode::Normal => normal_key(state, key),
        AppMode::Search => {
            search_key(state, key);
            Vec::new()
        }
        AppMode::Prompt => {
            prompt_key(state, key);
            Vec::new()
        }
        AppMode::Confirm(_) => confirm_key(state, key),
        AppMode::Menu(_) => menu_key(state, key),
        AppMode::ThemePicker(_) => picker_key(state, key),
        AppMode::Help => {
            state.mode = AppMode::Normal;
            Vec::new()
        }
    }
}

/// Translate a mouse event into navigation. Hit-testing matches `ui.rs`:
/// the sidebar is 24 columns wide, starting at row 2 (border) with section
/// rows from 3; the list table's data rows begin at row 4.
pub fn handle_mouse(state: &mut AppState, e: MouseEvent) -> Vec<Effect> {
    // Overlays and prompts own input; the pointer only navigates in Normal.
    if !matches!(state.mode, AppMode::Normal) {
        return Vec::new();
    }
    match e.kind {
        MouseEventKind::ScrollUp => {
            if state.panel == Panel::List {
                state.list_idx = state.list_idx.saturating_sub(1);
            }
        }
        MouseEventKind::ScrollDown => {
            if state.panel == Panel::List && state.list_idx + 1 < state.filtered.len() {
                state.list_idx += 1;
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let col = e.column as i32;
            let row = e.row as i32;
            if col < 24 {
                // Sidebar: pick the section under the click, then jump to the list.
                let idx = ((row - 3).clamp(0, state.sections.len() as i32 - 1)) as usize;
                if idx != state.section_idx {
                    state.section_idx = idx;
                    state.apply_section();
                }
                state.panel = Panel::List;
            } else if row >= 4 && !state.filtered.is_empty() {
                state.list_idx = ((row - 4) as usize).min(state.filtered.len() - 1);
                state.panel = Panel::List;
            }
        }
        _ => {}
    }
    Vec::new()
}

fn search_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => state.mode = AppMode::Normal,
        KeyCode::Esc => {
            state.search.clear();
            state.apply_section();
            state.mode = AppMode::Normal;
        }
        KeyCode::Backspace => {
            state.search.pop();
            state.apply_section();
        }
        KeyCode::Char(c) => {
            state.search.push(c);
            state.apply_section();
        }
        _ => {}
    }
}

fn prompt_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let name = state.prompt_buffer.trim().to_string();
            state.prompt_buffer.clear();
            state.mode = AppMode::Normal;
            if !name.is_empty() {
                let action = if state.sections[state.section_idx] == Section::Taps {
                    ModalAction::Tap(name.clone())
                } else {
                    ModalAction::Install(name.clone(), false)
                };
                state.mode = AppMode::Confirm(modal(format!("Install '{}'? (y/n)", name), action));
            }
        }
        KeyCode::Tab => {
            let cands = completions(state);
            if let Some(pos) = cands.iter().position(|c| *c == state.prompt_buffer) {
                state.prompt_buffer = cands[(pos + 1) % cands.len()].clone();
            } else if let Some(first) = cands.first() {
                state.prompt_buffer = first.clone();
            }
        }
        KeyCode::Esc => {
            state.prompt_buffer.clear();
            state.mode = AppMode::Normal;
        }
        KeyCode::Backspace => {
            state.prompt_buffer.pop();
        }
        KeyCode::Char(c) => state.prompt_buffer.push(c),
        _ => {}
    }
}

fn confirm_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    let Some(modal) = state.take_modal() else {
        return Vec::new();
    };
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => modal_effects(state, &modal),
        _ => Vec::new(),
    }
}

fn menu_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    let last = crate::state::MENU_ACTIONS.len() - 1;
    let idx = match state.mode {
        AppMode::Menu(i) => i,
        _ => return Vec::new(),
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('x') => {
            state.mode = AppMode::Normal;
            Vec::new()
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.mode = AppMode::Menu((idx + 1) % crate::state::MENU_ACTIONS.len());
            Vec::new()
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.mode = AppMode::Menu(idx.checked_sub(1).unwrap_or(last));
            Vec::new()
        }
        KeyCode::Enter => {
            state.mode = AppMode::Normal;
            menu_effects(state, idx)
        }
        KeyCode::Char(c) => {
            state.mode = AppMode::Normal;
            match c {
                'u' => menu_effects(state, 0),
                'R' => menu_effects(state, 1),
                'r' => menu_effects(state, 2),
                'i' => menu_effects(state, 3),
                'd' => menu_effects(state, 4),
                'p' => menu_effects(state, 5),
                'o' => menu_effects(state, 6),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn picker_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    let pick = match state.mode {
        AppMode::ThemePicker(i) => i,
        _ => return Vec::new(),
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('t') => {
            state.mode = AppMode::Normal;
            Vec::new()
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.mode = AppMode::ThemePicker((pick + 1) % crate::theme::THEMES.len());
            Vec::new()
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.mode = AppMode::ThemePicker(
                pick.checked_sub(1)
                    .unwrap_or(crate::theme::THEMES.len() - 1),
            );
            Vec::new()
        }
        KeyCode::Enter => {
            let chosen = crate::theme::THEMES[pick];
            state.theme = chosen;
            state.mode = AppMode::Normal;
            vec![Effect::SaveTheme(chosen)]
        }
        _ => Vec::new(),
    }
}

/// Build a `brew verb [--cask] name` command vector.
fn cmd(verb: &str, cask: bool, name: &str) -> Vec<String> {
    let mut a = vec![verb.into()];
    if cask {
        a.push("--cask".into());
    }
    a.push(name.into());
    a
}

/// Stage a brew command: mark the command runner active, clear the output
/// pane, record the `$ brew …` prompt line, and hand the command back to the
/// runtime as an effect.
fn brew_effect(state: &mut AppState, args: &[String]) -> Vec<Effect> {
    state.cmd_active = true;
    state.output.clear();
    state.output_offset = 0;
    state.output.push(format!("$ brew {}", args.join(" ")));
    vec![Effect::RunBrew(vec![args.to_vec()])]
}

fn install_confirm_text(p: &crate::brew::Package) -> String {
    match p.tap.as_deref().filter(|t| !is_official_tap(t)) {
        Some(tap) => format!("Install '{}' from tap '{tap}' (unverified)? (y/n)", p.name),
        None => format!("Install '{}'? (y/n)", p.name),
    }
}

fn normal_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Char('/') => {
            state.mode = AppMode::Search;
            Vec::new()
        }
        KeyCode::Char('I') => {
            if state.sections[state.section_idx] == Section::Brewfile {
                let missing = state.brewfile_missing();
                state.mode = AppMode::Confirm(modal(
                    format!("Install {missing} missing Brewfile packages? (y/n)"),
                    ModalAction::BrewfileInstall,
                ));
            } else {
                // Explicit "type the name" install.
                state.prompt_buffer.clear();
                state.mode = AppMode::Prompt;
            }
            Vec::new()
        }
        KeyCode::Char('R') => {
            if state.sections[state.section_idx] == Section::Brewfile {
                state.mode = AppMode::Confirm(modal(
                    format!(
                        "Remove all {} Brewfile packages? (y/n)",
                        state.brewfile.len()
                    ),
                    ModalAction::BrewfileRemove,
                ));
            }
            Vec::new()
        }
        KeyCode::Char('v') => {
            if let Some(p) = state.selected().cloned()
                && !p.cask
                && state.sections[state.section_idx] != Section::Services
            {
                state.cmd_active = true;
                state.output.clear();
                state.output.push(format!("$ brew vulns {}", p.name));
                vec![Effect::RunVulnScan(p.name)]
            } else {
                state
                    .output
                    .push("vuln scan only available for formulae".into());
                Vec::new()
            }
        }
        KeyCode::Char('s') => {
            if state.sections[state.section_idx] == Section::Services
                && let Some(p) = state.selected().cloned()
            {
                let running = p.service_status.as_deref() == Some("started");
                let verb = if running { "stop" } else { "start" };
                brew_effect(state, &["services".into(), verb.into(), p.name])
            } else {
                Vec::new()
            }
        }
        KeyCode::Char('S') => {
            state.sort = state.sort.next();
            state.apply_section();
            state.output.push(format!("sort: {}", state.sort.label()));
            Vec::new()
        }
        KeyCode::Char('D') => brew_effect(state, &["doctor".into()]),
        KeyCode::Char('C') => brew_effect(state, &["config".into()]),
        KeyCode::Char('W') => {
            let target = crate::self_update::target_triple();
            let url = crate::self_update::download_url(target);
            let exe = std::env::current_exe().unwrap_or_else(|_| "lazybrew".into());
            state.mode = AppMode::Confirm(modal(
                crate::self_update::plan_text(&url, &exe),
                ModalAction::SelfUpdate,
            ));
            Vec::new()
        }
        KeyCode::Char('B') => {
            if let Some(path) = state.brewfile_path.clone() {
                brew_effect(
                    state,
                    &["bundle".into(), "check".into(), format!("--file={path}")],
                )
            } else {
                state
                    .output
                    .push("no Brewfile loaded — start lazybrew with -f <path-or-url>".into());
                Vec::new()
            }
        }
        KeyCode::Char('t') => {
            state.mode = AppMode::ThemePicker(crate::theme::index_of(&state.theme));
            Vec::new()
        }
        KeyCode::Char('x') => {
            if state.selected().is_some() {
                state.mode = AppMode::Menu(0);
            }
            Vec::new()
        }
        KeyCode::Char('?') => {
            state.mode = AppMode::Help;
            Vec::new()
        }
        KeyCode::Char('e') => {
            let home = dirs::home_dir().unwrap_or_else(|| ".".into());
            let path = home.join("Brewfile");
            let fx = brew_effect(
                state,
                &[
                    "bundle".into(),
                    "dump".into(),
                    "--force".into(),
                    format!("--file={}", path.display()),
                ],
            );
            state
                .output
                .push(format!("Brewfile written to {}", path.display()));
            fx
        }
        KeyCode::Char('u') => {
            if let Some(p) = state.selected().cloned() {
                state.mode = AppMode::Confirm(modal(
                    format!("Upgrade '{}'? (y/n)", p.name),
                    ModalAction::Upgrade(p.name, p.cask),
                ));
            }
            Vec::new()
        }
        KeyCode::Char('r') => {
            if state.sections[state.section_idx] == Section::Taps {
                if let Some(p) = state.selected().cloned() {
                    state.mode = AppMode::Confirm(modal(
                        format!("Untap '{}'? (y/n)", p.name),
                        ModalAction::Untap(p.name),
                    ));
                }
            } else if let Some(p) = state.selected().cloned() {
                state.mode = AppMode::Confirm(modal(
                    format!("Remove '{}'? (y/n)", p.name),
                    ModalAction::Remove(p.name, p.cask),
                ));
            }
            Vec::new()
        }
        KeyCode::Char('A') => {
            state.mode = AppMode::Confirm(modal(
                format!(
                    "Upgrade all {} outdated packages? (y/n)",
                    state.count_for(Section::Outdated)
                ),
                ModalAction::UpgradeAll,
            ));
            Vec::new()
        }
        KeyCode::Char('K') => {
            state.mode = AppMode::Confirm(modal("Run 'brew cleanup'? (y/n)", ModalAction::Cleanup));
            Vec::new()
        }
        KeyCode::Char('n') => {
            state.mode = AppMode::Confirm(modal(
                "Run 'brew autoremove'? (y/n)",
                ModalAction::Autoremove,
            ));
            Vec::new()
        }
        KeyCode::Char('U') => {
            state.mode = AppMode::Confirm(modal("Run 'brew update'? (y/n)", ModalAction::Update));
            Vec::new()
        }
        KeyCode::Char('i') => {
            if state.sections[state.section_idx] == Section::Taps {
                // Adding a NEW tap still needs a typed name.
                state.prompt_buffer.clear();
                state.mode = AppMode::Prompt;
            } else if let Some(p) = state.selected().cloned() {
                if p.installed_version.is_some() || p.service_status.is_some() {
                    let detail = p.installed_version.unwrap_or_else(|| "installed".into());
                    state.output.push(format!(
                        "{} is already installed ({}) — u upgrades, r removes",
                        p.name, detail
                    ));
                } else {
                    state.mode = AppMode::Confirm(modal(
                        install_confirm_text(&p),
                        ModalAction::Install(p.name, p.cask),
                    ));
                }
            } else {
                // Nothing selected: fall back to typing a name.
                state.prompt_buffer.clear();
                state.mode = AppMode::Prompt;
            }
            Vec::new()
        }
        KeyCode::Char('q') => vec![Effect::Quit],
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
            state.panel = match state.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
            Vec::new()
        }
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
            state.panel = match state.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
            Vec::new()
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match state.panel {
                Panel::Sidebar => {
                    state.section_idx = (state.section_idx + 1) % state.sections.len();
                    state.apply_section();
                }
                Panel::List => {
                    if state.list_idx + 1 < state.filtered.len() {
                        state.list_idx += 1;
                    }
                }
            }
            Vec::new()
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match state.panel {
                Panel::Sidebar => {
                    state.section_idx = state
                        .section_idx
                        .checked_sub(1)
                        .unwrap_or(state.sections.len() - 1);
                    state.apply_section();
                }
                Panel::List => {
                    state.list_idx = state.list_idx.saturating_sub(1);
                }
            }
            Vec::new()
        }
        KeyCode::Enter => {
            if state.panel == Panel::Sidebar {
                state.panel = Panel::List;
            }
            Vec::new()
        }
        KeyCode::Esc => {
            if !state.search.is_empty() {
                state.search.clear();
                state.apply_section();
            }
            Vec::new()
        }
        KeyCode::PageUp => {
            if !state.output.is_empty() {
                state.output_offset += 1;
            }
            Vec::new()
        }
        KeyCode::PageDown => {
            state.output_offset = state.output_offset.saturating_sub(1);
            Vec::new()
        }
        KeyCode::Char('g') => {
            if state.panel == Panel::List {
                state.list_idx = 0;
            } else {
                state.section_idx = 0;
                state.apply_section();
            }
            Vec::new()
        }
        KeyCode::Char('G') => {
            if state.panel == Panel::List {
                state.list_idx = state.filtered.len().saturating_sub(1);
            } else {
                state.section_idx = state.sections.len() - 1;
                state.apply_section();
            }
            Vec::new()
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => vec![Effect::Quit],
        _ => Vec::new(),
    }
}

/// A confirmed dialog becomes concrete commands (or a self-update request)
/// plus the `$ …` header lines the old spawn path printed.
fn modal_effects(state: &mut AppState, modal: &Modal) -> Vec<Effect> {
    // Self-update does not go through brew; swap in the newest release binary.
    if matches!(modal.confirm, ModalAction::SelfUpdate) {
        let target = crate::self_update::target_triple();
        let url = crate::self_update::download_url(target);
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lazybrew"));
        state.cmd_active = true;
        state.output.clear();
        state.output_offset = 0;
        state
            .output
            .push(format!("$ curl -fsSL -o <tmp tarball> {url}"));
        state.output.push(format!("$ install to {}", exe.display()));
        return vec![Effect::RunSelfUpdate {
            url,
            target: target.to_string(),
            exe,
        }];
    }

    let commands: Vec<Vec<String>> = match &modal.confirm {
        ModalAction::Upgrade(name, cask) => vec![cmd("upgrade", *cask, name)],
        ModalAction::Reinstall(name, cask) => vec![cmd("reinstall", *cask, name)],
        ModalAction::Remove(name, cask) => vec![cmd("uninstall", *cask, name)],
        ModalAction::Install(name, cask) => vec![cmd("install", *cask, name)],
        ModalAction::Update => vec![vec!["update".into()]],
        ModalAction::UpgradeAll => vec![vec!["upgrade".into()]],
        ModalAction::Cleanup => vec![vec!["cleanup".into()]],
        ModalAction::Autoremove => vec![vec!["autoremove".into()]],
        ModalAction::InstallVulns => {
            vec![vec![
                "install".into(),
                "homebrew/brew-vulns/brew-vulns".into(),
            ]]
        }
        ModalAction::BrewfileInstall | ModalAction::BrewfileRemove => {
            brewfile_commands(state, matches!(modal.confirm, ModalAction::BrewfileInstall))
        }
        ModalAction::Tap(name) => vec![vec!["tap".into(), name.clone()]],
        ModalAction::Untap(name) => vec![vec!["untap".into(), name.clone()]],
        ModalAction::SelfUpdate => unreachable!("handled before the brew match"),
    };
    state.output.clear();
    state.output_offset = 0;
    for c in &commands {
        state.output.push(format!("$ brew {}", c.join(" ")));
    }
    state.cmd_active = true;
    vec![Effect::RunBrew(commands)]
}

/// Build the command sequence for batch Brewfile install/remove.
/// Taps are handled first on install so formulae resolve.
fn brewfile_commands(state: &AppState, install: bool) -> Vec<Vec<String>> {
    let taps: Vec<String> = state
        .brewfile_entries
        .iter()
        .filter(|e| e.kind == crate::brewfile::EntryKind::Tap)
        .map(|e| e.name.clone())
        .collect();
    let formulae: Vec<String> = state
        .brewfile
        .iter()
        .filter(|p| !p.cask)
        .map(|p| p.name.clone())
        .collect();
    let casks: Vec<String> = state
        .brewfile
        .iter()
        .filter(|p| p.cask)
        .map(|p| p.name.clone())
        .collect();

    let mut commands: Vec<Vec<String>> = Vec::new();
    if install {
        for t in &taps {
            commands.push(vec!["tap".into(), t.clone()]);
        }
        if !formulae.is_empty() {
            let mut a = vec!["install".into()];
            a.extend(formulae);
            commands.push(a);
        }
        if !casks.is_empty() {
            let mut a = vec!["install".into(), "--cask".into()];
            a.extend(casks);
            commands.push(a);
        }
    } else {
        if !formulae.is_empty() {
            let mut a = vec!["uninstall".into()];
            a.extend(formulae);
            commands.push(a);
        }
        if !casks.is_empty() {
            let mut a = vec!["uninstall".into(), "--cask".into()];
            a.extend(casks);
            commands.push(a);
        }
    }
    commands
}

/// The action menu for the selected package: indices match MENU_ACTIONS.
/// Items 0-2 confirm; 3-6 run brew immediately.
fn menu_effects(state: &mut AppState, idx: usize) -> Vec<Effect> {
    let Some(p) = state.selected().cloned() else {
        return Vec::new();
    };
    match idx {
        0 => {
            state.mode = AppMode::Confirm(modal(
                format!("Upgrade '{}'? (y/n)", p.name),
                ModalAction::Upgrade(p.name, p.cask),
            ));
            Vec::new()
        }
        1 => {
            state.mode = AppMode::Confirm(modal(
                format!("Reinstall '{}'? (y/n)", p.name),
                ModalAction::Reinstall(p.name, p.cask),
            ));
            Vec::new()
        }
        2 => {
            state.mode = AppMode::Confirm(modal(
                format!("Remove '{}'? (y/n)", p.name),
                ModalAction::Remove(p.name, p.cask),
            ));
            Vec::new()
        }
        3 => brew_effect(state, &["info".into(), p.name]),
        4 => brew_effect(state, &["deps".into(), p.name]),
        5 => {
            let verb = if p.pinned { "unpin" } else { "pin" };
            brew_effect(state, &[verb.into(), p.name])
        }
        6 => brew_effect(state, &["home".into(), p.name]),
        _ => Vec::new(),
    }
}
