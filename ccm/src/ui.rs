use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub const SIDEBAR_WIDTH: u16 = 22;

/// Splits the UI into (sidebar, terminal_pane) rects.
pub fn layout(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(10)])
        .split(area);
    (chunks[0], chunks[1])
}

pub fn draw(frame: &mut Frame, app: &App) {
    let (sidebar, pane) = layout(frame.area());
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " CCM ",
            Style::default().add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Each session entry is two lines: "dot name" then a dim short path.
    let mut lines: Vec<Line> = Vec::with_capacity(app.sessions.len() * 3 + 1);
    for (i, s) in app.sessions.iter().enumerate() {
        let selected = i == app.selected;
        let name_style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        let path_style = if selected {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", s.status.dot()),
                Style::default().fg(s.status.color()),
            ),
            Span::styled(pad(&s.name, (SIDEBAR_WIDTH as usize).saturating_sub(5)), name_style),
        ]));
        lines.push(Line::from(vec![Span::styled(
            pad(&format!("    {}", short_path(&s.path)), SIDEBAR_WIDTH as usize - 2),
            path_style,
        )]));
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled(
        " [Alt+N] new session ",
        Style::default().fg(Color::Cyan),
    )));

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}

fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let title = match app.active() {
        Some(s) => format!(" {} — {} ", s.name, s.path),
        None => " no session — Alt+N to create one ".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(session) = app.active() else {
        let hint = Paragraph::new(vec![
            Line::raw(""),
            Line::from(Span::styled(
                "  No sessions yet.",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from("  Press Alt+N to create one."),
            Line::from("  Press Ctrl+Q to quit."),
        ]);
        frame.render_widget(hint, inner);
        return;
    };

    // Render the vt100 screen directly into the inner rect.
    let parser = session.screen.lock();
    let screen = parser.screen();
    let rows = inner.height as usize;
    let cols = inner.width as usize;

    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row as u16, col as u16) else {
                continue;
            };
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
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            buf_cell.set_style(style);
        }
    }

    // Place the ratatui cursor where vt100 thinks it is.
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
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
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
