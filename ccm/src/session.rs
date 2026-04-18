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
    "thinking…",
    "thinking...",
    "running…",
    "running...",
    "working…",
    "working...",
];
const PROCESSING_CHANGE_MS: u128 = 1_200;
const IDLE_QUIET_MS: u128 = 3_000;

/// Classify a captured pane's visible text into a status, given how long ago
/// the text last changed.
pub fn classify(captured: &str, since_change: Duration) -> Status {
    if captured.trim().is_empty() {
        return Status::Unknown;
    }
    // Processing markers beat the prompt check — Claude Code always has the
    // `>` box visible even while thinking, and we don't want that to flip us
    // to NeedsInput in the middle of a run.
    let lower = captured.to_ascii_lowercase();
    if PROCESSING_MARKERS.iter().any(|m| lower.contains(m))
        || captured.chars().any(|c| SPINNER_CHARS.contains(&c))
    {
        return Status::Processing;
    }
    if needs_input(&lower, captured) {
        return Status::NeedsInput;
    }
    let ms = since_change.as_millis();
    if ms < PROCESSING_CHANGE_MS {
        return Status::Processing;
    }
    if ms >= IDLE_QUIET_MS {
        return Status::Idle;
    }
    Status::Processing
}

fn needs_input(lower: &str, raw: &str) -> bool {
    let phrase_hits = [
        "do you want to",
        "do you trust",
        "(y/n)",
        "[y/n]",
        "(yes/no)",
        "press enter",
        "❯ 1.",
        "❯ 2.",
    ]
    .iter()
    .any(|p| lower.contains(p));

    if phrase_hits {
        return true;
    }

    // Claude Code's input box — a line ending with `>` or `❯` after the
    // prompt glyph. Scan the last few non-empty lines to tolerate trailing
    // blank lines tmux adds.
    for line in raw.lines().rev().take(5) {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        if t.ends_with('>') || t.ends_with('❯') {
            return true;
        }
    }
    false
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

/// Best-effort parse of Claude Code's status line. Looks for a percent near
/// "context" and a token count near "tokens". Returns `(context_pct, tokens)`.
/// Either may be None if the pattern isn't in the captured text.
pub fn parse_claude_meta(text: &str) -> (Option<u8>, Option<String>) {
    let lower = text.to_ascii_lowercase();
    let context = find_context_pct(&lower);
    let tokens = find_tokens(&lower);
    (context, tokens)
}

fn find_context_pct(lower: &str) -> Option<u8> {
    // Patterns: "context: 12%", "12% context", "ctx 12%".
    let anchors = ["context", "ctx"];
    for anchor in anchors {
        let mut search: &str = lower;
        while let Some(i) = search.find(anchor) {
            let window_start = i.saturating_sub(12);
            let window_end = (i + anchor.len() + 12).min(search.len());
            let window = &search[window_start..window_end];
            if let Some(p) = extract_percent(window) {
                return Some(p);
            }
            search = &search[i + anchor.len()..];
        }
    }
    None
}

fn extract_percent(s: &str) -> Option<u8> {
    // Find "<digits>%" in s.
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'%' {
                return s[start..i].parse::<u32>().ok().map(|v| v.min(100) as u8);
            }
        }
        i += 1;
    }
    None
}

fn find_tokens(lower: &str) -> Option<String> {
    // Patterns: "12k tokens", "1,234 tokens", "tokens: 12k".
    let mut search: &str = lower;
    while let Some(i) = search.find("tokens") {
        let window_start = i.saturating_sub(16);
        let window_end = (i + "tokens".len() + 16).min(search.len());
        let window = &search[window_start..window_end];
        if let Some(n) = extract_token_number(window) {
            return Some(n);
        }
        search = &search[i + "tokens".len()..];
    }
    None
}

fn extract_token_number(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b',' || bytes[i] == b'.') {
                i += 1;
            }
            let mut end = i;
            if end < bytes.len() && (bytes[end] == b'k' || bytes[end] == b'K' || bytes[end] == b'm' || bytes[end] == b'M') {
                end += 1;
            }
            return Some(s[start..end].to_string());
        }
        i += 1;
    }
    None
}

/// Best-effort parse for the Claude 5-hour session indicator. Looks for
/// "session" near a percent or "reset <time>" phrase.
pub fn parse_session_usage(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let i = lower.find("session")?;
    let window_end = (i + 64).min(lower.len());
    let window = &lower[i..window_end];
    if let Some(p) = extract_percent(window) {
        return Some(format!("{p}%"));
    }
    if let Some(reset_at) = window.find("reset") {
        let tail = &window[reset_at..(reset_at + 32).min(window.len())];
        return Some(tail.trim().to_string());
    }
    None
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
