//! Input entry points.
//!
//! After the Action split these are thin wrappers: keys and mouse events are
//! boxed into [`crate::action::Action`] and folded in by
//! [`crate::update::update`]. The pure query helpers (`completions`,
//! `help_text`) live here so render code can reuse them.

use crate::state::{AppState, Section};

#[cfg(test)]
use crossterm::event::{KeyEvent, MouseEvent};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFlow {
    Continue,
    Quit,
}

/// Translate a key event into a state transition. Runs the update step and
/// reports whether the app should quit. This is the test-facing entry point:
/// the runtime drives `update()` directly.
#[cfg(test)]
pub fn handle_key(state: &mut AppState, key: KeyEvent) -> KeyFlow {
    if crate::update::update(state, crate::action::Action::Key(key))
        .contains(&crate::effect::Effect::Quit)
    {
        KeyFlow::Quit
    } else {
        KeyFlow::Continue
    }
}

/// Translate a mouse event into a state transition. Never quits.
#[cfg(test)]
pub fn handle_mouse(state: &mut AppState, e: MouseEvent) -> KeyFlow {
    crate::update::update(state, crate::action::Action::Mouse(e));
    KeyFlow::Continue
}

/// Candidate names for the type-a-name prompt: prefix matches against the
/// current section's data (catalog when loaded, else installed packages;
/// tap names in the Taps section). Sorted, deduped, capped for the hint row.
pub fn completions(state: &AppState) -> Vec<String> {
    let source: &[crate::brew::Package] = if state.sections[state.section_idx] == Section::Taps {
        &state.taps
    } else if !state.catalog.is_empty() {
        &state.catalog
    } else {
        &state.packages
    };
    let q = state.prompt_buffer.to_lowercase();
    let mut names: Vec<String> = source
        .iter()
        .map(|p| p.name.clone())
        .filter(|n| n.to_lowercase().starts_with(&q))
        .collect();
    names.sort();
    names.dedup();
    names.truncate(9);
    names
}

/// All keybindings, listed in the help overlay.
pub fn help_text() -> String {
    let rows: &[(&str, &str)] = &[
        ("j/k, up/down", "navigate"),
        ("h/l, tab", "switch panel"),
        ("/", "search"),
        ("esc", "clear search / close"),
        ("pgup/pgdn", "scroll output history"),
        ("g/G", "top / bottom"),
        ("i", "install selected package (tap prompt in Taps)"),
        ("u", "upgrade selected"),
        ("r", "remove selected (untap in Taps section)"),
        ("L", "link selected package (keg-only)"),
        ("Y", "unlink selected package"),
        ("A", "upgrade all outdated"),
        ("U", "brew update"),
        ("K", "brew cleanup"),
        ("n", "brew autoremove"),
        ("s", "start/stop service (Services section)"),
        ("v", "vulnerability scan (formulae)"),
        (
            "I",
            "install by typed name (tab completes, all in Brewfile)",
        ),
        ("R", "remove all (Brewfile section)"),
        ("i/r", "tap/untap (Taps section)"),
        ("x", "action menu (navigate j/k, enter to run)"),
        ("D", "brew doctor"),
        ("C", "brew config"),
        ("B", "brew bundle check (-f file)"),
        ("W", "update lazybrew from the GitHub release"),
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
    use crate::brew::Package;
    use crate::state::{AppMode, ModalAction, Panel, SortMode};
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

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
            tap: None,
        }
    }

    fn app_with(p: Package) -> AppState {
        let mut app = AppState {
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            packages: vec![p],
            ..AppState::default()
        };
        app.apply_section();
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    fn app_with_packages(names: &[&str]) -> AppState {
        let mut app = app_with(pkg(names[0], false, false));
        for n in &names[1..] {
            app.packages.push(pkg(n, false, false));
        }
        app.apply_section();
        app
    }

    #[test]
    fn wheel_scrolls_the_list_down_and_up() {
        let mut app = app_with_packages(&["git", "wget", "zlib"]);
        handle_key(&mut app, key('l')); // focus the list
        assert_eq!(app.list_idx, 0);
        handle_mouse(&mut app, mouse(MouseEventKind::ScrollDown, 50, 30));
        assert_eq!(app.list_idx, 1);
        handle_mouse(&mut app, mouse(MouseEventKind::ScrollDown, 50, 30));
        assert_eq!(app.list_idx, 2);
        // Bottom: wheel keeps it clamped.
        handle_mouse(&mut app, mouse(MouseEventKind::ScrollDown, 50, 30));
        assert_eq!(app.list_idx, 2);
        handle_mouse(&mut app, mouse(MouseEventKind::ScrollUp, 50, 30));
        assert_eq!(app.list_idx, 1);
    }

    #[test]
    fn click_selects_the_row_under_the_cursor() {
        let mut app = app_with_packages(&["git", "wget", "zlib"]);
        handle_mouse(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), 30, 6),
        );
        assert_eq!(
            app.list_idx, 2,
            "data row 0 is screen row 4, row 6 -> idx 2"
        );
        assert_eq!(app.panel, Panel::List);
    }

    #[test]
    fn click_on_the_sidebar_switches_section() {
        let mut app = app_with_packages(&["git", "wget", "zlib"]);
        // Row 4 -> sidebar item 1 (Outdated).
        handle_mouse(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), 5, 4),
        );
        assert_eq!(app.section_idx, 1);
        assert_eq!(app.panel, Panel::List);
        assert_eq!(app.sections[1], Section::Outdated);
    }

    #[test]
    fn mouse_is_ignored_while_an_overlay_is_open() {
        let mut app = app_with_packages(&["git", "wget", "zlib"]);
        handle_key(&mut app, key('/')); // search mode
        handle_mouse(&mut app, mouse(MouseEventKind::ScrollDown, 50, 30));
        assert_eq!(app.list_idx, 0, "scroll must not leak into search mode");
        assert!(matches!(app.mode, AppMode::Search));
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
    fn install_from_untrusted_tap_warns() {
        let mut app = app_with(pkg("envsubst", false, false));
        app.catalog = vec![{
            let mut p = pkg("envsubst", false, false);
            p.tap = Some("awslabs/git-secrets".into());
            p
        }];
        app.section_idx = 4; // Catalog
        app.apply_section();
        handle_key(&mut app, key('i'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(
            modal.text.contains("awslabs") && modal.text.contains("unverified"),
            "tap warning should be present: {}",
            modal.text
        );
    }

    #[test]
    fn install_from_official_tap_stays_plain() {
        let mut app = app_with(pkg("envsubst", false, false));
        app.catalog = vec![{
            let mut p = pkg("envsubst", false, false);
            p.tap = Some("homebrew/core".into());
            p
        }];
        app.section_idx = 4; // Catalog
        app.apply_section();
        handle_key(&mut app, key('i'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(
            !modal.text.contains("unverified"),
            "official tap should not warn: {}",
            modal.text
        );
    }

    #[test]
    fn capital_w_confirms_self_update() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(&mut app, key('W'));
        let AppMode::Confirm(modal) = app.mode else {
            panic!("expected confirm modal, got {:?}", app.mode);
        };
        assert!(
            modal.text.contains("GitHub release")
                && modal.text.contains("releases/latest/download/"),
            "self-update text should explain the download: {}",
            modal.text
        );
        assert!(matches!(modal.confirm, ModalAction::SelfUpdate));
    }

    #[test]
    fn page_keys_scrub_the_output_history() {
        let mut app = app_with(pkg("git", false, false));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()),
        ); // no-op on empty output
        assert_eq!(app.output_offset, 0);
        app.output = vec!["a".into(), "b".into(), "c".into()];
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()),
        );
        assert_eq!(app.output_offset, 1);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()),
        );
        assert_eq!(app.output_offset, 2);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()),
        );
        assert_eq!(
            app.output_offset, 0,
            "PageDown clamps at the tail (offset 0)"
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
    fn tab_completes_the_unique_candidate() {
        let mut app = app_with(pkg("wget", false, false));
        handle_key(&mut app, key('I'));
        for c in "wg".chars() {
            handle_key(&mut app, key(c));
        }
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(app.prompt_buffer, "wget");
    }

    #[test]
    fn tab_cycles_through_candidates() {
        let mut app = app_with_packages(&["wget", "wget2", "git"]);
        handle_key(&mut app, key('I'));
        for c in "wg".chars() {
            handle_key(&mut app, key(c));
        }
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(app.prompt_buffer, "wget");
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(app.prompt_buffer, "wget2");
    }

    #[test]
    fn tab_with_empty_buffer_fills_first_candidate() {
        let mut app = app_with_packages(&["zlib", "git", "wget"]);
        handle_key(&mut app, key('I'));
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(
            app.prompt_buffer, "git",
            "sorted first name fills the buffer"
        );
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
