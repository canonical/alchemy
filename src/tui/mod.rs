pub mod history;
pub mod input;
pub mod layout;
pub mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::Arc;
use crate::agent::{Agent, AgentConfig, FileEvent, ToolEvent};
use crate::providers::Provider;
use crate::tools::ToolRegistry;
use crate::types::{AgentResult, Message};

pub struct TuiApp {
    pub session_name: String,
    pub session_dir: String,
    /// Display messages (role + content) rendered in the conversation panel.
    pub messages: Vec<TuiMessage>,
    pub input: String,
    pub running: bool,
    pub agent_busy: bool,
    pub tools_log: Vec<ToolLogEntry>,
    pub files_log: Vec<FileLogEntry>,
    pub model_name: String,
    pub total_tokens: u64,
    pub steps: u32,
    /// Full LLM message history for multi-turn context (no system message).
    conversation_history: Vec<Message>,
    /// Receives the final result + updated history from the background agent task.
    pending: Option<tokio::sync::oneshot::Receiver<(AgentResult, Vec<Message>)>>,
    /// Receives real-time tool events from the background agent task.
    tool_rx: Option<tokio::sync::mpsc::Receiver<ToolEvent>>,
    /// Receives real-time file events from the background agent task.
    file_rx: Option<tokio::sync::mpsc::Receiver<FileEvent>>,
}

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
}

#[derive(Clone)]
pub struct FileLogEntry {
    pub path: String,
    pub operation: char,
}

impl TuiApp {
    pub fn new(session_name: String, session_dir: String, model_name: String) -> Self {
        Self {
            session_name,
            session_dir,
            messages: Vec::new(),
            input: String::new(),
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
        }
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
                            });
                        }
                        ToolEvent::Finished { name, duration_ms } => {
                            if let Some(entry) = self
                                .tools_log
                                .iter_mut()
                                .rev()
                                .find(|e| e.name == name && e.status == "⏳")
                            {
                                entry.status = "✓".into();
                                entry.duration_ms = duration_ms;
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
                    self.files_log.push(FileLogEntry { path, operation: op });
                }
            }

            // Check if the background agent task completed.
            if let Some(ref mut rx) = self.pending {
                if let Ok((result, mut new_history)) = rx.try_recv() {
                    self.agent_busy = false;
                    self.pending = None;
                    self.tool_rx = None;
                    self.file_rx = None;
                    self.steps += result.steps;
                    self.total_tokens += result.total_tokens;
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
                }
            }

            terminal.draw(|f| layout::draw(f, self))?;

            if !self.running {
                break;
            }

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key, &agent, &history_path);
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, agent: &Arc<Agent>, history_path: &str) {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                self.running = false;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.agent_busy {
                    self.agent_busy = false;
                    self.pending = None;
                } else {
                    self.running = false;
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.messages.clear();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                let msgs = self.messages.clone();
                let path = format!("{}/messages.jsonl", history_path);
                tokio::spawn(async move {
                    let _ = history::save_messages(&path, &msgs).await;
                });
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if !self.input.is_empty() && !self.agent_busy {
                    let user_msg = self.input.clone();
                    self.input.clear();
                    self.messages
                        .push(TuiMessage { role: "user".into(), content: user_msg.clone() });

                    self.tools_log.clear();
                    self.agent_busy = true;

                    let history = self.conversation_history.clone();
                    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                    let (tool_tx, tool_rx) = tokio::sync::mpsc::channel::<ToolEvent>(64);
                    let (file_tx, file_rx) = tokio::sync::mpsc::channel::<FileEvent>(64);
                    self.pending = Some(result_rx);
                    self.tool_rx = Some(tool_rx);
                    self.file_rx = Some(file_rx);

                    let arc = Arc::clone(agent);
                    tokio::spawn(async move {
                        let (result, new_history) = arc
                            .run_turn_with_events(history, user_msg, |_| {}, tool_tx, file_tx)
                            .await;
                        let _ = result_tx.send((result, new_history));
                    });
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Enter) => {
                self.input.push('\n');
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input.pop();
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                self.input.push(c);
            }
            (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                self.input.push(c.to_uppercase().next().unwrap_or(c));
            }
            _ => {}
        }
    }
}
