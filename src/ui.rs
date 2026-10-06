//! Rendering.

use crate::app::{App, Panel, spinner};
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

    let header = Paragraph::new(format!(
        " lazybrew — {} packages ({} outdated) — {} leaves",
        app.packages.len(),
        app.packages.iter().filter(|p| p.outdated).count(),
        app.leaves.len()
    ))
    .style(Style::default().fg(Color::Cyan).bold());
    f.render_widget(header, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(20),
            Constraint::Length(24),
        ])
        .split(chunks[2]);

    let output_text = if app.output.is_empty() {
        "(no command output yet)".to_string()
    } else {
        let start = app.output.len().saturating_sub(6);
        app.output[start..].join("\n")
    };
    let output_title = if app.cmd_rx.is_some() {
        format!(" Output {} ", spinner(app))
    } else {
        " Output ".to_string()
    };
    let output = Paragraph::new(output_text)
        .block(Block::default().title(output_title).borders(Borders::ALL));
    f.render_widget(output, chunks[3]);

    // Sidebar
    let items: Vec<ListItem> = app
        .sections
        .iter()
        .map(|s| ListItem::new(format!("{} ({})", s.title(), app.count_for(*s))))
        .collect();
    let sidebar = List::new(items)
        .block(
            Block::default()
                .title(" Sections ")
                .borders(Borders::ALL)
                .border_style(if app.panel == Panel::Sidebar {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).bold());
    let mut state = ListState::default();
    state.select(Some(app.section_idx));
    f.render_stateful_widget(sidebar, body[0], &mut state);

    // Search row (full width, above body)
    let search_style = if app.searching {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let cursor = if app.searching { "_" } else { "" };
    f.render_widget(
        Paragraph::new(format!("/{}{}", app.search, cursor)).style(search_style),
        chunks[1],
    );

    if app.installing {
        f.render_widget(
            Paragraph::new(format!("install package: {}_", app.install_input))
                .style(Style::default().fg(Color::Green)),
            chunks[1],
        );
    }

    let rows: Vec<Row> = app
        .filtered
        .iter()
        .map(|p| {
            let kind = if p.cask { "cask" } else { "brew" };
            let mark = if p.outdated { " *" } else { "" };
            Row::new(vec![
                Cell::from(format!("{}{}", p.name, mark)),
                Cell::from(p.version.clone()),
                Cell::from(kind),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(6),
        ],
    )
    .header(Row::new(vec!["Name", "Version", "Type"]).style(Style::default().bold().underlined()))
    .block(
        Block::default()
            .title(format!(" {} ", app.sections[app.section_idx].title()))
            .borders(Borders::ALL)
            .border_style(if app.panel == Panel::List {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            }),
    )
    .row_highlight_style(Style::default().bg(Color::DarkGray));
    let mut tstate = TableState::default();
    tstate.select(if app.filtered.is_empty() {
        None
    } else {
        Some(app.list_idx)
    });
    f.render_stateful_widget(table, body[1], &mut tstate);

    // Details
    let details_text = match app.selected() {
        Some(p) => {
            let vuln_line = match app.vulns.get(&p.name) {
                Some(v) if v.is_empty() => "Vulnerabilities: none\n".to_string(),
                Some(v) => format!("Vulnerabilities: {}\n", v.len()),
                None => String::new(),
            };
            let service_line = match &p.service_status {
                Some(st) => format!("Service: {st} (press s to toggle)\n"),
                None => String::new(),
            };
            format!(
                "Name: {}\nType: {}\nVersion: {}\nInstalled: {}\nOutdated: {}\nPinned: {}\n{}\n{}\n{}",
                p.name,
                if p.cask { "cask" } else { "formula" },
                p.version,
                p.installed_version.as_deref().unwrap_or("-"),
                p.outdated,
                p.pinned,
                service_line,
                vuln_line,
                p.desc
            )
        }
        None => "No package selected".to_string(),
    };
    let details = Paragraph::new(details_text)
        .wrap(Wrap { trim: true })
        .block(Block::default().title(" Details ").borders(Borders::ALL));
    f.render_widget(details, body[2]);

    // Footer
    let footer_text = if app.load_rx.is_some() {
        format!("{} loading Homebrew data...", spinner(app))
    } else if app.catalog_rx.is_some() {
        format!("{} loading catalog (once a day)...", spinner(app))
    } else {
        let full = "j/k nav | h/l switch | / search | i install | u upgrade | r remove | U update | x menu | esc clear | q quit";
        let compact = "j/k nav | / search | i/u/r ops | U update | x menu | ? help | q quit";
        if chunks[4].width as usize > full.len() {
            full
        } else {
            compact
        }
        .to_string()
    };
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Gray));
    f.render_widget(footer, chunks[4]);

    if let Some(modal) = &app.modal {
        let area = centered_rect(50, 20, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(modal.text.clone()).block(
                Block::default()
                    .title(" Confirm ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            area,
        );
    }

    if let Some(menu_idx) = app.menu {
        let items = ["u Upgrade", "r Remove", "i Info", "d Deps", "p Pin/Unpin"];
        let text = items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                if i == menu_idx {
                    format!("> {}", it)
                } else {
                    format!("  {}", it)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let area = centered_rect(30, 30, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(text).block(
                Block::default()
                    .title(" Actions ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            area,
        );
    }

    if app.help {
        let help_text = crate::input::help_text();
        let area = centered_rect(60, 60, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(help_text).block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
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
    use crate::app::{App, Panel, Section};
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

    #[test]
    fn renders_sections_and_packages() {
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
            services: Vec::new(),
            taps: Vec::new(),
            vulns: Default::default(),
            catalog_rx: None,
            help: false,
        };
        app.apply_section();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Sections"), "missing Sections: {}", text);
        assert!(text.contains("git"), "missing git");
        assert!(text.contains("Installed"), "missing Installed");
    }
}

#[cfg(test)]
mod preview {
    use super::*;
    use crate::app::{App, Panel, Section};
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
            services: Vec::new(),
            taps: Vec::new(),
            vulns: Default::default(),
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
