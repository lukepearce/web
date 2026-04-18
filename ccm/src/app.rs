use anyhow::Result;
use std::process::Command;
use std::time::Instant;

use crate::session::{
    classify, hash_capture, parse_claude_meta, parse_session_usage, Pane, Status,
};
use crate::tmux;

pub const SIDEBAR_MIN: u16 = 10;
pub const SIDEBAR_MAX: u16 = 60;
const SIDEBAR_STEP: u16 = 2;

pub struct Project {
    pub name: String,       // "floom"
    pub tmux_name: String,  // "ccm-floom"
    pub path: String,
    pub windows: Vec<WindowRow>,
    pub tmux_active_window: Option<u32>,
}

pub struct WindowRow {
    pub index: u32,
    pub name: String,
    pub status: Status,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub context_pct: Option<u8>,
    pub tokens: Option<String>,
    pub last_hash: u64,
    pub last_change: Instant,
    pub last_branch_check: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub project: usize,
    pub window: usize,
}

pub struct App {
    pub projects: Vec<Project>,
    pub selected: Option<Selection>,
    /// (project tmux name, window index) the pane is currently attached to.
    pub pane_target: Option<(String, u32)>,
    pub pane: Option<Pane>,

    pub prompt: Option<Prompt>,
    pub should_quit: bool,
    pub last_error: Option<String>,

    pub pane_rows: u16,
    pub pane_cols: u16,
    pub sidebar_width: u16,
    pub verbose: bool,
    pub session_usage: Option<String>,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub buffer: String,
    pub stage: PromptStage,
    pub stash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    NewProject,
    NewWindow,
    Rename,
    ConfirmCloseWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStage {
    First,
    Second,
}

impl Prompt {
    pub fn title(&self) -> &'static str {
        match (self.kind, self.stage) {
            (PromptKind::NewProject, PromptStage::First) => "New project — name:",
            (PromptKind::NewProject, PromptStage::Second) => "New project — path:",
            (PromptKind::NewWindow, _) => "New window name:",
            (PromptKind::Rename, _) => "Rename window to:",
            (PromptKind::ConfirmCloseWindow, _) => "Close window? [y/N]",
        }
    }
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(App {
            projects: Vec::new(),
            selected: None,
            pane_target: None,
            pane: None,
            prompt: None,
            should_quit: false,
            last_error: None,
            pane_rows: 24,
            pane_cols: 80,
            sidebar_width: 26,
            verbose: false,
            session_usage: None,
        })
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.selected?.project)
    }

    pub fn selected_window(&self) -> Option<&WindowRow> {
        let sel = self.selected?;
        self.projects.get(sel.project)?.windows.get(sel.window)
    }

    pub fn next(&mut self) {
        let Some(sel) = self.flat_step(1) else { return };
        self.selected = Some(sel);
    }

    pub fn prev(&mut self) {
        let Some(sel) = self.flat_step(-1) else { return };
        self.selected = Some(sel);
    }

    fn flat_step(&self, dir: i32) -> Option<Selection> {
        let flat = self.flatten();
        if flat.is_empty() {
            return None;
        }
        let cur = self.selected.and_then(|s| {
            flat.iter().position(|&(p, w)| p == s.project && w == s.window)
        });
        let n = flat.len() as i32;
        let next = match cur {
            Some(i) => (((i as i32) + dir).rem_euclid(n)) as usize,
            None => 0,
        };
        let (p, w) = flat[next];
        Some(Selection { project: p, window: w })
    }

    fn flatten(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (p, proj) in self.projects.iter().enumerate() {
            for w in 0..proj.windows.len() {
                out.push((p, w));
            }
        }
        out
    }

    pub fn grow_sidebar(&mut self) {
        self.sidebar_width = (self.sidebar_width + SIDEBAR_STEP).min(SIDEBAR_MAX);
    }

    pub fn shrink_sidebar(&mut self) {
        self.sidebar_width = self.sidebar_width.saturating_sub(SIDEBAR_STEP).max(SIDEBAR_MIN);
    }

    /// Rebuild the project/window tree from tmux, preserving per-window status
    /// state across refreshes.
    pub fn refresh_tree(&mut self) {
        if !tmux::is_installed() {
            return;
        }
        let sessions = match tmux::list_sessions() {
            Ok(s) => s,
            Err(_) => return,
        };
        let windows = tmux::list_windows(&sessions).unwrap_or_default();

        let mut new_projects: Vec<Project> = Vec::new();
        for full in &sessions {
            // Idempotently disable the tmux status bar so its clock tick
            // doesn't defeat our idle detection.
            tmux::disable_status_bar(full);

            let path = tmux::session_path(full).unwrap_or_else(|| ".".into());
            let name = tmux::short_name(full).to_string();
            let tmux_active_window = tmux::active_window(full);
            let mut proj_windows: Vec<WindowRow> = Vec::new();
            for w in windows.iter().filter(|w| &w.session == full) {
                let win_target = format!("{full}:{}", w.index);
                let win_path = tmux::session_path(&win_target);
                // Preserve per-window state across refreshes.
                let prior = self
                    .projects
                    .iter()
                    .find(|p| &p.tmux_name == full)
                    .and_then(|p| p.windows.iter().find(|r| r.index == w.index));
                let (status, last_hash, last_change, branch, last_branch_check, ctx, toks) =
                    match prior {
                        Some(r) => (
                            r.status,
                            r.last_hash,
                            r.last_change,
                            r.branch.clone(),
                            r.last_branch_check,
                            r.context_pct,
                            r.tokens.clone(),
                        ),
                        None => (Status::Unknown, 0, Instant::now(), None, None, None, None),
                    };
                proj_windows.push(WindowRow {
                    index: w.index,
                    name: w.name.clone(),
                    status,
                    path: win_path,
                    branch,
                    context_pct: ctx,
                    tokens: toks,
                    last_hash,
                    last_change,
                    last_branch_check,
                });
            }
            proj_windows.sort_by_key(|w| w.index);
            new_projects.push(Project {
                name,
                tmux_name: full.clone(),
                path,
                windows: proj_windows,
                tmux_active_window,
            });
        }
        new_projects.sort_by(|a, b| a.name.cmp(&b.name));
        self.projects = new_projects;

        self.refresh_branches();

        // Clamp selection.
        if let Some(sel) = self.selected {
            let valid = self
                .projects
                .get(sel.project)
                .map(|p| sel.window < p.windows.len())
                .unwrap_or(false);
            if !valid {
                self.selected = self.flatten().first().map(|(p, w)| Selection {
                    project: *p,
                    window: *w,
                });
            }
        } else {
            self.selected = self.flatten().first().map(|(p, w)| Selection {
                project: *p,
                window: *w,
            });
        }
    }

    /// Poll `tmux capture-pane` for each window and reclassify status.
    /// Also extracts Claude Code context / token counts from the capture.
    pub fn poll_statuses(&mut self) {
        let now = Instant::now();
        let mut latest_usage: Option<String> = None;
        let active_target = self
            .selected_project()
            .zip(self.selected_window())
            .map(|(p, w)| (p.tmux_name.clone(), w.index));

        for proj in &mut self.projects {
            for row in &mut proj.windows {
                let target = format!("{}:{}", proj.tmux_name, row.index);
                let Some(text) = tmux::capture_pane(&target) else {
                    continue;
                };
                let h = hash_capture(&text);
                if h != row.last_hash {
                    row.last_hash = h;
                    row.last_change = now;
                }
                row.status = classify(&text, now.duration_since(row.last_change));
                let (ctx, toks) = parse_claude_meta(&text);
                if ctx.is_some() { row.context_pct = ctx; }
                if toks.is_some() { row.tokens = toks; }

                if active_target.as_ref() == Some(&(proj.tmux_name.clone(), row.index)) {
                    if let Some(u) = parse_session_usage(&text) {
                        latest_usage = Some(u);
                    }
                }
            }
        }

        if latest_usage.is_some() {
            self.session_usage = latest_usage;
        }
    }

    /// Refresh git branch per window (cheap but not free — only re-runs every
    /// 5s per window).
    fn refresh_branches(&mut self) {
        let now = Instant::now();
        for proj in &mut self.projects {
            for row in &mut proj.windows {
                let due = row
                    .last_branch_check
                    .map(|t| now.duration_since(t).as_secs() >= 5)
                    .unwrap_or(true);
                if !due {
                    continue;
                }
                row.last_branch_check = Some(now);
                let Some(path) = row.path.as_deref() else { continue };
                row.branch = current_branch(path);
            }
        }
    }

    /// If the tmux-side active window of the selected project has changed
    /// externally (e.g. user hit the tmux prefix+n inside), track it.
    pub fn follow_tmux_active(&mut self) {
        let Some(sel) = self.selected else { return };
        let Some(proj) = self.projects.get(sel.project) else { return };
        let Some(active) = proj.tmux_active_window else { return };
        let Some(current_idx) = proj.windows.get(sel.window).map(|w| w.index) else { return };
        if active == current_idx {
            return;
        }
        if let Some(wi) = proj.windows.iter().position(|w| w.index == active) {
            self.selected = Some(Selection { project: sel.project, window: wi });
        }
    }

    pub fn select_window_by_index(&mut self, idx: u32) {
        let Some(sel) = self.selected else { return };
        let Some(proj) = self.projects.get(sel.project) else { return };
        let Some(wi) = proj.windows.iter().position(|w| w.index == idx) else { return };
        let tmux_name = proj.tmux_name.clone();
        self.selected = Some(Selection { project: sel.project, window: wi });
        // Also tell tmux to move its own active window pointer for the session
        // so external attach-sessions stay in sync.
        tmux::select_window(&tmux_name, idx);
    }

    pub fn toggle_verbose(&mut self) {
        self.verbose = !self.verbose;
    }

    /// Make sure the right-pane pty is attached to the currently-selected
    /// window; respawn if the target changed.
    pub fn sync_pane(&mut self) {
        let Some(sel) = self.selected else {
            self.pane = None;
            self.pane_target = None;
            return;
        };
        let Some(proj) = self.projects.get(sel.project) else {
            return;
        };
        let Some(win) = proj.windows.get(sel.window) else {
            return;
        };
        let target = (proj.tmux_name.clone(), win.index);
        if self.pane_target.as_ref() == Some(&target) && self.pane.is_some() {
            return;
        }

        match Pane::spawn_tmux(&proj.tmux_name, win.index, self.pane_rows, self.pane_cols) {
            Ok(p) => {
                self.pane = Some(p);
                self.pane_target = Some(target);
            }
            Err(e) => {
                self.last_error = Some(format!("attach: {e}"));
                self.pane = None;
                self.pane_target = None;
            }
        }
    }

    pub fn create_project(&mut self, name: String, path: String) -> Result<()> {
        if name.trim().is_empty() {
            anyhow::bail!("project name cannot be empty");
        }
        let name = name.trim().to_string();
        let path = if path.trim().is_empty() {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| ".".into())
        } else {
            shellexpand(path.trim())
        };
        let full = tmux::full_name(&name);
        if tmux::has_session(&full) {
            anyhow::bail!("tmux session '{full}' already exists");
        }
        tmux::new_session(&full, &path)?;
        self.refresh_tree();
        // Select the new project's first window.
        if let Some(p) = self.projects.iter().position(|p| p.tmux_name == full) {
            self.selected = Some(Selection { project: p, window: 0 });
        }
        Ok(())
    }

    pub fn create_window(&mut self, name: String) -> Result<()> {
        let Some(sel) = self.selected else {
            anyhow::bail!("no project selected");
        };
        let (full, path) = {
            let Some(proj) = self.projects.get(sel.project) else {
                anyhow::bail!("project missing");
            };
            (proj.tmux_name.clone(), proj.path.clone())
        };
        let win_name = if name.trim().is_empty() { "claude".into() } else { name.trim().to_string() };
        tmux::new_window(&full, &win_name, &path)?;
        self.refresh_tree();
        // Select the new window (highest index in the project).
        if let Some(pi) = self.projects.iter().position(|p| p.tmux_name == full) {
            if let Some(wi) = self.projects[pi]
                .windows
                .iter()
                .enumerate()
                .max_by_key(|(_, w)| w.index)
                .map(|(i, _)| i)
            {
                self.selected = Some(Selection { project: pi, window: wi });
            }
        }
        Ok(())
    }

    pub fn close_selected_window(&mut self) {
        let Some(sel) = self.selected else { return };
        let Some(proj) = self.projects.get(sel.project) else { return };
        let Some(win) = proj.windows.get(sel.window) else { return };
        let target = format!("{}:{}", proj.tmux_name, win.index);
        // Drop pane first so the attached client releases the window cleanly.
        self.pane = None;
        self.pane_target = None;
        let _ = tmux::kill_window(&target);
        self.refresh_tree();
    }

    pub fn rename_selected_window(&mut self, new_name: String) -> Result<()> {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            anyhow::bail!("window name cannot be empty");
        }
        let Some(sel) = self.selected else {
            return Ok(());
        };
        let Some(proj) = self.projects.get(sel.project) else {
            return Ok(());
        };
        let Some(win) = proj.windows.get(sel.window) else {
            return Ok(());
        };
        let target = format!("{}:{}", proj.tmux_name, win.index);
        tmux::rename_window(&target, &new_name)?;
        self.refresh_tree();
        Ok(())
    }
}

fn current_branch(path: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", path, "branch", "--show-current"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
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
