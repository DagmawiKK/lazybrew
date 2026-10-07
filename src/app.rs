//! Application state and brew command execution.

use crate::brew::{self, Package};
use crate::theme::Theme;
use std::sync::mpsc;

pub enum CmdEvent {
    Line(String),
    Done(bool),
    /// Vulnerability scan finished: package name, list of advisory summaries.
    Vulns(String, Vec<String>),
    /// `brew vulns` is not installed.
    VulnsMissing,
}

pub type LoadResult = (Vec<Package>, Vec<String>, Vec<Package>, Vec<Package>);

/// 90-day install analytics, `name -> install count`.
pub type Popularity = std::collections::HashMap<String, u64>;

/// The catalog payload streamed over its own channel.
pub type CatalogData = (Vec<Package>, Popularity);

/// Sections shown in the lazygit-style left sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Installed,
    Outdated,
    Casks,
    Leaves,
    Catalog,
    Services,
    Brewfile,
    Taps,
}

impl Section {
    pub fn title(&self) -> &'static str {
        match self {
            Section::Installed => "Installed",
            Section::Outdated => "Outdated",
            Section::Casks => "Casks",
            Section::Leaves => "Leaves",
            Section::Catalog => "Catalog",
            Section::Services => "Services",
            Section::Brewfile => "Brewfile",
            Section::Taps => "Taps",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Sidebar,
    List,
}

/// How the active section's list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    /// Source order (brew/API order).
    Natural,
    /// Case-insensitive alphabetical.
    Name,
    /// By 90-day install popularity, descending.
    Installs,
}

impl SortMode {
    pub const ALL: [SortMode; 3] = [SortMode::Natural, SortMode::Name, SortMode::Installs];

    pub fn next(self) -> SortMode {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Natural => "natural",
            SortMode::Name => "name",
            SortMode::Installs => "installs",
        }
    }
}

/// The single explicit UI state. Exactly one mode is active at a time —
/// this replaces the earlier set of independent booleans/options (searching,
/// installing, modal, menu, help, theme_picker) that could silently collide.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Browsing the sidebar/list; all default keys active.
    Normal,
    /// `/` search is being typed; input goes into `App.search`.
    Search,
    /// Type-a-name prompt (`I`, or `i` without a valid selection);
    /// input goes into `App.prompt_buffer`.
    Prompt,
    /// Confirmation dialog.
    Confirm(Modal),
    /// Action menu for the selected package.
    Menu(usize),
    /// Theme picker overlay.
    ThemePicker(usize),
    /// Help overlay — any key closes it.
    Help,
}

pub struct App {
    pub packages: Vec<Package>,
    pub filtered: Vec<Package>,
    pub section_idx: usize,
    pub list_idx: usize,
    pub panel: Panel,
    pub sort: SortMode,
    pub leaves: Vec<String>,
    pub search: String,
    pub prompt_buffer: String,
    /// The active UI state (exactly one mode).
    pub mode: AppMode,
    pub output: Vec<String>,
    pub cmd_rx: Option<mpsc::Receiver<CmdEvent>>,
    pub frame: usize,
    pub load_rx: Option<mpsc::Receiver<LoadResult>>,
    pub catalog_rx: Option<mpsc::Receiver<CatalogData>>,
    pub catalog: Vec<Package>,
    /// 90-day install analytics, by package name.
    pub installs: Popularity,
    pub taps: Vec<Package>,
    pub services: Vec<Package>,
    pub vulns: std::collections::HashMap<String, Vec<String>>,
    /// Active color theme.
    pub theme: Theme,
    /// Sections actually shown in the sidebar (Brewfile only in -f mode).
    pub sections: Vec<Section>,
    /// Entries parsed from the -f Brewfile.
    pub brewfile_entries: Vec<crate::brewfile::Entry>,
    /// Brewfile entries resolved against installed + catalog data.
    pub brewfile: Vec<Package>,
}

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner(app: &App) -> char {
    SPINNER[app.frame % SPINNER.len()]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Modal {
    pub text: String,
    pub confirm: ModalAction,
}

/// The actions menu items, in index order; the active index is carried by
/// `AppMode::Menu`. Shortcut letters lead each label.
pub const MENU_ACTIONS: &[&str] = &[
    "u  Upgrade",
    "R  Reinstall",
    "r  Remove",
    "i  Info",
    "d  Deps",
    "p  Pin/Unpin",
];

/// Convenience constructor for confirm dialogs.
pub fn modal(text: impl Into<String>, confirm: ModalAction) -> Modal {
    Modal {
        text: text.into(),
        confirm,
    }
}

impl App {
    /// Take and close the confirm dialog, if one is open.
    pub fn take_modal(&mut self) -> Option<Modal> {
        match std::mem::replace(&mut self.mode, AppMode::Normal) {
            AppMode::Confirm(m) => Some(m),
            other => {
                self.mode = other;
                None
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModalAction {
    Upgrade(String, bool), // name, is_cask
    Reinstall(String, bool),
    Remove(String, bool),
    Install(String, bool),
    Update,
    UpgradeAll,
    Cleanup,
    Autoremove,
    InstallVulns,
    BrewfileInstall,
    BrewfileRemove,
    Tap(String),
    Untap(String),
}

impl App {
    pub fn apply_section(&mut self) {
        let section = self.sections[self.section_idx];
        let source: &[Package] = match section {
            Section::Catalog => &self.catalog,
            Section::Services => &self.services,
            Section::Brewfile => &self.brewfile,
            Section::Taps => &self.taps,
            _ => &self.packages,
        };
        let q = self.search.to_lowercase();
        self.filtered = source
            .iter()
            .filter(|p| match section {
                Section::Catalog | Section::Services | Section::Brewfile | Section::Taps => true,
                Section::Installed => true,
                Section::Outdated => p.outdated,
                Section::Casks => p.cask,
                Section::Leaves => !p.cask && self.leaves.iter().any(|l| l == &p.name),
            })
            .filter(|p| {
                q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.desc.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        let installs = &self.installs;
        match self.sort {
            SortMode::Natural => {}
            SortMode::Name => self.filtered.sort_by_key(|p| p.name.to_lowercase()),
            SortMode::Installs => self.filtered.sort_by(|x, y| {
                let ix = installs.get(&x.name).copied().unwrap_or(0);
                let iy = installs.get(&y.name).copied().unwrap_or(0);
                iy.cmp(&ix)
            }),
        }
        self.list_idx = 0;
    }

    /// Count of items in a section, for the sidebar display.
    pub fn count_for(&self, section: Section) -> usize {
        match section {
            Section::Installed => self.packages.len(),
            Section::Outdated => self.packages.iter().filter(|p| p.outdated).count(),
            Section::Casks => self.packages.iter().filter(|p| p.cask).count(),
            Section::Leaves => self.leaves.len(),
            Section::Catalog => self.catalog.len(),
            Section::Services => self.services.len(),
            Section::Brewfile => self.brewfile.len(),
            Section::Taps => self.taps.len(),
        }
    }

    /// Resolve Brewfile entries against installed and catalog data.
    pub fn refresh_brewfile(&mut self) {
        let mut resolved = Vec::with_capacity(self.brewfile_entries.len());
        for entry in &self.brewfile_entries {
            let want_cask = matches!(entry.kind, crate::brewfile::EntryKind::Cask);
            if matches!(entry.kind, crate::brewfile::EntryKind::Tap) {
                continue;
            }
            let pkg = self
                .packages
                .iter()
                .chain(self.catalog.iter())
                .find(|p| p.name == entry.name && p.cask == want_cask)
                .cloned()
                .unwrap_or_else(|| Package {
                    name: entry.name.clone(),
                    desc: "(from Brewfile)".into(),
                    version: "?".into(),
                    cask: want_cask,
                    outdated: false,
                    installed_version: None,
                    pinned: false,
                    service_status: None,
                    deprecation: None,
                });
            resolved.push(pkg);
        }
        self.brewfile = resolved;
    }

    /// True when a resolved Brewfile package is not installed yet.
    pub fn brewfile_missing(&self) -> usize {
        self.brewfile
            .iter()
            .filter(|p| p.installed_version.is_none())
            .count()
    }

    pub fn selected(&self) -> Option<&Package> {
        self.filtered.get(self.list_idx)
    }
}

pub fn spawn_load_thread() -> mpsc::Receiver<LoadResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let pkgs = brew::load_installed().unwrap_or_default();
        let leaves = brew::load_leaves();
        let services = brew::load_services();
        let taps = brew::load_taps();
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

pub fn run_menu_action(app: &mut App, idx: usize) {
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
        _ => {}
    }
}

pub fn run_modal_action(app: &mut App, modal: &Modal) {
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
    };
    spawn_brew_multi(app, commands);
}

/// Build the command sequence for batch Brewfile install/remove.
/// Taps are handled first on install so formulae resolve.
fn brewfile_commands(app: &App, install: bool) -> Vec<Vec<String>> {
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

pub fn spawn_brew(app: &mut App, args: &[String]) {
    spawn_brew_multi(app, vec![args.to_vec()]);
}

/// Run a sequence of brew commands sequentially, streaming output for each.
pub fn spawn_brew_multi(app: &mut App, commands: Vec<Vec<String>>) {
    app.output.clear();
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
        .env("HOMEBREW_NO_AUTO_UPDATE", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
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
pub fn spawn_vuln_scan(app: &mut App, name: String) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, outdated: bool, cask: bool) -> Package {
        Package {
            name: name.into(),
            desc: format!("{} desc", name),
            version: "1.0".into(),
            cask,
            outdated,
            installed_version: Some("1.0".into()),
            pinned: false,
            service_status: None,
            deprecation: None,
        }
    }

    #[test]
    fn filters_sections() {
        let mut app = App {
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            brewfile_entries: Vec::new(),
            brewfile: Vec::new(),
            packages: vec![
                pkg("git", false, false),
                pkg("openssl", true, false),
                pkg("firefox", true, true),
            ],
            filtered: Vec::new(),
            section_idx: 1, // Outdated
            list_idx: 0,
            panel: Panel::Sidebar,
            sort: SortMode::Natural,
            leaves: vec![],
            catalog: Vec::new(),
            installs: Default::default(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: Vec::new(),
            cmd_rx: None,
            frame: 0,
            load_rx: None,
            services: Vec::new(),
            taps: Vec::new(),
            vulns: Default::default(),
            catalog_rx: None,
            theme: crate::theme::DEFAULT,
        };
        app.apply_section();
        assert_eq!(app.filtered.len(), 2);
        app.search = "firef".into();
        app.apply_section();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.filtered[0].name, "firefox");
    }

    fn bare(packages: Vec<Package>) -> App {
        let mut app = App {
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            brewfile_entries: Vec::new(),
            brewfile: Vec::new(),
            packages,
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 0,
            panel: Panel::Sidebar,
            sort: SortMode::Natural,
            leaves: vec![],
            catalog: Vec::new(),
            installs: Default::default(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: Vec::new(),
            cmd_rx: None,
            frame: 0,
            load_rx: None,
            services: Vec::new(),
            taps: Vec::new(),
            vulns: Default::default(),
            catalog_rx: None,
            theme: crate::theme::DEFAULT,
        };
        app.apply_section();
        app
    }

    #[test]
    fn sort_modes_reorder_the_list() {
        let mut app = bare(vec![
            pkg("openssl", false, false),
            pkg("git", false, false),
            pkg("zlib", false, false),
        ]);
        app.installs.insert("git".into(), 10);
        app.installs.insert("zlib".into(), 100);

        app.sort = SortMode::Name;
        app.apply_section();
        let names: Vec<&str> = app.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["git", "openssl", "zlib"]);

        app.sort = SortMode::Installs;
        app.apply_section();
        let names: Vec<&str> = app.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["zlib", "git", "openssl"]);

        app.sort = SortMode::Natural;
        app.apply_section();
        let names: Vec<&str> = app.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["openssl", "git", "zlib"]);
    }
}
