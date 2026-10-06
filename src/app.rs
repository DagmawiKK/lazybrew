//! Application state and brew command execution.

use crate::brew::{self, Package};
use std::sync::mpsc;

pub enum CmdEvent {
    Line(String),
    Done(bool),
}

pub type LoadResult = (Vec<Package>, Vec<String>);

/// Sections shown in the lazygit-style left sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Installed,
    Outdated,
    Casks,
    Leaves,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::Installed,
        Section::Outdated,
        Section::Casks,
        Section::Leaves,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Section::Installed => "Installed",
            Section::Outdated => "Outdated",
            Section::Casks => "Casks",
            Section::Leaves => "Leaves",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Sidebar,
    List,
}

pub struct App {
    pub packages: Vec<Package>,
    pub filtered: Vec<Package>,
    pub section_idx: usize,
    pub list_idx: usize,
    pub panel: Panel,
    pub leaves: Vec<String>,
    pub search: String,
    pub searching: bool,
    pub installing: bool,
    pub install_input: String,
    pub output: Vec<String>,
    pub cmd_rx: Option<mpsc::Receiver<CmdEvent>>,
    pub modal: Option<Modal>,
    pub menu: Option<usize>,
    pub frame: usize,
    pub load_rx: Option<mpsc::Receiver<LoadResult>>,
    pub help: bool,
}

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner(app: &App) -> char {
    SPINNER[app.frame % SPINNER.len()]
}

pub struct Modal {
    pub text: String,
    pub confirm: ModalAction,
}

pub enum ModalAction {
    Upgrade(String, bool), // name, is_cask
    Remove(String, bool),
    Install(String),
    Update,
}

impl App {
    pub fn apply_section(&mut self) {
        let section = Section::ALL[self.section_idx];
        let q = self.search.to_lowercase();
        self.filtered = self
            .packages
            .iter()
            .filter(|p| match section {
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
        self.list_idx = 0;
    }

    pub fn section_counts(&self) -> [usize; 4] {
        let outdated = self.packages.iter().filter(|p| p.outdated).count();
        let casks = self.packages.iter().filter(|p| p.cask).count();
        [self.packages.len(), outdated, casks, self.leaves.len()]
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
        let _ = tx.send((pkgs, leaves));
    });
    rx
}

pub fn run_menu_action(app: &mut App, idx: usize) {
    let Some(p) = app.selected().cloned() else {
        return;
    };
    match idx {
        0 => {
            app.modal = Some(Modal {
                text: format!("Upgrade '{}'? (y/n)", p.name),
                confirm: ModalAction::Upgrade(p.name, p.cask),
            });
        }
        1 => {
            app.modal = Some(Modal {
                text: format!("Remove '{}'? (y/n)", p.name),
                confirm: ModalAction::Remove(p.name, p.cask),
            });
        }
        2 => spawn_brew(app, &["info".into(), p.name]),
        3 => spawn_brew(app, &["deps".into(), p.name]),
        4 => {
            let verb = if p.pinned { "unpin" } else { "pin" };
            spawn_brew(app, &[verb.into(), p.name]);
        }
        _ => {}
    }
}

pub fn run_modal_action(app: &mut App, modal: &Modal) {
    let args: Vec<String> = match &modal.confirm {
        ModalAction::Upgrade(name, cask) => {
            let mut a = vec!["upgrade".into()];
            if *cask {
                a.push("--cask".into());
            }
            a.push(name.clone());
            a
        }
        ModalAction::Remove(name, cask) => {
            let mut a = vec!["uninstall".into()];
            if *cask {
                a.push("--cask".into());
            }
            a.push(name.clone());
            a
        }
        ModalAction::Install(name) => vec!["install".into(), name.clone()],
        ModalAction::Update => vec!["update".into()],
    };
    spawn_brew(app, &args);
}

pub fn spawn_brew(app: &mut App, args: &[String]) {
    app.output.clear();
    app.output.push(format!("$ brew {}", args.join(" ")));
    let (tx, rx) = mpsc::channel();
    let args: Vec<String> = args.to_vec();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("brew");
        cmd.args(&args)
            .env("NONINTERACTIVE", "1")
            .env("HOMEBREW_NO_AUTO_UPDATE", "0")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(CmdEvent::Line(format!("spawn error: {}", e)));
                let _ = tx.send(CmdEvent::Done(false));
                return;
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
        let _ = tx.send(CmdEvent::Done(status));
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
        }
    }

    #[test]
    fn filters_sections() {
        let mut app = App {
            packages: vec![
                pkg("git", false, false),
                pkg("openssl", true, false),
                pkg("firefox", true, true),
            ],
            filtered: Vec::new(),
            section_idx: 1, // Outdated
            list_idx: 0,
            panel: Panel::Sidebar,
            leaves: vec![],
            search: String::new(),
            searching: false,
            installing: false,
            install_input: String::new(),
            output: Vec::new(),
            cmd_rx: None,
            modal: None,
            menu: None,
            frame: 0,
            load_rx: None,
            help: false,
        };
        app.apply_section();
        assert_eq!(app.filtered.len(), 2);
        app.search = "firef".into();
        app.apply_section();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.filtered[0].name, "firefox");
    }
}
