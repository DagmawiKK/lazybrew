//! Legacy aggregator while the App split lands.
//!
//! The model lives in `state.rs`, the handlers in `update.rs`, and the
//! input seam in `action.rs`. Commit B replaces this module with
//! `effect.rs` + `exec.rs` + `runtime.rs` and deletes it.
//!
//! For now it re-exports the state types (so `crate::app::App` etc. still
//! resolve) and hosts the background command plumbing, which still mutates
//! `App` directly.

/// Compatibility name while the split lands; all references to
/// `crate::app::App` resolve to the model in `state.rs`.
pub use crate::state::AppState as App;
pub use crate::state::*;

use std::sync::mpsc;

/// Background load of installed packages + leaves + services + taps.
pub fn spawn_load_thread() -> mpsc::Receiver<LoadResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let pkgs = crate::brew::load_installed().unwrap_or_default();
        let leaves = crate::brew::load_leaves();
        let services = crate::brew::load_services();
        let taps = crate::brew::load_taps();
        let _ = tx.send((pkgs, leaves, services, taps));
    });
    rx
}

/// Spawn the catalog fetch separately so the installed list is never
/// blocked behind the (much larger) remote catalog download.
pub fn spawn_catalog_thread(installed: Vec<crate::brew::Package>) -> mpsc::Receiver<CatalogData> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let catalog = crate::catalog::load_catalog(&installed).unwrap_or_default();
        let _ = tx.send(catalog);
    });
    rx
}

pub fn run_menu_action(app: &mut AppState, idx: usize) {
    let Some(p) = app.selected().cloned() else {
        return;
    };
    // Indices match MENU_ACTIONS.
    match idx {
        0 => {
            app.mode = AppMode::Confirm(modal(
                format!("Upgrade '{}'? (y/n)", p.name),
                ModalAction::Upgrade(p.name, p.cask),
            ))
        }
        1 => {
            app.mode = AppMode::Confirm(modal(
                format!("Reinstall '{}'? (y/n)", p.name),
                ModalAction::Reinstall(p.name, p.cask),
            ))
        }
        2 => {
            app.mode = AppMode::Confirm(modal(
                format!("Remove '{}'? (y/n)", p.name),
                ModalAction::Remove(p.name, p.cask),
            ))
        }
        3 => spawn_brew(app, &["info".into(), p.name]),
        4 => spawn_brew(app, &["deps".into(), p.name]),
        5 => {
            let verb = if p.pinned { "unpin" } else { "pin" };
            spawn_brew(app, &[verb.into(), p.name]);
        }
        6 => spawn_brew(app, &["home".into(), p.name]),
        _ => {}
    }
}

pub fn run_modal_action(app: &mut AppState, modal: &Modal) {
    // Self-update does not go through brew; swap in the newest release binary.
    if matches!(modal.confirm, ModalAction::SelfUpdate) {
        spawn_self_update(app);
        return;
    }
    let commands: Vec<Vec<String>> = match &modal.confirm {
        ModalAction::Upgrade(name, cask) => {
            let mut a = vec!["upgrade".into()];
            if *cask {
                a.push("--cask".into());
            }
            a.push(name.clone());
            vec![a]
        }
        ModalAction::Remove(name, cask) => {
            let mut a = vec!["uninstall".into()];
            if *cask {
                a.push("--cask".into());
            }
            a.push(name.clone());
            vec![a]
        }
        ModalAction::Reinstall(name, cask) => {
            let mut a = vec!["reinstall".into()];
            if *cask {
                a.push("--cask".into());
            }
            a.push(name.clone());
            vec![a]
        }
        ModalAction::Install(name, cask) => {
            let mut cmd = vec!["install".into()];
            if *cask {
                cmd.push("--cask".into());
            }
            cmd.push(name.clone());
            vec![cmd]
        }
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
            brewfile_commands(app, matches!(modal.confirm, ModalAction::BrewfileInstall))
        }
        ModalAction::Tap(name) => vec![vec!["tap".into(), name.clone()]],
        ModalAction::Untap(name) => vec![vec!["untap".into(), name.clone()]],
        ModalAction::SelfUpdate => unreachable!("handled before the brew match"),
    };
    spawn_brew_multi(app, commands);
}

/// Build the command sequence for batch Brewfile install/remove.
/// Taps are handled first on install so formulae resolve.
fn brewfile_commands(app: &AppState, install: bool) -> Vec<Vec<String>> {
    let taps: Vec<String> = app
        .brewfile_entries
        .iter()
        .filter(|e| e.kind == crate::brewfile::EntryKind::Tap)
        .map(|e| e.name.clone())
        .collect();
    let formulae: Vec<String> = app
        .brewfile
        .iter()
        .filter(|p| !p.cask)
        .map(|p| p.name.clone())
        .collect();
    let casks: Vec<String> = app
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

pub fn spawn_brew(app: &mut AppState, args: &[String]) {
    spawn_brew_multi(app, vec![args.to_vec()]);
}

/// Run a sequence of brew commands sequentially, streaming output for each.
pub fn spawn_brew_multi(app: &mut AppState, commands: Vec<Vec<String>>) {
    app.output.clear();
    app.output_offset = 0;
    for c in &commands {
        app.output.push(format!("$ brew {}", c.join(" ")));
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut all_ok = true;
        for args in commands {
            if !run_brew_command(&tx, &args) {
                all_ok = false;
            }
        }
        let _ = tx.send(CmdEvent::Done(all_ok));
    });
    app.cmd_rx = Some(rx);
}

/// Run one brew command, streaming each output line to `tx`.
/// Returns true if the command exited successfully.
fn run_brew_command(tx: &mpsc::Sender<CmdEvent>, args: &[String]) -> bool {
    let mut cmd = std::process::Command::new("brew");
    cmd.args(args)
        .env("NONINTERACTIVE", "1")
        .env("HOMEBREW_NO_AUTO_UPDATE", "0");
    stream_cmd(tx, &mut cmd)
}

/// Spawn `cmd`, stream stdout+stderr line-by-line to `tx`, and report success.
/// Also used by `self_update::run` for the curl/tar steps.
pub(crate) fn stream_cmd(tx: &mpsc::Sender<CmdEvent>, cmd: &mut std::process::Command) -> bool {
    let mut child = match cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(CmdEvent::Line(format!("spawn error: {}", e)));
            return false;
        }
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    let t1 = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            let _ = tx_out.send(CmdEvent::Line(line));
        }
    });
    let t2 = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let _ = tx_err.send(CmdEvent::Line(line));
        }
    });
    let status = child.wait().map(|s| s.success()).unwrap_or(false);
    let _ = t1.join();
    let _ = t2.join();
    status
}

/// Update lazybrew itself: download the newest GitHub release tarball and
/// replace the running binary. Progress streams through the output pane,
/// spawned like any other background command.
pub fn spawn_self_update(app: &mut AppState) {
    let target = crate::self_update::target_triple();
    let url = crate::self_update::download_url(target);
    let exe = std::env::current_exe().unwrap_or_else(|_| "lazybrew".into());
    app.output.clear();
    app.output_offset = 0;
    app.output
        .push(format!("$ curl -fsSL -o <tmp tarball> {url}"));
    app.output.push(format!("$ install to {}", exe.display()));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ok = crate::self_update::run(&tx, &url, target, &exe);
        let _ = tx.send(CmdEvent::Done(ok));
    });
    app.cmd_rx = Some(rx);
}

/// Scan the selected package for known vulnerabilities.
/// Streams human-readable output while scanning, then caches the JSON result.
pub fn spawn_vuln_scan(app: &mut AppState, name: String) {
    app.output.clear();
    app.output.push(format!("$ brew vulns {name}"));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let status = std::process::Command::new("brew")
            .args(["vulns", &name])
            .env("NONINTERACTIVE", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        let out = match status {
            Ok(o) => o,
            Err(e) => {
                let _ = tx.send(CmdEvent::Line(format!("error: {e}")));
                let _ = tx.send(CmdEvent::Done(false));
                return;
            }
        };
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let err_text = String::from_utf8_lossy(&out.stderr).to_string();
        if !err_text.contains("vulns") || out.status.code() == Some(127) {
            // command not found / unknown command
            if err_text.contains("No such file")
                || err_text.contains("unknown command")
                || err_text.contains("not found")
            {
                let _ = tx.send(CmdEvent::VulnsMissing);
                return;
            }
        }
        for line in text.lines().chain(err_text.lines()) {
            let _ = tx.send(CmdEvent::Line(line.to_string()));
        }
        // Structured result (best effort)
        let json = std::process::Command::new("brew")
            .args(["vulns", &name, "--json"])
            .env("NONINTERACTIVE", "1")
            .output();
        let mut vulns = Vec::new();
        if let Ok(j) = json
            && j.status.success()
            && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&j.stdout)
            && let Some(arr) = parsed.as_array()
        {
            for entry in arr {
                if let Some(list) = entry.get("vulnerabilities").and_then(|v| v.as_array()) {
                    for v in list {
                        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("CVE");
                        let summary = v
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or("(no summary)");
                        vulns.push(format!("[{id}] {summary}"));
                    }
                }
            }
        }
        let _ = tx.send(CmdEvent::Vulns(name, vulns));
        let _ = tx.send(CmdEvent::Done(
            !out.stdout.is_empty() || out.status.success(),
        ));
    });
    app.cmd_rx = Some(rx);
}
