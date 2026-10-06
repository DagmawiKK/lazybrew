//! Central color palette, themes, and style helpers.
//!
//! Every color used by the UI lives in a [`Theme`]. The user can switch
//! themes at runtime with `t`; the choice is persisted to the XDG config dir.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

/// One complete color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// Title / accent color.
    pub accent: Color,
    /// Installed / success.
    pub good: Color,
    /// Outdated / warning.
    pub warn: Color,
    /// Disabled / failure.
    pub bad: Color,
    /// Casks.
    pub cask: Color,
    /// Dim text.
    pub dim: Color,
    /// Footer key chip background.
    pub key_bg: Color,
    /// Selected row background (breathing pair).
    pub hl_bg: Color,
    pub hl_bg_dim: Color,
    /// Header / table header background.
    pub bar_bg: Color,
    /// Subtle row banding.
    pub band_bg: Color,
}

impl Theme {
    /// Accent style for panel titles.
    pub fn title(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// A bordered block: rounded, dim by default, accent when focused.
    pub fn panel_block(&self, label: &str, focused: bool) -> Block<'_> {
        Block::default()
            .title(label.to_string())
            .title_style(if focused {
                Style::default()
                    .fg(self.accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                self.title()
            })
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if focused {
                Style::default().fg(self.accent)
            } else {
                Style::default().fg(self.dim)
            })
    }

    /// Footer key chip: ` j ` on a dark background.
    pub fn key_chip(&self, _key: &str) -> Style {
        Style::default()
            .fg(self.accent)
            .bg(self.key_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for one line of streamed command output.
    pub fn output_line(&self, line: &str) -> Style {
        if line.starts_with('$') {
            Style::default()
                .fg(self.accent)
                .add_modifier(Modifier::BOLD)
        } else if line.contains("== FAILED ==") {
            Style::default().fg(self.bad).add_modifier(Modifier::BOLD)
        } else if line.contains("== done ==") {
            Style::default().fg(self.good).add_modifier(Modifier::BOLD)
        } else if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
            Style::default().fg(self.bad)
        } else if line.contains("warning") || line.contains("Warning") {
            Style::default().fg(self.warn)
        } else {
            Style::default()
        }
    }

    /// Pulse color, cycling slowly with the frame.
    pub fn pulse(&self, frame: usize) -> Color {
        let hues = [self.good, self.warn, self.accent, self.cask];
        hues[(frame / 8) % hues.len()]
    }
}

/// Built-in themes, selectable with `t`.
pub const THEMES: [Theme; 6] = [
    Theme {
        name: "Lazybrew",
        accent: Color::LightCyan,
        good: Color::LightGreen,
        warn: Color::LightYellow,
        bad: Color::LightRed,
        cask: Color::LightMagenta,
        dim: Color::DarkGray,
        key_bg: Color::Rgb(45, 55, 65),
        hl_bg: Color::Rgb(52, 73, 85),
        hl_bg_dim: Color::Rgb(44, 62, 73),
        bar_bg: Color::Rgb(30, 40, 48),
        band_bg: Color::Rgb(24, 30, 36),
    },
    Theme {
        name: "btop",
        accent: Color::Rgb(140, 196, 254),
        good: Color::Rgb(169, 216, 140),
        warn: Color::Rgb(235, 203, 139),
        bad: Color::Rgb(236, 140, 140),
        cask: Color::Rgb(248, 167, 226),
        dim: Color::Rgb(100, 114, 136),
        key_bg: Color::Rgb(50, 60, 76),
        hl_bg: Color::Rgb(58, 72, 92),
        hl_bg_dim: Color::Rgb(50, 63, 81),
        bar_bg: Color::Rgb(34, 44, 58),
        band_bg: Color::Rgb(27, 36, 48),
    },
    Theme {
        name: "Dracula",
        accent: Color::Rgb(189, 147, 249),
        good: Color::Rgb(80, 250, 123),
        warn: Color::Rgb(241, 250, 140),
        bad: Color::Rgb(255, 85, 85),
        cask: Color::Rgb(255, 121, 198),
        dim: Color::Rgb(98, 114, 164),
        key_bg: Color::Rgb(55, 57, 73),
        hl_bg: Color::Rgb(68, 71, 90),
        hl_bg_dim: Color::Rgb(58, 61, 80),
        bar_bg: Color::Rgb(40, 42, 54),
        band_bg: Color::Rgb(33, 34, 44),
    },
    Theme {
        name: "Nord",
        accent: Color::Rgb(136, 192, 208),
        good: Color::Rgb(163, 190, 140),
        warn: Color::Rgb(235, 203, 139),
        bad: Color::Rgb(191, 97, 106),
        cask: Color::Rgb(180, 142, 173),
        dim: Color::Rgb(76, 86, 106),
        key_bg: Color::Rgb(59, 66, 82),
        hl_bg: Color::Rgb(67, 76, 94),
        hl_bg_dim: Color::Rgb(58, 66, 82),
        bar_bg: Color::Rgb(46, 52, 64),
        band_bg: Color::Rgb(38, 44, 56),
    },
    Theme {
        name: "Gruvbox",
        accent: Color::Rgb(131, 165, 152),
        good: Color::Rgb(184, 187, 38),
        warn: Color::Rgb(250, 189, 47),
        bad: Color::Rgb(251, 73, 52),
        cask: Color::Rgb(254, 128, 25),
        dim: Color::Rgb(146, 131, 113),
        key_bg: Color::Rgb(60, 56, 54),
        hl_bg: Color::Rgb(80, 73, 66),
        hl_bg_dim: Color::Rgb(68, 62, 57),
        bar_bg: Color::Rgb(40, 40, 40),
        band_bg: Color::Rgb(33, 33, 33),
    },
    Theme {
        name: "Solarized",
        accent: Color::Rgb(38, 139, 210),
        good: Color::Rgb(133, 153, 0),
        warn: Color::Rgb(181, 137, 0),
        bad: Color::Rgb(220, 50, 47),
        cask: Color::Rgb(211, 54, 130),
        dim: Color::Rgb(101, 123, 131),
        key_bg: Color::Rgb(0, 60, 76),
        hl_bg: Color::Rgb(0, 74, 92),
        hl_bg_dim: Color::Rgb(0, 62, 78),
        bar_bg: Color::Rgb(0, 43, 54),
        band_bg: Color::Rgb(7, 54, 66),
    },
];

/// The default theme.
pub const DEFAULT: Theme = THEMES[0];

fn config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("lazybrew").join("theme"))
}

/// Load the persisted theme name; falls back to the default.
pub fn load() -> Theme {
    let Some(path) = config_path() else {
        return DEFAULT;
    };
    let Ok(name) = std::fs::read_to_string(path) else {
        return DEFAULT;
    };
    THEMES
        .iter()
        .find(|t| t.name == name.trim())
        .copied()
        .unwrap_or(DEFAULT)
}

/// Persist the chosen theme name.
pub fn save(theme: &Theme) {
    if let Some(path) = config_path()
        && let Some(dir) = path.parent()
    {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, theme.name);
    }
}

/// Index of a theme in [`THEMES`], by name.
pub fn index_of(theme: &Theme) -> usize {
    THEMES
        .iter()
        .position(|t| t.name == theme.name)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_first_theme() {
        assert_eq!(DEFAULT.name, "Lazybrew");
        assert_eq!(index_of(&DEFAULT), 0);
    }

    #[test]
    fn all_themes_named_uniquely() {
        for (i, a) in THEMES.iter().enumerate() {
            for b in THEMES.iter().skip(i + 1) {
                assert_ne!(a.name, b.name);
            }
        }
    }
}
