//! Keyboard handling.

use crate::app::{
    App, Modal, ModalAction, Panel, Section, run_menu_action, run_modal_action, spawn_brew,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum KeyFlow {
    Continue,
    Quit,
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> KeyFlow {
    if app.help {
        app.help = false;
        return KeyFlow::Continue;
    }

    if let Some(pick) = app.theme_picker {
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') => app.theme_picker = None,
            KeyCode::Down | KeyCode::Char('j') => {
                app.theme_picker = Some((pick + 1) % crate::theme::THEMES.len())
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.theme_picker = Some(
                    pick.checked_sub(1)
                        .unwrap_or(crate::theme::THEMES.len() - 1),
                )
            }
            KeyCode::Enter => {
                let chosen = crate::theme::THEMES[pick];
                app.theme = chosen;
                app.theme_picker = None;
                crate::theme::save(&chosen);
            }
            _ => {}
        }
        return KeyFlow::Continue;
    }

    if let Some(menu_idx) = app.menu {
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => app.menu = None,
            KeyCode::Down | KeyCode::Char('j') => app.menu = Some((menu_idx + 1) % 5),
            KeyCode::Up | KeyCode::Char('k') => {
                app.menu = Some(menu_idx.checked_sub(1).unwrap_or(4))
            }
            KeyCode::Enter => {
                app.menu = None;
                run_menu_action(app, menu_idx);
            }
            KeyCode::Char(c) => match c {
                'u' => {
                    app.menu = None;
                    run_menu_action(app, 0);
                }
                'r' => {
                    app.menu = None;
                    run_menu_action(app, 1);
                }
                'i' => {
                    app.menu = None;
                    run_menu_action(app, 2);
                }
                'd' => {
                    app.menu = None;
                    run_menu_action(app, 3);
                }
                'p' => {
                    app.menu = None;
                    run_menu_action(app, 4);
                }
                _ => {}
            },
            _ => {}
        }
        return KeyFlow::Continue;
    }

    if let Some(modal) = app.modal.take() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                run_modal_action(app, &modal);
                return KeyFlow::Continue;
            }
            _ => {}
        }
        return KeyFlow::Continue;
    }

    if app.installing {
        match key.code {
            KeyCode::Enter => {
                let name = app.install_input.trim().to_string();
                app.installing = false;
                app.install_input.clear();
                if !name.is_empty() {
                    let action = if app.sections[app.section_idx] == Section::Taps {
                        ModalAction::Tap(name.clone())
                    } else {
                        ModalAction::Install(name.clone())
                    };
                    app.modal = Some(Modal {
                        text: format!("Install '{}'? (y/n)", name),
                        confirm: action,
                    });
                }
            }
            KeyCode::Esc => {
                app.installing = false;
                app.install_input.clear();
            }
            KeyCode::Backspace => {
                app.install_input.pop();
            }
            KeyCode::Char(c) => app.install_input.push(c),
            _ => {}
        }
        return KeyFlow::Continue;
    }

    if app.searching {
        match key.code {
            KeyCode::Enter => app.searching = false,
            KeyCode::Esc => {
                app.search.clear();
                app.apply_section();
                app.searching = false;
            }
            KeyCode::Backspace => {
                app.search.pop();
                app.apply_section();
            }
            KeyCode::Char(c) => {
                app.search.push(c);
                app.apply_section();
            }
            _ => {}
        }
        return KeyFlow::Continue;
    }

    match key.code {
        KeyCode::Char('/') => {
            app.searching = true;
        }
        KeyCode::Char('I') => {
            if app.sections[app.section_idx] == Section::Brewfile {
                let missing = app.brewfile_missing();
                app.modal = Some(Modal {
                    text: format!("Install {missing} missing Brewfile packages? (y/n)"),
                    confirm: ModalAction::BrewfileInstall,
                });
            }
        }
        KeyCode::Char('R') => {
            if app.sections[app.section_idx] == Section::Brewfile {
                app.modal = Some(Modal {
                    text: format!("Remove all {} Brewfile packages? (y/n)", app.brewfile.len()),
                    confirm: ModalAction::BrewfileRemove,
                });
            }
        }
        KeyCode::Char('v') => {
            if let Some(p) = app.selected().cloned()
                && !p.cask
                && app.sections[app.section_idx] != Section::Services
            {
                crate::app::spawn_vuln_scan(app, p.name);
            } else {
                app.output
                    .push("vuln scan only available for formulae".into());
            }
        }
        KeyCode::Char('s') => {
            if app.sections[app.section_idx] == Section::Services
                && let Some(p) = app.selected().cloned()
            {
                let running = p.service_status.as_deref() == Some("started");
                let verb = if running { "stop" } else { "start" };
                spawn_brew(app, &["services".into(), verb.into(), p.name]);
            }
        }
        KeyCode::Char('t') => {
            app.theme_picker = Some(crate::theme::index_of(&app.theme));
        }
        KeyCode::Char('x') => {
            if app.selected().is_some() {
                app.menu = Some(0);
            }
        }
        KeyCode::Char('?') => app.help = true,
        KeyCode::Char('e') => {
            let home = dirs::home_dir().unwrap_or_else(|| ".".into());
            let path = home.join("Brewfile");
            spawn_brew(
                app,
                &[
                    "bundle".into(),
                    "dump".into(),
                    "--force".into(),
                    format!("--file={}", path.display()),
                ],
            );
            app.output
                .push(format!("Brewfile written to {}", path.display()));
        }
        KeyCode::Char('u') => {
            if let Some(p) = app.selected() {
                let p = p.clone();
                app.modal = Some(Modal {
                    text: format!("Upgrade '{}'? (y/n)", p.name),
                    confirm: ModalAction::Upgrade(p.name, p.cask),
                });
            }
        }
        KeyCode::Char('r') => {
            if app.sections[app.section_idx] == Section::Taps {
                if let Some(p) = app.selected().cloned() {
                    app.modal = Some(Modal {
                        text: format!("Untap '{}'? (y/n)", p.name),
                        confirm: ModalAction::Untap(p.name),
                    });
                }
            } else if let Some(p) = app.selected().cloned() {
                app.modal = Some(Modal {
                    text: format!("Remove '{}'? (y/n)", p.name),
                    confirm: ModalAction::Remove(p.name, p.cask),
                });
            }
        }
        KeyCode::Char('A') => {
            app.modal = Some(Modal {
                text: format!(
                    "Upgrade all {} outdated packages? (y/n)",
                    app.count_for(Section::Outdated)
                ),
                confirm: ModalAction::UpgradeAll,
            });
        }
        KeyCode::Char('K') => {
            app.modal = Some(Modal {
                text: "Run 'brew cleanup'? (y/n)".into(),
                confirm: ModalAction::Cleanup,
            });
        }
        KeyCode::Char('n') => {
            app.modal = Some(Modal {
                text: "Run 'brew autoremove'? (y/n)".into(),
                confirm: ModalAction::Autoremove,
            });
        }
        KeyCode::Char('U') => {
            app.modal = Some(Modal {
                text: "Run 'brew update'? (y/n)".into(),
                confirm: ModalAction::Update,
            });
        }
        KeyCode::Char('i') => {
            app.searching = false;
            app.installing = true;
        }
        KeyCode::Char('q') => return KeyFlow::Quit,
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
            app.panel = match app.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
        }
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
            app.panel = match app.panel {
                Panel::Sidebar => Panel::List,
                Panel::List => Panel::Sidebar,
            };
        }
        KeyCode::Down | KeyCode::Char('j') => match app.panel {
            Panel::Sidebar => {
                app.section_idx = (app.section_idx + 1) % app.sections.len();
                app.apply_section();
            }
            Panel::List => {
                if app.list_idx + 1 < app.filtered.len() {
                    app.list_idx += 1;
                }
            }
        },
        KeyCode::Up | KeyCode::Char('k') => match app.panel {
            Panel::Sidebar => {
                app.section_idx = app
                    .section_idx
                    .checked_sub(1)
                    .unwrap_or(app.sections.len() - 1);
                app.apply_section();
            }
            Panel::List => {
                app.list_idx = app.list_idx.saturating_sub(1);
            }
        },
        KeyCode::Enter => {
            if app.panel == Panel::Sidebar {
                app.panel = Panel::List;
            }
        }
        KeyCode::Esc => {
            if !app.search.is_empty() {
                app.search.clear();
                app.apply_section();
            }
        }
        KeyCode::Char('g') => {
            if app.panel == Panel::List {
                app.list_idx = 0;
            } else {
                app.section_idx = 0;
                app.apply_section();
            }
        }
        KeyCode::Char('G') => {
            if app.panel == Panel::List {
                app.list_idx = app.filtered.len().saturating_sub(1);
            } else {
                app.section_idx = app.sections.len() - 1;
                app.apply_section();
            }
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return KeyFlow::Quit;
        }
        _ => {}
    }
    KeyFlow::Continue
}

/// All keybindings, listed in the help overlay.
pub fn help_text() -> String {
    let rows: &[(&str, &str)] = &[
        ("j/k, up/down", "navigate"),
        ("h/l, tab", "switch panel"),
        ("/", "search"),
        ("esc", "clear search / close"),
        ("g/G", "top / bottom"),
        ("i", "install package (or tap in Taps section)"),
        ("u", "upgrade selected"),
        ("r", "remove selected (untap in Taps section)"),
        ("A", "upgrade all outdated"),
        ("U", "brew update"),
        ("K", "brew cleanup"),
        ("n", "brew autoremove"),
        ("s", "start/stop service (Services section)"),
        ("v", "vulnerability scan (formulae)"),
        ("I/R", "install/remove all (Brewfile section)"),
        ("i/r", "tap/untap (Taps section)"),
        ("x", "action menu (info/deps/pin)"),
        ("t", "theme picker"),
        ("e", "export Brewfile to ~/Brewfile"),
        ("?", "this help"),
        ("q", "quit"),
    ];
    let mut out = String::from("lazybrew keybindings\n\n");
    for (key, desc) in rows {
        out.push_str(&format!("{key:16} {desc}\n"));
    }
    out
}
