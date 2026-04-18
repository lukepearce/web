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

type Term = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

fn setup_terminal() -> Result<Term> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste).context("enter alt screen")?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
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

    // Initial pane size based on terminal geometry.
    recompute_pane_size(terminal, &mut app);

    app.refresh_tree();
    app.poll_statuses();
    app.sync_pane();

    let tick = Duration::from_millis(50);
    let mut last_status_poll = Instant::now();
    let mut last_tree_refresh = Instant::now();
    let mut error_shown_at: Option<Instant> = app.last_error.as_ref().map(|_| Instant::now());

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        recompute_pane_size(terminal, &mut app);
        if let Some(pane) = app.pane.as_mut() {
            pane.resize(app.pane_rows, app.pane_cols);
        }

        if event::poll(tick)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Release => {}
                Event::Key(k) => handle_key(&mut app, k)?,
                Event::Paste(text) => handle_paste(&app, &text)?,
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last_status_poll.elapsed() >= Duration::from_millis(800) {
            app.poll_statuses();
            last_status_poll = Instant::now();
        }
        if last_tree_refresh.elapsed() >= Duration::from_secs(3) {
            app.refresh_tree();
            last_tree_refresh = Instant::now();
        }

        app.sync_pane();

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

fn recompute_pane_size(terminal: &Term, app: &mut App) {
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
}

fn handle_paste(app: &App, text: &str) -> Result<()> {
    let Some(pane) = app.pane.as_ref() else { return Ok(()) };
    let mut buf = Vec::with_capacity(text.len() + 12);
    buf.extend_from_slice(b"\x1b[200~");
    buf.extend_from_slice(text.as_bytes());
    buf.extend_from_slice(b"\x1b[201~");
    pane.write(&buf)?;
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.prompt.is_some() {
        handle_prompt_key(app, key);
        return Ok(());
    }

    match keys::classify(key) {
        Action::Quit => app.should_quit = true,
        Action::NextSession => app.next(),
        Action::PrevSession => app.prev(),
        Action::ShrinkSidebar => app.shrink_sidebar(),
        Action::GrowSidebar => app.grow_sidebar(),
        Action::NewProject => {
            app.prompt = Some(Prompt {
                kind: PromptKind::NewProject,
                buffer: String::new(),
                stage: PromptStage::First,
                stash: None,
            });
        }
        Action::NewWindow => {
            if app.selected_project().is_some() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::NewWindow,
                    buffer: "claude".into(),
                    stage: PromptStage::First,
                    stash: None,
                });
            }
        }
        Action::CloseSession => {
            if app.selected_window().is_some() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::ConfirmCloseWindow,
                    buffer: String::new(),
                    stage: PromptStage::First,
                    stash: None,
                });
            }
        }
        Action::RenameSession => {
            if let Some(w) = app.selected_window() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::Rename,
                    buffer: w.name.clone(),
                    stage: PromptStage::First,
                    stash: None,
                });
            }
        }
        Action::Detach => app.should_quit = true,
        Action::PassThrough => {
            if let Some(bytes) = encode_key(key) {
                if let Some(pane) = app.pane.as_ref() {
                    if let Err(e) = pane.write(&bytes) {
                        app.last_error = Some(format!("pty write: {e}"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn handle_prompt_key(app: &mut App, key: KeyEvent) {
    let Some(prompt) = app.prompt.as_mut() else { return };

    match key.code {
        KeyCode::Esc => app.prompt = None,
        KeyCode::Backspace => {
            prompt.buffer.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            prompt.buffer.push(c);
        }
        KeyCode::Enter => submit_prompt(app),
        _ => {}
    }
}

fn submit_prompt(app: &mut App) {
    let Some(prompt) = app.prompt.take() else { return };

    match prompt.kind {
        PromptKind::NewProject => match prompt.stage {
            PromptStage::First => {
                let default_path = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "~".into());
                app.prompt = Some(Prompt {
                    kind: PromptKind::NewProject,
                    buffer: default_path,
                    stage: PromptStage::Second,
                    stash: Some(prompt.buffer),
                });
            }
            PromptStage::Second => {
                let name = prompt.stash.unwrap_or_default();
                let path = prompt.buffer;
                if let Err(e) = app.create_project(name, path) {
                    app.last_error = Some(format!("new project: {e}"));
                }
            }
        },
        PromptKind::NewWindow => {
            if let Err(e) = app.create_window(prompt.buffer) {
                app.last_error = Some(format!("new window: {e}"));
            }
        }
        PromptKind::Rename => {
            if let Err(e) = app.rename_selected_window(prompt.buffer) {
                app.last_error = Some(format!("rename: {e}"));
            }
        }
        PromptKind::ConfirmCloseWindow => {
            if matches!(prompt.buffer.trim(), "y" | "Y" | "yes") {
                app.close_selected_window();
            }
        }
    }
}

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

    if out.is_empty() { None } else { Some(out) }
}

fn arrow_seq(letter: u8, ctrl: bool, shift: bool) -> &'static [u8] {
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
