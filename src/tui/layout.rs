use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use crate::tui::TuiApp;

pub fn draw(f: &mut Frame, app: &mut TuiApp) {
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

fn draw_status_bar(f: &mut Frame, app: &mut TuiApp, area: Rect) {
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

fn draw_main_area(f: &mut Frame, app: &mut TuiApp, area: Rect) {
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

fn render_message_lines(prefix: &str, content: &str, color: Color, inner_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for source_line in content.lines() {
        let full = format!("{}{}", prefix, source_line);
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
    lines
}

fn draw_conversation(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize; // subtract borders
    let visible_height = area.height.saturating_sub(2) as usize;

    let mut lines = Vec::new();
    for msg in &app.messages {
        let prefix = if msg.role == "user" { "You: " } else { "Alchemy: " };
        let color = if msg.role == "user" { Color::Cyan } else { Color::Green };
        lines.extend(render_message_lines(prefix, &msg.content, color, inner_width));
        lines.push(Line::from(""));
    }

    // Ghost message: streaming in-progress assistant response.
    if let Some(ref content) = app.streaming_content {
        lines.extend(render_message_lines("Alchemy: ", &format!("{}▋", content), Color::Green, inner_width));
        lines.push(Line::from(""));
    }

    let total_lines = lines.len() as u16;
    let max_scroll = total_lines.saturating_sub(visible_height as u16);
    app.conv_max_scroll = max_scroll;
    let scroll = if app.conv_follow {
        max_scroll
    } else {
        (app.conv_scroll as u16).min(max_scroll)
    };

    let border_style = if app.focused_panel == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("💬 Conversation")
                .border_style(border_style),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

fn draw_side_panels(f: &mut Frame, app: &mut TuiApp, area: Rect) {
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
    let tools_border = if app.focused_panel == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let tools_max = (tool_lines.len() as u16).saturating_sub(chunks[0].height.saturating_sub(2));
    let tools = Paragraph::new(tool_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("🔧 Tools")
                .border_style(tools_border),
        )
        .scroll(((app.tools_scroll as u16).min(tools_max), 0));
    f.render_widget(tools, chunks[0]);

    // File activity panel
    let file_lines: Vec<Line> = app.files_log.iter().map(|fl| {
        Line::from(format!("{} {}", fl.operation, fl.path))
    }).collect();
    let files_border = if app.focused_panel == 2 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let files_max = (file_lines.len() as u16).saturating_sub(chunks[1].height.saturating_sub(2));
    let files = Paragraph::new(file_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("📁 Files")
                .border_style(files_border),
        )
        .scroll(((app.files_scroll as u16).min(files_max), 0));
    f.render_widget(files, chunks[1]);
}

fn draw_input(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let input_text = if app.agent_busy {
        "⏳ Agent is working...".to_string()
    } else {
        format!("> {}_", app.input)
    };
    let p = Paragraph::new(input_text)
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(p, area);
}
