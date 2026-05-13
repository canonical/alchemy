pub mod layout;
pub mod widgets;
pub mod input;
pub mod history;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use crate::agent::{Agent, AgentConfig};
use crate::tools::ToolRegistry;
use crate::providers::Provider;

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
    pub operation: char, // 'R' or 'W'
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
        if let Ok(msgs) = history::load_messages(&format!("{}/messages.jsonl", history_path)).await {
            self.messages = msgs;
        }

        let agent = Agent::new(config, provider, registry);

        while self.running {
            terminal.draw(|f| layout::draw(f, self))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match self.handle_key(key, &agent, &history_path).await {
                        Ok(()) => {}
                        Err(_) => break,
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent, agent: &Agent, history_path: &str) -> Result<()> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                self.running = false;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.agent_busy {
                    self.agent_busy = false;
                } else {
                    self.running = false;
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.messages.clear();
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if !self.input.is_empty() && !self.agent_busy {
                    let user_msg = self.input.clone();
                    self.input.clear();
                    self.messages.push(TuiMessage { role: "user".into(), content: user_msg.clone() });

                    self.agent_busy = true;
                    let result = agent.run(user_msg).await;
                    self.agent_busy = false;
                    self.steps += result.steps;

                    let answer = result.answer.unwrap_or_else(|| {
                        result.error.unwrap_or_else(|| "No response".to_string())
                    });
                    self.messages.push(TuiMessage { role: "assistant".into(), content: answer });

                    // Save to history
                    let _ = history::save_messages(
                        &format!("{}/messages.jsonl", history_path),
                        &self.messages,
                    ).await;
                }
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
        Ok(())
    }
}
