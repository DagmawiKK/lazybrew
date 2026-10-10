//! Scenario and regression tests driven through the public `update()` API.
//!
//! The whole input surface funnels through `update(state, Action) -> Vec<Effect>`,
//! so these tests can script real user flows with no TTY, no threads, and no
//! sleeps, and assert the exact commands that will run. Two extra nets sit on
//! top of the scenario tests:
//!
//! - every transition is checked against `AppState::check_invariants` (both
//!   here and via a `debug_assert!` inside `update`), and
//! - a transition-table test walks every mode × key/mouse combination and a
//!   seeded fuzz test drives random action sequences, so a future change that
//!   breaks a structural rule fails instantly.

use crate::action::Action;
use crate::brew::Package;
use crate::brewfile::{Entry, EntryKind};
use crate::effect::Effect;
use crate::state::{AppMode, AppState, ModalAction, Panel, Section};
use crate::theme::THEMES;
use crate::update::update;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

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

fn service(name: &str, status: Option<&str>) -> Package {
    let mut p = pkg(name, false, false);
    p.service_status = status.map(String::from);
    p
}

fn tap(name: &str) -> Package {
    let mut p = pkg(name, true, false);
    p.tap = Some(name.into());
    p
}

/// A populated, applied model in the Installed section.
fn base() -> AppState {
    let mut s = AppState {
        packages: vec![pkg("git", false, false), pkg("wget", false, false)],
        ..AppState::default()
    };
    s.apply_section();
    s
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
}

fn kcode(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Fold actions into the state, collecting the effects and asserting the
/// model stays coherent after every single step.
fn drive(state: &mut AppState, actions: &[Action]) -> Vec<Effect> {
    let mut effects = Vec::new();
    for (i, a) in actions.iter().enumerate() {
        effects.extend(update(state, a.clone()));
        state
            .check_invariants()
            .unwrap_or_else(|v| panic!("step {i}: state invariant broken: {v}"));
    }
    effects
}

fn typed(name: &str) -> Vec<Action> {
    name.chars().map(|c| Action::Key(key(c))).collect()
}

/// Registry index of the command whose label contains `needle`.
fn cmd_index(needle: &str) -> usize {
    crate::registry::commands()
        .iter()
        .position(|c| c.label.contains(needle))
        .unwrap_or_else(|| panic!("no registry command matching {needle:?}"))
}

/// Open the action menu (`x`) and navigate down to the command whose label
/// contains `needle`, then run it — exercising the index-based menu path that
/// keyless commands rely on.
fn drive_menu_to(state: &mut AppState, needle: &str) -> Vec<Effect> {
    let mut actions = vec![Action::Key(key('x'))];
    for _ in 0..cmd_index(needle) {
        actions.push(Action::Key(key('j')));
    }
    actions.push(Action::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::empty(),
    )));
    drive(state, &actions)
}

// --------------------------------------------------------------------------
// Scenario tests: exact effects for real user flows
// --------------------------------------------------------------------------

#[test]
fn install_flow_emits_exactly_one_brew_install() {
    let mut s = base();
    let fx = drive(&mut s, &[Action::Key(key('i')), Action::Key(key('y'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec!["install".into(), "git".into()]])]
    );
    assert!(s.output.iter().any(|l| l == "$ brew install git"));
    assert!(s.cmd_active, "command runner should be marked active");
    assert_eq!(s.output_offset, 0);
}

#[test]
fn done_marks_the_result_and_requests_reload() {
    let mut s = base();
    s.cmd_active = true;
    let fx = drive(&mut s, &[Action::CmdDone(true)]);
    assert_eq!(fx, [Effect::Reload]);
    assert_eq!(s.output.last().map(String::as_str), Some("== done =="));
    assert!(!s.cmd_active);
    assert!(
        s.loading_packages,
        "a finished command triggers a data reload"
    );
}

#[test]
fn cask_install_passes_the_cask_flag() {
    let mut s = base();
    s.packages.push(pkg("firefox", false, true));
    s.apply_section();
    s.list_idx = 2; // move onto firefox
    let fx = drive(&mut s, &[Action::Key(key('i')), Action::Key(key('y'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec![
            "install".into(),
            "--cask".into(),
            "firefox".into()
        ]])]
    );
}

#[test]
fn upgrade_all_confirms_with_the_outdated_count() {
    let mut s = base();
    s.packages[0].outdated = true;
    s.apply_section();
    drive(&mut s, &[Action::Key(key('A'))]);
    let AppMode::Confirm(modal) = &s.mode else {
        panic!("expected confirm, got {:?}", s.mode);
    };
    assert!(modal.text.contains("1 outdated"));
    let fx = drive(&mut s, &[Action::Key(key('y'))]);
    assert_eq!(fx, [Effect::RunBrew(vec![vec!["upgrade".into()]])]);
}

#[test]
fn menu_pin_and_unpin_emit_a_direct_brew_command() {
    let mut s = base();
    let fx = drive(&mut s, &[Action::Key(key('x')), Action::Key(key('p'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec!["pin".into(), "git".into()]])]
    );

    s.filtered[0].pinned = true;
    let fx = drive(&mut s, &[Action::Key(key('x')), Action::Key(key('p'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec!["unpin".into(), "git".into()]])]
    );
}

#[test]
fn link_and_unlink_hotkeys_stage_brew_commands() {
    let mut s = base();
    assert_eq!(
        drive(&mut s, &[Action::Key(key('L'))]),
        [Effect::RunBrew(vec![vec!["link".into(), "git".into()]])]
    );

    let mut s = base();
    assert_eq!(
        drive(&mut s, &[Action::Key(key('Y'))]),
        [Effect::RunBrew(vec![vec!["unlink".into(), "git".into()]])]
    );
}

#[test]
fn registry_declares_unique_shortcuts_and_keeps_the_classic_ones() {
    let mut seen = std::collections::HashSet::new();
    for cmd in crate::registry::commands() {
        if let Some(KeyCode::Char(c)) = cmd.key {
            assert!(seen.insert(c), "two commands claim the shortcut '{c}'");
        }
        assert!(!cmd.label.trim().is_empty(), "empty label in the registry");
        assert!(
            !cmd.desc.trim().is_empty(),
            "empty description in the registry"
        );
    }
    for c in ['u', 'R', 'r', 'i', 'd', 'p', 'o'] {
        assert!(seen.contains(&c), "classic shortcut '{c}' disappeared");
    }
}

#[test]
fn menu_reaches_keyless_commands_by_navigation() {
    let mut s = base();
    assert_eq!(
        drive_menu_to(&mut s, "Link"),
        [Effect::RunBrew(vec![vec!["link".into(), "git".into()]])]
    );

    let mut s = base();
    assert_eq!(
        drive_menu_to(&mut s, "Unlink"),
        [Effect::RunBrew(vec![vec!["unlink".into(), "git".into()]])]
    );

    let mut s = base();
    assert_eq!(
        drive_menu_to(&mut s, "Uses"),
        [Effect::RunBrew(vec![vec![
            "uses".into(),
            "--installed".into(),
            "git".into()
        ]])]
    );

    let mut s = base();
    assert_eq!(
        drive_menu_to(&mut s, "Missing"),
        [Effect::RunBrew(vec![vec!["missing".into(), "git".into()]])]
    );
}

#[test]
fn registry_gates_commands_to_their_context() {
    // `brew uses` is formula-only: inert on a selected cask.
    let mut cask = base();
    cask.packages.push(pkg("firefox", true, true));
    cask.apply_section();
    cask.list_idx = 2;
    let uses = cmd_index("Uses");
    assert!(crate::registry::run_index(&mut cask, uses).is_empty());

    // Service commands only fire inside the Services section.
    let restart = cmd_index("Restart");
    let run = cmd_index("Run service");
    let mut installed = base();
    assert!(crate::registry::run_index(&mut installed, restart).is_empty());
    assert!(crate::registry::run_index(&mut installed, run).is_empty());

    let mut services = AppState {
        sections: vec![
            Section::Services,
            Section::Installed,
            Section::Outdated,
            Section::Casks,
            Section::Leaves,
            Section::Catalog,
        ],
        section_idx: 0,
        services: vec![service("nginx", Some("started"))],
        ..AppState::default()
    };
    services.apply_section();
    assert_eq!(
        crate::registry::run_index(&mut services, restart),
        [Effect::RunBrew(vec![vec![
            "services".into(),
            "restart".into(),
            "nginx".into()
        ]])]
    );
    assert_eq!(
        crate::registry::run_index(&mut services, run),
        [Effect::RunBrew(vec![vec![
            "services".into(),
            "run".into(),
            "nginx".into()
        ]])]
    );
}

#[test]
fn tap_add_and_untap_flow_from_the_taps_section() {
    let mut s = AppState {
        taps: vec![tap("homebrew/cask-drivers")],
        section_idx: 6, // Taps (last in the default sidebar)
        ..AppState::default()
    };
    s.apply_section();
    assert_eq!(s.selected().unwrap().name, "homebrew/cask-drivers");

    // 'r' untaps the selected tap.
    let fx = drive(&mut s, &[Action::Key(key('r')), Action::Key(key('y'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec![
            "untap".into(),
            "homebrew/cask-drivers".into()
        ]])]
    );

    // 'i' opens the type-a-name prompt; a typed tap installs it.
    let mut actions = vec![Action::Key(key('i'))];
    actions.extend(typed("homebrew/foo"));
    actions.push(Action::Key(kcode(KeyCode::Enter)));
    actions.push(Action::Key(key('y')));
    let fx = drive(&mut s, &actions);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec![
            "tap".into(),
            "homebrew/foo".into()
        ]])]
    );
}

#[test]
fn brewfile_install_runs_taps_first_then_formulae() {
    let mut s = AppState {
        sections: vec![
            Section::Brewfile,
            Section::Installed,
            Section::Outdated,
            Section::Casks,
            Section::Leaves,
            Section::Catalog,
            Section::Services,
        ],
        section_idx: 0,
        brewfile_entries: vec![
            Entry {
                kind: EntryKind::Tap,
                name: "homebrew/cask-fonts".into(),
            },
            Entry {
                kind: EntryKind::Brew,
                name: "hack".into(),
            },
        ],
        brewfile: vec![pkg("hack", false, false)],
        ..AppState::default()
    };
    s.apply_section();
    let fx = drive(&mut s, &[Action::Key(key('I')), Action::Key(key('y'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![
            vec!["tap".into(), "homebrew/cask-fonts".into()],
            vec!["install".into(), "hack".into()],
        ])]
    );
    assert_eq!(
        &s.output,
        &["$ brew tap homebrew/cask-fonts", "$ brew install hack"]
    );
}

#[test]
fn vuln_scan_streams_then_caches_the_result() {
    let mut s = base();
    let fx = drive(&mut s, &[Action::Key(key('v'))]);
    assert_eq!(fx, [Effect::RunVulnScan("git".into())]);
    assert!(s.output.iter().any(|l| l == "$ brew vulns git"));
    assert!(s.cmd_active);

    drive(
        &mut s,
        &[Action::VulnsScanned(
            "git".into(),
            vec!["[CVE-1] shellcheck".into()],
        )],
    );
    assert!(s.output.iter().any(|l| l == "git: 1 vulnerabilities"));
    assert_eq!(s.vulns.get("git").map(Vec::len), Some(1));
}

#[test]
fn vuln_scan_on_a_cask_is_rejected() {
    let mut s = base();
    s.packages.push(pkg("firefox", true, true));
    s.apply_section();
    s.list_idx = 2;
    let fx = drive(&mut s, &[Action::Key(key('v'))]);
    assert!(fx.is_empty());
    assert!(s.output.iter().any(|l| l.contains("formulae")));
}

#[test]
fn vulns_missing_prompts_to_install_brew_vulns() {
    let mut s = base();
    drive(&mut s, &[Action::VulnsMissing]);
    let AppMode::Confirm(modal) = &s.mode else {
        panic!("expected confirm, got {:?}", s.mode);
    };
    assert!(matches!(modal.confirm, ModalAction::InstallVulns));
    let fx = drive(&mut s, &[Action::Key(key('y'))]);
    assert_eq!(
        fx,
        [Effect::RunBrew(vec![vec![
            "install".into(),
            "homebrew/brew-vulns/brew-vulns".into()
        ]])]
    );
}

#[test]
fn doctor_and_config_are_direct_effects() {
    let mut s = base();
    assert_eq!(
        drive(&mut s, &[Action::Key(key('D'))]),
        [Effect::RunBrew(vec![vec!["doctor".into()]])]
    );
    assert_eq!(
        drive(&mut s, &[Action::Key(key('C'))]),
        [Effect::RunBrew(vec![vec!["config".into()]])]
    );
}

#[test]
fn services_section_toggles_start_then_stop() {
    let mut s = AppState {
        sections: vec![
            Section::Services,
            Section::Installed,
            Section::Outdated,
            Section::Casks,
            Section::Leaves,
            Section::Catalog,
        ],
        section_idx: 0,
        services: vec![service("nginx", None)],
        ..AppState::default()
    };
    s.apply_section();
    assert_eq!(
        drive(&mut s, &[Action::Key(key('s'))]),
        [Effect::RunBrew(vec![vec![
            "services".into(),
            "start".into(),
            "nginx".into()
        ]])]
    );
    s.services[0].service_status = Some("started".into());
    s.apply_section();
    assert_eq!(
        drive(&mut s, &[Action::Key(key('s'))]),
        [Effect::RunBrew(vec![vec![
            "services".into(),
            "stop".into(),
            "nginx".into()
        ]])]
    );
}

#[test]
fn self_update_requests_the_release_swap() {
    let mut s = base();
    let fx = drive(&mut s, &[Action::Key(key('W')), Action::Key(key('y'))]);
    assert_eq!(fx.len(), 1, "self-update is one effect, not a brew command");
    match &fx[0] {
        Effect::RunSelfUpdate { url, target, exe } => {
            assert!(
                url.contains("releases/latest/download/"),
                "url should point at the release tarball: {url}"
            );
            assert!(!target.is_empty());
            assert!(exe.file_name().is_some());
        }
        other => panic!("expected RunSelfUpdate, got {other:?}"),
    }
    assert!(s.output.iter().any(|l| l.starts_with("$ curl")));
    assert!(s.cmd_active);
}

#[test]
fn theme_picker_enter_persists_the_choice() {
    let mut s = base();
    let fx = drive(
        &mut s,
        &[
            Action::Key(key('t')),
            Action::Key(key('j')),
            Action::Key(kcode(KeyCode::Enter)),
        ],
    );
    assert_eq!(fx, [Effect::SaveTheme(THEMES[1])]);
    assert_eq!(s.mode, AppMode::Normal);
    assert_eq!(s.theme, THEMES[1]);
}

#[test]
fn quit_key_returns_the_quit_effect() {
    let mut s = base();
    assert_eq!(drive(&mut s, &[Action::Key(key('q'))]), [Effect::Quit]);
}

#[test]
fn staging_a_command_resets_scroll_and_previous_output() {
    let mut s = base();
    s.output = vec!["old line".into(), "another".into()];
    s.output_offset = 2;
    let fx = drive(&mut s, &[Action::Key(key('D'))]);
    assert_eq!(fx, [Effect::RunBrew(vec![vec!["doctor".into()]])]);
    assert_eq!(s.output, ["$ brew doctor"]);
    assert_eq!(s.output_offset, 0, "a new command snaps back to the tail");
}

#[test]
fn the_reload_chain_flags_clear_in_order() {
    let mut s = base();
    drive(
        &mut s,
        &[Action::InstalledLoaded((
            vec![pkg("git", true, false)],
            vec!["git".into()],
            vec![],
            vec![],
        ))],
    );
    assert!(!s.loading_packages);
    assert!(
        s.loading_catalog,
        "catalog refresh is pending after installed data"
    );
    assert_eq!(s.packages.len(), 1, "installed list replaced");

    drive(
        &mut s,
        &[Action::CatalogLoaded((vec![], Default::default()))],
    );
    assert!(!s.loading_catalog);
}

// --------------------------------------------------------------------------
// Mouse navigation
// --------------------------------------------------------------------------

#[test]
fn sidebar_click_switches_section_then_focuses_the_list() {
    let mut s = base();
    drive(
        &mut s,
        &[Action::Mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            5,
            4,
        ))],
    );
    assert_eq!(s.section_idx, 1, "row 4 -> sidebar item 1");
    assert_eq!(s.panel, Panel::List);
}

// --------------------------------------------------------------------------
// The regression nets
// --------------------------------------------------------------------------

fn key_matrix() -> Vec<KeyEvent> {
    let mut v: Vec<KeyEvent> =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/?' .,-"
            .chars()
            .map(key)
            .collect();
    for code in [
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        v.push(kcode(code));
    }
    for n in 1..=12 {
        v.push(KeyEvent::new(KeyCode::F(n), KeyModifiers::empty()));
    }
    v.push(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    v
}

fn mouse_matrix() -> Vec<MouseEvent> {
    vec![
        mouse(MouseEventKind::ScrollUp, 50, 10),
        mouse(MouseEventKind::ScrollDown, 50, 10),
        mouse(MouseEventKind::Down(MouseButton::Left), 5, 4),
        mouse(MouseEventKind::Down(MouseButton::Left), 5, 30),
        mouse(MouseEventKind::Down(MouseButton::Left), 30, 6),
        mouse(MouseEventKind::Down(MouseButton::Left), 30, 2),
        mouse(MouseEventKind::Up(MouseButton::Left), 5, 4),
        mouse(MouseEventKind::Drag(MouseButton::Left), 30, 7),
        mouse(MouseEventKind::Moved, 30, 7),
    ]
}

fn state_in_mode(mode: AppMode) -> AppState {
    let mut s = base();
    s.mode = mode;
    s
}

/// The transition table: every mode tolerates every key and mouse event
/// without panicking or leaving the model structurally broken. Since every
/// key funnels through `Action`, this table is effectively the keymap spec —
/// adding a binding that collides with an existing mode fails here.
#[test]
fn every_mode_tolerates_every_key_and_mouse_event() {
    let modes = [
        AppMode::Normal,
        AppMode::Search,
        AppMode::Prompt,
        AppMode::Confirm(crate::state::modal(
            "irrelevant text",
            ModalAction::Install("git".into(), false),
        )),
        AppMode::Menu(0),
        AppMode::ThemePicker(0),
        AppMode::Help,
    ];
    for mode in &modes {
        for k in key_matrix() {
            let mut s = state_in_mode(mode.clone());
            update(&mut s, Action::Key(k));
            s.check_invariants()
                .unwrap_or_else(|v| panic!("mode {mode:?}: {v}"));
        }
        for m in mouse_matrix() {
            let mut s = state_in_mode(mode.clone());
            update(&mut s, Action::Mouse(m));
            s.check_invariants()
                .unwrap_or_else(|v| panic!("mode {mode:?}: {v}"));
        }
    }
}

/// A deterministic fuzz sweep: random action sequences (including background
/// loads and re-entrant navigation) must never break an invariant.
#[test]
fn fuzz_driving_actions_never_breaks_invariants() {
    let mut s = base();
    let kitchen_sink = vec![
        Action::Key(key('j')),
        Action::Key(key('k')),
        Action::Key(key('g')),
        Action::Key(key('G')),
        Action::Key(key('l')),
        Action::Key(key('h')),
        Action::Key(key('i')),
        Action::Key(key('y')),
        Action::Key(key('x')),
        Action::Key(key('p')),
        Action::Key(key('D')),
        Action::Key(key('B')),
        Action::Key(key('?')),
        Action::Key(kcode(KeyCode::PageUp)),
        Action::Key(kcode(KeyCode::PageDown)),
        Action::Key(kcode(KeyCode::Enter)),
        Action::Key(kcode(KeyCode::Esc)),
        Action::CmdLine("some output line".into()),
        Action::CmdDone(true),
        Action::CmdDone(false),
        Action::VulnsScanned("git".into(), vec![]),
        Action::VulnsMissing,
        Action::InstalledLoaded((
            vec![pkg("git", true, false)],
            vec!["git".into()],
            vec![],
            vec![],
        )),
        Action::CatalogLoaded((vec![], Default::default())),
        Action::Mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 4)),
        Action::Mouse(mouse(MouseEventKind::ScrollDown, 50, 10)),
    ];

    // xorshift64: deterministic across runs and platforms.
    let mut seed: u64 = 0xDECAFBAD_DEADBEEF;
    for step in 0..300 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let pick = (seed as usize) % kitchen_sink.len();
        update(&mut s, kitchen_sink[pick].clone());
        s.check_invariants()
            .unwrap_or_else(|v| panic!("fuzz step {step}: {v}"));
    }
}
