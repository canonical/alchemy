use ratatui::style::Color;

/// All named colors used across the TUI, collected in one place so that
/// every `draw_*` function can receive a `&ThemePalette` instead of
/// hardcoding `Color::X` literals.
#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub name: &'static str,
    /// Background color for all panels and the input box.
    /// Use `Color::Reset` on dark themes to inherit the terminal background.
    pub panel_bg: Color,
    // Status bar
    pub status_bg: Color,
    pub status_fg: Color,
    // Conversation
    pub user_fg: Color,
    pub assistant_fg: Color,
    // Borders
    pub focused_border: Color,
    pub normal_border: Color,
    // Tool log
    pub tool_success: Color,
    pub tool_error: Color,
    pub tool_spinner: Color,
    // File log fade (newly added → fading → settled)
    pub file_new: Color,
    pub file_mid: Color,
    pub file_old: Color,
    // Input box
    pub input_fg: Color,
    pub input_cursor_bg: Color,
    pub input_cursor_fg: Color,
    pub input_busy_fg: Color,
    // Help overlay
    pub help_border: Color,
    pub help_header: Color,
    pub help_key: Color,
    pub help_desc: Color,
    pub help_sep: Color,
    // Markdown body
    pub md_text: Color,
    pub md_code_fg: Color,
    pub md_code_bg: Color,
    pub md_quote: Color,
    pub md_heading_h1: Color,
    pub md_heading_h2: Color,
    pub md_heading_h3: Color,
    pub md_separator: Color,
    pub md_list: Color,
}

// ── Built-in themes ───────────────────────────────────────────────────────────

const DARK: ThemePalette = ThemePalette {
    name: "dark",
    panel_bg: Color::Reset,
    status_bg: Color::Blue,
    status_fg: Color::White,
    user_fg: Color::Cyan,
    assistant_fg: Color::Green,
    focused_border: Color::Yellow,
    normal_border: Color::Reset,
    tool_success: Color::Green,
    tool_error: Color::Red,
    tool_spinner: Color::Yellow,
    file_new: Color::Yellow,
    file_mid: Color::LightYellow,
    file_old: Color::Green,
    input_fg: Color::White,
    input_cursor_bg: Color::White,
    input_cursor_fg: Color::Black,
    input_busy_fg: Color::Yellow,
    help_border: Color::Yellow,
    help_header: Color::Yellow,
    help_key: Color::Cyan,
    help_desc: Color::White,
    help_sep: Color::DarkGray,
    md_text: Color::Reset,
    md_code_fg: Color::Cyan,
    md_code_bg: Color::Black,
    md_quote: Color::DarkGray,
    md_heading_h1: Color::Magenta,
    md_heading_h2: Color::Magenta,
    md_heading_h3: Color::LightMagenta,
    md_separator: Color::DarkGray,
    md_list: Color::DarkGray,
};

const LIGHT: ThemePalette = ThemePalette {
    name: "light",
    panel_bg: Color::Rgb(245, 245, 245),   // near-white fill for all panels
    status_bg: Color::Rgb(70, 130, 180),   // steel-blue
    status_fg: Color::White,
    user_fg: Color::Rgb(0, 80, 160),       // dark blue
    assistant_fg: Color::Rgb(0, 110, 50),  // dark green
    focused_border: Color::Rgb(200, 120, 0), // amber
    normal_border: Color::Rgb(150, 150, 150),
    tool_success: Color::Rgb(0, 130, 50),
    tool_error: Color::Rgb(200, 0, 0),
    tool_spinner: Color::Rgb(180, 100, 0),
    file_new: Color::Rgb(180, 100, 0),
    file_mid: Color::Rgb(100, 100, 0),
    file_old: Color::Rgb(0, 130, 50),
    input_fg: Color::Rgb(20, 20, 20),
    input_cursor_bg: Color::Rgb(20, 20, 20),
    input_cursor_fg: Color::Rgb(245, 245, 245),
    input_busy_fg: Color::Rgb(180, 100, 0),
    help_border: Color::Rgb(200, 120, 0),
    help_header: Color::Rgb(160, 80, 0),
    help_key: Color::Rgb(0, 80, 160),
    help_desc: Color::Rgb(30, 30, 30),
    help_sep: Color::Rgb(140, 140, 140),
    md_text: Color::Rgb(20, 20, 20),
    md_code_fg: Color::Rgb(0, 80, 160),
    md_code_bg: Color::Rgb(220, 220, 220),
    md_quote: Color::Rgb(100, 100, 100),
    md_heading_h1: Color::Rgb(130, 0, 130),
    md_heading_h2: Color::Rgb(130, 0, 130),
    md_heading_h3: Color::Rgb(150, 30, 150),
    md_separator: Color::Rgb(150, 150, 150),
    md_list: Color::Rgb(100, 100, 100),
};

// Dracula: https://draculatheme.com/contribute#color-palette
const DRACULA: ThemePalette = ThemePalette {
    name: "dracula",
    panel_bg: Color::Rgb(40, 42, 54),      // Background
    status_bg: Color::Rgb(68, 71, 90),    // Current Line
    status_fg: Color::Rgb(248, 248, 242), // Foreground
    user_fg: Color::Rgb(139, 233, 253),   // Cyan
    assistant_fg: Color::Rgb(80, 250, 123), // Green
    focused_border: Color::Rgb(255, 121, 198), // Pink
    normal_border: Color::Rgb(98, 114, 164),   // Comment
    tool_success: Color::Rgb(80, 250, 123),
    tool_error: Color::Rgb(255, 85, 85),   // Red
    tool_spinner: Color::Rgb(255, 184, 108), // Orange
    file_new: Color::Rgb(255, 184, 108),
    file_mid: Color::Rgb(241, 250, 140),   // Yellow
    file_old: Color::Rgb(80, 250, 123),
    input_fg: Color::Rgb(248, 248, 242),
    input_cursor_bg: Color::Rgb(248, 248, 242),
    input_cursor_fg: Color::Rgb(40, 42, 54), // Background
    input_busy_fg: Color::Rgb(255, 184, 108),
    help_border: Color::Rgb(255, 121, 198),
    help_header: Color::Rgb(255, 121, 198),
    help_key: Color::Rgb(139, 233, 253),
    help_desc: Color::Rgb(248, 248, 242),
    help_sep: Color::Rgb(98, 114, 164),
    md_text: Color::Rgb(248, 248, 242),
    md_code_fg: Color::Rgb(139, 233, 253),
    md_code_bg: Color::Rgb(40, 42, 54),
    md_quote: Color::Rgb(98, 114, 164),
    md_heading_h1: Color::Rgb(189, 147, 249), // Purple
    md_heading_h2: Color::Rgb(189, 147, 249),
    md_heading_h3: Color::Rgb(255, 121, 198),
    md_separator: Color::Rgb(98, 114, 164),
    md_list: Color::Rgb(98, 114, 164),
};

// Solarized Dark: https://ethanschoonover.com/solarized/
const SOLARIZED: ThemePalette = ThemePalette {
    name: "solarized",
    panel_bg: Color::Rgb(0, 43, 54),       // base03
    status_bg: Color::Rgb(0, 43, 54),     // base03
    status_fg: Color::Rgb(131, 148, 150), // base0
    user_fg: Color::Rgb(38, 139, 210),    // blue
    assistant_fg: Color::Rgb(133, 153, 0), // green
    focused_border: Color::Rgb(181, 137, 0), // yellow
    normal_border: Color::Rgb(88, 110, 117),  // base01
    tool_success: Color::Rgb(133, 153, 0),
    tool_error: Color::Rgb(220, 50, 47),   // red
    tool_spinner: Color::Rgb(181, 137, 0),
    file_new: Color::Rgb(203, 75, 22),     // orange
    file_mid: Color::Rgb(181, 137, 0),
    file_old: Color::Rgb(133, 153, 0),
    input_fg: Color::Rgb(131, 148, 150),
    input_cursor_bg: Color::Rgb(131, 148, 150),
    input_cursor_fg: Color::Rgb(0, 43, 54),
    input_busy_fg: Color::Rgb(181, 137, 0),
    help_border: Color::Rgb(181, 137, 0),
    help_header: Color::Rgb(181, 137, 0),
    help_key: Color::Rgb(38, 139, 210),
    help_desc: Color::Rgb(131, 148, 150),
    help_sep: Color::Rgb(88, 110, 117),
    md_text: Color::Rgb(131, 148, 150),
    md_code_fg: Color::Rgb(42, 161, 152),  // cyan
    md_code_bg: Color::Rgb(7, 54, 66),     // base02
    md_quote: Color::Rgb(88, 110, 117),
    md_heading_h1: Color::Rgb(211, 54, 130), // magenta
    md_heading_h2: Color::Rgb(211, 54, 130),
    md_heading_h3: Color::Rgb(108, 113, 196), // violet
    md_separator: Color::Rgb(88, 110, 117),
    md_list: Color::Rgb(88, 110, 117),
};

pub const THEMES: [ThemePalette; 4] = [DARK, LIGHT, DRACULA, SOLARIZED];

// ── Persistence ───────────────────────────────────────────────────────────────

fn theme_file_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            std::path::PathBuf::from(home).join(".config")
        });
    base.join("alchemy").join("theme")
}

/// Load the persisted theme index. Returns `0` (Dark) on any error.
pub fn load_theme() -> usize {
    let path = theme_file_path();
    let Ok(content) = std::fs::read_to_string(&path) else { return 0 };
    let name = content.trim();
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

/// Persist the theme name for `idx` to `~/.config/alchemy/theme`.
pub fn save_theme(idx: usize) {
    let path = theme_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, THEMES[idx].name);
}

fn panels_file_path() -> std::path::PathBuf {
    theme_file_path().with_file_name("panels")
}

/// Load persisted panel visibility. Returns `(show_tools, show_files)`,
/// defaulting to `(true, true)` on any error.
pub fn load_panels() -> (bool, bool) {
    let Ok(content) = std::fs::read_to_string(panels_file_path()) else {
        return (true, true);
    };
    let mut show_tools = true;
    let mut show_files = true;
    for line in content.lines() {
        match line.trim() {
            "tools=false" => show_tools = false,
            "tools=true"  => show_tools = true,
            "files=false" => show_files = false,
            "files=true"  => show_files = true,
            _ => {}
        }
    }
    (show_tools, show_files)
}

/// Persist panel visibility to `~/.config/alchemy/panels`.
pub fn save_panels(show_tools: bool, show_files: bool) {
    let path = panels_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = format!("tools={}\nfiles={}\n", show_tools, show_files);
    let _ = std::fs::write(&path, content);
}
