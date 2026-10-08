//! The effect executor: background threads that carry out loads and commands.
//!
//! Each spawn function owns a channel and hands the receiver to the runtime,
//! which converts the typed events it receives into
//! [`crate::action::Action`]s and feeds them back to `update`. Nothing here
//! touches the model.

use crate::brew::Package;
use crate::state::{CatalogData, LoadResult};
use std::path::PathBuf;
use std::sync::mpsc;

/// Events streamed by a running background command (brew, vulns, self-update).
pub enum CmdEvent {
    Line(String),
    Done(bool),
    /// Vulnerability scan finished: package name, list of advisory summaries.
    Vulns(String, Vec<String>),
    /// `brew vulns` is not installed.
    VulnsMissing,
}

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
pub fn spawn_catalog_thread(installed: Vec<Package>) -> mpsc::Receiver<CatalogData> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let catalog = crate::catalog::load_catalog(&installed).unwrap_or_default();
        let _ = tx.send(catalog);
    });
    rx
}

/// Run a sequence of brew commands sequentially, streaming output for each.
pub fn spawn_brew_thread(commands: Vec<Vec<String>>) -> mpsc::Receiver<CmdEvent> {
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
    rx
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

/// Scan the selected package for known vulnerabilities.
/// Streams human-readable output while scanning, then caches the JSON result.
pub fn spawn_vuln_thread(name: String) -> mpsc::Receiver<CmdEvent> {
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
    rx
}

/// Update lazybrew itself: download the newest GitHub release tarball and
/// replace the running binary. Progress streams through the output pane,
/// spawned like any other background command.
pub fn spawn_self_update_thread(
    url: String,
    target: String,
    exe: PathBuf,
) -> mpsc::Receiver<CmdEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ok = crate::self_update::run(&tx, &url, &target, &exe);
        let _ = tx.send(CmdEvent::Done(ok));
    });
    rx
}
