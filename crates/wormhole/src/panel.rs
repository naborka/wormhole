//! Terminal shell around the pure TUI: raw mode, key decoding, drawing.
//!
//! Every decision — what a key means, what is on the screen, how much of
//! it fits the terminal — lives in `wormhole_core::tui`. This file turns
//! a `Screen` into cells and reads keys back, and decides nothing: the
//! widths, the window, the cuts and the words all arrive made.

use std::io::{Stdout, stdout};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as Cells, Table, TableState};
use ratatui::{Terminal, TerminalOptions};
use wormhole_core::home::Scan;
use wormhole_core::tui::{
    self, Act, Action, Hint, Key, Level, Page, Press, Screen, Tone, Tui, Viewport,
};

/// What the user chose in the panel. The terminal is already restored
/// when this returns, so the caller can run the session on it and come
/// back.
pub enum Pick {
    /// Join this running box (`wormhole attach <id>`).
    Attach(String),
    /// Ask where to start this idle box, then start it there.
    Workspaces(String),
    /// Start another box in the current workspace.
    New,
    Quit,
}

/// Everything the panel needs from the host it is running on.
///
/// One interface rather than a closure per verb: the panel gained four
/// tabs and the argument list was becoming the interface without being
/// written down as one. Every method here is I/O, which is why none of
/// it is in `wormhole-core`.
pub trait Host {
    /// Every box this host keeps, and what the scan could not read.
    fn boxes(&mut self) -> Scan;
    /// The rows of a tab, gathered when it is opened. `widened` is the
    /// store tab's view state, which only matters to what is gathered.
    fn gather(&mut self, tab: tui::Tab, widened: bool) -> tui::Sheet;
    /// Something done without leaving the screen, reported back.
    fn act(&mut self, what: &Act) -> Result<(), String>;
    /// The whole recipe a role would run, or why it cannot be read.
    fn recipe(&mut self, name: &str) -> Result<String, String>;
    /// Installs a role by the reference typed, or says why not.
    fn install(&mut self, reference: &str) -> Result<(), String>;
    /// Runs one of this tool's own commands and hands back what it
    /// printed, so the palette runs the line it showed.
    fn run(&mut self, argv: &[String]) -> Result<String, String>;
}

pub fn run(tui: &mut Tui, host: &mut dyn Host) -> Result<Pick, String> {
    let mut raw = Raw::enter()?;
    let scan = host.boxes();
    tui.refresh(scan);
    loop {
        let viewport = raw.viewport();
        let screen = tui.screen(viewport, crate::now_unix(), &|path: &Path| path.is_dir());
        raw.draw(&screen)?;
        // A key, or a second's timeout — only the timeout rescans, and
        // only the box list: a cursor move cannot have changed what is
        // running, and walking the store or probing the host once a
        // second would be its own defect.
        if !event::poll(Duration::from_secs(1)).map_err(|e| format!("cannot poll keys: {e}"))? {
            let scan = host.boxes();
            tui.refresh(scan);
            continue;
        }
        let press = match next_input()? {
            // The draw at the top of the loop is the redraw, and ratatui
            // has resized its own buffers by then. Without this the panel
            // kept whatever the old geometry left on the screen until
            // something else changed the text.
            Input::Resized | Input::Ignored => continue,
            Input::Press(press) => press,
        };
        let action = tui.update(press, viewport);
        if let Some(pick) = perform(&mut raw, tui, host, action)? {
            return Ok(pick);
        }
    }
}

/// Does one thing the screen asked for, or says which way to leave.
///
/// One body, because a palette entry is the same act a key is: the list
/// of commands would otherwise be a second, drifting way to do what the
/// keys already do.
fn perform(
    raw: &mut Raw,
    tui: &mut Tui,
    host: &mut dyn Host,
    action: Action,
) -> Result<Option<Pick>, String> {
    match action {
        Action::Redraw => {}
        Action::Attach(id) => return Ok(Some(Pick::Attach(id))),
        Action::Workspaces(id) => return Ok(Some(Pick::Workspaces(id))),
        Action::New => return Ok(Some(Pick::New)),
        // The panel owns this terminal, so an act that failed has
        // nowhere else to be said. Handed back to the TUI rather than
        // swallowed, or an act that could not happen looks exactly
        // like one that did.
        Action::Do(what) => {
            let outcome = host.act(&what);
            let scan = host.boxes();
            tui.refresh(scan);
            // An act on another tab changed what that tab shows, so
            // its rows are gathered again rather than left stale.
            if tui.tab() != tui::Tab::Boxes {
                let sheet = host.gather(tui.tab(), tui.widened());
                tui.show(tui.tab(), sheet);
            }
            tui.acted(&what, outcome);
        }
        // Drawn on the terminal this loop already holds, rather than
        // on one of its own: leaving the alternate screen and coming
        // straight back flashes the shell in between.
        Action::Read(text) => read_on(raw, &text, tui::Reading::Message).map(|_| ())?,
        Action::Gather(tab) => {
            let sheet = host.gather(tab, tui.widened());
            tui.show(tab, sheet);
        }
        // The recipe a role would run, on the same screen `role
        // show` prints and the approval screen draws.
        Action::Show(name) => {
            let text = host.recipe(&name).unwrap_or_else(|why| why);
            read_on(raw, &text, tui::Reading::Message)?;
        }
        Action::Install => {
            let mut asking = tui::Ask::new("install the role in which directory?");
            while let Some(reference) = ask_on(raw, &mut asking)? {
                match host.install(&reference) {
                    Err(why) => asking.refuse(why),
                    Ok(()) => {
                        let sheet = host.gather(tui::Tab::Roles, tui.widened());
                        tui.show(tui::Tab::Roles, sheet);
                        break;
                    }
                }
            }
        }
        Action::Keep => {
            let mut asking = tui::Ask::new("keep a value for which variable?");
            let Some(name) = ask_on(raw, &mut asking)? else {
                return Ok(None);
            };
            let mut value = tui::Ask::new(format!("{name}, kept for every box:")).masked();
            if let Some(value) = ask_on(raw, &mut value)? {
                let what = Act::Keep(name, value);
                let outcome = host.act(&what);
                let sheet = host.gather(tui::Tab::Secrets, tui.widened());
                tui.show(tui::Tab::Secrets, sheet);
                tui.acted(&what, outcome);
            }
        }
        Action::Name(id) => {
            let mut ask = tui::Ask::new(format!("name box {id}:"));
            let outcome = loop {
                let Some(name) = ask_on(raw, &mut ask)? else {
                    break None;
                };
                let what = Act::Rename(id.clone(), name);
                match host.act(&what) {
                    // Kept on the screen with the reason, because a
                    // name refused for one character is not a reason
                    // to type the whole thing again.
                    Err(why) => ask.refuse(why),
                    Ok(()) => break Some(what),
                }
            };
            let scan = host.boxes();
            tui.refresh(scan);
            if let Some(what) = outcome {
                tui.acted(&what, Ok(()));
            }
        }
        Action::Quit => return Ok(Some(Pick::Quit)),
        Action::Goto(tab) => {
            let next = tui.goto(tab);
            return perform(raw, tui, host, next);
        }
        // Run as itself, captured, and read. What the palette shows is
        // the line, so running anything else would make the line a lie.
        Action::Run(argv) => {
            let text = host.run(&argv).unwrap_or_else(|why| why);
            read_on(raw, &text, tui::Reading::Message)?;
        }
        Action::Commands(entries) => {
            let width = usize::from(raw.viewport().width);
            let items: Vec<String> = entries.iter().map(|entry| entry.line(width)).collect();
            let mut picker = tui::Picker::with_heading(tui::COMMANDS_HEADING, items);
            if let Some(at) = choose_on(raw, &mut picker)? {
                // The same body a key goes through, so a command run
                // from the list and the key that runs it cannot differ.
                return perform(raw, tui, host, entries[at].action.clone());
            }
        }
    }
    Ok(None)
}

/// Shows a list until the user picks an item or backs out.
pub fn choose(items: &[String]) -> Result<Option<usize>, String> {
    choose_from("start a new box from:", items)
}

/// Same list, with a heading that says what is being picked.
pub fn choose_from(heading: &'static str, items: &[String]) -> Result<Option<usize>, String> {
    choose_at(heading, items, 0)
}

/// The same list, with the cursor already on the item a caller that
/// asked nobody would have taken — so agreeing with the screen costs one
/// key rather than a search down the list.
pub fn choose_at(
    heading: &'static str,
    items: &[String],
    at: usize,
) -> Result<Option<usize>, String> {
    let mut picker = tui::Picker::with_heading(heading, items.to_vec()).starting_at(at);
    let mut raw = Raw::enter()?;
    choose_on(&mut raw, &mut picker)
}

/// Shows what a start can be told, and returns the arguments it became.
pub fn fill(mut form: tui::Form) -> Result<Option<Vec<String>>, String> {
    let mut raw = Raw::enter()?;
    loop {
        let screen = form.screen(raw.viewport());
        raw.draw(&screen)?;
        let Input::Press(press) = next_input()? else {
            continue;
        };
        match form.update(press, raw.viewport()) {
            tui::FormAction::Redraw => {}
            tui::FormAction::Start(argv) => return Ok(Some(argv)),
            tui::FormAction::Back => return Ok(None),
        }
    }
}

/// The loops behind the screens above, on a terminal somebody else is
/// holding. Every screen the panel opens over itself goes through these,
/// so a nested screen never re-enters the alternate screen — which
/// flashes the shell on the way in and on the way out.
fn choose_on(raw: &mut Raw, picker: &mut tui::Picker) -> Result<Option<usize>, String> {
    loop {
        let screen = picker.screen(raw.viewport());
        raw.draw(&screen)?;
        let Input::Press(press) = next_input()? else {
            continue;
        };
        match picker.update(press, raw.viewport()) {
            tui::PickAction::Redraw => {}
            tui::PickAction::Chosen(index) => return Ok(Some(index)),
            tui::PickAction::Back => return Ok(None),
            tui::PickAction::Keys => {
                read_on(raw, &tui::keys_page(tui::on::PICK), tui::Reading::Message)?;
            }
        }
    }
}

fn ask_on(raw: &mut Raw, asking: &mut tui::Ask) -> Result<Option<String>, String> {
    loop {
        let screen = asking.screen(raw.viewport());
        raw.draw(&screen)?;
        let Input::Press(press) = next_input()? else {
            continue;
        };
        match asking.update(press) {
            tui::AskAction::Redraw => {}
            tui::AskAction::Given(value) => return Ok(Some(value)),
            tui::AskAction::Back => return Ok(None),
        }
    }
}

/// Shows a block of text until the user confirms (enter) or backs out
/// (q), scrolling it with the same keys the lists use.
///
/// Scrolling is the point rather than a comfort: the permission preview
/// runs past forty lines, and before this the end of it — the `[image]
/// build` lines, which are the part that executes — was drawn below the
/// bottom of the terminal and could not be reached.
pub fn confirm(text: &str) -> Result<bool, String> {
    read(text, tui::Reading::Confirm)
}

/// A line under whatever the terminal already shows, and a wait for any
/// key — for a session that ended badly, whose words would otherwise be
/// covered by the panel the moment it came back. No alternate screen, so
/// nothing is hidden. Errors on the way are said and swallowed: there is
/// no better place for them than this terminal.
pub fn pause(text: &str) {
    use std::io::Write as _;
    let mut out = stdout();
    let _ = writeln!(out, "{text}. any key for the panel");
    let _ = out.flush();
    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("cannot wait for a key: {e}");
        return;
    }
    loop {
        match next_input() {
            Ok(Input::Press(_)) | Err(_) => break,
            Ok(Input::Resized | Input::Ignored) => {}
        }
    }
    let _ = terminal::disable_raw_mode();
}

/// The same screen, for something there is nothing to agree to — a
/// refusal the panel has to hand back rather than print, because it owns
/// the terminal and anything written there draws over the box list.
///
/// Offers no `enter`: a key on the screen that does nothing is a screen
/// that lies about what can be done on it.
pub fn message(text: &str) -> Result<(), String> {
    read(text, tui::Reading::Message).map(|_| ())
}

fn read(text: &str, reading: tui::Reading) -> Result<bool, String> {
    let mut raw = Raw::enter()?;
    read_on(&mut raw, text, reading)
}

fn read_on(raw: &mut Raw, text: &str, reading: tui::Reading) -> Result<bool, String> {
    let kind = reading.keys();
    let mut at = 0usize;
    loop {
        let viewport = raw.viewport();
        let page = tui::page(text, viewport, at, reading);
        raw.read(&page)?;
        let Input::Press(press) = next_input()? else {
            continue;
        };
        match tui::command(press, kind) {
            Some(Key::Enter) if reading == tui::Reading::Confirm => return Ok(true),
            Some(Key::Quit) => return Ok(false),
            // The keys of this screen, then back to what was being read.
            Some(Key::Help) => {
                read_on(raw, &tui::keys_page(kind), tui::Reading::Message)?;
            }
            _ => at = tui::scroll_to(text, viewport, at, press),
        }
    }
}

/// What came back from the terminal, once the things only a terminal can
/// say are separated from what the screens decide about.
enum Input {
    Press(Press),
    Resized,
    Ignored,
}

/// The next event, reported rather than interpreted.
///
/// This file no longer knows that `w` is the workspace key: it says a `w`
/// was typed, and the screen says whether that is a key or a letter in a
/// filter. `Ctrl-C` is the one exception, because leaving is the one
/// thing a terminal owes whatever is drawn on it.
fn next_input() -> Result<Input, String> {
    let happening = event::read().map_err(|e| format!("cannot read keys: {e}"))?;
    let key = match happening {
        Event::Resize(_, _) => return Ok(Input::Resized),
        Event::Key(key) => key,
        _ => return Ok(Input::Ignored),
    };
    if key.kind == KeyEventKind::Release {
        return Ok(Input::Ignored);
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(Input::Press(Press::Typed('q')));
    }
    Ok(match key.code {
        KeyCode::Up => Input::Press(Press::Up),
        KeyCode::Down => Input::Press(Press::Down),
        KeyCode::Home => Input::Press(Press::Home),
        KeyCode::End => Input::Press(Press::End),
        KeyCode::PageUp => Input::Press(Press::PageUp),
        KeyCode::PageDown => Input::Press(Press::PageDown),
        KeyCode::Enter => Input::Press(Press::Enter),
        KeyCode::Esc => Input::Press(Press::Escape),
        KeyCode::Backspace => Input::Press(Press::Backspace),
        KeyCode::Tab => Input::Press(Press::Tab),
        KeyCode::Char(typed) => Input::Press(Press::Typed(typed)),
        _ => Input::Ignored,
    })
}

/// Raw mode, the alternate screen and the drawing surface, undone on drop
/// — also when a screen dies on an error, so the shell is never left with
/// a broken terminal.
struct Raw {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Raw {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|e| format!("cannot enter raw mode: {e}"))?;
        execute!(stdout(), EnterAlternateScreen, cursor::Hide)
            .map_err(|e| format!("cannot enter the alternate screen: {e}"))?;
        let backend = CrosstermBackend::new(stdout());
        // A pty opened without a window size reports zero by zero, and a
        // drawing surface of nothing draws nothing — a blank panel, for
        // as long as it is up. So a terminal that cannot say how big it
        // is is drawn at the fallback size instead: fixed, because a
        // backend that cannot measure itself cannot be resized against
        // either.
        let measured = terminal::size()
            .ok()
            .filter(|(width, height)| *width > 0 && *height > 0);
        let terminal = match measured {
            Some(_) => Terminal::new(backend),
            None => Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: ratatui::Viewport::Fixed(Rect::new(
                        0,
                        0,
                        Viewport::FALLBACK.width,
                        Viewport::FALLBACK.height,
                    )),
                },
            ),
        }
        .map_err(|e| format!("cannot take the terminal: {e}"))?;
        Ok(Raw { terminal })
    }

    /// How big this terminal is, or what a terminal that cannot be
    /// measured is taken to be — a size of zero draws nothing at all,
    /// which is what a pty opened without one reports.
    fn viewport(&self) -> Viewport {
        self.terminal
            .size()
            .map_or(Viewport::FALLBACK, |size| Viewport {
                width: size.width,
                height: size.height,
            })
            .or_fallback()
    }

    fn draw(&mut self, screen: &Screen) -> Result<(), String> {
        self.terminal
            .draw(|frame| render(frame, screen))
            .map(|_| ())
            .map_err(|e| format!("cannot draw: {e}"))
    }

    fn read(&mut self, page: &Page) -> Result<(), String> {
        self.terminal
            .draw(|frame| render_page(frame, page))
            .map(|_| ())
            .map_err(|e| format!("cannot draw: {e}"))
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

/// Whether this terminal is to be coloured at all.
///
/// `NO_COLOR` is the convention, and a tool whose output people pipe and
/// paste has no business ignoring it. Emphasis survives without colour:
/// every tone is also said in words.
fn coloured() -> bool {
    static COLOURED: OnceLock<bool> = OnceLock::new();
    *COLOURED.get_or_init(|| std::env::var_os("NO_COLOR").is_none_or(|set| set.is_empty()))
}

fn tone(tone: Tone) -> Style {
    if !coloured() {
        return match tone {
            Tone::Dim => Style::new().add_modifier(Modifier::DIM),
            Tone::Bad => Style::new().add_modifier(Modifier::BOLD),
            _ => Style::new(),
        };
    }
    match tone {
        Tone::Plain => Style::new(),
        Tone::Dim => Style::new().fg(Color::DarkGray),
        Tone::Good => Style::new().fg(Color::Green),
        Tone::Bad => Style::new().fg(Color::Red),
    }
}

fn level(level: Level) -> Style {
    match level {
        Level::Info => tone(Tone::Good),
        Level::Refusal => tone(Tone::Bad),
        Level::Question => Style::new().add_modifier(Modifier::BOLD),
    }
}

fn lines(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

/// Draws a screen, in the order and at the sizes it already decided.
fn render(frame: &mut ratatui::Frame, screen: &Screen) {
    let table = if screen.rows.is_empty() {
        u16::from(screen.empty.is_some())
    } else {
        u16::from(screen.has_header()) + lines(screen.rows.len())
    };
    let note = screen
        .note
        .as_ref()
        .map_or(0, |note| lines(note.lines.len()));
    // The hints and the note sit at the bottom, so a list that grows and
    // shrinks never moves them.
    let [
        heading,
        rows,
        scroll,
        summary,
        detail,
        notices,
        _gap,
        said,
        hints,
    ] = Layout::vertical([
        Constraint::Length(u16::from(screen.heading.is_some()) * 2),
        Constraint::Length(table),
        Constraint::Length(u16::from(screen.scroll.is_some())),
        Constraint::Length(u16::from(screen.summary.is_some())),
        Constraint::Length(lines(screen.detail.len())),
        Constraint::Length(lines(screen.notices.len())),
        Constraint::Min(0),
        Constraint::Length(note),
        Constraint::Length(lines(screen.hints.len())),
    ])
    .areas(frame.area());

    if let Some(title) = &screen.heading {
        frame.render_widget(
            Paragraph::new(title.as_str()).style(Style::new().add_modifier(Modifier::BOLD)),
            heading,
        );
    }
    if let Some(nothing) = &screen.empty {
        frame.render_widget(
            Paragraph::new(nothing.as_str()).style(tone(Tone::Dim)),
            rows,
        );
    } else {
        render_table(frame, screen, rows);
    }
    if let Some(more) = &screen.scroll {
        frame.render_widget(Paragraph::new(more.label()).style(tone(Tone::Dim)), scroll);
    }
    if let Some(said) = &screen.summary {
        frame.render_widget(Paragraph::new(said.clone()), summary);
    }
    frame.render_widget(
        Paragraph::new(
            screen
                .detail
                .iter()
                .map(|field| Line::styled(field.text.clone(), tone(field.tone)))
                .collect::<Vec<_>>(),
        ),
        detail,
    );
    frame.render_widget(
        Paragraph::new(
            screen
                .notices
                .iter()
                .map(|notice| Line::styled(notice.clone(), tone(Tone::Bad)))
                .collect::<Vec<_>>(),
        ),
        notices,
    );
    if let Some(note) = &screen.note {
        frame.render_widget(
            Paragraph::new(
                note.lines
                    .iter()
                    .map(|line| Line::styled(line.clone(), level(note.level)))
                    .collect::<Vec<_>>(),
            ),
            said,
        );
    }
    frame.render_widget(Paragraph::new(hint_lines(&screen.hints, None)), hints);
}

fn render_table(frame: &mut ratatui::Frame, screen: &Screen, area: Rect) {
    let widths: Vec<Constraint> = screen
        .columns
        .iter()
        .map(|column| Constraint::Length(column.width))
        .collect();
    let mut table = Table::new(
        screen
            .rows
            .iter()
            .map(|row| Cells::new(row.cells.clone()).style(tone(row.tone))),
        widths,
    )
    .column_spacing(2)
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("> ");
    if screen.has_header() {
        table = table.header(
            Cells::new(
                screen
                    .columns
                    .iter()
                    .map(|column| column.title.clone())
                    .collect::<Vec<_>>(),
            )
            .style(Style::new().add_modifier(Modifier::BOLD)),
        );
    }
    let mut state = TableState::new().with_selected(screen.cursor);
    frame.render_stateful_widget(table, area, &mut state);
}

/// The hints, with each keycap picked out from what it does. One body,
/// because two spellings of a footer drift the moment either grows.
fn hint_lines(hints: &[Vec<Hint>], trailing: Option<Span<'static>>) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = hints
        .iter()
        .map(|line| {
            let mut spans = Vec::new();
            for hint in line {
                if !spans.is_empty() {
                    spans.push(Span::raw(" ".repeat(tui::GAP)));
                }
                spans.push(Span::styled(
                    hint.cap,
                    Style::new().add_modifier(Modifier::BOLD),
                ));
                if !hint.does.is_empty() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::raw(hint.does));
                }
            }
            Line::from(spans)
        })
        .collect();
    if let Some(said) = trailing
        && let Some(last) = lines.last_mut()
    {
        last.push_span(Span::raw(" ".repeat(tui::GAP)));
        last.push_span(said);
    }
    lines
}

fn render_page(frame: &mut ratatui::Frame, page: &Page) {
    let [body, hints] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(lines(page.hints.len())),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(
            page.lines
                .iter()
                .map(|line| Line::raw(line.clone()))
                .collect::<Vec<_>>(),
        ),
        body,
    );
    let more = page
        .scroll
        .as_ref()
        .map(|more| Span::styled(more.label(), tone(Tone::Dim)));
    frame.render_widget(Paragraph::new(hint_lines(&page.hints, more)), hints);
}
