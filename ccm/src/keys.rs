use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Action {
    Quit,
    NextSession,
    PrevSession,
    NewSession,
    CloseSession,
    RenameSession,
    Detach,
    PassThrough,
}

pub fn classify(key: KeyEvent) -> Action {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (ctrl, alt, key.code) {
        (true, _, KeyCode::Char('q')) => Action::Quit,
        (_, true, KeyCode::Char('j') | KeyCode::Char('J')) => Action::NextSession,
        (_, true, KeyCode::Char('k') | KeyCode::Char('K')) => Action::PrevSession,
        (_, true, KeyCode::Char('n') | KeyCode::Char('N')) => Action::NewSession,
        (_, true, KeyCode::Char('w') | KeyCode::Char('W')) => Action::CloseSession,
        (_, true, KeyCode::Char('r') | KeyCode::Char('R')) => Action::RenameSession,
        (true, _, KeyCode::Char('d') | KeyCode::Char('D')) => Action::Detach,
        _ => Action::PassThrough,
    }
}
