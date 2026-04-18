use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::tmux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unknown,
    Processing,
    NeedsInput,
    Idle,
}

impl Status {
    pub fn dot(self) -> &'static str {
        match self {
            Status::Unknown => "○",
            _ => "●",
        }
    }

    pub fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Status::Processing => Color::Yellow,
            Status::NeedsInput => Color::Red,
            Status::Idle => Color::Green,
            Status::Unknown => Color::DarkGray,
        }
    }
}

const SPINNER_CHARS: &[char] = &[
    '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏',
    '◐', '◓', '◑', '◒', '✢', '✳', '∗', '✻', '✽',
];
const PROCESSING_MARKERS: &[&str] = &[
    "esc to interrupt",
    "esc to cancel",
    "(esc",
    "ctrl+c to interrupt",
];
const NEEDS_INPUT_MARKERS: &[&str] = &[
    "do you want to",
    "do you trust",
    "(y/n)",
    "[y/n]",
    "(yes/no)",
    "press enter",
    "❯ 1.",
    "❯ 2.",
    "❯ 3.",
    "1. yes",
    "approve this",
    "allow tool",
];
/// Strings Claude Code shows in its hint footer when it's idle and waiting
/// for the next user message — used to distinguish "Claude is up but not
/// processing" from a permission prompt.
const CLAUDE_IDLE_MARKERS: &[&str] = &[
    "shift+tab to cycle",
    "auto-accept",
    "plan mode on",
    "bypass permissions",
    "? for shortcuts",
    "ctrl+_ to undo",
];
const PROCESSING_CHANGE_MS: u128 = 1_500;

/// Classify a captured pane's visible text into a status. `since_change` is
/// only consulted as a fallback for non-Claude windows where we have no
/// content-based signal.
pub fn classify(captured: &str, since_change: Duration) -> Status {
    if captured.trim().is_empty() {
        return Status::Unknown;
    }
    let lower = captured.to_ascii_lowercase();

    // 1. Active processing wins over everything — Claude Code shows the input
    //    box and an "esc to interrupt" hint while running.
    if PROCESSING_MARKERS.iter().any(|m| lower.contains(m))
        || captured.chars().any(|c| SPINNER_CHARS.contains(&c))
    {
        return Status::Processing;
    }

    // 2. Specific decision/permission prompts.
    if NEEDS_INPUT_MARKERS.iter().any(|m| lower.contains(m)) {
        return Status::NeedsInput;
    }

    // 3. Claude Code is up but at rest — its idle footer is showing.
    if CLAUDE_IDLE_MARKERS.iter().any(|m| lower.contains(m)) {
        return Status::Idle;
    }

    // 4. Non-Claude window: fall back to "did the screen change recently".
    if since_change.as_millis() < PROCESSING_CHANGE_MS {
        return Status::Processing;
    }
    Status::Idle
}

/// The single right-side pty, attached to a specific tmux window. Respawned
/// whenever the sidebar selection changes.
pub struct Pane {
    pub screen: Arc<Mutex<vt100::Parser>>,
    pty_master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send + Sync>,
    alive: Arc<Mutex<bool>>,
    rows: u16,
    cols: u16,
}

impl Pane {
    pub fn spawn_tmux(session: &str, window_index: u32, rows: u16, cols: u16) -> Result<Self> {
        let (program, args) = tmux::attach_window_command(session, window_index);
        Pane::spawn_cmd(&program, &args, rows, cols)
    }

    fn spawn_cmd(program: &str, args: &[String], rows: u16, cols: u16) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(program);
        for a in args {
            cmd.arg(a);
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd).context("spawn pty command")?;
        drop(pair.slave);

        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let alive = Arc::new(Mutex::new(true));

        let reader = pair.master.try_clone_reader().context("clone pty reader")?;
        {
            let screen = screen.clone();
            let alive = alive.clone();
            thread::spawn(move || {
                reader_loop(reader, screen, alive);
            });
        }

        let writer = pair.master.take_writer().context("take pty writer")?;

        Ok(Pane {
            screen,
            pty_master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child,
            alive,
            rows,
            cols,
        })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.pty_master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.screen.lock().set_size(rows, cols);
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut w = self.writer.lock();
        w.write_all(bytes).context("pty write")?;
        w.flush().ok();
        Ok(())
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        *self.alive.lock() = false;
        let _ = self.child.kill();
    }
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    alive: Arc<Mutex<bool>>,
) {
    let mut buf = [0u8; 4096];
    loop {
        if !*alive.lock() {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                screen.lock().process(&buf[..n]);
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                thread::sleep(Duration::from_millis(50));
                return;
            }
        }
    }
}

/// Stable hash of visible text that ignores trailing whitespace.
pub fn hash_capture(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for line in s.lines() {
        line.trim_end().hash(&mut h);
        0u8.hash(&mut h);
    }
    h.finish()
}
