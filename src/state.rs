//! Application state: pure data plus the query/section logic that reads and
//! mutates it. No threads, no channels, no I/O — everything here is
//! deterministic and directly testable.
//!
//! This is the "model" of the Model–Update–View architecture: [`AppState`]
//! holds the data, [`crate::update::update`] mutates it one `Action` at a
//! time while returning [`crate::effect::Effect`]s for the runtime to run,
//! and [`crate::ui::render`] only ever reads it.

use crate::brew::Package;
use crate::theme::Theme;

/// Payload of a background `brew`/system data load.
pub type LoadResult = (Vec<Package>, Vec<String>, Vec<Package>, Vec<Package>);

/// 90-day install analytics, `name -> install count`.
pub type Popularity = std::collections::HashMap<String, u64>;

/// The catalog payload streamed over its own channel.
pub type CatalogData = (Vec<Package>, Popularity);

/// Sections shown in the lazygit-style left sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// `/` search is being typed; input goes into `AppState.search`.
    Search,
    /// Type-a-name prompt (`I`, or `i` without a valid selection);
    /// input goes into `AppState.prompt_buffer`.
    Prompt,
    /// Confirmation dialog.
    Confirm(Modal),
    /// Action menu over the command registry ([`crate::registry`]).
    Menu(usize),
    /// Theme picker overlay.
    ThemePicker(usize),
    /// Help overlay — any key closes it.
    Help,
}

/// The whole application model. Plain data only — the background receivers
/// (`cmd_rx`/`load_rx`/`catalog_rx`) previously stored here now live in the
/// runtime, which forwards their events back into [`crate::update::update`]
/// as [`crate::action::Action`]s.
#[derive(Debug, Clone)]
pub struct AppState {
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
    /// Rows scrolled back from the output tail (PageUp/PageDown).
    pub output_offset: usize,
    /// A background command (brew/vulns/self-update) is running.
    pub cmd_active: bool,
    /// The installed-data load is pending (footer spinner).
    pub loading_packages: bool,
    /// The remote catalog load is pending (footer spinner).
    pub loading_catalog: bool,
    pub frame: usize,
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
    /// The `-f` Brewfile path/URL, when given.
    pub brewfile_path: Option<String>,
    /// Entries parsed from the -f Brewfile.
    pub brewfile_entries: Vec<crate::brewfile::Entry>,
    /// Brewfile entries resolved against installed + catalog data.
    pub brewfile: Vec<Package>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 0,
            panel: Panel::Sidebar,
            sort: SortMode::Natural,
            leaves: Vec::new(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: Vec::new(),
            output_offset: 0,
            cmd_active: false,
            // The runtime spawns the first installed-data load at boot.
            loading_packages: true,
            loading_catalog: false,
            frame: 0,
            catalog: Vec::new(),
            installs: Default::default(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
            theme: crate::theme::DEFAULT,
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
                Section::Taps,
            ],
            brewfile_path: None,
            brewfile_entries: Vec::new(),
            brewfile: Vec::new(),
        }
    }
}

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner(state: &AppState) -> char {
    SPINNER[state.frame % SPINNER.len()]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Modal {
    pub text: String,
    pub confirm: ModalAction,
}

/// Convenience constructor for confirm dialogs.
pub fn modal(text: impl Into<String>, confirm: ModalAction) -> Modal {
    Modal {
        text: text.into(),
        confirm,
    }
}

/// True for taps maintained by Homebrew itself. Everything else is an
/// untrusted (third-party) tap that deserves a warning.
pub fn is_official_tap(tap: &str) -> bool {
    tap == "homebrew/core" || tap.starts_with("homebrew/cask")
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
    /// Swap in the newest GitHub release binary.
    SelfUpdate,
}

impl AppState {
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
                    tap: None,
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

    /// Structural invariants the model must hold after every transition.
    /// Checked in `update` under debug assertions and after every step of
    /// the scenario/fuzz regression tests — a future change that breaks one
    /// of these rules fails with a named violation instead of a weird render.
    /// Compiled for debug builds and tests only; release builds have no
    /// caller (the `update` hook is itself debug-only).
    #[cfg(any(test, debug_assertions))]
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.sections.is_empty() {
            return Err("sections is empty".into());
        }
        if self.section_idx >= self.sections.len() {
            return Err(format!(
                "section_idx {} out of range ({})",
                self.section_idx,
                self.sections.len()
            ));
        }
        let len = self.filtered.len();
        if len > 0 && self.list_idx >= len {
            return Err(format!("list_idx {} out of range ({})", self.list_idx, len));
        }
        if len == 0 && self.list_idx != 0 {
            return Err(format!(
                "list_idx {} is non-zero on an empty list",
                self.list_idx
            ));
        }
        // PageUp only moves the output view back when there are lines to
        // scroll; every command/load reset clears both together.
        if self.output.is_empty() && self.output_offset != 0 {
            return Err(format!(
                "output_offset {} with an empty output pane",
                self.output_offset
            ));
        }
        if let AppMode::Menu(i) = self.mode
            && i >= crate::registry::commands().len()
        {
            return Err(format!(
                "menu index {} out of range ({})",
                i,
                crate::registry::commands().len()
            ));
        }
        if let AppMode::ThemePicker(i) = self.mode
            && i >= crate::theme::THEMES.len()
        {
            return Err(format!(
                "theme picker index {} out of range ({})",
                i,
                crate::theme::THEMES.len()
            ));
        }
        // The visible list must be a subset of its section's source data —
        // apply_section is the only writer, so this catches any future
        // direct mutation of `filtered`.
        let source: &[Package] = match self.sections[self.section_idx] {
            Section::Catalog => &self.catalog,
            Section::Services => &self.services,
            Section::Brewfile => &self.brewfile,
            Section::Taps => &self.taps,
            _ => &self.packages,
        };
        for p in &self.filtered {
            if !source.iter().any(|s| s.name == p.name && s.cask == p.cask) {
                return Err(format!(
                    "filtered contains '{}' which is not in the active section source",
                    p.name
                ));
            }
        }
        Ok(())
    }
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
            tap: None,
        }
    }

    #[test]
    fn filters_sections() {
        let mut state = AppState {
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            packages: vec![
                pkg("git", false, false),
                pkg("openssl", true, false),
                pkg("firefox", true, true),
            ],
            section_idx: 1, // Outdated
            ..AppState::default()
        };
        state.apply_section();
        assert_eq!(state.filtered.len(), 2);
        state.search = "firef".into();
        state.apply_section();
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(state.filtered[0].name, "firefox");
    }

    fn bare(packages: Vec<Package>) -> AppState {
        let mut state = AppState {
            packages,
            sections: vec![
                Section::Installed,
                Section::Outdated,
                Section::Casks,
                Section::Leaves,
                Section::Catalog,
                Section::Services,
            ],
            ..AppState::default()
        };
        state.apply_section();
        state
    }

    #[test]
    fn sort_modes_reorder_the_list() {
        let mut state = bare(vec![
            pkg("openssl", false, false),
            pkg("git", false, false),
            pkg("zlib", false, false),
        ]);
        state.installs.insert("git".into(), 10);
        state.installs.insert("zlib".into(), 100);

        state.sort = SortMode::Name;
        state.apply_section();
        let names: Vec<&str> = state.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["git", "openssl", "zlib"]);

        state.sort = SortMode::Installs;
        state.apply_section();
        let names: Vec<&str> = state.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["zlib", "git", "openssl"]);

        state.sort = SortMode::Natural;
        state.apply_section();
        let names: Vec<&str> = state.filtered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["openssl", "git", "zlib"]);
    }
}
