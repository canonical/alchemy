use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render a markdown string into a list of styled ratatui Lines.
/// Used for assistant messages in the TUI conversation panel.
pub fn render(input: &str) -> Vec<Line<'static>> {
    let parser = Parser::new(input);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default().fg(Color::Green);

    let mut in_code_block = false;
    let mut list_depth: usize = 0;
    let style_stack: &mut Vec<Style> = &mut Vec::new();

    let flush = |cur: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>| {
        lines.push(Line::from(std::mem::take(cur)));
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush(&mut current, &mut lines);
                    style_stack.push(style);
                    style = heading_style(level);
                }
                Tag::Strong => {
                    style_stack.push(style);
                    style = style.add_modifier(Modifier::BOLD);
                }
                Tag::Emphasis => {
                    style_stack.push(style);
                    style = style.add_modifier(Modifier::ITALIC);
                }
                Tag::BlockQuote(_) => {
                    flush(&mut current, &mut lines);
                    current.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                    style_stack.push(style);
                    style = style.add_modifier(Modifier::ITALIC).fg(Color::Gray);
                }
                Tag::CodeBlock(kind) => {
                    flush(&mut current, &mut lines);
                    in_code_block = true;
                    let lang = if let CodeBlockKind::Fenced(s) = &kind {
                        s.as_ref()
                    } else {
                        ""
                    };
                    if !lang.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("┌─ {} ─", lang),
                            Style::default().fg(Color::DarkGray),
                        )));
                    } else {
                        lines.push(Line::from(Span::styled(
                            "┌─",
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    style_stack.push(style);
                    style = Style::default().fg(Color::Cyan);
                }
                Tag::List(_) => {
                    list_depth += 1;
                    flush(&mut current, &mut lines);
                }
                Tag::Item => {
                    flush(&mut current, &mut lines);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    current.push(Span::raw(format!("{}• ", indent)));
                }
                Tag::Paragraph => {
                    flush(&mut current, &mut lines);
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Heading(_) | TagEnd::Strong | TagEnd::Emphasis | TagEnd::BlockQuote(_) => {
                    if let Some(prev) = style_stack.pop() {
                        style = prev;
                    }
                    if matches!(end, TagEnd::Heading(_)) {
                        flush(&mut current, &mut lines);
                    }
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    flush(&mut current, &mut lines);
                    lines.push(Line::from(Span::styled(
                        "└─",
                        Style::default().fg(Color::DarkGray),
                    )));
                    if let Some(prev) = style_stack.pop() {
                        style = prev;
                    }
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    flush(&mut current, &mut lines);
                }
                TagEnd::Item => {
                    flush(&mut current, &mut lines);
                }
                TagEnd::Paragraph => {
                    flush(&mut current, &mut lines);
                    lines.push(Line::from(""));
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    for src_line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(src_line.to_string(), style),
                        ]));
                    }
                } else {
                    current.push(Span::styled(text.into_string(), style));
                }
            }
            Event::Code(code) => {
                current.push(Span::styled(
                    format!("`{}`", code),
                    Style::default().fg(Color::Cyan).bg(Color::Black),
                ));
            }
            Event::SoftBreak => {
                current.push(Span::raw(" "));
            }
            Event::HardBreak => {
                flush(&mut current, &mut lines);
            }
            Event::Rule => {
                flush(&mut current, &mut lines);
                lines.push(Line::from(Span::styled(
                    "──────",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        lines.push(Line::from(current));
    }
    // Trim trailing empty lines.
    while lines.last().map(|l| l.spans.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

fn heading_style(level: HeadingLevel) -> Style {
    let base = Style::default().add_modifier(Modifier::BOLD);
    match level {
        HeadingLevel::H1 => base.fg(Color::Magenta).add_modifier(Modifier::UNDERLINED),
        HeadingLevel::H2 => base.fg(Color::Magenta),
        HeadingLevel::H3 => base.fg(Color::LightMagenta),
        _ => base.fg(Color::White),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_text(lines: &[Line]) -> String {
        lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_plain_text() {
        let out = render("hello world");
        let text = lines_to_text(&out);
        assert!(text.contains("hello world"), "{:?}", text);
    }

    #[test]
    fn renders_inline_code() {
        let out = render("use the `read_file` tool");
        let text = lines_to_text(&out);
        assert!(text.contains("`read_file`"), "{:?}", text);
    }

    #[test]
    fn renders_fenced_code_block() {
        let out = render("```rust\nfn main() {}\n```");
        let text = lines_to_text(&out);
        assert!(text.contains("rust"), "{:?}", text);
        assert!(text.contains("fn main() {}"), "{:?}", text);
    }

    #[test]
    fn renders_list_items() {
        let out = render("- first\n- second\n");
        let text = lines_to_text(&out);
        assert!(text.contains("• first"), "{:?}", text);
        assert!(text.contains("• second"), "{:?}", text);
    }
}
