//! Rendering.

use crate::app::{App, AppMode, Panel, Section, SortMode, spinner};
use crate::brew::DeprecationKind;
use crate::catalog;
use crate::theme::{THEMES, Theme};
use ratatui::{prelude::*, widgets::*};

pub fn render(f: &mut Frame, app: &App) {
    let th = app.theme;
    // Paint the whole frame with the theme background so the UI never
    // inherits an unexpected terminal background.
    f.render_widget(
        Block::default().style(Style::default().bg(th.bg).fg(th.fg)),
        f.area(),
    );

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
    let th = app.theme;
    let pulse_frame = if app.cmd_rx.is_some() {
        app.frame * 3
    } else {
        app.frame
    };
    let dot = Span::styled("● ", Style::default().fg(th.pulse(pulse_frame)));
    let title = Span::styled("lazybrew", Style::default().fg(th.fg).bold());
    let sep = Span::styled(" │ ", Style::default().fg(th.dim));
    let stat = |label: &'static str, value: String, color: Color| {
        (
            Span::styled(label, Style::default().fg(th.dim)),
            Span::styled(value, Style::default().fg(color).bold()),
        )
    };
    let (l1, v1) = stat(" pkgs", app.packages.len().to_string(), th.fg);
    let (l2, v2) = stat(
        " outdated",
        app.count_for(Section::Outdated).to_string(),
        th.warn,
    );
    let (l3, v3) = stat(
        " leaves",
        app.count_for(Section::Leaves).to_string(),
        th.good,
    );
    let (l4, v4) = stat(" catalog", app.catalog.len().to_string(), Color::LightBlue);
    let (l5, v5) = stat(" taps", app.count_for(Section::Taps).to_string(), th.cask);

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
        Paragraph::new(line).style(Style::default().bg(th.bar_bg)),
        area,
    );
}

fn render_search_row(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    match app.mode {
        AppMode::Prompt => {
            let hint = crate::input::completions(app);
            let hint_txt = if hint.is_empty() {
                String::new()
            } else {
                format!(" ⇥ {}", hint.join(" "))
            };
            f.render_widget(
                Paragraph::new(format!(" install ▸ {}_{} ", app.prompt_buffer, hint_txt))
                    .style(Style::default().fg(th.good).bg(th.bar_bg).bold()),
                area,
            );
        }
        AppMode::Search => f.render_widget(
            Paragraph::new(format!(" search ▸ /{}_ ", app.search))
                .style(Style::default().fg(th.warn).bg(th.bar_bg).bold()),
            area,
        ),
        _ => {
            let text = if app.search.is_empty() {
                " press / to search ".to_string()
            } else {
                format!(" search ▸ /{} ", app.search)
            };
            f.render_widget(
                Paragraph::new(text).style(Style::default().fg(th.dim).bg(th.bar_bg)),
                area,
            );
        }
    }
}

fn section_color(th: &Theme, section: Section) -> Color {
    match section {
        Section::Installed => th.good,
        Section::Outdated => th.warn,
        Section::Casks => th.cask,
        Section::Leaves => Color::LightBlue,
        Section::Catalog => Color::LightCyan,
        Section::Services => th.fg,
        Section::Brewfile => th.good,
        Section::Taps => th.dim,
    }
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let items: Vec<ListItem> = app
        .sections
        .iter()
        .map(|s| {
            let color = section_color(&th, *s);
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::styled(s.title(), Style::default().fg(th.fg)),
                Span::styled(
                    format!(" ({})", app.count_for(*s)),
                    Style::default().fg(th.dim),
                ),
            ]))
        })
        .collect();

    let focused = app.panel == Panel::Sidebar;
    let sidebar = List::new(items)
        .block(th.panel_block(" Sections ", focused))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().bg(th.hl_bg).bold());
    let mut state = ListState::default();
    state.select(Some(app.section_idx));
    f.render_stateful_widget(sidebar, area, &mut state);
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let show_installs =
        app.sections[app.section_idx] == Section::Catalog || app.sort == SortMode::Installs;
    let rows: Vec<Row> = app
        .filtered
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let installed = p.installed_version.is_some() || p.service_status.is_some();
            let marker = if p
                .deprecation
                .as_ref()
                .is_some_and(|d| d.kind == DeprecationKind::Disabled)
            {
                Span::styled("× ", Style::default().fg(th.bad))
            } else if p.deprecation.is_some() {
                Span::styled("! ", Style::default().fg(th.warn))
            } else if installed {
                Span::styled("● ", Style::default().fg(th.good))
            } else {
                Span::styled("○ ", Style::default().fg(th.dim))
            };
            let name_style = match &p.deprecation {
                Some(d) if d.kind == DeprecationKind::Disabled => Style::default()
                    .fg(th.bad)
                    .add_modifier(Modifier::CROSSED_OUT | Modifier::BOLD),
                Some(_) => Style::default().fg(th.warn).add_modifier(Modifier::BOLD),
                None if p.outdated => Style::default().fg(th.warn).bold(),
                None => Style::default().fg(th.fg),
            };
            let name = Cell::from(Line::from(vec![marker, Span::styled(&p.name, name_style)]));

            let version_style = if p.outdated {
                Style::default().fg(th.warn)
            } else {
                Style::default().fg(th.dim)
            };
            let version = Cell::from(Span::styled(&p.version, version_style));

            let (tag, tag_style) = if p.cask {
                ("[C]", th.cask)
            } else if p.service_status.is_some() {
                ("[S]", th.fg)
            } else {
                ("[F]", th.good)
            };
            let kind = Cell::from(Span::styled(tag, Style::default().fg(tag_style)));

            let installs = if show_installs {
                let n = app.installs.get(&p.name).copied().unwrap_or(0);
                let txt = if n == 0 {
                    "-".to_string()
                } else {
                    catalog::format_count(n)
                };
                Some(Cell::from(Span::styled(txt, Style::default().fg(th.dim))))
            } else {
                None
            };

            let band = if i % 2 == 1 {
                Style::default().bg(th.band_bg)
            } else {
                Style::default()
            };
            let mut cells = vec![name, version, kind];
            if let Some(c) = installs {
                cells.push(c);
            }
            Row::new(cells).style(band)
        })
        .collect();

    // Breathing highlight for the selected row.
    let hl_bg = if (app.frame / 10).is_multiple_of(2) {
        th.hl_bg
    } else {
        th.hl_bg_dim
    };

    let focused = app.panel == Panel::List;
    let section_tint = section_color(&th, app.sections[app.section_idx]);
    let (widths, header): (Vec<Constraint>, Vec<&str>) = if show_installs {
        (
            vec![
                Constraint::Min(20),
                Constraint::Length(14),
                Constraint::Length(6),
                Constraint::Length(9),
            ],
            vec![" Name", "Version", "Type", "90d"],
        )
    } else {
        (
            vec![
                Constraint::Min(20),
                Constraint::Length(14),
                Constraint::Length(6),
            ],
            vec![" Name", "Version", "Type"],
        )
    };
    let table = Table::new(rows, widths)
        .header(
            Row::new(header).style(
                Style::default()
                    .fg(th.accent)
                    .bg(th.bar_bg)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            th.panel_block(
                &format!(" {} ", app.sections[app.section_idx].title()),
                focused,
            )
            .title_style(
                Style::default()
                    .fg(section_tint)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .row_highlight_style(Style::default().bg(hl_bg).add_modifier(Modifier::BOLD));

    let mut tstate = TableState::default();
    tstate.select(if app.filtered.is_empty() {
        None
    } else {
        Some(app.list_idx)
    });
    f.render_stateful_widget(table, area, &mut tstate);
}

fn kv_line<'a>(th: &Theme, key: &'a str, value: Span<'a>, value_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, Style::default().fg(th.dim)),
        Span::styled(value.content, value_style),
    ])
}

fn render_details(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let mut lines: Vec<Line> = Vec::new();
    match app.selected() {
        Some(p) => {
            lines.push(kv_line(
                &th,
                "name        ",
                Span::raw(p.name.clone()),
                Style::default().fg(th.fg).bold(),
            ));
            lines.push(kv_line(
                &th,
                "type        ",
                Span::raw(if p.cask { "cask" } else { "formula" }),
                Style::default().fg(if p.cask { th.cask } else { th.good }),
            ));
            lines.push(kv_line(
                &th,
                "version     ",
                Span::raw(p.version.clone()),
                Style::default().fg(th.fg),
            ));
            let installed_style = if p.installed_version.is_some() {
                Style::default().fg(th.good)
            } else {
                Style::default().fg(th.dim)
            };
            lines.push(kv_line(
                &th,
                "installed   ",
                Span::raw(p.installed_version.clone().unwrap_or_else(|| "-".into())),
                installed_style,
            ));
            let (state, style) = if p.outdated {
                ("outdated", Style::default().fg(th.warn))
            } else {
                ("current", Style::default().fg(th.good))
            };
            lines.push(kv_line(&th, "state       ", Span::raw(state), style));
            if let Some(n) = app.installs.get(&p.name).copied()
                && n > 0
            {
                lines.push(kv_line(
                    &th,
                    "installs    ",
                    Span::raw(format!("{} (90d)", catalog::format_count(n))),
                    Style::default().fg(th.dim),
                ));
            }
            if let Some(d) = &p.deprecation {
                let (label, style) = match d.kind {
                    DeprecationKind::Deprecated => {
                        ("deprecated   ", Style::default().fg(th.warn).bold())
                    }
                    DeprecationKind::Disabled => {
                        ("disabled    ", Style::default().fg(th.bad).bold())
                    }
                };
                let reason = if d.reason.is_empty() {
                    "(no reason given)".to_string()
                } else {
                    d.reason.clone()
                };
                lines.push(kv_line(&th, label, Span::raw(reason), style));
                if let Some(repl) = &d.replacement {
                    lines.push(kv_line(
                        &th,
                        "replaces    ",
                        Span::raw(format!("→ {repl}")),
                        style,
                    ));
                }
            }
            if p.pinned {
                lines.push(kv_line(
                    &th,
                    "pinned      ",
                    Span::raw("yes"),
                    Style::default().fg(th.warn),
                ));
            }
            if let Some(st) = &p.service_status {
                lines.push(kv_line(
                    &th,
                    "service     ",
                    Span::raw(format!("{st}  (s toggles)")),
                    if st == "started" {
                        Style::default().fg(th.good)
                    } else {
                        Style::default().fg(th.dim)
                    },
                ));
            }
            if let Some(v) = app.vulns.get(&p.name) {
                lines.push(kv_line(
                    &th,
                    "vulns       ",
                    Span::raw(v.len().to_string()),
                    if v.is_empty() {
                        Style::default().fg(th.good)
                    } else {
                        Style::default().fg(th.bad).bold()
                    },
                ));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                p.desc.clone(),
                Style::default().fg(th.dim),
            )));
        }
        None => lines.push(Line::from(Span::styled(
            "no selection",
            Style::default().fg(th.dim),
        ))),
    }

    let details = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(th.panel_block(" Details ", false));
    f.render_widget(details, area);
}

fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let inner_height = area.height.saturating_sub(2) as usize;
    let total = app.output.len();
    let offset = app.output_offset.min(total.saturating_sub(inner_height));
    let end = total.saturating_sub(offset);
    let start = end.saturating_sub(inner_height);
    let shown: Vec<Line> = app.output[start..end]
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), th.output_line(l))))
        .collect();

    let mut title: Vec<Span> = Vec::new();
    if app.cmd_rx.is_some() {
        title.push(Span::raw(" Output "));
        title.push(Span::styled(
            spinner(app).to_string(),
            Style::default().fg(th.pulse(app.frame)).bold(),
        ));
        title.push(Span::raw(" "));
    } else if offset > 0 {
        title.push(Span::raw(" Output "));
        title.push(Span::styled(
            format!("⇡ {offset} "),
            Style::default().fg(th.warn).bold(),
        ));
    } else {
        title.push(Span::raw(" Output "));
    }
    let output = Paragraph::new(shown).block(
        Block::default()
            .title(Line::from(title))
            .title_style(th.title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(th.dim)),
    );
    f.render_widget(output, area);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    if app.load_rx.is_some() || app.catalog_rx.is_some() {
        let what = if app.load_rx.is_some() {
            "loading Homebrew data"
        } else {
            "loading catalog (once a day)"
        };
        f.render_widget(
            Paragraph::new(format!("{} {what}...", spinner(app)))
                .style(Style::default().fg(th.accent)),
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
        ("S", "sort"),
        ("x", "menu"),
        ("t", "theme"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let full: Vec<Span> = hints
        .iter()
        .flat_map(|(k, d)| {
            vec![
                Span::styled(format!(" {k} "), th.key_chip(k)),
                Span::styled(format!(" {d}  "), Style::default().fg(th.dim)),
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
            ("t", "theme"),
            ("?", "help"),
            ("q", "quit"),
        ];
        let spans: Vec<Span> = compact
            .iter()
            .flat_map(|(k, d)| {
                vec![
                    Span::styled(format!(" {k} "), th.key_chip(k)),
                    Span::styled(format!(" {d}  "), Style::default().fg(th.dim)),
                ]
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn render_overlays(f: &mut Frame, app: &App) {
    let th = app.theme;
    match &app.mode {
        AppMode::Confirm(modal) => {
            let area = centered_rect(52, 18, f.area());
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(modal.text.clone())
                    .style(Style::default().fg(th.fg).bg(th.bar_bg))
                    .wrap(Wrap { trim: false })
                    .block(th.panel_block(" Confirm ", true)),
                area,
            );
        }
        AppMode::Menu(menu_idx) => {
            let lines: Vec<Line> = crate::app::MENU_ACTIONS
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    if i == *menu_idx {
                        Line::from(Span::styled(
                            format!(" ▸ {it} "),
                            Style::default().fg(th.bg).bg(th.accent).bold(),
                        ))
                    } else {
                        Line::from(Span::styled(
                            format!("   {it} "),
                            Style::default().fg(th.dim),
                        ))
                    }
                })
                .collect();
            let area = centered_rect(30, 34, f.area());
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines)
                    .style(Style::default().bg(th.bar_bg))
                    .block(th.panel_block(" Actions ", true)),
                area,
            );
        }
        AppMode::ThemePicker(pick) => render_theme_picker(f, app, *pick),
        AppMode::Help => {
            let help = crate::input::help_text();
            let lines: Vec<Line> = help
                .lines()
                .map(|l| {
                    if l.trim().is_empty() {
                        Line::from("")
                    } else if l.starts_with("lazybrew") {
                        Line::from(Span::styled(
                            l,
                            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
                        ))
                    } else {
                        let key = l.get(..16).unwrap_or(l);
                        let rest = l.get(16..).unwrap_or("");
                        Line::from(vec![
                            Span::styled(format!(" {key}"), th.key_chip(key.trim())),
                            Span::styled(rest, Style::default().fg(th.dim)),
                        ])
                    }
                })
                .collect();
            let area = centered_rect(64, 66, f.area());
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines)
                    .style(Style::default().bg(th.bar_bg))
                    .block(th.panel_block(" Help ", true)),
                area,
            );
        }
        _ => {}
    }
}

fn render_theme_picker(f: &mut Frame, app: &App, pick: usize) {
    let th = app.theme;
    let lines: Vec<Line> = THEMES
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if i == pick {
                Span::styled("▸ ", Style::default().fg(th.accent).bold())
            } else {
                Span::raw("  ")
            };
            let mut name = Span::styled(
                t.name.to_string(),
                if i == pick {
                    Style::default().fg(th.fg).bold()
                } else {
                    Style::default().fg(th.dim)
                },
            );
            if t.name == app.theme.name {
                name = Span::styled(
                    format!("{}  ✓", t.name),
                    Style::default().fg(th.good).bold(),
                );
            }
            Line::from(vec![
                marker,
                Span::styled("■ ", Style::default().fg(t.accent)),
                Span::styled("■ ", Style::default().fg(t.good)),
                Span::styled("■ ", Style::default().fg(t.warn)),
                Span::styled("■ ", Style::default().fg(t.bad)),
                Span::raw(" "),
                name,
            ])
        })
        .collect();

    let area = centered_rect(42, 56, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(th.bar_bg))
            .block(th.panel_block(" Theme — j/k + enter, esc ", true)),
        area,
    );
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
    use crate::brew::{Deprecation, DeprecationKind, Package};
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
            deprecation: None,
            tap: None,
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
            brewfile_path: None,
            brewfile_entries: Vec::new(),
            brewfile: Vec::new(),
            packages: vec![pkg("git", false, false), pkg("firefox", true, true)],
            filtered: Vec::new(),
            section_idx: 0,
            list_idx: 0,
            panel: Panel::Sidebar,
            sort: SortMode::Natural,
            leaves: vec![],
            catalog: Vec::new(),
            installs: Default::default(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: Vec::new(),
            output_offset: 0,
            cmd_rx: None,
            frame: 0,
            load_rx: None,
            catalog_rx: None,
            theme: crate::theme::DEFAULT,
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
        // The theme background must cover the whole frame (no terminal bg leak).
        let th = app.theme;
        assert_eq!(
            buf[(100, 15)].bg,
            th.bg,
            "blank cell keeps theme background"
        );
        assert_eq!(buf[(0, 0)].bg, th.bar_bg, "header band uses bar background");
    }

    #[test]
    fn details_shows_installs_count() {
        let mut app = test_app();
        app.installs.insert("git".into(), 1_401_497);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("1.4M"), "installs count: {}", text);
        assert!(text.contains("90d"), "label: {}", text);
    }

    #[test]
    fn deprecated_package_badged_and_detailed() {
        let mut app = test_app();
        app.packages[0].deprecation = Some(Deprecation {
            kind: DeprecationKind::Deprecated,
            reason: "superseded".into(),
            replacement: Some("git-new".into()),
        });
        app.apply_section();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("! "), "deprecated marker: {}", text);
        assert!(text.contains("deprecated"), "reason line: {}", text);
        assert!(text.contains("git-new"), "replacement: {}", text);
    }

    #[test]
    fn theme_picker_lists_all_themes() {
        let mut app = test_app();
        app.mode = AppMode::ThemePicker(1);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        for t in crate::theme::THEMES {
            assert!(
                text.contains(t.name),
                "missing theme {} in {}",
                t.name,
                text
            );
        }
        assert!(text.contains("✓"), "current theme should be checkmarked");
    }

    #[test]
    fn selected_row_is_highlighted() {
        let app = test_app();
        let th = app.theme;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        // Row 1 (header) and row 2 (selected) should differ in background.
        let header_bg = buf[(30, 3)].bg;
        let sel_bg = buf[(30, 4)].bg;
        assert_eq!(header_bg, th.bar_bg, "table header band");
        assert!(
            sel_bg == th.hl_bg || sel_bg == th.hl_bg_dim,
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
            deprecation: None,
            tap: None,
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
            brewfile_path: None,
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
            sort: SortMode::Natural,
            leaves: vec!["git".into(), "zsh".into()],
            catalog: Vec::new(),
            installs: Default::default(),
            taps: Vec::new(),
            services: Vec::new(),
            vulns: Default::default(),
            search: String::new(),
            prompt_buffer: String::new(),
            mode: AppMode::Normal,
            output: vec!["$ brew install git".into(), "== done ==".into()],
            output_offset: 2,
            cmd_rx: None,
            frame: 3,
            load_rx: None,
            catalog_rx: None,
            theme: crate::theme::DEFAULT,
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
