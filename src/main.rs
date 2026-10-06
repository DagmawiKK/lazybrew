mod brew;

use anyhow::Result;
use brew::Package;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, widgets::*};
use std::io;
use std::sync::mpsc;
use std::time::Duration;

enum CmdEvent {
    Line(String),
    Done(bool),
}

type LoadResult = (Vec<Package>, Vec<String>);

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
    installing: bool,
    install_input: String,
    output: Vec<String>,
    cmd_rx: Option<mpsc::Receiver<CmdEvent>>,
    modal: Option<Modal>,
    menu: Option<usize>,
    frame: usize,
    load_rx: Option<mpsc::Receiver<LoadResult>>,
    help: bool,
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner(app: &App) -> char {
    SPINNER[app.frame % SPINNER.len()]
}

struct Modal {
    text: String,
    confirm: ModalAction,
}

enum ModalAction {
    Upgrade(String, bool), // name, is_cask
    Remove(String, bool),
    Install(String),
    Update,
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
        [self.packages.len(), outdated, casks, self.leaves.len()]
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

fn spawn_load_thread() -> mpsc::Receiver<LoadResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let pkgs = brew::load_installed().unwrap_or_default();
        let leaves = brew::load_leaves();
        let _ = tx.send((pkgs, leaves));
    });
    rx
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let load_rx = spawn_load_thread();

    let mut app = App {
        packages: Vec::new(),
        filtered: Vec::new(),
        section_idx: 0,
        list_idx: 0,
        panel: Panel::Sidebar,
        leaves: Vec::new(),
        search: String::new(),
        searching: false,
        installing: false,
        install_input: String::new(),
        output: Vec::new(),
        cmd_rx: None,
        modal: None,
        menu: None,
        frame: 0,
        load_rx: Some(load_rx),
        help: false,
    };
    app.apply_section();

    loop {
        app.frame = app.frame.wrapping_add(1);
        terminal.draw(|f| render(f, &app))?;

        let mut loaded = None;
        if let Some(rx) = &app.load_rx
            && let Ok(res) = rx.try_recv()
        {
            loaded = Some(res);
        }
        if let Some((pkgs, leaves)) = loaded {
            app.packages = pkgs;
            app.leaves = leaves;
            app.apply_section();
            app.load_rx = None;
        }

        // Drain background command events
        let mut finished = false;
        if let Some(rx) = &app.cmd_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    CmdEvent::Line(l) => app.output.push(l),
                    CmdEvent::Done(ok) => {
                        app.output.push(if ok {
                            "== done ==".into()
                        } else {
                            "== FAILED ==".into()
                        });
                        finished = true;
                    }
                }
            }
        }
        if finished {
            app.cmd_rx = None;
            app.load_rx = Some(spawn_load_thread());
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if app.help {
                app.help = false;
                continue;
            }

            if let Some(menu_idx) = app.menu {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('x') => app.menu = None,
                    KeyCode::Down | KeyCode::Char('j') => app.menu = Some((menu_idx + 1) % 5),
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.menu = Some(menu_idx.checked_sub(1).unwrap_or(4))
                    }
                    KeyCode::Enter => {
                        app.menu = None;
                        run_menu_action(&mut app, menu_idx);
                    }
                    KeyCode::Char(c) => match c {
                        'u' => {
                            app.menu = None;
                            run_menu_action(&mut app, 0);
                        }
                        'r' => {
                            app.menu = None;
                            run_menu_action(&mut app, 1);
                        }
                        'i' => {
                            app.menu = None;
                            run_menu_action(&mut app, 2);
                        }
                        'd' => {
                            app.menu = None;
                            run_menu_action(&mut app, 3);
                        }
                        'p' => {
                            app.menu = None;
                            run_menu_action(&mut app, 4);
                        }
                        _ => {}
                    },
                    _ => {}
                }
                continue;
            }

            if let Some(modal) = app.modal.take() {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        run_modal_action(&mut app, &modal);
                        continue;
                    }
                    _ => continue,
                }
            }

            if app.installing {
                match key.code {
                    KeyCode::Enter => {
                        let name = app.install_input.trim().to_string();
                        app.installing = false;
                        app.install_input.clear();
                        if !name.is_empty() {
                            app.modal = Some(Modal {
                                text: format!("Install '{}'? (y/n)", name),
                                confirm: ModalAction::Install(name),
                            });
                        }
                    }
                    KeyCode::Esc => {
                        app.installing = false;
                        app.install_input.clear();
                    }
                    KeyCode::Backspace => {
                        app.install_input.pop();
                    }
                    KeyCode::Char(c) => app.install_input.push(c),
                    _ => {}
                }
                continue;
            }

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
                KeyCode::Char('x') => {
                    if app.selected().is_some() {
                        app.menu = Some(0);
                    }
                }
                KeyCode::Char('?') => app.help = true,
                KeyCode::Char('e') => {
                    let home = dirs::home_dir().unwrap_or_else(|| ".".into());
                    let path = home.join("Brewfile");
                    spawn_brew(
                        &mut app,
                        &[
                            "bundle".into(),
                            "dump".into(),
                            "--force".into(),
                            format!("--file={}", path.display()),
                        ],
                    );
                    app.output
                        .push(format!("Brewfile written to {}", path.display()));
                }
                KeyCode::Char('u') => {
                    if let Some(p) = app.selected() {
                        let p = p.clone();
                        app.modal = Some(Modal {
                            text: format!("Upgrade '{}'? (y/n)", p.name),
                            confirm: ModalAction::Upgrade(p.name, p.cask),
                        });
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(p) = app.selected() {
                        let p = p.clone();
                        app.modal = Some(Modal {
                            text: format!("Remove '{}'? (y/n)", p.name),
                            confirm: ModalAction::Remove(p.name, p.cask),
                        });
                    }
                }
                KeyCode::Char('U') => {
                    app.modal = Some(Modal {
                        text: "Run 'brew update'? (y/n)".into(),
                        confirm: ModalAction::Update,
                    });
                }
                KeyCode::Char('i') => {
                    app.searching = false;
                    app.installing = true;
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
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

fn run_menu_action(app: &mut App, idx: usize) {
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

fn run_modal_action(app: &mut App, modal: &Modal) {
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

fn spawn_brew(app: &mut App, args: &[String]) {
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

fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
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
            Constraint::Percentage(55),
            Constraint::Percentage(25),
        ])
        .split(chunks[1]);

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
    f.render_widget(output, chunks[2]);

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

    if app.installing {
        f.render_widget(
            Paragraph::new(format!("install package: {}_", app.install_input))
                .style(Style::default().fg(Color::Green)),
            list_area[0],
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
    tstate.select(if app.filtered.is_empty() {
        None
    } else {
        Some(app.list_idx)
    });
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
    let footer_text = if app.load_rx.is_some() {
        format!("{} loading Homebrew data...", spinner(app))
    } else {
        "j/k nav | h/l switch | / search | i install | u upgrade | r remove | U update | x menu | esc clear | q quit".to_string()
    };
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Gray));
    f.render_widget(footer, chunks[3]);

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
        let help_text = "lazybrew keybindings\n\n\
            j/k or up/down   navigate\n\
            h/l or tab       switch panel\n\
            /                search\n\
            esc              clear search\n\
            g/G              top/bottom\n\
            i                install package\n\
            u / r            upgrade / remove\n\
            U                brew update\n\
            x                action menu\n\
            ?                this help\n\
            q                quit";
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
