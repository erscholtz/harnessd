//! Terminal dashboard for daemon and cache status.

use std::io::{self, Stdout};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::dashboard::DashboardSnapshot;

/// Run the interactive dashboard until the user quits.
pub async fn run() -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    let mut snapshot = crate::dashboard::collect().await;
    let refresh_interval = Duration::from_secs(1);
    let poll_interval = Duration::from_millis(250);
    let mut last_refresh = Instant::now();

    loop {
        terminal
            .draw(|frame| render(frame, &snapshot))
            .context("failed to draw dashboard")?;

        if event::poll(poll_interval)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('r') => {
                            snapshot = crate::dashboard::collect().await;
                            last_refresh = Instant::now();
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_refresh.elapsed() >= refresh_interval {
            snapshot = crate::dashboard::collect().await;
            last_refresh = Instant::now();
        }
    }

    drop(terminal);
    restore_terminal()?;
    crate::commands::teardown_runtime().await?;
    Ok(())
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

fn render(frame: &mut ratatui::Frame<'_>, snapshot: &DashboardSnapshot) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(8),
        ])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[1]);
    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[2]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(root[3]);

    frame.render_widget(header(snapshot), root[0]);
    frame.render_widget(daemon_panel(snapshot), top[0]);
    frame.render_widget(cache_panel(snapshot), top[1]);
    frame.render_widget(metrics_panel(snapshot), middle[0]);
    frame.render_widget(paths_panel(snapshot), middle[1]);
    frame.render_widget(recent_panel(snapshot), bottom[0]);
    frame.render_widget(help_panel(snapshot), bottom[1]);
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
