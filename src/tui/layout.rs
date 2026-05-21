use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use crate::tui::TuiApp;
use crate::tui::theme::ThemePalette;

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

    // Help overlay drawn last so it floats above everything.
    if app.show_help {
        draw_help_overlay(f, f.area(), app.theme(), app.help_scroll, &mut app.help_max_scroll);
    }
}

fn draw_status_bar(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();
    let left = format!(
        " Alchemy  │ {}  │ {}  │ ⏱ {} steps  │ 📊 {}k tokens  │ 🎨 {}",
        app.session_name,
        app.model_name,
        app.steps,
        app.total_tokens / 1000,
        t.name,
    );
    let right = "? help ";

    let pad = (area.width as usize).saturating_sub(left.len() + right.len());
    let full = format!("{}{}{}", left, " ".repeat(pad), right);

    let p = Paragraph::new(Line::from(vec![
        Span::styled(full, Style::default().bg(t.status_bg).fg(t.status_fg)),
    ]));
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

fn render_message_lines(
    prefix: &str,
    content: &str,
    t: &ThemePalette,
    is_user: bool,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let color = if is_user { t.user_fg } else { t.assistant_fg };
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

/// Render an assistant message: bold prefix line then markdown-styled body.
fn render_assistant_message(prefix: &str, content: &str, t: &ThemePalette) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        prefix.trim_end().to_string(),
        Style::default().fg(t.assistant_fg).add_modifier(Modifier::BOLD),
    )));
    lines.extend(crate::tui::markdown::render(content, t));
    lines
}

fn draw_conversation(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();
    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_height = area.height.saturating_sub(2) as usize;

    let mut lines = Vec::new();
    for msg in &app.messages {
        if msg.role == "user" {
            lines.extend(render_message_lines("You: ", &msg.content, t, true, inner_width));
        } else {
            lines.extend(render_assistant_message("Alchemy:", &msg.content, t));
        }
        lines.push(Line::from(""));
    }

    if let Some(ref content) = app.streaming_content {
        lines.extend(render_assistant_message("Alchemy:", &format!("{}▋", content), t));
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
        Style::default().fg(t.focused_border)
    } else {
        Style::default().fg(t.normal_border)
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
    let t = app.theme();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Tool execution panel.
    let spinner = crate::tui::widgets::spinner_frame(app.tick);
    let tool_lines: Vec<Line> = app.tools_log.iter().map(|entry| {
        if entry.status == "⏳" {
            Line::from(Span::styled(
                format!("{} {}", spinner, entry.name),
                Style::default().fg(t.tool_spinner),
            ))
        } else {
            let status_color = if entry.success { t.tool_success } else { t.tool_error };
            Line::from(vec![
                Span::styled(entry.status.clone(), Style::default().fg(status_color)),
                Span::raw(format!(" {} ({}ms)", entry.name, entry.duration_ms)),
            ])
        }
    }).collect();
    let tools_border = if app.focused_panel == 1 {
        Style::default().fg(t.focused_border)
    } else {
        Style::default().fg(t.normal_border)
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

    // File activity panel — fade newly-added rows.
    const FADE_TICKS: usize = 20;
    let file_lines: Vec<Line> = app.files_log.iter().map(|fl| {
        let age = app.tick.saturating_sub(fl.added_tick);
        let style = if age < FADE_TICKS / 3 {
            Style::default().fg(t.file_new).add_modifier(Modifier::BOLD)
        } else if age < (2 * FADE_TICKS) / 3 {
            Style::default().fg(t.file_mid)
        } else if age < FADE_TICKS {
            Style::default().fg(t.file_old)
        } else {
            Style::default()
        };
        Line::from(Span::styled(format!("{} {}", fl.operation, fl.path), style))
    }).collect();
    let files_border = if app.focused_panel == 2 {
        Style::default().fg(t.focused_border)
    } else {
        Style::default().fg(t.normal_border)
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
    let t = app.theme();

    let (input_widget, title_style, border_color) = if app.agent_busy {
        let spinner = crate::tui::widgets::spinner_frame(app.tick);
        let content = format!("{} Working…  Ctrl+C to interrupt", spinner);
        let p = Paragraph::new(Span::styled(
            content,
            Style::default().fg(t.input_busy_fg).add_modifier(Modifier::BOLD),
        ));
        (p, Style::default().fg(t.input_busy_fg).add_modifier(Modifier::BOLD), t.input_busy_fg)
    } else {
        let cursor = app.input_cursor.min(app.input.len());
        let before = &app.input[..cursor];
        let (cursor_char, after) = if cursor < app.input.len() {
            let next = {
                let mut p = cursor + 1;
                while p < app.input.len() && !app.input.is_char_boundary(p) { p += 1; }
                p
            };
            (&app.input[cursor..next], &app.input[next..])
        } else {
            ("█", "")
        };

        let spans = vec![
            Span::styled(format!("> {}", before), Style::default().fg(t.input_fg)),
            Span::styled(
                cursor_char.to_string(),
                Style::default().bg(t.input_cursor_bg).fg(t.input_cursor_fg),
            ),
            Span::styled(after.to_string(), Style::default().fg(t.input_fg)),
        ];
        let p = Paragraph::new(Line::from(spans));
        (p, Style::default().fg(t.input_fg), t.input_fg)
    };

    let p = input_widget.block(
        Block::default()
            .borders(Borders::ALL)
            .title("Input")
            .title_style(title_style)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(p, area);
}

/// Returns a centered `Rect` sized `w × h` within `r`, clamped to fit.
fn centered_fixed(w: u16, h: u16, r: Rect) -> Rect {
    let popup_w = w.min(r.width);
    let popup_h = h.min(r.height);
    let x = r.x + (r.width.saturating_sub(popup_w)) / 2;
    let y = r.y + (r.height.saturating_sub(popup_h)) / 2;
    Rect::new(x, y, popup_w, popup_h)
}

fn draw_help_overlay(
    f: &mut Frame,
    area: Rect,
    t: &ThemePalette,
    scroll: usize,
    max_scroll_out: &mut u16,
) {
    // Fill the terminal leaving a 1-cell margin on each side.
    let popup_area = centered_fixed(
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
        area,
    );
    f.render_widget(Clear, popup_area);

    let header = Style::default().fg(t.help_header).add_modifier(Modifier::BOLD);
    let key    = Style::default().fg(t.help_key).add_modifier(Modifier::BOLD);
    let desc   = Style::default().fg(t.help_desc);
    let sep    = Style::default().fg(t.help_sep);

    let inner_w = popup_area.width.saturating_sub(4) as usize;
    let divider = Line::from(Span::styled("─".repeat(inner_w), sep));

    macro_rules! kline {
        ($k:expr, $d:expr) => {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{:<20}", $k), key),
                Span::styled($d, desc),
            ])
        };
    }
    macro_rules! section {
        ($title:expr) => {
            Line::from(Span::styled(format!("  {}", $title), header))
        };
    }

    let lines: Vec<Line> = vec![
        Line::from(""),
        section!("Navigation"),
        divider.clone(),
        kline!("Tab",              "Cycle panel focus (conv → tools → files)"),
        kline!("Ctrl+↑ / Ctrl+↓", "Scroll focused panel (5 lines)"),
        Line::from(""),
        section!("Prompt History"),
        divider.clone(),
        kline!("↑ / ↓",           "Browse previously sent prompts"),
        Line::from(""),
        section!("Cursor Editing"),
        divider.clone(),
        kline!("← / →",           "Move cursor left / right"),
        kline!("Home / End",       "Jump to start / end of input"),
        kline!("Backspace",        "Delete character before cursor"),
        kline!("Delete",           "Delete character at cursor"),
        Line::from(""),
        section!("Messaging"),
        divider.clone(),
        kline!("Enter",            "Send message"),
        kline!("Esc",              "Clear input / exit history"),
        Line::from(""),
        section!("Appearance"),
        divider.clone(),
        kline!("Ctrl+T",           "Cycle theme (Dark→Light→Dracula→Solarized)"),
        Line::from(""),
        section!("Session"),
        divider.clone(),
        kline!("Ctrl+S",           "Save session to disk"),
        kline!("Ctrl+L",           "Clear conversation display"),
        Line::from(""),
        section!("Agent / Exit"),
        divider.clone(),
        kline!("Ctrl+C (busy)",    "Interrupt running agent"),
        kline!("Ctrl+C (idle)",    "Exit"),
        kline!("Ctrl+D",           "Exit"),
        Line::from(""),
        section!("This Overlay"),
        divider.clone(),
        kline!("↑ ↓ PgUp PgDn",   "Scroll"),
        kline!("Home / End",       "Jump to top / bottom"),
        kline!("? / Esc / q",      "Close overlay"),
        Line::from(""),
    ];

    // Compute max scroll: total lines minus visible inner height.
    let inner_h = popup_area.height.saturating_sub(2); // subtract border rows
    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(inner_h);
    *max_scroll_out = max_scroll;
    let clamped_scroll = (scroll as u16).min(max_scroll);

    // Scroll indicator shown in title when content overflows.
    let title = if max_scroll > 0 {
        let pct = if max_scroll == 0 { 100 } else {
            ((clamped_scroll as u32 * 100) / max_scroll as u32).min(100)
        };
        format!("  ⌨  Key Bindings  [{:3}%]  ", pct)
    } else {
        "  ⌨  Key Bindings  ".to_string()
    };

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(header)
                .border_style(Style::default().fg(t.help_border)),
        )
        .scroll((clamped_scroll, 0));

    f.render_widget(p, popup_area);
}

