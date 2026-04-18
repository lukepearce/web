use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Action {
    Quit,
    NextSession,
    PrevSession,
    NewProject,
    NewWindow,
    CloseSession,
    RenameSession,
    Detach,
    ShrinkSidebar,
    GrowSidebar,
    ToggleVerbose,
    SelectWindow(u32),
    PassThrough,
}

pub fn classify(key: KeyEvent) -> Action {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (ctrl, alt, key.code) {
        (true, _, KeyCode::Char('q')) => Action::Quit,
        (_, true, KeyCode::Char('j') | KeyCode::Char('J')) => Action::NextSession,
        (_, true, KeyCode::Char('k') | KeyCode::Char('K')) => Action::PrevSession,
        (_, true, KeyCode::Char('h') | KeyCode::Char('H')) => Action::ShrinkSidebar,
        (_, true, KeyCode::Char('l') | KeyCode::Char('L')) => Action::GrowSidebar,
        (_, true, KeyCode::Char('n') | KeyCode::Char('N')) => Action::NewProject,
        (_, true, KeyCode::Char('a') | KeyCode::Char('A')) => Action::NewWindow,
        (_, true, KeyCode::Char('w') | KeyCode::Char('W')) => Action::CloseSession,
        (_, true, KeyCode::Char('r') | KeyCode::Char('R')) => Action::RenameSession,
        (_, true, KeyCode::Char('t') | KeyCode::Char('T')) => Action::ToggleVerbose,
        (_, true, KeyCode::Char(c @ '0'..='9')) => {
            Action::SelectWindow(c.to_digit(10).unwrap())
        }
        (true, _, KeyCode::Char('d') | KeyCode::Char('D')) => Action::Detach,
        _ => Action::PassThrough,
    }
}
