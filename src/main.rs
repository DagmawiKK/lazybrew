mod brew;

use anyhow::Result;
use brew::Package;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use std::io;

/// Sections shown in the lazygit-style left sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Installed,
    Outdated,
    Casks,
    Leaves,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::Installed,
        Section::Outdated,
        Section::Casks,
        Section::Leaves,
    ];

    fn title(&self) -> &'static str {
        match self {
            Section::Installed => "Installed",
            Section::Outdated => "Outdated",
            Section::Casks => "Casks",
            Section::Leaves => "Leaves",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Sidebar,
    List,
}

struct App {
    packages: Vec<Package>,
    filtered: Vec<Package>,
    section_idx: usize,
    list_idx: usize,
    panel: Panel,
    leaves: Vec<String>,
    search: String,
    searching: bool,
}

impl App {
    fn apply_section(&mut self) {
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

    fn section_counts(&self) -> [usize; 4] {
        let outdated = self.packages.iter().filter(|p| p.outdated).count();
        let casks = self.packages.iter().filter(|p| p.cask).count();
        [
            self.packages.len(),
            outdated,
            casks,
            self.leaves.len(),
        ]
    }

    fn selected(&self) -> Option<&Package> {
        self.filtered.get(self.list_idx)
    }
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let packages = brew::load_installed().unwrap_or_default();
    let leaves: Vec<String> = std::process::Command::new("brew")
        .arg("leaves")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut app = App {
        packages,
        filtered: Vec::new(),
        section_idx: 0,
        list_idx: 0,
        panel: Panel::Sidebar,
        leaves,
        search: String::new(),
        searching: false,
    };
    app.apply_section();

    loop {
        terminal.draw(|f| render(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if app.searching {
                match key.code {
                    KeyCode::Enter => app.searching = false,
                    KeyCode::Esc => {
                        app.search.clear();
                        app.apply_section();
                        app.searching = false;
                    }
                    KeyCode::Backspace => {
                        app.search.pop();
                        app.apply_section();
                    }
                    KeyCode::Char(c) => {
                        app.search.push(c);
                        app.apply_section();
                    }
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('/') => {
                    app.searching = true;
                }
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    app.panel = match app.panel {
                        Panel::Sidebar => Panel::List,
                        Panel::List => Panel::Sidebar,
                    };
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    app.panel = match app.panel {
                        Panel::Sidebar => Panel::List,
                        Panel::List => Panel::Sidebar,
                    };
                }
                KeyCode::Down | KeyCode::Char('j') => match app.panel {
                    Panel::Sidebar => {
                        app.section_idx = (app.section_idx + 1) % Section::ALL.len();
                        app.apply_section();
                    }
                    Panel::List => {
                        if app.list_idx + 1 < app.filtered.len() {
                            app.list_idx += 1;
                        }
                    }
                },
                KeyCode::Up | KeyCode::Char('k') => match app.panel {
                    Panel::Sidebar => {
                        app.section_idx = app
                            .section_idx
                            .checked_sub(1)
                            .unwrap_or(Section::ALL.len() - 1);
                        app.apply_section();
                    }
                    Panel::List => {
                        app.list_idx = app.list_idx.saturating_sub(1);
                    }
                },
                KeyCode::Enter => {
                    if app.panel == Panel::Sidebar {
                        app.panel = Panel::List;
                    }
                }
                KeyCode::Esc => {
                    if !app.search.is_empty() {
                        app.search.clear();
                        app.apply_section();
                    }
                }
                KeyCode::Char('g') => {
                    if app.panel == Panel::List {
                        app.list_idx = 0;
                    } else {
                        app.section_idx = 0;
                        app.apply_section();
                    }
                }
                KeyCode::Char('G') => {
                    if app.panel == Panel::List {
                        app.list_idx = app.filtered.len().saturating_sub(1);
                    } else {
                        app.section_idx = Section::ALL.len() - 1;
                        app.apply_section();
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(())
                }
                _ => {}
            }
        }
    }
}

fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(f.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Percentage(55), Constraint::Percentage(25)])
        .split(chunks[0]);

    // Sidebar
    let counts = app.section_counts();
    let items: Vec<ListItem> = Section::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let line = format!("{} ({})", s.title(), counts[i]);
            ListItem::new(line)
        })
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

    // Package list
    let list_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(body[1]);

    let search_style = if app.searching {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let cursor = if app.searching { "_" } else { "" };
    f.render_widget(
        Paragraph::new(format!("/{}{}", app.search, cursor)).style(search_style),
        list_area[0],
    );

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
    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(14), Constraint::Length(6)])
        .header(
            Row::new(vec!["Name", "Version", "Type"])
                .style(Style::default().bold().underlined()),
        )
        .block(
            Block::default()
                .title(format!(" {} ", Section::ALL[app.section_idx].title()))
                .borders(Borders::ALL)
                .border_style(if app.panel == Panel::List {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray));
    let mut tstate = TableState::default();
    tstate.select(if app.filtered.is_empty() { None } else { Some(app.list_idx) });
    f.render_stateful_widget(table, list_area[1], &mut tstate);

    // Details
    let details_text = match app.selected() {
        Some(p) => format!(
            "Name: {}\nType: {}\nVersion: {}\nInstalled: {}\nOutdated: {}\nPinned: {}\n\n{}",
            p.name,
            if p.cask { "cask" } else { "formula" },
            p.version,
            p.installed_version.as_deref().unwrap_or("?"),
            p.outdated,
            p.pinned,
            p.desc
        ),
        None => "No package selected".to_string(),
    };
    let details = Paragraph::new(details_text)
        .wrap(Wrap { trim: true })
        .block(Block::default().title(" Details ").borders(Borders::ALL));
    f.render_widget(details, body[2]);

    // Footer
    let footer = Paragraph::new("j/k nav | h/l or tab switch | / search | esc clear | g/G top/bottom | q quit")
        .style(Style::default().fg(Color::Gray));
    f.render_widget(footer, chunks[1]);
}
