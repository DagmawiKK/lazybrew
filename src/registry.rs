//! The command registry: every user-triggerable operation is one row of data.
//!
//! This is the scalable home for new features. A feature is a [`Command`]
//! row — a label, a one-line description, an optional shortcut letter, a
//! context predicate (`applies`) and a `run` function that returns
//! [`crate::effect::Effect`]s through the same pipeline as key input. The
//! action menu (`x`) renders a plain view over this table, and the
//! transition-table/registry tests guarantee no two commands ever claim the
//! same shortcut letter. New commands are registered *without* a shortcut by
//! default — they are reachable by navigating the menu (and, later, a
//! searchable palette) — so the keymap never grows as features land.

use crate::effect::Effect;
use crate::state::{AppMode, AppState, ModalAction, Section, modal};
use crate::update::brew_effect;
use crossterm::event::KeyCode;

/// A single user-triggerable operation.
pub struct Command {
    /// Menu row text, e.g. `"u  Upgrade"` — the shortcut letter first when
    /// the command has one, three spaces of indent otherwise.
    pub label: &'static str,
    /// One-line description, shown under the menu block title.
    pub desc: &'static str,
    /// Menu shortcut letter, when the command deserves one.
    pub key: Option<KeyCode>,
    /// Context gate: only run when this is true for the current view.
    applies: fn(&AppState) -> bool,
    /// Produce the state changes and effects for this command.
    run: fn(&mut AppState) -> Vec<Effect>,
}

fn any_selection(state: &AppState) -> bool {
    state.selected().is_some()
}

/// `brew uses` answers "what depends on this formula" and is formula-only.
fn formula_selection(state: &AppState) -> bool {
    state.selected().is_some_and(|p| !p.cask)
}

/// `brew services` actions only make sense on the Services section.
fn in_services_section(state: &AppState) -> bool {
    state.sections.get(state.section_idx) == Some(&Section::Services) && state.selected().is_some()
}

/// Open the confirmation dialog for a command.
fn confirm(state: &mut AppState, text: &str, action: ModalAction) -> Vec<Effect> {
    state.mode = AppMode::Confirm(modal(text.to_string(), action));
    Vec::new()
}

fn selected_package(state: &AppState) -> Option<crate::brew::Package> {
    state.selected().cloned()
}

fn run_upgrade(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => confirm(
            state,
            &format!("Upgrade '{}'? (y/n)", p.name),
            ModalAction::Upgrade(p.name, p.cask),
        ),
        None => Vec::new(),
    }
}

fn run_reinstall(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => confirm(
            state,
            &format!("Reinstall '{}'? (y/n)", p.name),
            ModalAction::Reinstall(p.name, p.cask),
        ),
        None => Vec::new(),
    }
}

fn run_remove(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => confirm(
            state,
            &format!("Remove '{}'? (y/n)", p.name),
            ModalAction::Remove(p.name, p.cask),
        ),
        None => Vec::new(),
    }
}

fn run_info(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["info".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_deps(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["deps".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_pin(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => {
            let verb = if p.pinned { "unpin" } else { "pin" };
            brew_effect(state, &[verb.into(), p.name])
        }
        None => Vec::new(),
    }
}

fn run_home(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["home".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_link(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["link".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_unlink(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["unlink".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_uses(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["uses".into(), "--installed".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_missing(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["missing".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_restart(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["services".into(), "restart".into(), p.name]),
        None => Vec::new(),
    }
}

fn run_run(state: &mut AppState) -> Vec<Effect> {
    match selected_package(state) {
        Some(p) => brew_effect(state, &["services".into(), "run".into(), p.name]),
        None => Vec::new(),
    }
}

/// All commands, in menu order (grouped: manage, inspect, link, health,
/// services). Existing rows keep their classic labels and shortcut letters
/// so muscle memory and position-based navigation are untouched.
static COMMANDS: [Command; 13] = [
    Command {
        label: "u  Upgrade",
        desc: "Upgrade the selected package",
        key: Some(KeyCode::Char('u')),
        applies: any_selection,
        run: run_upgrade,
    },
    Command {
        label: "R  Reinstall",
        desc: "Reinstall the selected package",
        key: Some(KeyCode::Char('R')),
        applies: any_selection,
        run: run_reinstall,
    },
    Command {
        label: "r  Remove",
        desc: "Uninstall the selected package",
        key: Some(KeyCode::Char('r')),
        applies: any_selection,
        run: run_remove,
    },
    Command {
        label: "i  Info",
        desc: "Show brew info for the package",
        key: Some(KeyCode::Char('i')),
        applies: any_selection,
        run: run_info,
    },
    Command {
        label: "d  Deps",
        desc: "Show the package dependencies",
        key: Some(KeyCode::Char('d')),
        applies: any_selection,
        run: run_deps,
    },
    Command {
        label: "p  Pin/Unpin",
        desc: "Prevent or allow upgrades",
        key: Some(KeyCode::Char('p')),
        applies: any_selection,
        run: run_pin,
    },
    Command {
        label: "o  Home",
        desc: "Open the homepage in a browser",
        key: Some(KeyCode::Char('o')),
        applies: any_selection,
        run: run_home,
    },
    Command {
        label: "   Link",
        desc: "Symlink keg-only files into prefix",
        key: None,
        applies: any_selection,
        run: run_link,
    },
    Command {
        label: "   Unlink",
        desc: "Remove the package symlinks",
        key: None,
        applies: any_selection,
        run: run_unlink,
    },
    Command {
        label: "   Uses (installed)",
        desc: "Installed formulae using this one",
        key: None,
        applies: formula_selection,
        run: run_uses,
    },
    Command {
        label: "   Missing deps",
        desc: "Check keg for missing dependencies",
        key: None,
        applies: any_selection,
        run: run_missing,
    },
    Command {
        label: "   Restart service",
        desc: "Restart the service",
        key: None,
        applies: in_services_section,
        run: run_restart,
    },
    Command {
        label: "   Run service once",
        desc: "Run the service once",
        key: None,
        applies: in_services_section,
        run: run_run,
    },
];

/// The static, ordered command table — the single source for menus, help,
/// and (later) a searchable command palette.
pub fn commands() -> &'static [Command] {
    &COMMANDS
}

/// Index of the command whose shortcut letter is `c`, if any.
pub fn shortcut_index(c: char) -> Option<usize> {
    COMMANDS
        .iter()
        .position(|cmd| cmd.key == Some(KeyCode::Char(c)))
}

/// Run the command at `idx`, but only when its context gate holds. The menu
/// shows the whole table today; `applies` keeps each command inert outside
/// its own territory, which is the same predicate a palette will filter on.
pub fn run_index(state: &mut AppState, idx: usize) -> Vec<Effect> {
    let Some(cmd) = COMMANDS.get(idx) else {
        return Vec::new();
    };
    if (cmd.applies)(state) {
        (cmd.run)(state)
    } else {
        Vec::new()
    }
}
