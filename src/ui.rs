//! Rendering.

use crate::app::{App, Panel, Section, spinner};
use crate::theme as th;
use ratatui::{prelude::*, widgets::*};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);
    render_search_row(f, app, chunks[1]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(20),
            Constraint::Length(24),
        ])
        .split(chunks[2]);

    render_sidebar(f, app, body[0]);
    render_table(f, app, body[1]);
    render_details(f, app, body[2]);
    render_output(f, app, chunks[3]);
    render_footer(f, app, chunks[4]);
    render_overlays(f, app);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let dot = Span::styled("● ", Style::default().fg(th::pulse(app.frame)));
    let title = Span::styled("lazybrew", Style::default().fg(Color::White).bold());
    let sep = Span::styled(" │ ", Style::default().fg(th::DIM));
    let stat = |label: &'static str, value: String, color: Color| {
        (
            Span::styled(label, Style::default().fg(th::DIM)),
            Span::styled(value, Style::default().fg(color).bold()),
        )
    };
    let (l1, v1) = stat(" pkgs", app.packages.len().to_string(), Color::White);
    let (l2, v2) = stat(
        " outdated",
        app.count_for(Section::Outdated).to_string(),
        th::WARN,
    );
    let (l3, v3) = stat(
        " leaves",
        app.count_for(Section::Leaves).to_string(),
        th::GOOD,
    );
    let (l4, v4) = stat(" catalog", app.catalog.len().to_string(), Color::LightBlue);
    let (l5, v5) = stat(" taps", app.count_for(Section::Taps).to_string(), th::CASK);

    let line = Line::from(vec![
        dot,
        title,
        sep.clone(),
        v1,
        l1,
        sep.clone(),
        v2,
        l2,
        sep.clone(),
        v3,
        l3,
        sep.clone(),
        v4,
        l4,
        sep,
        v5,
        l5,
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(th::BAR_BG)),
        area,
    );
}

fn render_search_row(f: &mut Frame, app: &App, area: Rect) {
    if app.installing {
        f.render_widget(
            Paragraph::new(format!(" install ▸ {}_ ", app.install_input))
                .style(Style::default().fg(th::GOOD).bg(th::BAR_BG).bold()),
            area,
        );
    } else if app.searching {
        f.render_widget(
            Paragraph::new(format!(" search ▸ /{}_ ", app.search))
                .style(Style::default().fg(th::WARN).bg(th::BAR_BG).bold()),
            area,
        );
    } else {
        let text = if app.search.is_empty() {
            " press / to search ".to_string()
        } else {
            format!(" search ▸ /{} ", app.search)
        };
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(th::DIM).bg(th::BAR_BG)),
            area,
        );
    }
}

fn section_color(section: Section) -> Color {
    match section {
        Section::Installed => th::GOOD,
        Section::Outdated => th::WARN,
        Section::Casks => th::CASK,
        Section::Leaves => Color::LightBlue,
        Section::Catalog => Color::LightCyan,
        Section::Services => Color::White,
        Section::Brewfile => th::GOOD,
        Section::Taps => Color::Gray,
    }
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .sections
        .iter()
        .map(|s| {
            let color = section_color(*s);
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::styled(s.title(), Style::default().fg(Color::White)),
                Span::styled(
                    format!(" ({})", app.count_for(*s)),
                    Style::default().fg(th::DIM),
                ),
            ]))
        })
        .collect();

    let focused = app.panel == Panel::Sidebar;
    let sidebar = List::new(items)
        .block(th::panel_block(" Sections ", focused))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().bg(th::HL_BG).bold());
    let mut state = ListState::default();
    state.select(Some(app.section_idx));
    f.render_stateful_widget(sidebar, area, &mut state);
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let rows: Vec<Row> = app
        .filtered
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let installed = p.installed_version.is_some() || p.service_status.is_some();
            let marker = if installed {
                Span::styled("● ", Style::default().fg(th::GOOD))
            } else {
                Span::styled("○ ", Style::default().fg(th::DIM))
            };
            let name_style = if p.outdated {
                Style::default().fg(th::WARN).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let name = Cell::from(Line::from(vec![marker, Span::styled(&p.name, name_style)]));

            let version_style = if p.outdated {
                Style::default().fg(th::WARN)
            } else {
                Style::default().fg(th::DIM)
            };
            let version = Cell::from(Span::styled(&p.version, version_style));

            let (tag, tag_style) = if p.cask {
                ("[C]", th::CASK)
            } else if p.service_status.is_some() {
                ("[S]", Color::White)
            } else {
                ("[F]", th::GOOD)
            };
            let kind = Cell::from(Span::styled(tag, Style::default().fg(tag_style)));

            let band = if i % 2 == 1 {
                Style::default().bg(th::BAND_BG)
            } else {
                Style::default()
            };
            Row::new(vec![name, version, kind]).style(band)
        })
        .collect();

    // Breathing highlight for the selected row.
    let hl_bg = if (app.frame / 10).is_multiple_of(2) {
        th::HL_BG
    } else {
        th::HL_BG_DIM
    };

    let focused = app.panel == Panel::List;
    let table = Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new(vec![" Name", "Version", "Type"]).style(
            Style::default()
                .fg(th::ACCENT)
                .bg(th::BAR_BG)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(th::panel_block(
        &format!(" {} ", app.sections[app.section_idx].title()),
        focused,
    ))
    .row_highlight_style(Style::default().bg(hl_bg).add_modifier(Modifier::BOLD));

    let mut tstate = TableState::default();
    tstate.select(if app.filtered.is_empty() {
        None
    } else {
        Some(app.list_idx)
    });
    f.render_stateful_widget(table, area, &mut tstate);
}

fn kv_line<'a>(key: &'a str, value: Span<'a>, value_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, Style::default().fg(th::DIM)),
        Span::styled(value.content, value_style),
    ])
}

fn render_details(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    match app.selected() {
        Some(p) => {
            lines.push(kv_line(
                "name        ",
                Span::raw(p.name.clone()),
                Style::default().fg(Color::White).bold(),
            ));
            lines.push(kv_line(
                "type        ",
                Span::raw(if p.cask { "cask" } else { "formula" }),
                Style::default().fg(if p.cask { th::CASK } else { th::GOOD }),
            ));
            lines.push(kv_line(
                "version     ",
                Span::raw(p.version.clone()),
                Style::default().fg(Color::White),
            ));
            let installed_style = if p.installed_version.is_some() {
                Style::default().fg(th::GOOD)
            } else {
                Style::default().fg(th::DIM)
            };
            lines.push(kv_line(
                "installed   ",
                Span::raw(p.installed_version.clone().unwrap_or_else(|| "-".into())),
                installed_style,
            ));
            let (state, style) = if p.outdated {
                ("outdated", Style::default().fg(th::WARN))
            } else {
                ("current", Style::default().fg(th::GOOD))
            };
            lines.push(kv_line("state       ", Span::raw(state), style));
            if p.pinned {
                lines.push(kv_line(
                    "pinned      ",
                    Span::raw("yes"),
                    Style::default().fg(th::WARN),
                ));
            }
            if let Some(st) = &p.service_status {
                lines.push(kv_line(
                    "service     ",
                    Span::raw(format!("{st}  (s toggles)")),
                    if st == "started" {
                        Style::default().fg(th::GOOD)
                    } else {
                        Style::default().fg(th::DIM)
                    },
                ));
            }
            if let Some(v) = app.vulns.get(&p.name) {
                lines.push(kv_line(
                    "vulns       ",
                    Span::raw(v.len().to_string()),
                    if v.is_empty() {
                        Style::default().fg(th::GOOD)
                    } else {
                        Style::default().fg(th::BAD).bold()
                    },
                ));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                p.desc.clone(),
                Style::default().fg(Color::Gray),
            )));
        }
        None => lines.push(Line::from(Span::styled(
            "no selection",
            Style::default().fg(th::DIM),
        ))),
    }

    let details = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(th::panel_block(" Details ", false));
    f.render_widget(details, area);
}

fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let shown: Vec<Line> = app
        .output
        .iter()
        .skip(app.output.len().saturating_sub(inner_height))
        .map(|l| Line::from(Span::styled(l.clone(), th::output_line(l))))
        .collect();

    let title = if app.cmd_rx.is_some() {
        format!(" Output {} ", spinner(app))
    } else {
        " Output ".to_string()
    };
    let output = Paragraph::new(shown).block(
        Block::default()
            .title(title)
            .title_style(th::title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(th::DIM)),
    );
    f.render_widget(output, area);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    if app.load_rx.is_some() || app.catalog_rx.is_some() {
        let what = if app.load_rx.is_some() {
            "loading Homebrew data"
        } else {
            "loading catalog (once a day)"
        };
        f.render_widget(
            Paragraph::new(format!("{} {what}...", spinner(app)))
                .style(Style::default().fg(th::ACCENT)),
            area,
        );
        return;
    }

    let hints: &[(&str, &str)] = &[
        ("j/k", "nav"),
        ("h/l", "switch"),
        ("/", "search"),
        ("i", "install"),
        ("u", "upgrade"),
        ("r", "remove"),
        ("A", "all"),
        ("x", "menu"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let full: Vec<Span> = hints
        .iter()
        .flat_map(|(k, d)| {
            vec![
                Span::styled(format!(" {k} "), th::key_chip(k)),
                Span::styled(format!(" {d}  "), Style::default().fg(th::DIM)),
            ]
        })
        .collect();

    let line = Line::from(full);
    if line.width() <= area.width as usize {
        f.render_widget(Paragraph::new(line), area);
    } else {
        // Compact fallback: drop the middle hints on narrow terminals.
        let compact: &[(&str, &str)] = &[
            ("j/k", "nav"),
            ("/", "search"),
            ("x", "menu"),
            ("?", "help"),
            ("q", "quit"),
        ];
        let spans: Vec<Span> = compact
            .iter()
            .flat_map(|(k, d)| {
                vec![
                    Span::styled(format!(" {k} "), th::key_chip(k)),
                    Span::styled(format!(" {d}  "), Style::default().fg(th::DIM)),
                ]
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn render_overlays(f: &mut Frame, app: &App) {
    if let Some(modal) = &app.modal {
        let area = centered_rect(52, 18, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(modal.text.clone())
                .style(Style::default().fg(Color::White).bg(th::BAR_BG))
                .wrap(Wrap { trim: false })
                .block(th::panel_block(" Confirm ", true)),
            area,
        );
    }

    if let Some(menu_idx) = app.menu {
        let items = [
            "u  Upgrade",
            "r  Remove",
            "i  Info",
            "d  Deps",
            "p  Pin/Unpin",
        ];
        let lines: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                if i == menu_idx {
                    Line::from(Span::styled(
                        format!(" ▸ {it} "),
                        Style::default().fg(Color::Black).bg(th::ACCENT).bold(),
                    ))
                } else {
                    Line::from(Span::styled(
                        format!("   {it} "),
                        Style::default().fg(Color::Gray),
                    ))
                }
            })
            .collect();
        let area = centered_rect(30, 34, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(th::BAR_BG))
                .block(th::panel_block(" Actions ", true)),
            area,
        );
    }

    if app.help {
        let help = crate::input::help_text();
        let lines: Vec<Line> = help
            .lines()
            .map(|l| {
                if l.trim().is_empty() {
                    Line::from("")
                } else if l.starts_with("lazybrew") {
                    Line::from(Span::styled(
                        l,
                        Style::default().fg(th::ACCENT).add_modifier(Modifier::BOLD),
                    ))
                } else {
                    let key = l.get(..16).unwrap_or(l);
                    let rest = l.get(16..).unwrap_or("");
                    Line::from(vec![
                        Span::styled(format!(" {key}"), th::key_chip(key.trim())),
                        Span::styled(rest, Style::default().fg(Color::Gray)),
                    ])
                }
            })
            .collect();
        let area = centered_rect(64, 66, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(th::BAR_BG))
                .block(th::panel_block(" Help ", true)),
            area,
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Panel};
    use crate::brew::Package;
    use ratatui::backend::TestBackend;

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
        }
    }

    fn test_app() -> App {
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
            packages: vec![pkg("git", false, false), pkg("firefox", true, true)],
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 0,
            panel: Panel::Sidebar,
            leaves: vec![],
            catalog: Vec::new(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
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
            catalog_rx: None,
            help: false,
        };
        app.apply_section();
        app
    }

    #[test]
    fn renders_sections_and_packages() {
        let app = test_app();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Sections"), "missing Sections: {}", text);
        assert!(text.contains("git"), "missing git");
        assert!(text.contains("Installed"), "missing Installed");
    }

    #[test]
    fn selected_row_is_highlighted() {
        let app = test_app();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        // Row 1 (header) and row 2 (selected) should differ in background.
        let header_bg = buf[(30, 3)].bg;
        let sel_bg = buf[(30, 4)].bg;
        assert_eq!(header_bg, th::BAR_BG, "table header band");
        assert!(
            sel_bg == th::HL_BG || sel_bg == th::HL_BG_DIM,
            "selected row bg"
        );
    }
}

#[cfg(test)]
mod preview {
    use super::*;
    use crate::app::{App, Panel};
    use crate::brew::Package;
    use ratatui::backend::TestBackend;

    #[test]
    fn print_layout() {
        let mk = |name: &str, outdated: bool, cask: bool| Package {
            name: name.into(),
            desc: format!("{name} desc"),
            version: "1.0".into(),
            cask,
            outdated,
            installed_version: Some("1.0".into()),
            pinned: false,
            service_status: None,
        };
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
                mk("git", false, false),
                mk("openssl@3", true, false),
                mk("firefox", true, true),
                mk("zsh", false, false),
            ],
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 1,
            panel: Panel::List,
            leaves: vec!["git".into(), "zsh".into()],
            catalog: Vec::new(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
            search: String::new(),
            searching: false,
            installing: false,
            install_input: String::new(),
            output: vec!["$ brew install git".into(), "== done ==".into()],
            cmd_rx: None,
            modal: None,
            menu: None,
            frame: 3,
            load_rx: None,
            catalog_rx: None,
            help: false,
        };
        app.apply_section();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer();
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            println!("{y:2}|{line}|");
        }
    }
}
