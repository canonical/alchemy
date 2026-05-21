pub mod history;
pub mod input;
pub mod layout;
pub mod markdown;
pub mod theme;
pub mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::Arc;
use crate::agent::{Agent, AgentConfig, FileEvent, StepEvent, ToolEvent};
use crate::providers::Provider;
use crate::tools::ToolRegistry;
use crate::types::{AgentResult, Message};

// ── Cursor helpers ────────────────────────────────────────────────────────────

/// Move cursor one Unicode scalar value to the left.
fn cursor_left(s: &str, pos: usize) -> usize {
    if pos == 0 { return 0; }
    let mut p = pos - 1;
    while p > 0 && !s.is_char_boundary(p) { p -= 1; }
    p
}

/// Move cursor one Unicode scalar value to the right.
fn cursor_right(s: &str, pos: usize) -> usize {
    if pos >= s.len() { return s.len(); }
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) { p += 1; }
    p
}

/// A loaded skill shown in the Ctrl+Shift+S overlay.
#[derive(Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub scripts: Vec<String>,
}

/// A group of tools from one MCP server, shown in the Ctrl+Shift+M overlay.
#[derive(Clone)]
pub struct McpEntry {
    pub server: String,
    pub tools: Vec<String>,
}

pub struct TuiApp {
    pub session_name: String,
    pub session_dir: String,
    pub messages: Vec<TuiMessage>,
    pub input: String,
    /// Byte offset of the cursor within `input`.
    pub input_cursor: usize,
    pub running: bool,
    pub agent_busy: bool,
    pub tools_log: Vec<ToolLogEntry>,
    pub files_log: Vec<FileLogEntry>,
    pub model_name: String,
    pub total_tokens: u64,
    pub steps: u32,
    conversation_history: Vec<Message>,
    pending: Option<tokio::sync::oneshot::Receiver<(AgentResult, Vec<Message>)>>,
    tool_rx: Option<tokio::sync::mpsc::Receiver<ToolEvent>>,
    file_rx: Option<tokio::sync::mpsc::Receiver<FileEvent>>,
    token_rx: Option<tokio::sync::mpsc::Receiver<String>>,
    step_rx: Option<tokio::sync::mpsc::Receiver<StepEvent>>,
    streaming_content: Option<String>,
    pub focused_panel: usize,   // 0=conversation, 1=tools, 2=files
    pub conv_scroll: usize,
    pub tools_scroll: usize,
    pub files_scroll: usize,
    pub conv_follow: bool,
    pub conv_max_scroll: u16,
    pub tick: usize,
    turn_baseline_steps: u32,
    turn_baseline_tokens: u64,
    abort_handle: Option<tokio::task::AbortHandle>,
    /// Panel visibility toggles (Ctrl+Shift+T / Ctrl+Shift+F).
    pub show_tools: bool,
    pub show_files: bool,
    /// Help overlay state.
    pub show_help: bool,
    pub help_scroll: usize,
    pub help_max_scroll: u16,
    /// Skills info overlay state (Ctrl+Shift+S).
    pub show_skills: bool,
    pub skills_scroll: usize,
    pub skills_max_scroll: u16,
    pub skills_info: Vec<SkillEntry>,
    /// MCP info overlay state (Ctrl+Shift+M).
    pub show_mcp: bool,
    pub mcp_scroll: usize,
    pub mcp_max_scroll: u16,
    pub mcp_info: Vec<McpEntry>,
    /// Active color theme.
    pub theme_idx: usize,
    prompt_history: Vec<String>,
    history_idx: Option<usize>,
    history_draft: String,
}

const NUM_PANELS: usize = 3;

#[derive(Clone)]
pub struct TuiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone)]
pub struct ToolLogEntry {
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub success: bool,
}

#[derive(Clone)]
pub struct FileLogEntry {
    pub path: String,
    pub operation: char,
    /// Tick at which this entry was appended, used for the fade-in animation.
    pub added_tick: usize,
}

impl TuiApp {
    pub fn new(session_name: String, session_dir: String, model_name: String) -> Self {
        let (show_tools, show_files) = theme::load_panels();
        Self {
            session_name,
            session_dir,
            messages: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            running: true,
            agent_busy: false,
            tools_log: Vec::new(),
            files_log: Vec::new(),
            model_name,
            total_tokens: 0,
            steps: 0,
            conversation_history: Vec::new(),
            pending: None,
            tool_rx: None,
            file_rx: None,
            token_rx: None,
            step_rx: None,
            streaming_content: None,
            focused_panel: 0,
            conv_scroll: 0,
            tools_scroll: 0,
            files_scroll: 0,
            conv_follow: true,
            conv_max_scroll: 0,
            tick: 0,
            turn_baseline_steps: 0,
            turn_baseline_tokens: 0,
            abort_handle: None,
            show_tools,
            show_files,
            show_help: false,
            help_scroll: 0,
            help_max_scroll: 0,
            show_skills: false,
            skills_scroll: 0,
            skills_max_scroll: 0,
            skills_info: Vec::new(),
            show_mcp: false,
            mcp_scroll: 0,
            mcp_max_scroll: 0,
            mcp_info: Vec::new(),
            theme_idx: theme::load_theme(),
            prompt_history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
        }
    }

    pub fn set_skills_info(&mut self, entries: Vec<SkillEntry>) {
        self.skills_info = entries;
    }

    pub fn set_mcp_info(&mut self, entries: Vec<McpEntry>) {
        self.mcp_info = entries;
    }

    /// Returns the currently active color palette.
    pub fn theme(&self) -> &'static theme::ThemePalette {
        &theme::THEMES[self.theme_idx]
    }

    pub async fn run(
        &mut self,
        provider: Box<dyn Provider>,
        config: AgentConfig,
        registry: ToolRegistry,
    ) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Load session history (display only; LLM context starts fresh per session).
        let history_path = format!("{}/{}", self.session_dir, self.session_name);
        let _ = tokio::fs::create_dir_all(&history_path).await;
        if let Ok(msgs) =
            history::load_messages(&format!("{}/messages.jsonl", history_path)).await
        {
            self.messages = msgs;
        }

        // Write or update session.json metadata.
        let session_json_path = format!("{}/session.json", history_path);
        let now = chrono::Utc::now().to_rfc3339();
        let metadata = if let Ok(mut m) =
            history::load_session_metadata(&session_json_path).await
        {
            m.updated_at = now.clone();
            m
        } else {
            history::SessionMetadata {
                model: config.model.clone(),
                created_at: now.clone(),
                updated_at: now.clone(),
            }
        };
        let _ = history::save_session_metadata(&session_json_path, &metadata).await;

        let agent = Arc::new(Agent::new(config, provider, registry));

        loop {
            // Drain real-time tool events.
            if let Some(ref mut rx) = self.tool_rx {
                while let Ok(event) = rx.try_recv() {
                    match event {
                        ToolEvent::Started { name } => {
                            self.tools_log.push(ToolLogEntry {
                                name,
                                status: "⏳".into(),
                                duration_ms: 0,
                                success: true,
                            });
                        }
                        ToolEvent::Finished { name, duration_ms, success } => {
                            if let Some(entry) = self
                                .tools_log
                                .iter_mut()
                                .rev()
                                .find(|e| e.name == name && e.status == "⏳")
                            {
                                entry.status = if success { "✓".into() } else { "✗".into() };
                                entry.duration_ms = duration_ms;
                                entry.success = success;
                            }
                        }
                    }
                }
            }

            // Drain real-time file events.
            if let Some(ref mut rx) = self.file_rx {
                while let Ok(event) = rx.try_recv() {
                    let (path, op) = match event {
                        FileEvent::Read { path } => (path, 'R'),
                        FileEvent::Write { path } => (path, 'W'),
                    };
                    self.files_log.push(FileLogEntry { path, operation: op, added_tick: self.tick });
                }
            }

            // Drain streaming tokens.
            if let Some(ref mut rx) = self.token_rx {
                while let Ok(token) = rx.try_recv() {
                    self.streaming_content
                        .get_or_insert_with(String::new)
                        .push_str(&token);
                }
            }

            // Drain per-step progress so the status bar updates live.
            if let Some(ref mut rx) = self.step_rx {
                while let Ok(ev) = rx.try_recv() {
                    self.steps = self.turn_baseline_steps + ev.steps;
                    self.total_tokens = self.turn_baseline_tokens + ev.total_tokens;
                }
            }

            // Check if the background agent task completed.
            if let Some(ref mut rx) = self.pending {
                if let Ok((result, mut new_history)) = rx.try_recv() {
                    self.agent_busy = false;
                    self.pending = None;
                    self.tool_rx = None;
                    self.file_rx = None;
                    self.token_rx = None;
                    self.step_rx = None;
                    self.streaming_content = None;
                    self.abort_handle = None;
                    // Authoritative final counts (overwrites live StepEvent values).
                    self.steps = self.turn_baseline_steps + result.steps;
                    self.total_tokens = self.turn_baseline_tokens + result.total_tokens;
                    self.turn_baseline_steps = self.steps;
                    self.turn_baseline_tokens = self.total_tokens;
                    agent.compact_history(&mut new_history);
                    self.conversation_history = new_history;

                    let answer = result.answer.unwrap_or_else(|| {
                        result.error.unwrap_or_else(|| "No response".to_string())
                    });
                    self.messages
                        .push(TuiMessage { role: "assistant".into(), content: answer });

                    let _ = history::save_messages(
                        &format!("{}/messages.jsonl", history_path),
                        &self.messages,
                    )
                    .await;

                    // Update session.json updated_at timestamp.
                    if let Ok(mut m) = history::load_session_metadata(&session_json_path).await {
                        m.updated_at = chrono::Utc::now().to_rfc3339();
                        let _ = history::save_session_metadata(&session_json_path, &m).await;
                    }
                }
            }

            self.tick = self.tick.wrapping_add(1);
            terminal.draw(|f| layout::draw(f, self))?;

            if !self.running {
                break;
            }

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key, &agent, &history_path);
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, agent: &Arc<Agent>, history_path: &str) {
        // Skills overlay captures all keys.
        if self.show_skills {
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Esc)
                | (KeyModifiers::NONE, KeyCode::Char('q')) => {
                    self.show_skills = false;
                    self.skills_scroll = 0;
                }
                (KeyModifiers::ALT, KeyCode::Char('s')) => {
                    self.show_skills = false;
                    self.skills_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::Up) => {
                    self.skills_scroll = self.skills_scroll.saturating_sub(1);
                }
                (KeyModifiers::NONE, KeyCode::Down) => {
                    self.skills_scroll = self.skills_scroll
                        .saturating_add(1)
                        .min(self.skills_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::PageUp) => {
                    self.skills_scroll = self.skills_scroll.saturating_sub(10);
                }
                (KeyModifiers::NONE, KeyCode::PageDown) => {
                    self.skills_scroll = self.skills_scroll
                        .saturating_add(10)
                        .min(self.skills_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    self.skills_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    self.skills_scroll = self.skills_max_scroll as usize;
                }
                _ => {}
            }
            return;
        }

        // MCP overlay captures all keys.
        if self.show_mcp {
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Esc)
                | (KeyModifiers::NONE, KeyCode::Char('q')) => {
                    self.show_mcp = false;
                    self.mcp_scroll = 0;
                }
                (KeyModifiers::ALT, KeyCode::Char('m')) => {
                    self.show_mcp = false;
                    self.mcp_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::Up) => {
                    self.mcp_scroll = self.mcp_scroll.saturating_sub(1);
                }
                (KeyModifiers::NONE, KeyCode::Down) => {
                    self.mcp_scroll = self.mcp_scroll
                        .saturating_add(1)
                        .min(self.mcp_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::PageUp) => {
                    self.mcp_scroll = self.mcp_scroll.saturating_sub(10);
                }
                (KeyModifiers::NONE, KeyCode::PageDown) => {
                    self.mcp_scroll = self.mcp_scroll
                        .saturating_add(10)
                        .min(self.mcp_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    self.mcp_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    self.mcp_scroll = self.mcp_max_scroll as usize;
                }
                _ => {}
            }
            return;
        }

        // Help overlay captures all keys for scrolling and dismissal.
        if self.show_help {
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('?'))
                | (KeyModifiers::NONE, KeyCode::Esc)
                | (KeyModifiers::NONE, KeyCode::Char('q')) => {
                    self.show_help = false;
                    self.help_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::Up) => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                (KeyModifiers::NONE, KeyCode::Down) => {
                    self.help_scroll = self.help_scroll
                        .saturating_add(1)
                        .min(self.help_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::PageUp) => {
                    self.help_scroll = self.help_scroll.saturating_sub(10);
                }
                (KeyModifiers::NONE, KeyCode::PageDown) => {
                    self.help_scroll = self.help_scroll
                        .saturating_add(10)
                        .min(self.help_max_scroll as usize);
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    self.help_scroll = 0;
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    self.help_scroll = self.help_max_scroll as usize;
                }
                _ => {}
            }
            return;
        }

        match (key.modifiers, key.code) {
            // ── Help overlay ─────────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Char('?')) if self.input.is_empty() => {
                self.show_help = true;
                self.help_scroll = 0;
            }

            // ── Panel visibility toggles ─────────────────────────────────────
            (KeyModifiers::ALT, KeyCode::Char('t')) => {
                self.show_tools = !self.show_tools;
                theme::save_panels(self.show_tools, self.show_files);
            }
            (KeyModifiers::ALT, KeyCode::Char('f')) => {
                self.show_files = !self.show_files;
                theme::save_panels(self.show_tools, self.show_files);
            }
            (KeyModifiers::ALT, KeyCode::Char('s')) => {
                self.show_skills = true;
                self.skills_scroll = 0;
            }
            (KeyModifiers::ALT, KeyCode::Char('m')) => {
                self.show_mcp = true;
                self.mcp_scroll = 0;
            }

            // ── Exit / interrupt ─────────────────────────────────────────────
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                self.running = false;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.agent_busy {
                    if let Some(h) = self.abort_handle.take() {
                        h.abort();
                    }
                    self.agent_busy = false;
                    self.pending = None;
                    self.tool_rx = None;
                    self.file_rx = None;
                    self.token_rx = None;
                    self.step_rx = None;
                    self.streaming_content = None;
                    self.turn_baseline_steps = self.steps;
                    self.turn_baseline_tokens = self.total_tokens;
                } else {
                    self.running = false;
                }
            }

            // ── Session management ───────────────────────────────────────────
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.messages.clear();
            }
            (KeyModifiers::ALT, KeyCode::Char('c')) => {
                self.theme_idx = (self.theme_idx + 1) % theme::THEMES.len();
                theme::save_theme(self.theme_idx);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                let msgs = self.messages.clone();
                let path = format!("{}/messages.jsonl", history_path);
                tokio::spawn(async move {
                    let _ = history::save_messages(&path, &msgs).await;
                });
            }

            // ── Send message ─────────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if !self.input.is_empty() && !self.agent_busy {
                    let user_msg = self.input.clone();

                    // Push to prompt history and reset history navigation.
                    self.prompt_history.push(user_msg.clone());
                    self.history_idx = None;
                    self.history_draft.clear();
                    self.input.clear();
                    self.input_cursor = 0;

                    self.messages
                        .push(TuiMessage { role: "user".into(), content: user_msg.clone() });
                    self.conv_follow = true;
                    self.tools_log.clear();
                    self.agent_busy = true;

                    let history = self.conversation_history.clone();
                    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                    let (tool_tx, tool_rx) = tokio::sync::mpsc::channel::<ToolEvent>(64);
                    let (file_tx, file_rx) = tokio::sync::mpsc::channel::<FileEvent>(64);
                    self.pending = Some(result_rx);
                    self.tool_rx = Some(tool_rx);
                    self.file_rx = Some(file_rx);

                    let (token_tx, token_rx) = tokio::sync::mpsc::channel::<String>(256);
                    self.token_rx = Some(token_rx);
                    let (step_tx, step_rx) = tokio::sync::mpsc::channel::<StepEvent>(16);
                    self.step_rx = Some(step_rx);
                    self.streaming_content = None;
                    self.turn_baseline_steps = self.steps;
                    self.turn_baseline_tokens = self.total_tokens;

                    let arc = Arc::clone(agent);
                    let join_handle = tokio::spawn(async move {
                        let (result, new_history) = arc
                            .run_turn_with_events(history, user_msg, token_tx, tool_tx, file_tx, step_tx)
                            .await;
                        let _ = result_tx.send((result, new_history));
                    });
                    self.abort_handle = Some(join_handle.abort_handle());
                }
            }

            // ── Esc: clear input ─────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.input.clear();
                self.input_cursor = 0;
                self.history_idx = None;
                self.history_draft.clear();
            }

            // ── Prompt history navigation ────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.prompt_history.is_empty() {
                    return;
                }
                match self.history_idx {
                    None => {
                        self.history_draft = self.input.clone();
                        let idx = self.prompt_history.len() - 1;
                        self.history_idx = Some(idx);
                        self.input = self.prompt_history[idx].clone();
                        self.input_cursor = self.input.len();
                    }
                    Some(0) => {}
                    Some(i) => {
                        let idx = i - 1;
                        self.history_idx = Some(idx);
                        self.input = self.prompt_history[idx].clone();
                        self.input_cursor = self.input.len();
                    }
                }
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                match self.history_idx {
                    None => {}
                    Some(i) if i + 1 >= self.prompt_history.len() => {
                        self.input = self.history_draft.clone();
                        self.history_idx = None;
                        self.history_draft.clear();
                        self.input_cursor = self.input.len();
                    }
                    Some(i) => {
                        let idx = i + 1;
                        self.history_idx = Some(idx);
                        self.input = self.prompt_history[idx].clone();
                        self.input_cursor = self.input.len();
                    }
                }
            }

            // ── Cursor movement ──────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.input_cursor = cursor_left(&self.input, self.input_cursor);
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.input_cursor = cursor_right(&self.input, self.input_cursor);
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.input_cursor = 0;
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.input_cursor = self.input.len();
            }

            // ── Panel scrolling (Ctrl+Up / Ctrl+Down) ───────────────────────
            (KeyModifiers::CONTROL, KeyCode::Up) => {
                match self.focused_panel {
                    0 => {
                        self.conv_follow = false;
                        self.conv_scroll = self.conv_scroll.saturating_sub(5);
                    }
                    1 => { self.tools_scroll = self.tools_scroll.saturating_sub(5); }
                    2 => { self.files_scroll = self.files_scroll.saturating_sub(5); }
                    _ => {}
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Down) => {
                match self.focused_panel {
                    0 => {
                        self.conv_scroll += 5;
                        if self.conv_scroll as u16 >= self.conv_max_scroll {
                            self.conv_follow = true;
                            self.conv_scroll = self.conv_max_scroll as usize;
                        }
                    }
                    1 => { self.tools_scroll += 5; }
                    2 => { self.files_scroll += 5; }
                    _ => {}
                }
            }

            // ── Panel focus cycling ──────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.focused_panel = (self.focused_panel + 1) % NUM_PANELS;
            }

            // ── Deletion ─────────────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.input_cursor > 0 {
                    let prev = cursor_left(&self.input, self.input_cursor);
                    self.input.drain(prev..self.input_cursor);
                    self.input_cursor = prev;
                }
            }
            (KeyModifiers::NONE, KeyCode::Delete) => {
                if self.input_cursor < self.input.len() {
                    let next = cursor_right(&self.input, self.input_cursor);
                    self.input.drain(self.input_cursor..next);
                }
            }

            // ── Character insertion ──────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Char(c))
            | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers == KeyModifiers::SHIFT {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                };
                self.input.insert(self.input_cursor, ch);
                self.input_cursor += ch.len_utf8();
            }

            _ => {}
        }
    }
}
