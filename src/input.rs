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
                    app.modal = Some(Modal {
                        text: format!("Install '{}'? (y/n)", name),
                        confirm: ModalAction::Install(name),
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
            if let Some(p) = app.selected() {
                let p = p.clone();
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
                    app.section_counts()[1]
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
                app.section_idx = (app.section_idx + 1) % Section::ALL.len();
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
                    .unwrap_or(Section::ALL.len() - 1);
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
                app.section_idx = Section::ALL.len() - 1;
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
        ("x", "action menu (info/deps/pin)"),
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
