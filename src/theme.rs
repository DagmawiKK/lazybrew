//! Central color palette and style helpers (btop-flavored, terminal-safe).

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

/// Title / accent color.
pub const ACCENT: Color = Color::LightCyan;
/// Installed / success.
pub const GOOD: Color = Color::LightGreen;
/// Outdated / warning.
pub const WARN: Color = Color::LightYellow;
/// Disabled / failure.
pub const BAD: Color = Color::LightRed;
/// Casks.
pub const CASK: Color = Color::LightMagenta;
/// Dim text.
pub const DIM: Color = Color::DarkGray;
/// Footer key chip background.
pub const KEY_BG: Color = Color::Rgb(45, 55, 65);
/// Selected row background (breathing pair).
pub const HL_BG: Color = Color::Rgb(52, 73, 85);
pub const HL_BG_DIM: Color = Color::Rgb(44, 62, 73);
/// Header / table header background.
pub const BAR_BG: Color = Color::Rgb(30, 40, 48);
/// Subtle row banding.
pub const BAND_BG: Color = Color::Rgb(24, 30, 36);

/// Accent style for panel titles.
pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Style for a bordered block: rounded, dim by default, accent when focused.
pub fn panel_block(label: &str, focused: bool) -> Block<'static> {
    Block::default()
        .title(label.to_string())
        .title_style(if focused {
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            title()
        })
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        })
}

/// Footer key chip: ` j ` on a dark background.
pub fn key_chip(_key: &str) -> Style {
    Style::default()
        .fg(ACCENT)
        .bg(KEY_BG)
        .add_modifier(Modifier::BOLD)
}

/// Style for one line of streamed command output.
pub fn output_line(line: &str) -> Style {
    if line.starts_with('$') {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else if line.contains("== FAILED ==") {
        Style::default().fg(BAD).add_modifier(Modifier::BOLD)
    } else if line.contains("== done ==") {
        Style::default().fg(GOOD).add_modifier(Modifier::BOLD)
    } else if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
        Style::default().fg(BAD)
    } else if line.contains("warning") || line.contains("Warning") {
        Style::default().fg(WARN)
    } else {
        Style::default()
    }
}

/// Pulse color for the header status dot, cycling slowly with the frame.
pub fn pulse(frame: usize) -> Color {
    const HUES: [Color; 5] = [
        Color::LightGreen,
        Color::LightYellow,
        Color::LightCyan,
        Color::LightBlue,
        Color::LightMagenta,
    ];
    HUES[(frame / 8) % HUES.len()]
}
