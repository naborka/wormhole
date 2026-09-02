//! Terminal shell around the pure TUI: raw mode, key decoding, redraw.
//! Every decision — what a key means, what the screen shows — lives in
//! `wormhole_core::tui`.

use std::io::{Write, stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use wormhole_core::home::Scan;
use wormhole_core::tui::{Act, Action, Key, Tui};

/// What the user chose in the panel. The terminal is already restored
/// when this returns, so the caller can exec straight into it.
pub enum Pick {
    /// Join this running box (`wormhole attach <id>`).
    Attach(String),
    /// Start this idle box again, in its own workspace.
    Resume(String),
    /// Start another box in the current workspace.
    New,
    Quit,
}

/// Shows the panel until the user quits or picks something to do next.
///
/// `act` is everything the panel does without leaving the screen. One
/// caller rather than one per verb, so a new act is a `tui::Act` variant
/// and nothing here changes shape.
pub fn run(
    mut boxes: impl FnMut() -> Scan,
    mut act: impl FnMut(&Act) -> Result<(), String>,
) -> Result<Pick, String> {
    let _restore = RawScreen::enter()?;
    let mut tui = Tui::default();
    tui.refresh(boxes());
    let mut shown = String::new();
    loop {
        draw(&tui, &mut shown)?;
        // A key, or a second's timeout — only the timeout rescans; a
        // cursor move cannot have changed what is running.
        if !event::poll(Duration::from_secs(1)).map_err(|e| format!("cannot poll keys: {e}"))? {
            tui.refresh(boxes());
            continue;
        }
        let Some(key) = next_key()? else { continue };
        match tui.update(key) {
            Action::Redraw => {}
            Action::Attach(id) => return Ok(Pick::Attach(id)),
            Action::Resume(id) => return Ok(Pick::Resume(id)),
            Action::New => return Ok(Pick::New),
            // The panel owns this terminal, so an act that failed has
            // nowhere else to be said. Handed back to the TUI rather than
            // swallowed, or an act that could not happen looks exactly
            // like one that did.
            Action::Do(what) => {
                let outcome = act(&what);
                tui.refresh(boxes());
                tui.acted(&what, outcome);
            }
            Action::Quit => return Ok(Pick::Quit),
        }
    }
}

/// One raw-mode screen: draws `view`, feeds each key to `on_key`, and
/// returns its first `Some`. The body behind every static panel screen.
fn screen<T>(
    mut view: impl FnMut() -> String,
    mut on_key: impl FnMut(Key) -> Option<T>,
) -> Result<T, String> {
    let _restore = RawScreen::enter()?;
    let mut shown = String::new();
    loop {
        draw_text(&view(), &mut shown)?;
        let Some(key) = next_key()? else { continue };
        if let Some(done) = on_key(key) {
            return Ok(done);
        }
    }
}

/// Shows a list until the user picks an item or backs out.
pub fn choose(items: &[String]) -> Result<Option<usize>, String> {
    let picker = std::cell::RefCell::new(wormhole_core::tui::Picker::new(items.to_vec()));
    screen(
        || picker.borrow().view(),
        |key| match picker.borrow_mut().update(key) {
            wormhole_core::tui::PickAction::Redraw => None,
            wormhole_core::tui::PickAction::Chosen(index) => Some(Some(index)),
            wormhole_core::tui::PickAction::Back => Some(None),
        },
    )
}

/// Shows a text screen until the user confirms (enter) or backs out (q).
pub fn confirm(text: &str) -> Result<bool, String> {
    screen(
        || text.to_owned(),
        |key| match key {
            Key::Enter => Some(true),
            Key::Quit => Some(false),
            _ => None,
        },
    )
}

/// The next decoded key press, `None` for events the panel ignores.
fn next_key() -> Result<Option<Key>, String> {
    let happening = event::read().map_err(|e| format!("cannot read keys: {e}"))?;
    let Event::Key(key) = happening else {
        return Ok(None);
    };
    if key.kind == KeyEventKind::Release {
        return Ok(None);
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(Some(Key::Quit));
    }
    Ok(decode(key.code))
}

/// Only what a terminal decides is decided here. Which *character* means
/// which key lives in `wormhole_core::tui`, beside the hints that name it.
fn decode(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Esc => Some(Key::Quit),
        KeyCode::Char(typed) => Key::from_char(typed),
        _ => None,
    }
}

/// Redraws only when the view changed; an idle list stays flicker-free.
fn draw(tui: &Tui, shown: &mut String) -> Result<(), String> {
    draw_text(&tui.view(crate::now_unix()), shown)
}

fn draw_text(view: &str, shown: &mut String) -> Result<(), String> {
    if view == shown {
        return Ok(());
    }
    let mut out = stdout();
    execute!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))
        .map_err(|e| format!("cannot clear the screen: {e}"))?;
    for line in view.lines() {
        // Raw mode turns off the newline's carriage return.
        write!(out, "{line}\r\n").map_err(|e| format!("cannot draw: {e}"))?;
    }
    out.flush().map_err(|e| format!("cannot draw: {e}"))?;
    *shown = view.to_owned();
    Ok(())
}

/// Raw mode plus the alternate screen, undone on drop — also when the
/// panel dies on an error, so the shell is never left with a broken
/// terminal.
struct RawScreen;

impl RawScreen {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|e| format!("cannot enter raw mode: {e}"))?;
        execute!(stdout(), EnterAlternateScreen, cursor::Hide)
            .map_err(|e| format!("cannot enter the alternate screen: {e}"))?;
        Ok(RawScreen)
    }
}

impl Drop for RawScreen {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
