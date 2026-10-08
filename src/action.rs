//! Everything that can happen in the app, as data.
//!
//! Input (keys/mouse), background loads (installed data, catalog), and
//! background command output all funnel through here into
//! [`crate::update::update`], which folds them into
//! [`crate::state::AppState`] and returns effects to run. Keeping the whole
//! input surface as an exhaustible enum is what makes the
//! transition-table and fuzz regression tests possible.

use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A key was pressed (decoded in `update`, dispatched by mode).
    Key(KeyEvent),
    /// A mouse event arrived (only meaningful in `Normal` mode).
    Mouse(MouseEvent),
    /// Background load of installed packages finished.
    InstalledLoaded(crate::state::LoadResult),
    /// Background catalog + analytics download finished.
    CatalogLoaded(crate::state::CatalogData),
    /// One line of output from a running background command.
    CmdLine(String),
    /// A background command sequence finished.
    CmdDone(bool),
    /// A vulnerability scan finished for a package.
    VulnsScanned(String, Vec<String>),
    /// `brew vulns` is not installed.
    VulnsMissing,
}
