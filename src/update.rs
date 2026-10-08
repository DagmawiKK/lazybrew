//! The pure update step: fold one [`Action`] into the [`AppState`] and
//! return the effects the runtime should run.
//!
//! This module is the heart of the Model–Update–View architecture. It must
//! stay free of threads, channels, and terminal I/O so every transition can
//! be driven and asserted in tests.

use crate::action::Action;
use crate::input::{KeyFlow, completions};
use crate::state::{AppMode, AppState, ModalAction, Panel, Section, is_official_tap, modal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

/// Fold `action` into `state` and return a flow hint (Quit when the user
/// asked to leave). Spawning commands etc. is deferred to the caller via the
/// old command plumbing for now; commit B replaces that with `Effect`s.
pub fn update(state: &mut AppState, action: Action) -> KeyFlow {
    match action {
        Action::Key(key) => handle_key(state, key),
        Action::Mouse(e) => handle_mouse(state, e),
        Action::Tick => KeyFlow::Continue,
        Action::InstalledLoaded(_)
        | Action::CatalogLoaded(_)
        | Action::CmdLine(_)
        | Action::CmdDone(_)
        | Action::VulnsScanned(_, _)
        | Action::VulnsMissing => KeyFlow::Continue,
        Action::Quit => KeyFlow::Quit,
    }
}

/// Translate a mouse event into navigation. Hit-testing matches `ui.rs`:
/// the sidebar is 24 columns wide, starting at row 2 (border) with section
/// rows from 3; the list table's data rows begin at row 4.
pub fn handle_mouse(state: &mut AppState, e: MouseEvent) -> KeyFlow {
    // Overlays and prompts own input; the pointer only navigates in Normal.
    if !matches!(state.mode, AppMode::Normal) {
        return KeyFlow::Continue;
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
    KeyFlow::Continue
}

pub fn handle_key(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    // Exactly one mode is active; dispatch by it. Overlays handle their own
    // keys and never leak into list navigation (or vice versa).
    match state.mode {
        AppMode::Normal => handle_normal(state, key),
        AppMode::Search => handle_search(state, key),
        AppMode::Prompt => handle_prompt(state, key),
        AppMode::Confirm(_) => handle_confirm(state, key),
        AppMode::Menu(_) => handle_menu(state, key),
        AppMode::ThemePicker(_) => handle_theme_picker(state, key),
        AppMode::Help => {
            state.mode = AppMode::Normal;
            KeyFlow::Continue
        }
    }
}

fn handle_search(state: &mut AppState, key: KeyEvent) -> KeyFlow {
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
    KeyFlow::Continue
}

fn handle_prompt(state: &mut AppState, key: KeyEvent) -> KeyFlow {
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
    KeyFlow::Continue
}

fn handle_confirm(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    let Some(modal) = state.take_modal() else {
        return KeyFlow::Continue;
    };
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => crate::app::run_modal_action(state, &modal),
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_menu(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    let last = crate::state::MENU_ACTIONS.len() - 1;
    let idx = match state.mode {
        AppMode::Menu(i) => i,
        _ => return KeyFlow::Continue,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('x') => state.mode = AppMode::Normal,
        KeyCode::Down | KeyCode::Char('j') => {
            state.mode = AppMode::Menu((idx + 1) % crate::state::MENU_ACTIONS.len())
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.mode = AppMode::Menu(idx.checked_sub(1).unwrap_or(last))
        }
        KeyCode::Enter => {
            state.mode = AppMode::Normal;
            crate::app::run_menu_action(state, idx);
        }
        KeyCode::Char(c) => match c {
            'u' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 0);
            }
            'R' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 1);
            }
            'r' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 2);
            }
            'i' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 3);
            }
            'd' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 4);
            }
            'p' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 5);
            }
            'o' => {
                state.mode = AppMode::Normal;
                crate::app::run_menu_action(state, 6);
            }
            _ => {}
        },
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_theme_picker(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    let pick = match state.mode {
        AppMode::ThemePicker(i) => i,
        _ => return KeyFlow::Continue,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('t') => state.mode = AppMode::Normal,
        KeyCode::Down | KeyCode::Char('j') => {
            state.mode = AppMode::ThemePicker((pick + 1) % crate::theme::THEMES.len())
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.mode = AppMode::ThemePicker(
                pick.checked_sub(1)
                    .unwrap_or(crate::theme::THEMES.len() - 1),
            )
        }
        KeyCode::Enter => {
            let chosen = crate::theme::THEMES[pick];
            state.theme = chosen;
            state.mode = AppMode::Normal;
            crate::theme::save(&chosen);
        }
        _ => {}
    }
    KeyFlow::Continue
}

fn install_confirm_text(p: &crate::brew::Package) -> String {
    match p.tap.as_deref().filter(|t| !is_official_tap(t)) {
        Some(tap) => format!("Install '{}' from tap '{tap}' (unverified)? (y/n)", p.name),
        None => format!("Install '{}'? (y/n)", p.name),
    }
}

fn handle_normal(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    match key.code {
        KeyCode::Char('/') => state.mode = AppMode::Search,
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
        }
        KeyCode::Char('v') => {
            if let Some(p) = state.selected().cloned()
                && !p.cask
                && state.sections[state.section_idx] != Section::Services
            {
                crate::app::spawn_vuln_scan(state, p.name);
            } else {
                state
                    .output
                    .push("vuln scan only available for formulae".into());
            }
        }
        KeyCode::Char('s') => {
            if state.sections[state.section_idx] == Section::Services
                && let Some(p) = state.selected().cloned()
            {
                let running = p.service_status.as_deref() == Some("started");
                let verb = if running { "stop" } else { "start" };
                crate::app::spawn_brew(state, &["services".into(), verb.into(), p.name]);
            }
        }
        KeyCode::Char('S') => {
            state.sort = state.sort.next();
            state.apply_section();
            state.output.push(format!("sort: {}", state.sort.label()));
        }
        KeyCode::Char('D') => crate::app::spawn_brew(state, &["doctor".into()]),
        KeyCode::Char('C') => crate::app::spawn_brew(state, &["config".into()]),
        KeyCode::Char('W') => {
            let target = crate::self_update::target_triple();
            let url = crate::self_update::download_url(target);
            let exe = std::env::current_exe().unwrap_or_else(|_| "lazybrew".into());
            state.mode = AppMode::Confirm(modal(
                crate::self_update::plan_text(&url, &exe),
                ModalAction::SelfUpdate,
            ));
        }
        KeyCode::Char('B') => {
            if let Some(path) = state.brewfile_path.clone() {
                crate::app::spawn_brew(
                    state,
                    &["bundle".into(), "check".into(), format!("--file={path}")],
                );
            } else {
                state
                    .output
                    .push("no Brewfile loaded — start lazybrew with -f <path-or-url>".into());
            }
        }
        KeyCode::Char('t') => {
            state.mode = AppMode::ThemePicker(crate::theme::index_of(&state.theme));
        }
        KeyCode::Char('x') => {
            if state.selected().is_some() {
                state.mode = AppMode::Menu(0);
            }
        }
        KeyCode::Char('?') => state.mode = AppMode::Help,
        KeyCode::Char('e') => {
            let home = dirs::home_dir().unwrap_or_else(|| ".".into());
            let path = home.join("Brewfile");
            crate::app::spawn_brew(
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
        }
        KeyCode::Char('u') => {
            if let Some(p) = state.selected() {
                let p = p.clone();
                state.mode = AppMode::Confirm(modal(
                    format!("Upgrade '{}'? (y/n)", p.name),
                    ModalAction::Upgrade(p.name, p.cask),
                ));
            }
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
        }
        KeyCode::Char('A') => {
            state.mode = AppMode::Confirm(modal(
                format!(
                    "Upgrade all {} outdated packages? (y/n)",
                    state.count_for(Section::Outdated)
                ),
                ModalAction::UpgradeAll,
            ));
        }
        KeyCode::Char('K') => {
            state.mode = AppMode::Confirm(modal("Run 'brew cleanup'? (y/n)", ModalAction::Cleanup));
        }
        KeyCode::Char('n') => {
            state.mode = AppMode::Confirm(modal(
                "Run 'brew autoremove'? (y/n)",
                ModalAction::Autoremove,
            ));
        }
        KeyCode::Char('U') => {
            state.mode = AppMode::Confirm(modal("Run 'brew update'? (y/n)", ModalAction::Update));
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
        }
        KeyCode::Char('q') => return KeyFlow::Quit,
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
            state.panel = match state.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
        }
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
            state.panel = match state.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
        }
        KeyCode::Down | KeyCode::Char('j') => match state.panel {
            Panel::Sidebar => {
                state.section_idx = (state.section_idx + 1) % state.sections.len();
                state.apply_section();
            }
            Panel::List => {
                if state.list_idx + 1 < state.filtered.len() {
                    state.list_idx += 1;
                }
            }
        },
        KeyCode::Up | KeyCode::Char('k') => match state.panel {
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
        },
        KeyCode::Enter => {
            if state.panel == Panel::Sidebar {
                state.panel = Panel::List;
            }
        }
        KeyCode::Esc => {
            if !state.search.is_empty() {
                state.search.clear();
                state.apply_section();
            }
        }
        KeyCode::PageUp => {
            if !state.output.is_empty() {
                state.output_offset += 1;
            }
        }
        KeyCode::PageDown => {
            state.output_offset = state.output_offset.saturating_sub(1);
        }
        KeyCode::Char('g') => {
            if state.panel == Panel::List {
                state.list_idx = 0;
            } else {
                state.section_idx = 0;
                state.apply_section();
            }
        }
        KeyCode::Char('G') => {
            if state.panel == Panel::List {
                state.list_idx = state.filtered.len().saturating_sub(1);
            } else {
                state.section_idx = state.sections.len() - 1;
                state.apply_section();
            }
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return KeyFlow::Quit;
        }
        _ => {}
    }
    KeyFlow::Continue
}
