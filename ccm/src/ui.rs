use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::app::App;

/// Splits the UI into (sidebar, terminal_pane) rects.
pub fn layout(area: Rect, sidebar_width: u16) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(10)])
        .split(area);
    (chunks[0], chunks[1])
}

/// Inner (content) rect of the terminal pane, matching what `draw_terminal`
/// builds. Exposed so main.rs can size the pty to match.
pub fn terminal_inner(area: Rect) -> Rect {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::new(1, 1, 0, 0))
        .inner(area)
}

pub fn draw(frame: &mut Frame, app: &App) {
    let (sidebar, pane) = layout(frame.area(), app.sidebar_width);
    draw_sidebar(frame, app, sidebar);
    draw_terminal(frame, app, pane);

    if let Some(prompt) = &app.prompt {
        draw_prompt(frame, prompt);
    }
    if let Some(err) = &app.last_error {
        draw_toast(frame, err);
    }
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.session_usage {
        Some(u) => format!(" CCM · {u} "),
        None => " CCM ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (pi, proj) in app.projects.iter().enumerate() {
        lines.push(Line::from(vec![Span::styled(
            format!(" {}", proj.name),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(vec![Span::styled(
            pad(&format!(" {}", short_path(&proj.path)), content_width),
            Style::default().fg(Color::DarkGray),
        )]));
        for (wi, win) in proj.windows.iter().enumerate() {
            let selected = app
                .selected
                .map(|s| s.project == pi && s.window == wi)
                .unwrap_or(false);
            let caret = if selected { "▸" } else { " " };
            let name_style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::raw(format!("{caret} ")),
                Span::styled(
                    format!("{} ", win.status.dot()),
                    Style::default().fg(win.status.color()),
                ),
                Span::styled(
                    format!(" {} ", win.name),
                    name_style,
                ),
                Span::styled(
                    format!(" {}", win.index),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            // Metadata line under each window.
            let meta = meta_line(app.verbose, win);
            if !meta.is_empty() {
                lines.push(Line::from(Span::styled(
                    pad(&format!("     {meta}"), content_width),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        " Alt+N project   Alt+A window",
        Style::default().fg(Color::Cyan),
    )));
    lines.push(Line::from(Span::styled(
        " Alt+1-9 switch  Alt+T toggle",
        Style::default().fg(Color::Cyan),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn meta_line(verbose: bool, w: &crate::app::WindowRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = &w.branch {
        parts.push(format!(" {b}"));
    }
    if verbose {
        if let Some(c) = w.context_pct {
            parts.push(format!("ctx {c}%"));
        }
        if let Some(t) = &w.tokens {
            parts.push(format!("{t} tok"));
        }
    }
    parts.join("  ")
}

fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let title = match (app.selected_project(), app.selected_window()) {
        (Some(p), Some(w)) => format!(" {} · {} ", p.name, w.name),
        _ => " no window ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::new(1, 1, 0, 0))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(pane) = app.pane.as_ref() else {
        let hint = Paragraph::new(vec![
            Line::raw(""),
            Line::from(Span::styled(
                "  No window selected.",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from("  Alt+N  new project"),
            Line::from("  Alt+A  new window in project"),
            Line::from("  Alt+J/K  navigate"),
            Line::from("  Alt+H/L  resize sidebar"),
            Line::from("  Ctrl+Q  quit"),
        ]);
        frame.render_widget(hint, inner);
        return;
    };

    let parser = pane.screen.lock();
    let screen = parser.screen();
    let rows = inner.height as usize;
    let cols = inner.width as usize;

    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row as u16, col as u16) else { continue };
            let x = inner.x + col as u16;
            let y = inner.y + row as u16;
            if x >= inner.x + inner.width || y >= inner.y + inner.height {
                continue;
            }
            let buf_cell = &mut frame.buffer_mut()[(x, y)];
            let contents = cell.contents();
            if contents.is_empty() {
                buf_cell.set_char(' ');
            } else {
                buf_cell.set_symbol(&contents);
            }
            let mut style = Style::default()
                .fg(conv_color(cell.fgcolor(), Color::Reset))
                .bg(conv_color(cell.bgcolor(), Color::Reset));
            if cell.bold() { style = style.add_modifier(Modifier::BOLD); }
            if cell.italic() { style = style.add_modifier(Modifier::ITALIC); }
            if cell.underline() { style = style.add_modifier(Modifier::UNDERLINED); }
            if cell.inverse() { style = style.add_modifier(Modifier::REVERSED); }
            buf_cell.set_style(style);
        }
    }

    if !screen.hide_cursor() {
        let (cy, cx) = screen.cursor_position();
        let x = inner.x + cx;
        let y = inner.y + cy;
        if x < inner.x + inner.width && y < inner.y + inner.height {
            frame.set_cursor_position((x, y));
        }
    }
}

fn draw_prompt(frame: &mut Frame, prompt: &crate::app::Prompt) {
    let area = frame.area();
    let width = area.width.clamp(30, 60);
    let height = 3u16;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let rect = Rect { x, y, width, height };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
        prompt.title(),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let text = format!("{}_", prompt.buffer);
    frame.render_widget(Paragraph::new(text), inner);
}

fn draw_toast(frame: &mut Frame, msg: &str) {
    let area = frame.area();
    let width = area.width.min((msg.len() as u16) + 4).max(20);
    let rect = Rect {
        x: area.x + area.width.saturating_sub(width) - 1,
        y: area.y + area.height.saturating_sub(3),
        width,
        height: 3,
    };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
        " error ",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(msg), inner);
}

fn conv_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn pad(s: &str, width: usize) -> String {
    let mut out = s.chars().take(width).collect::<String>();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn short_path(p: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Some(home_str) = home.to_str() {
            if let Some(rest) = p.strip_prefix(home_str) {
                return format!("~{rest}");
            }
        }
    }
    p.to_string()
}
