use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
            Status::Processing => "●",
            Status::NeedsInput => "●",
            Status::Idle => "●",
            Status::Unknown => "○",
        }
    }

    /// Ratatui-friendly color for the dot glyph.
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

const TAIL_BUF_BYTES: usize = 500;
const SPINNER_CHARS: &[char] = &[
    '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏',
];
const PROCESSING_RECENT_MS: u128 = 2_000;
const IDLE_QUIET_MS: u128 = 3_000;

pub struct Session {
    pub name: String,
    pub tmux_name: String,
    pub path: String,
    pub status: Status,

    /// vt100 screen used to render the pane. Shared with the reader thread.
    pub screen: Arc<Mutex<vt100::Parser>>,

    /// Rolling tail of recent output for status heuristics.
    tail: Arc<Mutex<VecDeque<u8>>>,
    last_activity: Arc<Mutex<Instant>>,

    pty_master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send + Sync>,

    rows: u16,
    cols: u16,
}

impl Session {
    /// Spawn a new session. `rows`/`cols` are the *pane* size that will render
    /// the terminal — the pty is sized to match so vt100's screen aligns with
    /// the Ratatui area.
    pub fn spawn(name: String, path: String, rows: u16, cols: u16) -> Result<Self> {
        let tmux_name = tmux::full_name(&name);

        // Ensure a tmux session exists so we get persistence / reattach.
        if tmux::is_installed() && !tmux::has_session(&tmux_name) {
            tmux::new_session(&tmux_name, &path)
                .context("creating tmux session for ccm session")?;
        }

        let (cmd_program, cmd_args) = if tmux::is_installed() {
            tmux::attach_command(&tmux_name, &path)
        } else {
            // Fallback: just spawn the user's shell in the target dir.
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            (shell, vec![])
        };

        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(cmd_program);
        for a in cmd_args {
            cmd.arg(a);
        }
        cmd.cwd(&path);
        // Pass through a sensible TERM so ncurses apps render nicely.
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd).context("spawn pty command")?;
        // Drop slave so the child owns the only slave fd; otherwise closing the
        // child won't signal EOF to the reader.
        drop(pair.slave);

        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let tail: Arc<Mutex<VecDeque<u8>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_BUF_BYTES)));
        let last_activity = Arc::new(Mutex::new(Instant::now()));

        let reader = pair
            .master
            .try_clone_reader()
            .context("clone pty reader")?;
        {
            let screen = screen.clone();
            let tail = tail.clone();
            let last_activity = last_activity.clone();
            thread::spawn(move || {
                reader_loop(reader, screen, tail, last_activity);
            });
        }

        let writer = pair
            .master
            .take_writer()
            .context("take pty writer")?;

        Ok(Session {
            name,
            tmux_name,
            path,
            status: Status::Unknown,
            screen,
            tail,
            last_activity,
            pty_master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child,
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

    /// Update `self.status` from the rolling tail + last_activity.
    pub fn recompute_status(&mut self) {
        let now = Instant::now();
        let last = *self.last_activity.lock();
        let since_ms = now.duration_since(last).as_millis();

        // Snapshot the tail as a UTF-8 lossy string — it's at most 500 bytes.
        let tail_bytes: Vec<u8> = self.tail.lock().iter().copied().collect();
        let tail = String::from_utf8_lossy(&tail_bytes);

        // Needs-input patterns win over processing/idle.
        if needs_input(&tail) {
            self.status = Status::NeedsInput;
            return;
        }

        let has_spinner = tail.chars().any(|c| SPINNER_CHARS.contains(&c));
        if has_spinner || since_ms < PROCESSING_RECENT_MS {
            self.status = Status::Processing;
            return;
        }

        if since_ms >= IDLE_QUIET_MS {
            self.status = Status::Idle;
            return;
        }

        // Output within [2s, 3s) ago — keep whatever we last thought.
        if self.status == Status::Unknown {
            self.status = Status::Processing;
        }
    }
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    tail: Arc<Mutex<VecDeque<u8>>>,
    last_activity: Arc<Mutex<Instant>>,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF — child exited.
                return;
            }
            Ok(n) => {
                let chunk = &buf[..n];
                screen.lock().process(chunk);
                {
                    let mut t = tail.lock();
                    for &b in chunk {
                        if t.len() == TAIL_BUF_BYTES {
                            t.pop_front();
                        }
                        t.push_back(b);
                    }
                }
                *last_activity.lock() = Instant::now();
            }
            Err(e) => {
                // EIO / interrupted — bail out.
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // Give the UI a moment before exiting.
                thread::sleep(Duration::from_millis(50));
                return;
            }
        }
    }
}

fn needs_input(tail: &str) -> bool {
    // Lowercase scan for the textual heuristics.
    let lower = tail.to_ascii_lowercase();
    let phrase_hits = [
        "do you want to",
        "allow",
        "(y/n)",
        "[y/n]",
        "press enter",
    ]
    .iter()
    .any(|p| lower.contains(p));

    if phrase_hits {
        return true;
    }

    // Claude Code's input box ends with `│ >` or similar — a bare trailing `>`
    // on the last non-empty line is the documented heuristic.
    let trimmed = tail.trim_end_matches(&['\n', '\r', ' '][..]);
    if let Some(last_line) = trimmed.lines().last() {
        if last_line.trim_end().ends_with('>') {
            return true;
        }
    }
    false
}
