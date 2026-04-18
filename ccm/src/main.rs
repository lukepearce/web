mod app;
mod keys;
mod session;
mod tmux;
mod ui;

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crate::app::{App, Prompt, PromptKind, PromptStage};
use crate::keys::Action;

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

type Term = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> Result<Term> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("create terminal")?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Term) -> Result<()> {
    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )
    .ok();
    terminal.show_cursor().ok();
    Ok(())
}

fn run(terminal: &mut Term) -> Result<()> {
    let mut app = App::new()?;

    // Figure out initial pane size from the terminal so newly-bootstrapped
    // sessions open with a correct pty size.
    let size = terminal.size().unwrap_or_default();
    let initial_rect = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let (_, pane) = ui::layout(initial_rect, app.sidebar_width);
    let inner = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .inner(pane);
    app.pane_rows = inner.height.max(1);
    app.pane_cols = inner.width.max(1);

    if let Err(e) = app.bootstrap() {
        app.last_error = Some(format!("bootstrap: {e}"));
    }

    let tick = Duration::from_millis(50);
    let mut last_status = Instant::now();
    let mut error_shown_at: Option<Instant> = if app.last_error.is_some() {
        Some(Instant::now())
    } else {
        None
    };

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Sync active session pty size with the rendered inner rect.
        let size = terminal.size().unwrap_or_default();
        let rect = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        let (_, pane) = ui::layout(rect, app.sidebar_width);
        let inner = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .inner(pane);
        app.pane_rows = inner.height.max(1);
        app.pane_cols = inner.width.max(1);
        for s in &mut app.sessions {
            s.resize(app.pane_rows, app.pane_cols);
        }

        // Poll for events, but don't block longer than one tick so status dots
        // and spinner animation stay fresh.
        if event::poll(tick)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Release => {}
                Event::Key(k) => handle_key(&mut app, k)?,
                Event::Paste(text) => handle_paste(&app, &text)?,
                Event::Resize(_, _) => { /* handled on next draw */ }
                _ => {}
            }
        }

        if last_status.elapsed() >= Duration::from_millis(200) {
            app.update_statuses();
            last_status = Instant::now();
        }

        // Clear transient error toast after 4s.
        if let Some(t) = error_shown_at {
            if t.elapsed() >= Duration::from_secs(4) {
                app.last_error = None;
                error_shown_at = None;
            }
        } else if app.last_error.is_some() {
            error_shown_at = Some(Instant::now());
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_paste(app: &App, text: &str) -> Result<()> {
    let Some(s) = app.active() else {
        return Ok(());
    };
    // Wrap with bracketed paste markers so the target app (shell / Claude
    // Code) knows this is pasted content.
    let mut buf = Vec::with_capacity(text.len() + 12);
    buf.extend_from_slice(b"\x1b[200~");
    buf.extend_from_slice(text.as_bytes());
    buf.extend_from_slice(b"\x1b[201~");
    s.write(&buf)?;
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Prompt overrides normal key handling.
    if app.prompt.is_some() {
        handle_prompt_key(app, key);
        return Ok(());
    }

    match keys::classify(key) {
        Action::Quit => app.should_quit = true,
        Action::NextSession => app.next(),
        Action::PrevSession => app.prev(),
        Action::NewSession => {
            app.prompt = Some(Prompt {
                kind: PromptKind::NewSession,
                buffer: String::new(),
                stage: PromptStage::First,
                stash: None,
            });
        }
        Action::CloseSession => {
            if app.active().is_some() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::ConfirmClose,
                    buffer: String::new(),
                    stage: PromptStage::First,
                    stash: None,
                });
            }
        }
        Action::RenameSession => {
            if let Some(s) = app.active() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::Rename,
                    buffer: s.name.clone(),
                    stage: PromptStage::First,
                    stash: None,
                });
            }
        }
        Action::ShrinkSidebar => app.shrink_sidebar(),
        Action::GrowSidebar => app.grow_sidebar(),
        Action::Detach => {
            // Drop out of the TUI — sessions persist in tmux. A later ccm
            // launch will pick them up.
            app.should_quit = true;
        }
        Action::PassThrough => {
            if let Some(bytes) = encode_key(key) {
                if let Some(active) = app.active() {
                    if let Err(e) = active.write(&bytes) {
                        app.last_error = Some(format!("pty write: {e}"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn handle_prompt_key(app: &mut App, key: KeyEvent) {
    let Some(prompt) = app.prompt.as_mut() else {
        return;
    };

    match key.code {
        KeyCode::Esc => {
            app.prompt = None;
        }
        KeyCode::Backspace => {
            prompt.buffer.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            prompt.buffer.push(c);
        }
        KeyCode::Enter => {
            submit_prompt(app);
        }
        _ => {}
    }
}

fn submit_prompt(app: &mut App) {
    let Some(prompt) = app.prompt.take() else {
        return;
    };

    match prompt.kind {
        PromptKind::NewSession => match prompt.stage {
            PromptStage::First => {
                // Ask for path next.
                let default_path = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "~".into());
                app.prompt = Some(Prompt {
                    kind: PromptKind::NewSession,
                    buffer: default_path,
                    stage: PromptStage::Second,
                    stash: Some(prompt.buffer),
                });
            }
            PromptStage::Second => {
                let name = prompt.stash.unwrap_or_default();
                let path = prompt.buffer;
                if let Err(e) = app.create_session(name, path) {
                    app.last_error = Some(format!("new session: {e}"));
                }
            }
        },
        PromptKind::Rename => {
            if let Err(e) = app.rename_active(prompt.buffer) {
                app.last_error = Some(format!("rename: {e}"));
            }
        }
        PromptKind::ConfirmClose => {
            let yes = matches!(prompt.buffer.trim(), "y" | "Y" | "yes");
            if yes {
                app.close_active();
            }
        }
    }
}

/// Translate a crossterm KeyEvent into the byte sequence a pty expects.
/// Covers the common cases; unknown combos are dropped.
fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    let mut out: Vec<u8> = Vec::new();
    if alt {
        out.push(0x1b);
    }

    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Map Ctrl+letter to its control code; Ctrl+[space/\]/^/_ too.
                let code = match c {
                    '@' | ' ' => 0x00,
                    c @ 'a'..='z' => (c as u8) - b'a' + 1,
                    c @ 'A'..='Z' => (c as u8) - b'A' + 1,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '^' => 0x1e,
                    '_' | '?' => 0x1f,
                    _ => return None,
                };
                out.push(code);
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => out.push(0x7f),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Left => out.extend_from_slice(arrow_seq(b'D', ctrl, shift)),
        KeyCode::Right => out.extend_from_slice(arrow_seq(b'C', ctrl, shift)),
        KeyCode::Up => out.extend_from_slice(arrow_seq(b'A', ctrl, shift)),
        KeyCode::Down => out.extend_from_slice(arrow_seq(b'B', ctrl, shift)),
        KeyCode::Home => out.extend_from_slice(b"\x1b[H"),
        KeyCode::End => out.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(n) => {
            let seq: &[u8] = match n {
                1 => b"\x1bOP",
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                12 => b"\x1b[24~",
                _ => return None,
            };
            out.extend_from_slice(seq);
        }
        _ => return None,
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn arrow_seq(letter: u8, ctrl: bool, shift: bool) -> &'static [u8] {
    // CSI 1 ; <mod> <letter> for modified arrows.
    match (ctrl, shift) {
        (false, false) => match letter {
            b'A' => b"\x1b[A",
            b'B' => b"\x1b[B",
            b'C' => b"\x1b[C",
            b'D' => b"\x1b[D",
            _ => b"",
        },
        (true, false) => match letter {
            b'A' => b"\x1b[1;5A",
            b'B' => b"\x1b[1;5B",
            b'C' => b"\x1b[1;5C",
            b'D' => b"\x1b[1;5D",
            _ => b"",
        },
        (false, true) => match letter {
            b'A' => b"\x1b[1;2A",
            b'B' => b"\x1b[1;2B",
            b'C' => b"\x1b[1;2C",
            b'D' => b"\x1b[1;2D",
            _ => b"",
        },
        (true, true) => match letter {
            b'A' => b"\x1b[1;6A",
            b'B' => b"\x1b[1;6B",
            b'C' => b"\x1b[1;6C",
            b'D' => b"\x1b[1;6D",
            _ => b"",
        },
    }
}
