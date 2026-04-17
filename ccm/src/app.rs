use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::session::Session;
use crate::tmux;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub name: String,
    pub path: String,
    pub tmux_session: String,
    #[serde(default)]
    pub claude_conversation_id: Option<String>,
    pub last_active: chrono::DateTime<chrono::Utc>,
}

impl SessionMeta {
    pub fn new(name: &str, path: &str) -> Self {
        SessionMeta {
            name: name.to_string(),
            path: path.to_string(),
            tmux_session: tmux::full_name(name),
            claude_conversation_id: None,
            last_active: chrono::Utc::now(),
        }
    }
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: usize,

    /// Modal prompt state — when Some, keystrokes build up the prompt input
    /// instead of being forwarded to the pty.
    pub prompt: Option<Prompt>,

    pub should_quit: bool,
    pub last_error: Option<String>,

    pub sessions_dir: PathBuf,

    /// Last computed pane size — used when spawning a new session so its pty
    /// is sized right from the start.
    pub pane_rows: u16,
    pub pane_cols: u16,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub buffer: String,
    /// For NewSession: first we ask name, then path.
    pub stage: PromptStage,
    /// Captured first-stage value (session name), used on stage 2.
    pub stash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    NewSession,
    Rename,
    ConfirmClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStage {
    First,
    Second,
}

impl Prompt {
    pub fn title(&self) -> &'static str {
        match (self.kind, self.stage) {
            (PromptKind::NewSession, PromptStage::First) => "New session — name:",
            (PromptKind::NewSession, PromptStage::Second) => "New session — path:",
            (PromptKind::Rename, _) => "Rename session to:",
            (PromptKind::ConfirmClose, _) => "Close session? [y/N]",
        }
    }
}

impl App {
    pub fn new() -> Result<Self> {
        let sessions_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ccm")
            .join("sessions");
        std::fs::create_dir_all(&sessions_dir).context("create sessions dir")?;

        Ok(App {
            sessions: Vec::new(),
            selected: 0,
            prompt: None,
            should_quit: false,
            last_error: None,
            sessions_dir,
            pane_rows: 24,
            pane_cols: 80,
        })
    }

    pub fn active(&self) -> Option<&Session> {
        self.sessions.get(self.selected)
    }

    pub fn next(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.sessions.len();
    }

    pub fn prev(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.sessions.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Populate the session list from existing tmux sessions + metadata files.
    pub fn bootstrap(&mut self) -> Result<()> {
        let existing = tmux::list_sessions().unwrap_or_default();
        for full in existing {
            let short = tmux::short_name(&full).to_string();
            let meta_path = self.meta_path(&short);
            let path = if meta_path.exists() {
                match load_meta(&meta_path) {
                    Ok(m) => m.path,
                    Err(_) => tmux::session_path(&full).unwrap_or_else(|| ".".into()),
                }
            } else {
                tmux::session_path(&full).unwrap_or_else(|| ".".into())
            };

            match Session::spawn(short.clone(), path.clone(), self.pane_rows, self.pane_cols) {
                Ok(s) => {
                    self.sessions.push(s);
                    let _ = self.write_meta(&SessionMeta::new(&short, &path));
                }
                Err(e) => {
                    self.last_error = Some(format!("failed to attach {short}: {e}"));
                }
            }
        }
        Ok(())
    }

    pub fn create_session(&mut self, name: String, path: String) -> Result<()> {
        if name.trim().is_empty() {
            anyhow::bail!("session name cannot be empty");
        }
        let path = if path.trim().is_empty() {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| ".".into())
        } else {
            shellexpand(&path)
        };

        if self.sessions.iter().any(|s| s.name == name) {
            anyhow::bail!("session name '{name}' already exists");
        }

        let session = Session::spawn(name.clone(), path.clone(), self.pane_rows, self.pane_cols)?;
        self.sessions.push(session);
        self.selected = self.sessions.len() - 1;
        let _ = self.write_meta(&SessionMeta::new(&name, &path));
        Ok(())
    }

    pub fn close_active(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let s = self.sessions.remove(self.selected);
        let _ = tmux::kill_session(&s.tmux_name);
        let meta_path = self.meta_path(&s.name);
        let _ = std::fs::remove_file(meta_path);
        if self.selected >= self.sessions.len() && self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn rename_active(&mut self, new_name: String) -> Result<()> {
        if new_name.trim().is_empty() {
            anyhow::bail!("new name cannot be empty");
        }
        if self.sessions.iter().any(|s| s.name == new_name) {
            anyhow::bail!("session name '{new_name}' already exists");
        }
        let (old_short, old_tmux, path) = {
            let Some(active) = self.sessions.get(self.selected) else {
                return Ok(());
            };
            (active.name.clone(), active.tmux_name.clone(), active.path.clone())
        };
        let new_tmux = tmux::full_name(&new_name);
        tmux::rename_session(&old_tmux, &new_tmux)?;
        if let Some(active) = self.sessions.get_mut(self.selected) {
            active.name = new_name.clone();
            active.tmux_name = new_tmux;
        }
        let _ = std::fs::remove_file(self.meta_path(&old_short));
        let _ = self.write_meta(&SessionMeta::new(&new_name, &path));
        Ok(())
    }

    pub fn update_statuses(&mut self) {
        for s in &mut self.sessions {
            s.recompute_status();
        }
    }

    fn meta_path(&self, name: &str) -> PathBuf {
        self.sessions_dir.join(format!("{name}.toml"))
    }

    fn write_meta(&self, meta: &SessionMeta) -> Result<()> {
        let path = self.meta_path(&meta.name);
        let body = toml::to_string_pretty(meta).context("serialize meta")?;
        std::fs::write(&path, body).context("write meta file")?;
        Ok(())
    }
}

fn load_meta(path: &Path) -> Result<SessionMeta> {
    let body = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&body)?)
}

fn shellexpand(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().into_owned();
        }
    }
    s.to_string()
}
