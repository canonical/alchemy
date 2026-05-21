use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use crate::tui::TuiApp;
use crate::tui::theme::ThemePalette;

pub fn draw(f: &mut Frame, app: &mut TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),   // Status bar
            Constraint::Min(5),      // Main area
            Constraint::Length(1),   // Input prompt
        ])
        .split(f.area());

    draw_status_bar(f, app, chunks[0]);
    draw_main_area(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);

    // Overlays drawn last so they float above everything.
    if app.show_skills {
        let t = app.theme();
        let scroll = app.skills_scroll;
        let mut max_scroll = app.skills_max_scroll;
        draw_skills_overlay(f, f.area(), &app.skills_info.clone(), t, scroll, &mut max_scroll);
        app.skills_max_scroll = max_scroll;
    }
    if app.show_mcp {
        let t = app.theme();
        let scroll = app.mcp_scroll;
        let mut max_scroll = app.mcp_max_scroll;
        draw_mcp_overlay(f, f.area(), &app.mcp_info.clone(), t, scroll, &mut max_scroll);
        app.mcp_max_scroll = max_scroll;
    }
    if app.show_help {
        draw_help_overlay(f, f.area(), app.theme(), app.help_scroll, &mut app.help_max_scroll);
    }
}

fn draw_status_bar(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();

    // Panel indicators: bright when visible, dim when hidden.
    let tools_ind = if app.show_tools { "[T]" } else { "[-]" };
    let files_ind = if app.show_files { "[F]" } else { "[-]" };

    let left = format!(
        " Alchemy  │ {}  │ {}  │ ⏱ {} steps  │ 📊 {}k tokens  │ 🎨 {}  │ {} {}",
        app.session_name,
        app.model_name,
        app.steps,
        app.total_tokens / 1000,
        t.name,
        tools_ind,
        files_ind,
    );
    let right = "? help ";

    let pad = (area.width as usize).saturating_sub(left.width() + right.width());
    let full = format!("{}{}{}", left, " ".repeat(pad), right);

    let p = Paragraph::new(Line::from(vec![
        Span::styled(full, Style::default().bg(t.status_bg).fg(t.status_fg)),
    ]));
    f.render_widget(p, area);
}

fn draw_main_area(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let show_any_side = app.show_tools || app.show_files;
    if !show_any_side {
        draw_conversation(f, app, area);
        return;
    }
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
        if full.width() <= inner_width || inner_width == 0 {
            lines.push(Line::from(Span::styled(full, Style::default().fg(color))));
        } else {
            // Wrap by display columns so wide chars (emoji) don't overflow.
            let mut current = String::new();
            let mut current_w = 0usize;
            for ch in full.chars() {
                let ch_w = ch.width().unwrap_or(1);
                if current_w + ch_w > inner_width && current_w > 0 {
                    lines.push(Line::from(Span::styled(
                        current.clone(),
                        Style::default().fg(color),
                    )));
                    current.clear();
                    current_w = 0;
                }
                current.push(ch);
                current_w += ch_w;
            }
            if !current.is_empty() {
                lines.push(Line::from(Span::styled(current, Style::default().fg(color))));
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
                .border_style(border_style)
                .style(Style::default().bg(t.panel_bg)),
        )
        .style(Style::default().bg(t.panel_bg))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

fn draw_side_panels(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    match (app.show_tools, app.show_files) {
        (true, true) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            draw_tools_panel(f, app, chunks[0]);
            draw_files_panel(f, app, chunks[1]);
        }
        (true, false) => draw_tools_panel(f, app, area),
        (false, true) => draw_files_panel(f, app, area),
        (false, false) => {}
    }
}

fn draw_tools_panel(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();
    let spinner = crate::tui::widgets::spinner_frame(app.tick);
    let current_turn = app.turn_count;
    let mut tool_lines: Vec<Line> = Vec::new();
    let mut last_turn = u32::MAX;

    for entry in &app.tools_log {
        // Emit a turn-separator whenever the turn changes.
        if entry.turn != last_turn {
            last_turn = entry.turn;
            let is_current = entry.turn == current_turn;
            let sep_style = if is_current {
                Style::default().fg(t.focused_border).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.normal_border)
            };
            tool_lines.push(Line::from(Span::styled(
                format!("#{} {}", entry.turn, entry.time),
                sep_style,
            )));
        }
        let is_current = entry.turn == current_turn;
        if entry.status == "⏳" {
            tool_lines.push(Line::from(Span::styled(
                format!("  {} {}", spinner, entry.name),
                Style::default().fg(t.tool_spinner),
            )));
        } else {
            let status_color = if entry.success { t.tool_success } else { t.tool_error };
            let name_style = if is_current {
                Style::default()
            } else {
                Style::default().fg(t.normal_border)
            };
            tool_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(entry.status.clone(), Style::default().fg(status_color)),
                Span::styled(
                    format!(" {} ({}ms)", entry.name, entry.duration_ms),
                    name_style,
                ),
            ]));
        }
    }

    let border = if app.focused_panel == 1 {
        Style::default().fg(t.focused_border)
    } else {
        Style::default().fg(t.normal_border)
    };
    let max = (tool_lines.len() as u16).saturating_sub(area.height.saturating_sub(2));
    let tools = Paragraph::new(tool_lines)
        .block(Block::default().borders(Borders::ALL).title("🔧 Tools")
            .border_style(border).style(Style::default().bg(t.panel_bg)))
        .style(Style::default().bg(t.panel_bg))
        .scroll(((app.tools_scroll as u16).min(max), 0));
    f.render_widget(tools, area);
}

fn draw_files_panel(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();
    const FADE_TICKS: usize = 20;
    let current_turn = app.turn_count;
    let mut file_lines: Vec<Line> = Vec::new();
    let mut last_turn = u32::MAX;

    for fl in &app.files_log {
        // Emit a turn-separator whenever the turn changes.
        if fl.turn != last_turn {
            last_turn = fl.turn;
            let is_current = fl.turn == current_turn;
            let sep_style = if is_current {
                Style::default().fg(t.focused_border).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.normal_border)
            };
            file_lines.push(Line::from(Span::styled(
                format!("#{} {}", fl.turn, fl.time),
                sep_style,
            )));
        }
        let is_current = fl.turn == current_turn;
        let style = if is_current {
            let age = app.tick.saturating_sub(fl.added_tick);
            if age < FADE_TICKS / 3 {
                Style::default().fg(t.file_new).add_modifier(Modifier::BOLD)
            } else if age < (2 * FADE_TICKS) / 3 {
                Style::default().fg(t.file_mid)
            } else {
                Style::default().fg(t.file_old)
            }
        } else {
            Style::default().fg(t.normal_border)
        };
        file_lines.push(Line::from(Span::styled(
            format!("  {} {}", fl.operation, fl.path),
            style,
        )));
    }

    let border = if app.focused_panel == 2 {
        Style::default().fg(t.focused_border)
    } else {
        Style::default().fg(t.normal_border)
    };
    let max = (file_lines.len() as u16).saturating_sub(area.height.saturating_sub(2));
    let files = Paragraph::new(file_lines)
        .block(Block::default().borders(Borders::ALL).title("📁 Files")
            .border_style(border).style(Style::default().bg(t.panel_bg)))
        .style(Style::default().bg(t.panel_bg))
        .scroll(((app.files_scroll as u16).min(max), 0));
    f.render_widget(files, area);
}

fn draw_input(f: &mut Frame, app: &mut TuiApp, area: Rect) {
    let t = app.theme();

    let line = if app.agent_busy {
        let spinner = crate::tui::widgets::spinner_frame(app.tick);
        Line::from(vec![
            Span::styled(
                format!("{} Working…  Ctrl+C to interrupt", spinner),
                Style::default().fg(t.input_busy_fg).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        let cursor = app.input_cursor.min(app.input.len());
        let before = &app.input[..cursor];

        // Place the terminal cursor: "> "(2) + display-width of text before cursor.
        let before_cols: u16 = before.width() as u16;
        let cx = area.x + 2 + before_cols;
        let cy = area.y;
        if cx < area.x + area.width {
            f.set_cursor_position((cx, cy));
        }

        Line::from(vec![
            Span::styled("> ", Style::default().fg(t.input_fg).add_modifier(Modifier::BOLD)),
            Span::styled(app.input.clone(), Style::default().fg(t.input_fg)),
        ])
    };

    let p = Paragraph::new(line)
        .style(Style::default().bg(t.panel_bg));
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

    // Each column is roughly half the inner width; dividers fill their column.
    let col_w = popup_area.width.saturating_sub(4) as usize / 2;
    let div = || Line::from(Span::styled("─".repeat(col_w), sep));

    macro_rules! kline {
        ($k:expr, $d:expr) => {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{:<16}", $k), key),
                Span::styled($d, desc),
            ])
        };
    }
    macro_rules! section {
        ($title:expr) => {
            Line::from(Span::styled(format!("  {}", $title), header))
        };
    }

    // ── Left column ──────────────────────────────────────────────────────────
    let left_lines: Vec<Line> = vec![
        Line::from(""),
        section!("Navigation"),
        div(),
        kline!("Tab",            "Cycle focus: conv / tools / files"),
        kline!("PgUp / PgDn",   "Scroll conversation ±10 lines"),
        kline!("Alt+↑ / Alt+↓", "Scroll conversation ±10 lines"),
        kline!("Ctrl+↑ / ↓",    "Scroll focused panel ±5 lines"),
        Line::from(""),
        section!("Panel Visibility"),
        div(),
        kline!("Alt+T",         "Toggle Tools panel"),
        kline!("Alt+F",         "Toggle Files panel"),
        kline!("Alt+S",         "Skills info overlay"),
        kline!("Alt+M",         "MCP info overlay"),
        Line::from(""),
        section!("Prompt History"),
        div(),
        kline!("↑ / ↓",        "Browse previously sent prompts"),
        Line::from(""),
        section!("Cursor Editing"),
        div(),
        kline!("← / →",        "Move cursor left / right"),
        kline!("Home / End",    "Jump to start / end of line"),
        kline!("Backspace",     "Delete char before cursor"),
        kline!("Delete",        "Delete char at cursor"),
        Line::from(""),
    ];

    // ── Right column ─────────────────────────────────────────────────────────
    let right_lines: Vec<Line> = vec![
        Line::from(""),
        section!("Messaging"),
        div(),
        kline!("Enter",         "Send message"),
        kline!("Esc",           "Clear input / exit history"),
        Line::from(""),
        section!("Appearance"),
        div(),
        kline!("Alt+C",         "Cycle theme (Dark/Light/Dracula/Solarized)"),
        Line::from(""),
        section!("Session"),
        div(),
        kline!("Ctrl+S",        "Save session to disk"),
        kline!("Ctrl+L",        "Clear conversation"),
        Line::from(""),
        section!("Agent / Exit"),
        div(),
        kline!("Ctrl+C (busy)", "Interrupt running agent"),
        kline!("Ctrl+C (idle)", "Exit"),
        kline!("Ctrl+D",        "Exit"),
        Line::from(""),
        section!("This Overlay"),
        div(),
        kline!("↑↓ PgUp PgDn", "Scroll"),
        kline!("Home / End",    "Jump to top / bottom"),
        kline!("? / Esc / q",  "Close"),
        Line::from(""),
    ];

    // Compute max scroll: tallest column minus visible inner height.
    let inner_h = popup_area.height.saturating_sub(2);
    let total = left_lines.len().max(right_lines.len()) as u16;
    let max_scroll = total.saturating_sub(inner_h);
    *max_scroll_out = max_scroll;
    let clamped_scroll = (scroll as u16).min(max_scroll);

    let title = if max_scroll > 0 {
        let pct = ((clamped_scroll as u32 * 100) / max_scroll as u32).min(100);
        format!("  ⌨  Key Bindings  [{:3}%]  ", pct)
    } else {
        "  ⌨  Key Bindings  ".to_string()
    };

    // Render outer border block first; get its inner area for column splitting.
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(header)
        .border_style(Style::default().fg(t.help_border))
        .style(Style::default().bg(t.panel_bg));
    let inner_area = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Split inner area into two equal columns.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner_area);

    f.render_widget(
        Paragraph::new(left_lines)
            .style(Style::default().bg(t.panel_bg))
            .scroll((clamped_scroll, 0)),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(right_lines)
            .style(Style::default().bg(t.panel_bg))
            .scroll((clamped_scroll, 0)),
        cols[1],
    );
}

fn draw_info_overlay(
    f: &mut Frame,
    area: Rect,
    t: &ThemePalette,
    title: &str,
    lines: Vec<Line<'static>>,
    scroll: usize,
    max_scroll_out: &mut u16,
) {
    let popup_area = centered_fixed(
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
        area,
    );
    f.render_widget(Clear, popup_area);

    let inner_h = popup_area.height.saturating_sub(2);
    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(inner_h);
    *max_scroll_out = max_scroll;
    let clamped_scroll = (scroll as u16).min(max_scroll);

    let header_style = Style::default().fg(t.help_header).add_modifier(Modifier::BOLD);
    let display_title = if max_scroll > 0 {
        let pct = ((clamped_scroll as u32 * 100) / max_scroll as u32).min(100);
        format!("  {}  [{:3}%]  ", title, pct)
    } else {
        format!("  {}  ", title)
    };

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(display_title)
                .title_style(header_style)
                .border_style(Style::default().fg(t.help_border))
                .style(Style::default().bg(t.panel_bg)),
        )
        .style(Style::default().bg(t.panel_bg))
        .scroll((clamped_scroll, 0));
    f.render_widget(p, popup_area);
}

/// Word-wrap `text` to fit within `max_width` columns, indented by `indent` spaces.
/// Returns one `Line` per wrapped row, all styled with `style`.
fn wrap_desc(text: &str, indent: usize, max_width: usize, style: Style) -> Vec<Line<'static>> {
    let prefix = " ".repeat(indent);
    let usable = max_width.saturating_sub(indent).max(10);
    let mut result = Vec::new();
    let mut current = prefix.clone();
    let mut current_len = 0usize;

    for word in text.split_whitespace() {
        let word_w = word.width();
        if current_len == 0 {
            current.push_str(word);
            current_len = word_w;
        } else if current_len + 1 + word_w <= usable {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + word_w;
        } else {
            result.push(Line::from(Span::styled(current.clone(), style)));
            current = format!("{}{}", prefix, word);
            current_len = word_w;
        }
    }
    if current_len > 0 || result.is_empty() {
        result.push(Line::from(Span::styled(current, style)));
    }
    result
}

fn draw_skills_overlay(
    f: &mut Frame,
    area: Rect,
    skills: &[crate::tui::SkillEntry],
    t: &ThemePalette,
    scroll: usize,
    max_scroll_out: &mut u16,
) {
    let header = Style::default().fg(t.help_header).add_modifier(Modifier::BOLD);
    let key    = Style::default().fg(t.help_key).add_modifier(Modifier::BOLD);
    let desc   = Style::default().fg(t.help_desc);
    let sep    = Style::default().fg(t.help_sep);
    let inner_w = area.width.saturating_sub(6) as usize;
    let divider = || Line::from(Span::styled("─".repeat(inner_w), sep));

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];

    if skills.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No skills loaded.",
            Style::default().fg(t.help_desc),
        )));
    } else {
        for skill in skills {
            lines.push(Line::from(Span::styled(
                format!("  📦 {}", skill.name),
                header,
            )));
            lines.push(divider());
            if !skill.description.is_empty() {
                lines.extend(wrap_desc(&skill.description, 4, inner_w, desc));
            }
            for script in &skill.scripts {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("script: ".to_string(), key),
                    Span::styled(script.clone(), desc),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(Span::styled(
        "  Esc / q / Alt+S  close",
        Style::default().fg(t.help_sep),
    )));
    lines.push(Line::from(""));

    draw_info_overlay(f, area, t, "📦 Skills", lines, scroll, max_scroll_out);
}

fn draw_mcp_overlay(
    f: &mut Frame,
    area: Rect,
    mcps: &[crate::tui::McpEntry],
    t: &ThemePalette,
    scroll: usize,
    max_scroll_out: &mut u16,
) {
    let header = Style::default().fg(t.help_header).add_modifier(Modifier::BOLD);
    let key    = Style::default().fg(t.help_key).add_modifier(Modifier::BOLD);
    let desc   = Style::default().fg(t.help_desc);
    let muted  = Style::default().fg(t.help_sep);
    let sep    = Style::default().fg(t.help_sep);
    let inner_w = area.width.saturating_sub(6) as usize;
    let divider = || Line::from(Span::styled("─".repeat(inner_w), sep));

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];

    if mcps.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No MCP servers connected.",
            Style::default().fg(t.help_desc),
        )));
    } else {
        for entry in mcps {
            // Server heading: "🔌 servername  [stdio]  cmd"
            let transport_tag = if entry.transport.is_empty() {
                String::new()
            } else {
                format!("  [{}]", entry.transport)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  🔌 {}", entry.server), header),
                Span::styled(transport_tag, muted),
            ]));
            if !entry.endpoint.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    {}", entry.endpoint),
                    muted,
                )));
            }
            lines.push(divider());

            if entry.tools.is_empty() {
                lines.push(Line::from(Span::styled("    (no tools discovered)", muted)));
            } else {
                for tool in &entry.tools {
                    // Tool name row
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(tool.name.clone(), key),
                    ]));
                    // Description (word-wrapped, indented 6)
                    if !tool.description.is_empty() {
                        lines.extend(wrap_desc(&tool.description, 6, inner_w, desc));
                    }
                }
            }
            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(Span::styled(
        "  Esc / q / Alt+M  close",
        Style::default().fg(t.help_sep),
    )));
    lines.push(Line::from(""));

    draw_info_overlay(f, area, t, "🔌 MCP Servers", lines, scroll, max_scroll_out);
}
