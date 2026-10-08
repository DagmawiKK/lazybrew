//! Effects: the commands and side effects the model asks the runtime to run.
//!
//! `update` never touches a thread or a file; it only mutates the model and
//! returns effects. The runtime executes them and feeds the results back as
//! [`crate::action::Action`]s, keeping every state transition deterministic
//! and testable.

use crate::theme::Theme;
use std::path::PathBuf;

/// One unit of work requested by the update step.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Run one or more brew commands sequentially, streaming output to the
    /// output pane.
    RunBrew(Vec<Vec<String>>),
    /// Scan one formula for known vulnerabilities (`brew vulns`).
    RunVulnScan(String),
    /// Swap in the newest GitHub release binary for this platform.
    RunSelfUpdate {
        /// Full `releases/latest/download` URL of the tarball.
        url: String,
        /// The platform triple, names the asset (and the inner binary).
        target: String,
        /// Path of the running executable to replace.
        exe: PathBuf,
    },
    /// Persist the just-chosen theme to the XDG config file.
    SaveTheme(Theme),
    /// Installed data changed: re-fetch the remote catalog so installed
    /// status stays accurate.
    FetchCatalog,
    /// A foreground command sequence finished: reload installed data.
    Reload,
    /// Leave the app.
    Quit,
}
