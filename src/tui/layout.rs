use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use crate::tui::TuiApp;

pub fn draw(f: &mut Frame, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),   // Status bar
            Constraint::Min(5),      // Main area
            Constraint::Length(3),   // Input
        ])
        .split(f.area());

    draw_status_bar(f, app, chunks[0]);
    draw_main_area(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);
}

fn draw_status_bar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let status = format!(
        " Alchemy v0.1.0 │ {} │ ⏱ {} steps │ 📊 {}k tokens",
        app.model_name,
        app.steps,
        app.total_tokens / 1000,
    );
    let p = Paragraph::new(status)
        .style(Style::default().bg(Color::Blue).fg(Color::White));
    f.render_widget(p, area);
}

fn draw_main_area(f: &mut Frame, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);

    draw_conversation(f, app, chunks[0]);
    draw_side_panels(f, app, chunks[1]);
}

fn draw_conversation(f: &mut Frame, app: &TuiApp, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize; // subtract borders
    let visible_height = area.height.saturating_sub(2) as usize;

    let mut lines = Vec::new();
    for msg in &app.messages {
        let prefix = if msg.role == "user" { "You: " } else { "Alchemy: " };
        let color = if msg.role == "user" { Color::Cyan } else { Color::Green };
        for source_line in msg.content.lines() {
            let full = format!("{}{}", prefix, source_line);
            // Wrap long lines manually to count rendered rows accurately.
            if full.len() <= inner_width || inner_width == 0 {
                lines.push(Line::from(Span::styled(full, Style::default().fg(color))));
            } else {
                let chars: Vec<char> = full.chars().collect();
                for chunk in chars.chunks(inner_width) {
                    lines.push(Line::from(Span::styled(
                        chunk.iter().collect::<String>(),
                        Style::default().fg(color),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
    }

    // Always show the most recent content at the bottom.
    let scroll = (lines.len() as u16).saturating_sub(visible_height as u16);

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("💬 Conversation"))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

fn draw_side_panels(f: &mut Frame, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Tool execution panel
    let tool_lines: Vec<Line> = app.tools_log.iter().map(|t| {
        Line::from(format!("{} {} ({}ms)", t.status, t.name, t.duration_ms))
    }).collect();
    let tools = Paragraph::new(tool_lines)
        .block(Block::default().borders(Borders::ALL).title("🔧 Tools"));
    f.render_widget(tools, chunks[0]);

    // File activity panel
    let file_lines: Vec<Line> = app.files_log.iter().map(|fl| {
        Line::from(format!("{} {}", fl.operation, fl.path))
    }).collect();
    let files = Paragraph::new(file_lines)
        .block(Block::default().borders(Borders::ALL).title("📁 Files"));
    f.render_widget(files, chunks[1]);
}

fn draw_input(f: &mut Frame, app: &TuiApp, area: Rect) {
    let input_text = if app.agent_busy {
        "⏳ Agent is working...".to_string()
    } else {
        format!("> {}_", app.input)
    };
    let p = Paragraph::new(input_text)
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(p, area);
}
