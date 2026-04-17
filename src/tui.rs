//! Terminal dashboard for daemon and cache status.

use std::io::{self, Stdout};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::dashboard::DashboardSnapshot;

struct AppState {
    snapshot: DashboardSnapshot,
    selected_project: PathBuf,
    recent_projects: Vec<PathBuf>,
    overlay: OverlayState,
    status_message: Option<StatusMessage>,
}

enum OverlayState {
    None,
    // Lightweight picker for switching between the current and recent roots.
    ProjectPicker { selected: usize },
    Browser(BrowserState),
}

struct BrowserState {
    current_dir: PathBuf,
    selected: usize,
    entries: Vec<BrowserEntry>,
}

#[derive(Clone)]
enum BrowserEntry {
    UseCurrent,
    Parent(PathBuf),
    Directory(PathBuf),
}

#[derive(Clone)]
enum ProjectOption {
    Project(PathBuf),
    Browse,
}

struct StatusMessage {
    text: String,
    color: Color,
}

/// Run the interactive dashboard until the user quits.
pub async fn run(initial_project: Option<PathBuf>) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    let mut app = AppState::new(initial_project).await;
    let refresh_interval = Duration::from_secs(1);
    let poll_interval = Duration::from_millis(250);
    let mut last_refresh = Instant::now();

    loop {
        terminal
            .draw(|frame| render(frame, &app))
            .context("failed to draw dashboard")?;

        if event::poll(poll_interval)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if handle_key_event(&mut app, key.code).await {
                            break;
                        }
                        last_refresh = Instant::now();
                    }
                }
                _ => {}
            }
        }

        if last_refresh.elapsed() >= refresh_interval {
            app.refresh().await;
            last_refresh = Instant::now();
        }
    }

    drop(terminal);
    restore_terminal()?;
    crate::commands::teardown_runtime().await?;
    Ok(())
}

impl AppState {
    async fn new(initial_project: Option<PathBuf>) -> Self {
        let snapshot = crate::dashboard::collect().await;
        let fallback = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let selected_project = normalize_project_path(initial_project.unwrap_or(fallback));
        let recent_projects = load_recent_projects(&selected_project);

        Self {
            snapshot,
            selected_project,
            recent_projects,
            overlay: OverlayState::None,
            status_message: None,
        }
    }

    async fn refresh(&mut self) {
        self.snapshot = crate::dashboard::collect().await;
    }

    async fn choose_project(&mut self, project: PathBuf) {
        let project = normalize_project_path(project);
        match crate::commands::prefetch_runtime_path(&project).await {
            Ok(result) => {
                self.selected_project = project.clone();
                self.recent_projects = push_recent_project(self.recent_projects.clone(), &project);
                save_recent_projects(&self.recent_projects);
                self.set_status(
                    format!(
                        "Parsed {} files, found {} anchors, stored {} proposals from {}",
                        result.scanned_files,
                        result.anchors_found,
                        result.proposals_stored,
                        project.display()
                    ),
                    Color::Green,
                );
                self.overlay = OverlayState::None;
                self.refresh().await;
            }
            Err(error) => {
                self.set_status(format!("Project selection failed: {error}"), Color::Red);
                self.overlay = OverlayState::None;
            }
        }
    }

    fn set_status(&mut self, text: String, color: Color) {
        self.status_message = Some(StatusMessage { text, color });
    }
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("failed to initialize terminal")
}

fn restore_terminal() -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

async fn handle_key_event(app: &mut AppState, key: KeyCode) -> bool {
    let mut choose_project = None;
    let mut open_browser = None;
    let mut quit = false;

    match &mut app.overlay {
        OverlayState::ProjectPicker { selected } => {
            let options = project_options(&app.selected_project, &app.recent_projects);
            match key {
                KeyCode::Esc => app.overlay = OverlayState::None,
                KeyCode::Up => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if *selected + 1 < options.len() {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => match options.get(*selected).cloned() {
                    Some(ProjectOption::Project(path)) => choose_project = Some(path),
                    Some(ProjectOption::Browse) => {
                        open_browser = Some(app.selected_project.clone());
                    }
                    None => {}
                },
                _ => {}
            }
        }
        OverlayState::Browser(browser) => match key {
            KeyCode::Esc => app.overlay = OverlayState::None,
            KeyCode::Up => {
                if browser.selected > 0 {
                    browser.selected -= 1;
                }
            }
            KeyCode::Down => {
                if browser.selected + 1 < browser.entries.len() {
                    browser.selected += 1;
                }
            }
            KeyCode::Backspace | KeyCode::Left => {
                if let Some(parent) = browser.current_dir.parent() {
                    *browser = BrowserState::new(parent.to_path_buf());
                }
            }
            KeyCode::Enter => match browser.entries.get(browser.selected).cloned() {
                Some(BrowserEntry::UseCurrent) => {
                    choose_project = Some(browser.current_dir.clone())
                }
                Some(BrowserEntry::Parent(path)) | Some(BrowserEntry::Directory(path)) => {
                    *browser = BrowserState::new(path);
                }
                None => {}
            },
            _ => {}
        },
        OverlayState::None => match key {
            KeyCode::Char('q') | KeyCode::Esc => quit = true,
            KeyCode::Char('r') => app.refresh().await,
            KeyCode::Char('p') => app.overlay = OverlayState::ProjectPicker { selected: 0 },
            _ => {}
        },
    }

    if let Some(path) = open_browser {
        app.overlay = OverlayState::Browser(BrowserState::new(path));
    }
    if let Some(path) = choose_project {
        app.choose_project(path).await;
    }

    quit
}

fn render(frame: &mut ratatui::Frame<'_>, app: &AppState) {
    let snapshot = &app.snapshot;
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Min(8),
        ])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[2]);
    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[3]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(root[4]);

    frame.render_widget(header(snapshot), root[0]);
    frame.render_widget(project_panel(app), root[1]);
    frame.render_widget(daemon_panel(snapshot), top[0]);
    frame.render_widget(cache_panel(snapshot), top[1]);
    frame.render_widget(metrics_panel(snapshot), middle[0]);
    frame.render_widget(paths_panel(snapshot), middle[1]);
    frame.render_widget(recent_panel(snapshot), bottom[0]);
    frame.render_widget(help_panel(snapshot), bottom[1]);

    match &app.overlay {
        OverlayState::ProjectPicker { selected } => {
            render_project_picker(frame, app, *selected);
        }
        OverlayState::Browser(browser) => {
            render_browser(frame, browser);
        }
        OverlayState::None => {}
    }
}

fn header(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let status = if snapshot.remote_status.is_some() {
        if snapshot.runtime_health.warnings.is_empty() {
            styled("ONLINE", Color::Green)
        } else {
            styled("WARNING", Color::Yellow)
        }
    } else if !snapshot.runtime_health.warnings.is_empty() {
        styled("STALE STATE", Color::Yellow)
    } else if snapshot.lock_exists {
        styled("DEGRADED", Color::Yellow)
    } else {
        styled("OFFLINE", Color::Red)
    };

    Paragraph::new(Line::from(vec![
        Span::styled(
            "harnessd dashboard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        status,
        Span::raw(format!(
            "  refreshed {}",
            format_unix(snapshot.collected_at)
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"))
}

fn project_panel(app: &AppState) -> Paragraph<'static> {
    let mut lines = vec![
        kv("Project", app.selected_project.display().to_string()),
        Line::raw("Press p to choose a recent project or browse for a folder."),
    ];

    if let Some(status) = &app.status_message {
        lines.push(Line::styled(
            truncate(&status.text, 120),
            Style::default().fg(status.color),
        ));
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Project"))
        .wrap(Wrap { trim: true })
}

fn daemon_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let mut lines = vec![
        kv(
            "Daemon PID",
            snapshot
                .remote_status
                .as_ref()
                .map(|status| status.pid.to_string())
                .or_else(|| snapshot.daemon_pid.map(|pid| pid.to_string()))
                .unwrap_or_else(|| "not running".to_string()),
        ),
        kv("Lock file", bool_text(snapshot.lock_exists)),
        kv("IPC ready", bool_text(snapshot.ipc_ready)),
        kv(
            "Runtime warnings",
            snapshot.runtime_health.warnings.len().to_string(),
        ),
    ];

    if let Some(status) = &snapshot.remote_status {
        lines.push(kv("Started", format_unix(status.started_at)));
        lines.push(kv("Uptime", format_duration(status.uptime_secs)));
    } else if let Some(error) = &snapshot.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            truncate(error, 96),
            Style::default().fg(Color::Yellow),
        ));
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Daemon"))
        .wrap(Wrap { trim: true })
}

fn cache_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let cache = snapshot
        .remote_status
        .as_ref()
        .map(|status| &status.cache)
        .or(snapshot.local_cache.as_ref());

    let mut lines = vec![
        kv("DB exists", bool_text(snapshot.cache_db_exists)),
        kv("DB size", format_bytes(snapshot.cache_db_size_bytes)),
    ];

    if let Some(cache) = cache {
        lines.push(kv("Proposals", cache.total_proposals.to_string()));
        lines.push(kv("Payload size", format_bytes(cache.total_bytes as u64)));
        lines.push(kv(
            "Caps",
            format!("{} lines / {} bytes", cache.max_lines, cache.max_bytes),
        ));
        lines.push(kv(
            "Newest entry",
            cache
                .newest_timestamp
                .map(|ts| format_unix(ts as u64))
                .unwrap_or_else(|| "none".to_string()),
        ));
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Cache DB"))
        .wrap(Wrap { trim: true })
}

fn metrics_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let lines = if let Some(status) = &snapshot.remote_status {
        vec![
            kv("Total requests", status.metrics.total_requests.to_string()),
            kv("complete()", status.metrics.complete_requests.to_string()),
            kv("prefetch()", status.metrics.prefetch_requests.to_string()),
            kv("status()", status.metrics.status_requests.to_string()),
            kv("shutdown()", status.metrics.shutdown_requests.to_string()),
            kv(
                "Last request",
                status
                    .metrics
                    .last_request_at
                    .map(format_unix)
                    .unwrap_or_else(|| "none".to_string()),
            ),
        ]
    } else {
        vec![Line::raw(
            "Metrics are available once the daemon answers `status`.",
        )]
    };

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("RPC Metrics"))
        .wrap(Wrap { trim: true })
}

fn paths_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let lines = vec![
        kv("Runtime dir", snapshot.runtime_dir.display().to_string()),
        kv("IPC endpoint", snapshot.ipc_endpoint.clone()),
        kv("Lock path", snapshot.lock_path.display().to_string()),
        kv("Cache DB", snapshot.cache_db_path.display().to_string()),
    ];

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Paths"))
        .wrap(Wrap { trim: false })
}

fn recent_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let lines = if let Some(status) = &snapshot.remote_status {
        if status.recent_proposals.is_empty() {
            vec![Line::raw("No cached proposals yet.")]
        } else {
            status
                .recent_proposals
                .iter()
                .flat_map(|proposal| {
                    [
                        Line::styled(
                            format!(
                                "{}  [{}..{}]  {}",
                                proposal.label,
                                proposal.byte_start,
                                proposal.byte_end,
                                proposal.file_path
                            ),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Line::raw(format!(
                            "{}  {}",
                            format_unix(proposal.created_at as u64),
                            truncate(&proposal.snippet_preview, 110)
                        )),
                        Line::raw(""),
                    ]
                })
                .collect()
        }
    } else {
        vec![Line::raw(
            "Recent proposal previews require a live daemon connection.",
        )]
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent Proposals"),
        )
        .wrap(Wrap { trim: true })
}

fn help_panel(snapshot: &DashboardSnapshot) -> Paragraph<'static> {
    let mut lines = vec![
        Line::raw("q / Esc  quit + teardown"),
        Line::raw("r        refresh now"),
        Line::raw("p        choose project root"),
        Line::raw(""),
    ];

    if !snapshot.runtime_health.warnings.is_empty() {
        for warning in snapshot.runtime_health.warnings.iter().take(3) {
            lines.push(Line::styled(
                truncate(warning, 100),
                Style::default().fg(Color::Yellow),
            ));
        }
    } else if let Some(error) = &snapshot.error {
        lines.push(Line::styled(
            truncate(error, 100),
            Style::default().fg(Color::Yellow),
        ));
    } else {
        lines.push(Line::raw("Polling every second."));
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true })
}

fn render_project_picker(frame: &mut ratatui::Frame<'_>, app: &AppState, selected: usize) {
    let area = centered_rect(72, 55, frame.area());
    let options = project_options(&app.selected_project, &app.recent_projects);
    let items: Vec<ListItem<'static>> = options
        .iter()
        .map(|option| match option {
            ProjectOption::Project(path) => ListItem::new(path.display().to_string()),
            ProjectOption::Browse => ListItem::new("Browse..."),
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(selected));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Select Project"),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_browser(frame: &mut ratatui::Frame<'_>, browser: &BrowserState) {
    let area = centered_rect(80, 70, frame.area());
    let items: Vec<ListItem<'static>> = browser
        .entries
        .iter()
        .map(|entry| ListItem::new(browser_entry_label(entry)))
        .collect();
    let mut state = ListState::default();
    state.select(Some(browser.selected));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Browse {}", browser.current_dir.display())),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn kv(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Gray)),
        Span::raw(value),
    ])
}

fn bool_text(value: bool) -> String {
    if value {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}

fn styled(text: &str, color: Color) -> Span<'static> {
    Span::styled(
        text.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn format_unix(timestamp: u64) -> String {
    format!("{timestamp}s")
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}h {minutes:02}m {secs:02}s")
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = restore_terminal();
        }));
    }
}

impl BrowserState {
    fn new(start_dir: PathBuf) -> Self {
        let current_dir = normalize_project_path(start_dir);
        let entries = browser_entries(&current_dir);
        Self {
            current_dir,
            selected: 0,
            entries,
        }
    }
}

fn project_options(selected_project: &Path, recent_projects: &[PathBuf]) -> Vec<ProjectOption> {
    let mut options = vec![ProjectOption::Project(selected_project.to_path_buf())];
    for path in recent_projects {
        if path != selected_project {
            options.push(ProjectOption::Project(path.clone()));
        }
    }
    options.push(ProjectOption::Browse);
    options
}

fn browser_entries(current_dir: &Path) -> Vec<BrowserEntry> {
    let mut entries = vec![BrowserEntry::UseCurrent];
    if let Some(parent) = current_dir.parent() {
        entries.push(BrowserEntry::Parent(parent.to_path_buf()));
    }

    let mut directories: Vec<PathBuf> = std::fs::read_dir(current_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => Some(entry.path()),
            _ => None,
        })
        .collect();
    directories.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    entries.extend(directories.into_iter().map(BrowserEntry::Directory));
    entries
}

fn browser_entry_label(entry: &BrowserEntry) -> String {
    match entry {
        BrowserEntry::UseCurrent => "[Select this directory]".to_string(),
        BrowserEntry::Parent(path) => format!("[..] {}", path.display()),
        BrowserEntry::Directory(path) => {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<dir>");
            format!("{name}/")
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn normalize_project_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn load_recent_projects(selected_project: &Path) -> Vec<PathBuf> {
    let path = crate::paths::recent_projects_path();
    let recent = std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Vec<String>>(&contents).ok())
        .unwrap_or_default();

    // Keep the current project at the front so the picker always has a valid default.
    push_recent_project(
        recent.into_iter().map(PathBuf::from).collect(),
        selected_project,
    )
}

fn save_recent_projects(recent_projects: &[PathBuf]) {
    let path = crate::paths::recent_projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let serializable: Vec<String> = recent_projects
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&serializable) {
        std::fs::write(path, json).ok();
    }
}

fn push_recent_project(mut recent_projects: Vec<PathBuf>, project: &Path) -> Vec<PathBuf> {
    recent_projects.retain(|path| path != project);
    recent_projects.insert(0, project.to_path_buf());
    recent_projects.truncate(8);
    recent_projects
}
