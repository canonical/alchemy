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
use crate::agent::{Agent, AgentConfig};
use crate::providers::Provider;
use crate::tools::ToolRegistry;
use crate::types::AgentResult;

pub struct TuiApp {
    pub session_name: String,
    pub session_dir: String,
    pub messages: Vec<TuiMessage>,
    pub input: String,
    pub running: bool,
    pub agent_busy: bool,
    pub tools_log: Vec<ToolLogEntry>,
    pub files_log: Vec<FileLogEntry>,
    pub model_name: String,
    pub total_tokens: u64,
    pub steps: u32,
    /// Channel receiving the agent result from the background task.
    pending: Option<tokio::sync::oneshot::Receiver<AgentResult>>,
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
            pending: None,
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

        // Load session history
        let history_path = format!("{}/{}", self.session_dir, self.session_name);
        let _ = tokio::fs::create_dir_all(&history_path).await;
        if let Ok(msgs) =
            history::load_messages(&format!("{}/messages.jsonl", history_path)).await
        {
            self.messages = msgs;
        }

        // Wrap in Arc so the agent can be moved into spawned tasks.
        let agent = Arc::new(Agent::new(config, provider, registry));

        loop {
            // Check if a background agent task completed.
            if let Some(ref mut rx) = self.pending {
                if let Ok(result) = rx.try_recv() {
                    self.agent_busy = false;
                    self.pending = None;
                    self.steps += result.steps;

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
                    // Can't cancel in-flight task without a more complex abort handle;
                    // just mark as no longer busy so the user can send again.
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

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    self.pending = Some(rx);
                    self.agent_busy = true;

                    let arc = Arc::clone(agent);
                    tokio::spawn(async move {
                        let result = arc.run(user_msg).await;
                        let _ = tx.send(result);
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
