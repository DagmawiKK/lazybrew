//! Keyboard handling.

use crate::app::{
    App, AppMode, ModalAction, Panel, Section, modal, run_menu_action, run_modal_action, spawn_brew,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum KeyFlow {
    Continue,
    Quit,
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> KeyFlow {
    // Exactly one mode is active; dispatch by it. Overlays handle their own
    // keys and never leak into list navigation (or vice versa).
    match app.mode {
        AppMode::Normal => handle_normal(app, key),
        AppMode::Search => handle_search(app, key),
        AppMode::Prompt => handle_prompt(app, key),
        AppMode::Confirm(_) => handle_confirm(app, key),
        AppMode::Menu(_) => handle_menu(app, key),
        AppMode::ThemePicker(_) => handle_theme_picker(app, key),
        AppMode::Help => {
            app.mode = AppMode::Normal;
            KeyFlow::Continue
        }
    }
}

fn handle_search(app: &mut App, key: KeyEvent) -> KeyFlow {
    match key.code {
        KeyCode::Enter => app.mode = AppMode::Normal,
        KeyCode::Esc => {
            app.search.clear();
            app.apply_section();
            app.mode = AppMode::Normal;
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
    KeyFlow::Continue
}

fn handle_prompt(app: &mut App, key: KeyEvent) -> KeyFlow {
    match key.code {
        KeyCode::Enter => {
            let name = app.prompt_buffer.trim().to_string();
            app.prompt_buffer.clear();
            app.mode = AppMode::Normal;
            if !name.is_empty() {
                let action = if app.sections[app.section_idx] == Section::Taps {
                    ModalAction::Tap(name.clone())
                } else {
                    ModalAction::Install(name.clone(), false)
                };
                app.mode = AppMode::Confirm(modal(format!("Install '{}'? (y/n)", name), action));
            }
        }
        KeyCode::Esc => {
            app.prompt_buffer.clear();
            app.mode = AppMode::Normal;
        }
        KeyCode::Backspace => {
            app.prompt_buffer.pop();
        }
        KeyCode::Char(c) => app.prompt_buffer.push(c),
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_confirm(app: &mut App, key: KeyEvent) -> KeyFlow {
    let Some(modal) = app.take_modal() else {
        return KeyFlow::Continue;
    };
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => run_modal_action(app, &modal),
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_menu(app: &mut App, key: KeyEvent) -> KeyFlow {
    let last = crate::app::MENU_ACTIONS.len() - 1;
    let idx = match app.mode {
        AppMode::Menu(i) => i,
        _ => return KeyFlow::Continue,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('x') => app.mode = AppMode::Normal,
        KeyCode::Down | KeyCode::Char('j') => {
            app.mode = AppMode::Menu((idx + 1) % crate::app::MENU_ACTIONS.len())
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.mode = AppMode::Menu(idx.checked_sub(1).unwrap_or(last))
        }
        KeyCode::Enter => {
            app.mode = AppMode::Normal;
            run_menu_action(app, idx);
        }
        KeyCode::Char(c) => match c {
            'u' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 0);
            }
            'R' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 1);
            }
            'r' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 2);
            }
            'i' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 3);
            }
            'd' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 4);
            }
            'p' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 5);
            }
            'o' => {
                app.mode = AppMode::Normal;
                run_menu_action(app, 6);
            }
            _ => {}
        },
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_theme_picker(app: &mut App, key: KeyEvent) -> KeyFlow {
    let pick = match app.mode {
        AppMode::ThemePicker(i) => i,
        _ => return KeyFlow::Continue,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('t') => app.mode = AppMode::Normal,
        KeyCode::Down | KeyCode::Char('j') => {
            app.mode = AppMode::ThemePicker((pick + 1) % crate::theme::THEMES.len())
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.mode = AppMode::ThemePicker(
                pick.checked_sub(1)
                    .unwrap_or(crate::theme::THEMES.len() - 1),
            )
        }
        KeyCode::Enter => {
            let chosen = crate::theme::THEMES[pick];
            app.theme = chosen;
            app.mode = AppMode::Normal;
            crate::theme::save(&chosen);
        }
        _ => {}
    }
    KeyFlow::Continue
}

fn handle_normal(app: &mut App, key: KeyEvent) -> KeyFlow {
    match key.code {
        KeyCode::Char('/') => app.mode = AppMode::Search,
        KeyCode::Char('I') => {
            if app.sections[app.section_idx] == Section::Brewfile {
                let missing = app.brewfile_missing();
                app.mode = AppMode::Confirm(modal(
                    format!("Install {missing} missing Brewfile packages? (y/n)"),
                    ModalAction::BrewfileInstall,
                ));
            } else {
                // Explicit "type the name" install.
                app.prompt_buffer.clear();
                app.mode = AppMode::Prompt;
            }
        }
        KeyCode::Char('R') => {
            if app.sections[app.section_idx] == Section::Brewfile {
                app.mode = AppMode::Confirm(modal(
                    format!("Remove all {} Brewfile packages? (y/n)", app.brewfile.len()),
                    ModalAction::BrewfileRemove,
                ));
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
        KeyCode::Char('S') => {
            app.sort = app.sort.next();
            app.apply_section();
            app.output.push(format!("sort: {}", app.sort.label()));
        }
        KeyCode::Char('D') => spawn_brew(app, &["doctor".into()]),
        KeyCode::Char('C') => spawn_brew(app, &["config".into()]),
        KeyCode::Char('B') => {
            if let Some(path) = app.brewfile_path.clone() {
                spawn_brew(
                    app,
                    &["bundle".into(), "check".into(), format!("--file={path}")],
                );
            } else {
                app.output
                    .push("no Brewfile loaded — start lazybrew with -f <path-or-url>".into());
            }
        }
        KeyCode::Char('t') => {
            app.mode = AppMode::ThemePicker(crate::theme::index_of(&app.theme));
        }
        KeyCode::Char('x') => {
            if app.selected().is_some() {
                app.mode = AppMode::Menu(0);
            }
        }
        KeyCode::Char('?') => app.mode = AppMode::Help,
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
                app.mode = AppMode::Confirm(modal(
                    format!("Upgrade '{}'? (y/n)", p.name),
                    ModalAction::Upgrade(p.name, p.cask),
                ));
            }
        }
        KeyCode::Char('r') => {
            if app.sections[app.section_idx] == Section::Taps {
                if let Some(p) = app.selected().cloned() {
                    app.mode = AppMode::Confirm(modal(
                        format!("Untap '{}'? (y/n)", p.name),
                        ModalAction::Untap(p.name),
                    ));
                }
            } else if let Some(p) = app.selected().cloned() {
                app.mode = AppMode::Confirm(modal(
                    format!("Remove '{}'? (y/n)", p.name),
                    ModalAction::Remove(p.name, p.cask),
                ));
            }
        }
        KeyCode::Char('A') => {
            app.mode = AppMode::Confirm(modal(
                format!(
                    "Upgrade all {} outdated packages? (y/n)",
                    app.count_for(Section::Outdated)
                ),
                ModalAction::UpgradeAll,
            ));
        }
        KeyCode::Char('K') => {
            app.mode = AppMode::Confirm(modal("Run 'brew cleanup'? (y/n)", ModalAction::Cleanup));
        }
        KeyCode::Char('n') => {
            app.mode = AppMode::Confirm(modal(
                "Run 'brew autoremove'? (y/n)",
                ModalAction::Autoremove,
            ));
        }
        KeyCode::Char('U') => {
            app.mode = AppMode::Confirm(modal("Run 'brew update'? (y/n)", ModalAction::Update));
        }
        KeyCode::Char('i') => {
            if app.sections[app.section_idx] == Section::Taps {
                // Adding a NEW tap still needs a typed name.
                app.prompt_buffer.clear();
                app.mode = AppMode::Prompt;
            } else if let Some(p) = app.selected().cloned() {
                if p.installed_version.is_some() || p.service_status.is_some() {
                    let detail = p.installed_version.unwrap_or_else(|| "installed".into());
                    app.output.push(format!(
                        "{} is already installed ({}) — u upgrades, r removes",
                        p.name, detail
                    ));
                } else {
                    app.mode = AppMode::Confirm(modal(
                        format!("Install '{}'? (y/n)", p.name),
                        ModalAction::Install(p.name, p.cask),
                    ));
                }
            } else {
                // Nothing selected: fall back to typing a name.
                app.prompt_buffer.clear();
                app.mode = AppMode::Prompt;
            }
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
        ("i", "install selected package (tap prompt in Taps)"),
        ("u", "upgrade selected"),
        ("r", "remove selected (untap in Taps section)"),
        ("A", "upgrade all outdated"),
        ("U", "brew update"),
        ("K", "brew cleanup"),
        ("n", "brew autoremove"),
        ("s", "start/stop service (Services section)"),
        ("v", "vulnerability scan (formulae)"),
        ("I", "install by typed name (all in Brewfile)"),
        ("R", "remove all (Brewfile section)"),
        ("i/r", "tap/untap (Taps section)"),
        (
            "x",
            "action menu (upgrade/reinstall/remove/info/deps/pin/home)",
        ),
        ("D", "brew doctor"),
        ("C", "brew config"),
        ("B", "brew bundle check (-f file)"),
        ("t", "theme picker"),
        ("S", "sort mode (natural/name/installs)"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SortMode;
    use crate::brew::Package;

    fn pkg(name: &str, installed: bool, cask: bool) -> Package {
        Package {
            name: name.into(),
            desc: format!("{name} desc"),
            version: "1.0".into(),
            cask,
            outdated: false,
            installed_version: installed.then(|| "1.0".into()),
            pinned: false,
            service_status: None,
            deprecation: None,
        }
    }

    fn app_with(p: Package) -> App {
        let mut app = App {
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            brewfile_path: None,
            brewfile_entries: Vec::new(),
            brewfile: Vec::new(),
            packages: vec![p],
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 0,
            panel: Panel::Sidebar,
            sort: SortMode::Natural,
            leaves: Vec::new(),
            catalog: Vec::new(),
            installs: Default::default(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: Vec::new(),
            cmd_rx: None,
            frame: 0,
            load_rx: None,
            catalog_rx: None,
            theme: crate::theme::DEFAULT,
        };
        app.apply_section();
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }

    #[test]
    fn menu_reinstall_shortcut_confirms() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('x'));
        assert!(matches!(app.mode, AppMode::Menu(0)));
        handle_key(&mut app, key('R'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(modal.text.contains("Reinstall 'git'"));
        match modal.confirm {
            ModalAction::Reinstall(name, _) => assert_eq!(name, "git"),
            _ => panic!("expected Reinstall action"),
        }
    }

    #[test]
    fn capital_b_without_brewfile_reports_missing_file() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('B'));
        assert!(
            app.output.iter().any(|l| l.contains("-f")),
            "should explain -f usage: {:?}",
            app.output
        );
    }

    #[test]
    fn shift_s_cycles_sort_mode() {
        let mut app = app_with(pkg("zlib", false, false));
        assert_eq!(app.sort, SortMode::Natural);
        handle_key(&mut app, key('S'));
        assert_eq!(app.sort, SortMode::Name);
        handle_key(&mut app, key('S'));
        assert_eq!(app.sort, SortMode::Installs);
        handle_key(&mut app, key('S'));
        assert_eq!(app.sort, SortMode::Natural);
    }

    #[test]
    fn i_installs_the_selected_package_without_typing() {
        let mut app = app_with(pkg("wget", false, false));
        handle_key(&mut app, key('i'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(modal.text.contains("wget"));
        match modal.confirm {
            ModalAction::Install(name, cask) => {
                assert_eq!(name, "wget");
                assert!(!cask);
            }
            _ => panic!("expected Install action"),
        }
    }

    #[test]
    fn i_passes_cask_flag_through() {
        let mut app = app_with(pkg("firefox", false, true));
        handle_key(&mut app, key('i'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        match modal.confirm {
            ModalAction::Install(_, cask) => assert!(cask),
            _ => panic!("expected Install action"),
        }
    }

    #[test]
    fn i_on_installed_package_reports_and_skips_prompt() {
        let mut app = app_with(pkg("git", true, false));
        handle_key(&mut app, key('i'));
        assert_eq!(app.mode, AppMode::Normal);
        assert!(
            app.output.iter().any(|l| l.contains("already installed")),
            "output should explain it is already installed: {:?}",
            app.output
        );
    }

    #[test]
    fn capital_i_falls_back_to_typed_name() {
        let mut app = app_with(pkg("git", true, false));
        handle_key(&mut app, key('I'));
        assert!(
            matches!(app.mode, AppMode::Prompt),
            "I should open the type-a-name prompt"
        );
    }

    #[test]
    fn prompt_typing_ends_in_confirm_with_typed_name() {
        let mut app = app_with(pkg("git", true, false));
        handle_key(&mut app, key('I'));
        for c in "wget".chars() {
            handle_key(&mut app, key(c));
        }
        assert_eq!(app.prompt_buffer, "wget");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(modal.text.contains("wget"));
        match modal.confirm {
            ModalAction::Install(name, _) => assert_eq!(name, "wget"),
            _ => panic!("expected Install action"),
        }
    }

    #[test]
    fn esc_cancels_the_prompt() {
        let mut app = app_with(pkg("git", true, false));
        handle_key(&mut app, key('I'));
        handle_key(&mut app, key('w'));
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(app.mode, AppMode::Normal);
        assert!(app.prompt_buffer.is_empty());
    }

    #[test]
    fn search_typing_filters_and_esc_clears() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('/'));
        handle_key(&mut app, key('g'));
        assert_eq!(app.search, "g");
        assert_eq!(app.filtered.len(), 1);
        handle_key(&mut app, key('z'));
        assert!(app.filtered.is_empty(), "no package matches 'gz'");
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(app.mode, AppMode::Normal);
        assert!(app.search.is_empty());
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('?'));
        assert_eq!(app.mode, AppMode::Help);
        handle_key(&mut app, key('j'));
        assert_eq!(app.mode, AppMode::Normal);
    }

    #[test]
    fn theme_picker_navigates_with_jk_and_closes() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('t'));
        let AppMode::ThemePicker(initial) = app.mode else {
            panic!("expected theme picker, got {:?}", app.mode);
        };
        handle_key(&mut app, key('j'));
        let AppMode::ThemePicker(next) = app.mode else {
            panic!("expected theme picker after j");
        };
        assert_eq!(next, (initial + 1) % crate::theme::THEMES.len());
        handle_key(&mut app, key('k'));
        let AppMode::ThemePicker(prev) = app.mode else {
            panic!("expected theme picker after k");
        };
        assert_eq!(prev, initial);
        handle_key(&mut app, key('t'));
        assert_eq!(app.mode, AppMode::Normal);
    }
}
