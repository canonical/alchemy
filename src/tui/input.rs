/// Move cursor one Unicode scalar value to the left within `s`.
pub fn cursor_left(s: &str, pos: usize) -> usize {
    if pos == 0 { return 0; }
    let mut p = pos - 1;
    while p > 0 && !s.is_char_boundary(p) { p -= 1; }
    p
}

/// Move cursor one Unicode scalar value to the right within `s`.
pub fn cursor_right(s: &str, pos: usize) -> usize {
    if pos >= s.len() { return s.len(); }
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) { p += 1; }
    p
}

/// Visual row (0-based) and column of a byte `pos` inside a (possibly
/// multi-line) string, measured in Unicode display columns.
pub fn cursor_visual_pos(s: &str, pos: usize) -> (u16, u16) {
    use unicode_width::UnicodeWidthStr;
    let before = &s[..pos.min(s.len())];
    let lines: Vec<&str> = before.split('\n').collect();
    let row = (lines.len().saturating_sub(1)) as u16;
    let col = lines.last().map(|l| l.width() as u16).unwrap_or(0);
    (row, col)
}