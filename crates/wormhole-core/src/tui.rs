//! The control surface's decisions: what a key does to the box list, and
//! what the screen shows in a terminal of a given size. Terminal I/O lives
//! in the binary.
//!
//! The screen is data rather than text. A rendered string cannot be fitted
//! to a terminal by anything that does not know how wide it is, and the
//! renderer is not allowed to know anything else — so the size comes in
//! here, every decision about what fits is made here, and what goes out is
//! a [`Screen`] the binary draws without deciding anything.

use std::path::Path;

use crate::home::{Listing, Scan};
use crate::manifest::Manifest;
use crate::table;

/// The terminal a screen is being drawn for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}

impl Viewport {
    /// What a terminal that cannot be measured is taken to be. A size of
    /// zero would render nothing at all, which is worse than rendering
    /// for the size almost every terminal still is.
    pub const FALLBACK: Viewport = Viewport {
        width: 80,
        height: 24,
    };

    /// The size to draw for, with anything unusable replaced.
    #[must_use]
    pub fn or_fallback(self) -> Viewport {
        Viewport {
            width: if self.width == 0 {
                Self::FALLBACK.width
            } else {
                self.width
            },
            height: if self.height == 0 {
                Self::FALLBACK.height
            } else {
                self.height
            },
        }
    }

    fn width(self) -> usize {
        self.width as usize
    }

    fn height(self) -> usize {
        self.height as usize
    }
}

/// What a row or a field means, for a renderer that can say so with
/// colour. Every tone is also said in words, so a screen read without
/// colour loses nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    /// Absent, or nothing to say.
    Dim,
    /// Running, ready, proven.
    Good,
    /// Gone, refused, broken.
    Bad,
}

/// One column of the box table, already fitted to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub title: String,
    pub width: u16,
}

/// One row of a table, its cells already cut to their columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<String>,
    pub tone: Tone,
}

/// One line of the detail pane, already laid out and cut to the terminal.
///
/// A whole line rather than a name and a value: which end of a value is
/// dropped when it does not fit is a decision — a path keeps its tail and
/// everything else keeps its head — and decisions are not the renderer's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub text: String,
    pub tone: Tone,
}

/// A field before it knows how wide the screen is.
struct Says {
    name: &'static str,
    value: String,
    /// The value is a path, so it is cut from the head when it must be.
    path: bool,
    tone: Tone,
}

impl Says {
    fn said(name: &'static str, value: impl Into<String>) -> Says {
        Says {
            name,
            value: value.into(),
            path: false,
            tone: Tone::Plain,
        }
    }

    /// A field whose value is a dash reads as absent, and is toned as
    /// absent too.
    fn or_dash(name: &'static str, value: Option<&str>) -> Says {
        match value {
            Some(value) => Says::said(name, value),
            None => Says {
                tone: Tone::Dim,
                ..Says::said(name, "-")
            },
        }
    }

    fn toned(self, tone: Tone) -> Says {
        Says { tone, ..self }
    }
}

/// Which part of a list is on the screen, when not all of it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scroll {
    /// Index of the first row shown.
    pub first: usize,
    pub shown: usize,
    pub total: usize,
}

impl Scroll {
    /// What the screen says about the rows it is not showing. Here rather
    /// than in the renderer, like every other word on the screen.
    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "{}-{} of {}",
            self.first + 1,
            self.first + self.shown,
            self.total
        )
    }
}

/// One keycap and what pressing it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub cap: &'static str,
    pub does: &'static str,
}

/// Why a line is on the screen. A refusal and a report must not look the
/// same, and a question must not look like either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Refusal,
    Question,
}

/// The line under the hints, already wrapped to the terminal.
///
/// Wrapped here rather than by the renderer for the reason every other
/// measurement is made here: the height this screen claims to occupy has
/// to be the height it occupies, and a renderer that wraps is a renderer
/// that decides how many lines something takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub lines: Vec<String>,
    pub level: Level,
}

impl Note {
    /// The whole note as one string, for a caller that has no width to
    /// wrap to — a test, or a refusal printed off a terminal.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join(" ")
    }
}

/// What the panel has to say, before it is fitted to a terminal.
///
/// A [`Note`] is this wrapped to a width. Kept apart so the state machine
/// never holds text that was fitted to a terminal size it did not choose
/// — a resize would leave the old wrapping behind.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    text: String,
    level: Level,
}

/// Everything on the screen, fitted to the terminal it was asked for.
///
/// Nothing here exceeds the viewport: the rows are the visible ones, the
/// cells are cut to their columns, and the line counts add up to at most
/// the height. A renderer that draws this verbatim cannot overflow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screen {
    /// What this screen is choosing, when it is choosing something.
    pub heading: Option<String>,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// Index into `rows`, not into the whole list.
    pub cursor: Option<usize>,
    pub scroll: Option<Scroll>,
    /// What to say when there are no rows at all.
    pub empty: Option<String>,
    pub detail: Vec<Field>,
    /// One line under the table: a total, a verdict, a count.
    pub summary: Option<String>,
    pub notices: Vec<String>,
    pub note: Option<Note>,
    pub hints: Vec<Vec<Hint>>,
}

/// How many lines the hints may take before the words go. Two is a
/// footer; more is the screen becoming its own manual, which is what `?`
/// is for.
const HINT_LINES: usize = 2;

/// The hints packed into lines of `width`: whole where they fit, keycaps
/// alone where they do not.
///
/// Never a cut line, because the thing being cut is the list of what can
/// be done here. A cap with no word beside it is still a key somebody can
/// press; half a word is not, and a key dropped off the end is gone with
/// nothing saying it was there.
fn fit_hints(hints: Vec<Hint>, width: usize, room: usize) -> Vec<Vec<Hint>> {
    if room == 0 {
        return Vec::new();
    }
    let packed = pack(&hints, width, |hint| hint.cap.len() + 1 + hint.does.len());
    if packed.len() <= room.min(HINT_LINES) {
        return packed;
    }
    let caps: Vec<Hint> = hints
        .into_iter()
        .map(|hint| Hint { does: "", ..hint })
        .collect();
    let mut packed = pack(&caps, width, |hint| hint.cap.len());
    packed.truncate(room.min(HINT_LINES));
    packed
}

/// Greedily into lines no wider than `width`. A hint wider than the whole
/// line gets a line of its own rather than being dropped.
fn pack(hints: &[Hint], width: usize, wide: fn(&Hint) -> usize) -> Vec<Vec<Hint>> {
    let mut lines: Vec<Vec<Hint>> = Vec::new();
    let mut line: Vec<Hint> = Vec::new();
    let mut used = 0usize;
    for hint in hints {
        let gap = if line.is_empty() { 0 } else { GAP };
        if !line.is_empty() && used + gap + wide(hint) > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        used += if line.is_empty() { 0 } else { GAP } + wide(hint);
        line.push(*hint);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

impl Screen {
    /// The one nameless column a picker, a form and an input all draw in.
    #[must_use]
    fn one_column(width: usize) -> Vec<Column> {
        vec![Column {
            title: String::new(),
            width: u16::try_from(width).unwrap_or(u16::MAX),
        }]
    }

    /// Whether the table draws a row of column titles. A picker's one
    /// column has no title, and a blank line where a header would be is a
    /// line spent on nothing.
    #[must_use]
    pub fn has_header(&self) -> bool {
        !self.rows.is_empty() && self.columns.iter().any(|column| !column.title.is_empty())
    }

    /// How many lines this screen occupies.
    ///
    /// The definition of the layout, not a guess at it: this is what
    /// [`Tui::screen`] budgets against and what the renderer draws, so a
    /// part added to one without the other fails the height test rather
    /// than overflowing somebody's terminal.
    #[must_use]
    pub fn lines(&self) -> usize {
        // A heading and the blank line under it.
        usize::from(self.heading.is_some()) * 2
            + usize::from(self.has_header())
            + if self.rows.is_empty() {
                usize::from(self.empty.is_some())
            } else {
                self.rows.len()
            }
            + usize::from(self.scroll.is_some())
            + self.detail.len()
            + usize::from(self.summary.is_some())
            + self.notices.len()
            + self.note.as_ref().map_or(0, |note| note.lines.len())
            + self.hints.len()
    }
}

/// How many unreadable things the panel names before it stops listing
/// them. A store full of broken homes must not push the box list off the
/// screen, and the count says how much was left out.
const PROBLEMS_SHOWN: usize = 5;

/// How many lines a note may take before it is cut. Longer and the
/// screen is mostly note; shorter and a question about deleting an
/// agent's history loses the half saying what it deletes.
const NOTE_LINES: usize = 4;

/// How many rows of the list are held back for, whatever else wants the
/// screen. The list is what the panel is for; the detail pane and the
/// problems are what it says about the list.
const LIST_FLOOR: usize = 4;

/// The space between two columns, and between two hints. Public
/// because the renderer draws with it: a gap budgeted here and drawn
/// wider there is exactly as much overflow as the difference.
pub const GAP: usize = 2;

/// The column the cursor marker is drawn in, on every list. Counted here
/// rather than by the renderer, because a gutter the fitting did not know
/// about is one column of overflow on every row.
const GUTTER: usize = 2;

/// Which column of the box table absorbs the width left over, and which
/// columns go first when there is not enough. A path is what a person
/// scans for and the one thing with no bound on its length, so it takes
/// what is left; the name is dropped first because several boxes share
/// one and the detail pane shows it anyway.
const WORKSPACE: usize = 7;
const DROP_ORDER: &[usize] = &[1, 4, 5, 6, 2];

/// Narrower than this, a path column says nothing worth a column, so it
/// goes the way of the others and the detail pane carries it.
const PATH_MIN: usize = 12;

/// The keys the box list offers, and the words for them. One table, so
/// the screen cannot name a key the panel does not decode.
/// Which screen a binding belongs to. One bit each, because most keys
/// belong to several and a few belong to exactly one — `r` resets a box
/// and runs the probes again, and neither screen should offer the other's
/// word for it.
pub mod on {
    pub const BOXES: u8 = 1 << 0;
    pub const ROLES: u8 = 1 << 1;
    pub const STORE: u8 = 1 << 2;
    pub const SECRETS: u8 = 1 << 3;
    pub const DOCTOR: u8 = 1 << 4;
    /// A list to pick one thing from.
    pub const PICK: u8 = 1 << 5;
    /// A block of text with something to agree to.
    pub const CONFIRM: u8 = 1 << 6;
    /// A block of text with nothing to agree to.
    pub const MESSAGE: u8 = 1 << 7;

    pub const TABS: u8 = BOXES | ROLES | STORE | SECRETS | DOCTOR;
    pub const PAGES: u8 = CONFIRM | MESSAGE;
    pub const EVERY: u8 = TABS | PICK | PAGES;
}

/// What the terminal reported. Not what it means: which command a
/// character is depends on what the screen is doing with characters, and
/// a filter being typed is spelled with the same letters the commands are.
///
/// So the binary reports presses and this module decides, which is the
/// same division every other decision here follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Typed(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// One key, where it works, and what it does there.
///
/// The one table. The hints render from it, `?` renders from it, and a
/// press is decoded through it — so a keycap on a screen is a key that
/// screen answers to, by construction rather than by a test that has to
/// remember to check.
struct Binding {
    cap: &'static str,
    press: Press,
    key: Key,
    does: &'static str,
    on: u8,
    /// Whether it earns a place on the hint line. Movement is every
    /// terminal program's, and spending the hint line on it would push
    /// off the keys that are this program's.
    hinted: bool,
}

const fn bind(cap: &'static str, press: Press, key: Key, does: &'static str, on: u8) -> Binding {
    Binding {
        cap,
        press,
        key,
        does,
        on,
        hinted: true,
    }
}

const fn quiet(cap: &'static str, press: Press, key: Key, does: &'static str, on: u8) -> Binding {
    Binding {
        hinted: false,
        ..bind(cap, press, key, does, on)
    }
}

use on::{BOXES, CONFIRM, DOCTOR, EVERY, PAGES, PICK, ROLES, SECRETS, STORE, TABS};

const BINDINGS: &[Binding] = &[
    // Movement. Quiet where the screen's own keys are what the hint line
    // is for, hinted on a page, where moving *is* the screen's own key.
    bind("k", Press::Typed('k'), Key::Up, "scroll up", PAGES),
    bind("j", Press::Typed('j'), Key::Down, "scroll down", PAGES),
    quiet("k", Press::Typed('k'), Key::Up, "up", EVERY),
    quiet("up", Press::Up, Key::Up, "up", EVERY),
    quiet("j", Press::Typed('j'), Key::Down, "down", EVERY),
    quiet("down", Press::Down, Key::Down, "down", EVERY),
    quiet("g", Press::Typed('g'), Key::Top, "first", EVERY),
    quiet("home", Press::Home, Key::Top, "first", EVERY),
    quiet("G", Press::Typed('G'), Key::Bottom, "last", EVERY),
    quiet("end", Press::End, Key::Bottom, "last", EVERY),
    quiet("pgup", Press::PageUp, Key::PageUp, "a screen up", EVERY),
    quiet(
        "pgdn",
        Press::PageDown,
        Key::PageDown,
        "a screen down",
        EVERY,
    ),
    // The box list.
    bind("enter", Press::Enter, Key::Enter, "join, or start", BOXES),
    bind("n", Press::Typed('n'), Key::New, "new", BOXES),
    bind("d", Press::Typed('d'), Key::Stop, "stop", BOXES),
    bind("N", Press::Typed('N'), Key::Rename, "rename", BOXES),
    bind("r", Press::Typed('r'), Key::Reset, "reset", BOXES),
    bind("x", Press::Typed('x'), Key::Remove, "remove", BOXES),
    // Roles.
    bind("enter", Press::Enter, Key::Enter, "recipe", ROLES),
    bind("a", Press::Typed('a'), Key::Add, "install", ROLES),
    bind("x", Press::Typed('x'), Key::Remove, "uninstall", ROLES),
    // The store.
    bind(
        "x",
        Press::Typed('x'),
        Key::Remove,
        "give back the dead",
        STORE,
    ),
    bind(
        "u",
        Press::Typed('u'),
        Key::Widen,
        "also unreferenced",
        STORE,
    ),
    // Secrets.
    bind("s", Press::Typed('s'), Key::Add, "keep one", SECRETS),
    bind("x", Press::Typed('x'), Key::Remove, "forget", SECRETS),
    // The host.
    bind("r", Press::Typed('r'), Key::Reset, "probe again", DOCTOR),
    // Everywhere there are tabs.
    bind("tab", Press::Tab, Key::NextTab, "next tab", TABS),
    bind("/", Press::Typed('/'), Key::Filter, "filter", TABS),
    bind(":", Press::Typed(':'), Key::Palette, "commands", TABS),
    bind("q", Press::Typed('q'), Key::Quit, "quit", TABS),
    quiet("esc", Press::Escape, Key::Cancel, "clear or quit", TABS),
    // Picking one thing from a list.
    bind("enter", Press::Enter, Key::Enter, "choose", PICK),
    bind("/", Press::Typed('/'), Key::Filter, "filter", PICK),
    bind("q", Press::Typed('q'), Key::Quit, "back", PICK),
    quiet("esc", Press::Escape, Key::Cancel, "clear or back", PICK),
    // Reading a block of text.
    bind("enter", Press::Enter, Key::Enter, "start", CONFIRM),
    bind("q", Press::Typed('q'), Key::Quit, "back", PAGES),
    quiet("esc", Press::Escape, Key::Quit, "back", PAGES),
    // The one key that answers a question, named on the question itself.
    quiet("y", Press::Typed('y'), Key::Yes, "confirm", EVERY),
    // Last, so it is the last thing the hint line loses.
    bind("?", Press::Typed('?'), Key::Help, "keys", EVERY),
];

/// Which command this press is on this screen, if it is one.
///
/// Public because the binary asks: it reports what the terminal said and
/// this says what it means, which is the same division every other
/// decision here follows.
///
/// `None` for a press the screen does not answer to — which is what lets
/// `r` mean two things on two tabs without either of them guessing.
pub fn command(press: Press, on: u8) -> Option<Key> {
    BINDINGS
        .iter()
        .find(|binding| binding.press == press && binding.on & on != 0)
        .map(|binding| binding.key)
}

/// The keycap a screen names a key by, from the one table.
///
/// Prose that hardcodes a letter is prose that starts lying the moment
/// the binding moves, which is exactly what [`BINDINGS`] exists to stop.
#[must_use]
pub fn cap_for(key: Key, on: u8) -> &'static str {
    BINDINGS
        .iter()
        .find(|binding| binding.key == key && binding.on & on != 0)
        .map_or("?", |binding| binding.cap)
}
/// The keys this screen puts on its hint line.
fn hints_for(on: u8) -> Vec<Hint> {
    BINDINGS
        .iter()
        .filter(|binding| binding.hinted && binding.on & on != 0)
        .map(|binding| Hint {
            cap: binding.cap,
            does: binding.does,
        })
        .collect()
}

/// What `?` shows: every key of this screen, the quiet ones included.
///
/// Rendered from the same table the hint line and the decoder read, so a
/// key that works and is not on this page cannot exist.
#[must_use]
pub fn keys_page(on: u8) -> String {
    let mut said = String::from("keys on this screen\n\n");
    let mut rows: Vec<Vec<String>> = Vec::new();
    for binding in BINDINGS.iter().filter(|binding| binding.on & on != 0) {
        // One row per key, and a key spelled two ways — `k` and up — is
        // one row with both caps, because it is one key.
        match rows
            .iter_mut()
            .find(|row| row.get(1).is_some_and(|does| does == binding.does))
        {
            Some(row) => row[0] = format!("{}  {}", row[0], binding.cap),
            None => rows.push(vec![binding.cap.to_owned(), binding.does.to_owned()]),
        }
    }
    said.push_str(&crate::table::render(&rows));
    said.push_str("\nEvery screen has its own keys; this is the one you are on.\n");
    said
}

/// What the panel is looking at. The tool's own nouns, in the order the
/// help page groups them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tab {
    #[default]
    Boxes,
    Roles,
    Store,
    Secrets,
    Doctor,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Boxes,
        Tab::Roles,
        Tab::Store,
        Tab::Secrets,
        Tab::Doctor,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Tab::Boxes => "boxes",
            Tab::Roles => "roles",
            Tab::Store => "store",
            Tab::Secrets => "secrets",
            Tab::Doctor => "host",
        }
    }

    /// Which bindings this tab answers to.
    #[must_use]
    pub fn keys(self) -> u8 {
        match self {
            Tab::Boxes => on::BOXES,
            Tab::Roles => on::ROLES,
            Tab::Store => on::STORE,
            Tab::Secrets => on::SECRETS,
            Tab::Doctor => on::DOCTOR,
        }
    }

    fn next(self) -> Tab {
        let at = Tab::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Tab::ALL[(at + 1) % Tab::ALL.len()]
    }
}

/// One row of a tab that is not the box list: what it is keyed by, what
/// it shows, and what the pane under the list says about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SheetRow {
    /// What acts on this row name it: a role's name, a secret's name, a
    /// path in the store. The cursor holds this, not an index.
    pub key: String,
    pub cells: Vec<String>,
    pub tone: Tone,
    /// What the pane under the list says about this row, as label and
    /// value. Laid out by the same body every other tab uses, so a label
    /// one character longer cannot misalign one screen and not the rest.
    pub detail: Vec<(&'static str, String)>,
}

/// A tab's rows, gathered by the binary and rendered here.
///
/// The binary reads roles from disk, walks the store and runs the probes,
/// because all three are I/O. What the rows say, how wide they are, which
/// of them fit and what the keys do to them stays here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sheet {
    pub columns: Vec<String>,
    pub rows: Vec<SheetRow>,
    /// What to say when there are no rows, in this tab's own words.
    pub empty: String,
    /// One line under the table: a total, a verdict, a count.
    pub summary: Option<String>,
}

/// The box list and the cursor on it. Every box this host keeps, not only
/// the running ones: an idle box is the thing you go back to, and one the
/// control surface cannot show is one you cannot reach from it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tui {
    pub boxes: Vec<Listing>,
    /// What the last scan could not read, shown on the screen the panel
    /// draws. Nothing prints these while the panel is up.
    pub problems: Vec<String>,
    /// Which tab is up, and what the others hold.
    ///
    /// The store walk and the host probes are expensive enough that
    /// running them once a second would be its own defect, so a tab's
    /// rows are asked for when it is opened rather than gathered with
    /// the box list.
    tab: Tab,
    sheets: std::collections::BTreeMap<Tab, Sheet>,
    /// The row the cursor is on in each tab, by that tab's row key.
    ///
    /// Not an index. The box list is sorted by when each box last ran and
    /// it is rescanned once a second, so a box started in another
    /// terminal re-sorts it under the cursor — and `d` does not ask, so
    /// an index would stop whichever box had taken that row. One per tab,
    /// so coming back to a tab comes back to the row you left.
    selected: std::collections::BTreeMap<Tab, String>,
    /// What the last key could not do here, shown under the hints and
    /// cleared by the next key.
    ///
    /// Without it a key the panel understands but cannot act on — `d` on
    /// a box that is not running — redraws an unchanged screen, which is
    /// exactly what a key it does not understand already does. Two
    /// different answers must not look the same.
    note: Option<Said>,
    /// An act waiting to be answered for. Held here rather than on a
    /// screen of its own so the question, the keys that answer it and the
    /// refusals around it are all one testable state machine — the panel
    /// draws what this says and decides nothing.
    pending: Option<Act>,
    /// Whether the store tab counts what no box on this host references.
    /// View state, like the filter: nothing on disk moves when it flips.
    widened: bool,
    /// What the list is narrowed to. Empty is every row.
    filter: String,
    /// Whether the filter is being typed. While it is, a letter is a
    /// letter rather than the key it spells.
    typing: bool,
}

/// What the tab that is up has to show, once the filter has been applied.
///
/// One shape for every tab, so the cursor, the window, the fitting and
/// the drawing are one body rather than five.
struct Shown {
    columns: Vec<String>,
    cells: Vec<Vec<String>>,
    keys: Vec<String>,
    tones: Vec<Tone>,
    empty: String,
    summary: Option<String>,
}

impl Tui {
    /// Everything a box can be searched by, which is everything about it
    /// that a person knows.
    ///
    /// Read off the record rather than off the drawn row, so a filter
    /// matches the same boxes whatever the clock says — the drawn row
    /// carries an age, and an age changes every minute.
    fn found(&self, entry: &Listing) -> bool {
        // The common case, and the one on the redraw path: nothing is
        // being searched for, so nothing has to be gathered to search.
        if self.filter.is_empty() {
            return true;
        }
        let record = &entry.record;
        let mut about: Vec<String> = vec![
            record.id.clone(),
            if entry.running.is_some() {
                "running".to_owned()
            } else {
                "idle".to_owned()
            },
        ];
        about.extend(
            [
                record.alias.as_deref(),
                record.name.as_deref(),
                record.role.as_deref(),
                record.agent.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(str::to_owned),
        );
        about.extend(
            record
                .workspaces()
                .map(|workspace| workspace.display().to_string()),
        );
        matches(&about, &self.filter)
    }

    /// The boxes the filter leaves.
    fn rows(&self) -> Vec<&Listing> {
        self.boxes
            .iter()
            .filter(|entry| self.found(entry))
            .collect()
    }

    /// What the tab that is up shows, filtered.
    fn shown(&self, now_unix: u64) -> Shown {
        if self.tab == Tab::Boxes {
            let rows = self.rows();
            return Shown {
                columns: crate::home::list_header(),
                cells: rows
                    .iter()
                    .map(|entry| crate::home::list_row(entry, now_unix))
                    .collect(),
                keys: rows.iter().map(|entry| entry.record.id.clone()).collect(),
                tones: rows
                    .iter()
                    .map(|entry| {
                        if entry.running.is_some() {
                            Tone::Good
                        } else {
                            Tone::Plain
                        }
                    })
                    .collect(),
                empty: if self.boxes.is_empty() {
                    crate::home::NO_BOXES_YET.to_owned()
                } else {
                    format!("no box matches {}", self.filter)
                },
                summary: None,
            };
        }
        let sheet = self.sheets.get(&self.tab).cloned().unwrap_or_default();
        let rows: Vec<&SheetRow> = sheet
            .rows
            .iter()
            .filter(|row| matches(&row.cells, &self.filter))
            .collect();
        Shown {
            columns: sheet.columns,
            cells: rows.iter().map(|row| row.cells.clone()).collect(),
            keys: rows.iter().map(|row| row.key.clone()).collect(),
            tones: rows.iter().map(|row| row.tone).collect(),
            empty: if sheet.rows.is_empty() {
                sheet.empty
            } else {
                format!("nothing matches {}", self.filter)
            },
            summary: sheet.summary,
        }
    }

    /// What the cursor is on in the tab that is up.
    fn key(&self) -> Option<&str> {
        self.selected.get(&self.tab).map(String::as_str)
    }

    /// What the tab that is up is keyed by, filtered — and nothing else.
    ///
    /// Every caller that only has to find or move the cursor comes here
    /// rather than to [`Tui::shown`], which renders every cell of every
    /// row. A keypress asks this three or four times; rendering a table
    /// to answer "which row is the cursor on" was the whole of the cost.
    fn keys(&self) -> Vec<String> {
        match self.tab {
            Tab::Boxes => self
                .rows()
                .iter()
                .map(|entry| entry.record.id.clone())
                .collect(),
            tab => self
                .sheets
                .get(&tab)
                .into_iter()
                .flat_map(|sheet| &sheet.rows)
                .filter(|row| matches(&row.cells, &self.filter))
                .map(|row| row.key.clone())
                .collect(),
        }
    }

    /// Hands a tab the rows the binary gathered for it.
    pub fn show(&mut self, tab: Tab, sheet: Sheet) {
        self.sheets.insert(tab, sheet);
        if tab == self.tab {
            self.anchor();
        }
    }

    /// Whether the store tab should count what nothing references. Asked
    /// by the caller that gathers those rows, because gathering is I/O.
    #[must_use]
    pub fn widened(&self) -> bool {
        self.widened
    }

    /// Which tab the panel is on, for a caller that has to gather it.
    #[must_use]
    pub fn tab(&self) -> Tab {
        self.tab
    }
}

/// One cursor on a list of `len` rows: where every screen's movement
/// keys land, so no screen can drift on edge behavior.
///
/// A page is a screenful, so it is what the terminal is tall minus what
/// the screen spends on itself — near enough that one row of overlap
/// keeps a reader's place, which is what every pager does.
fn moved(selected: usize, len: usize, key: Key, viewport: Viewport) -> usize {
    let page = usize::from(viewport.or_fallback().height)
        .saturating_sub(4)
        .max(1);
    step(selected, len.saturating_sub(1), page, key)
}

/// The one cursor arithmetic: a row list and a block of text move the
/// same way, and a rule written twice is an edge got wrong twice.
fn step(at: usize, last: usize, page: usize, key: Key) -> usize {
    match key {
        Key::Up => at.saturating_sub(1),
        Key::Down => (at + 1).min(last),
        Key::Top => 0,
        Key::Bottom => last,
        Key::PageUp => at.saturating_sub(page),
        Key::PageDown => (at + page).min(last),
        _ => at,
    }
}

/// Name-and-value lines laid out in one column and cut to `width`.
///
/// One body for every tab: a label column guessed per builder misaligns
/// the moment a label gains a character, and which end of a value is
/// dropped — a path keeps its tail, everything else its head — is a
/// decision, not something five places should each make.
fn laid_out(says: Vec<Says>, width: usize) -> Vec<Field> {
    let column = says
        .iter()
        .map(|one| table::display_width(one.name))
        .max()
        .unwrap_or(0)
        + GAP;
    says.into_iter()
        .map(|one| {
            let room = width.saturating_sub(column);
            let value = if one.path {
                table::truncate_head(&one.value, room)
            } else {
                table::truncate_tail(&one.value, room)
            };
            Field {
                text: format!("{:column$}{value}", one.name).trim_end().to_owned(),
                tone: one.tone,
            }
        })
        .collect()
}

/// One character of a filter being typed, wherever it is being typed.
///
/// The box list and every picker narrow the same way, and a rule written
/// twice is a rule that drifts: `enter` keeps what is typed and gives the
/// letters back to the keys, `esc` takes the whole filter back, and half
/// a filter left behind is a list hiding rows for a reason nobody can see.
fn typing(filter: &mut String, typing: &mut bool, press: Press) {
    match press {
        Press::Enter => *typing = false,
        Press::Escape => {
            *typing = false;
            filter.clear();
        }
        Press::Backspace => {
            filter.pop();
        }
        Press::Typed(c) => filter.push(c),
        _ => {}
    }
}

/// Whether a row's cells hold what was typed, ignoring case.
///
/// Substring and not a pattern language: a filter is for finding the row
/// you already know is there, and every character of a regex is a
/// character somebody meant to search for.
fn matches(cells: &[String], filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let wanted = filter.to_lowercase();
    cells
        .iter()
        .any(|cell| cell.to_lowercase().contains(&wanted))
}

/// The rows a list shows, where its cursor lands among them, and what it
/// is not showing.
///
/// One body for the box table and every picker: a picker that scrolled
/// differently from the list it was opened over would be two answers to
/// one question, and the arithmetic here — the window, the cursor
/// rebased into it, whether a scroll line is owed — is that question.
fn listed(
    cells: Vec<Vec<String>>,
    tones: &[Tone],
    cursor: Option<usize>,
    room: usize,
) -> (Vec<Row>, Option<usize>, Option<Scroll>) {
    let total = cells.len();
    let window = window(total, cursor.unwrap_or(0), room);
    let rows = window
        .clone()
        .zip(cells.into_iter().skip(window.start))
        .map(|(at, cells)| Row {
            cells,
            tone: tones.get(at).copied().unwrap_or(Tone::Plain),
        })
        .collect();
    (
        rows,
        cursor
            .and_then(|at| at.checked_sub(window.start))
            .filter(|at| *at < window.len()),
        (!window.is_empty() && window.len() < total).then(|| Scroll {
            first: window.start,
            shown: window.len(),
            total,
        }),
    )
}

/// Which slice of `total` rows is on screen, given `room` lines for them.
///
/// A pure function of the cursor, with no remembered offset: two screens
/// drawn from one state are identical, and nothing can be left pointing
/// at a row that has gone. The cursor sits in the middle of the window
/// wherever the list is long enough for it to.
fn window(total: usize, cursor: usize, room: usize) -> std::ops::Range<usize> {
    let room = room.min(total);
    if room == 0 {
        return 0..0;
    }
    let first = cursor
        .saturating_sub(room / 2)
        .min(total.saturating_sub(room));
    first..first + room
}

/// The columns of the box table that fit `width`, and the cells cut to
/// them.
///
/// Columns go in [`DROP_ORDER`] until the rest fit, then the path column
/// takes whatever is left. Dropping rather than squeezing every column:
/// six columns cut to four characters each is a table that fits and says
/// nothing, and the detail pane holds everything dropped here.
fn fit_columns(rows: &[Vec<String>], width: usize) -> (Vec<Column>, Vec<Vec<String>>) {
    let count = rows.first().map_or(0, Vec::len);
    let natural: Vec<usize> = (0..count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| table::display_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let spread = |kept: &[usize]| -> usize {
        kept.iter().map(|&at| natural[at]).sum::<usize>() + GAP * kept.len().saturating_sub(1)
    };

    // A column whose every cell is the dash carries nothing, so it goes
    // before one that carries something, whatever the order below says.
    let blank = |column: usize| {
        rows.iter()
            .skip(1)
            .all(|row| matches!(row.get(column).map(String::as_str), None | Some("-" | "")))
    };
    let order = DROP_ORDER
        .iter()
        .filter(|at| blank(**at))
        .chain(DROP_ORDER.iter().filter(|at| !blank(**at)));

    let mut kept: Vec<usize> = (0..count).collect();
    for dropped in order {
        if spread(&kept) <= width {
            break;
        }
        kept.retain(|at| at != dropped);
    }
    // The path column is last and flexible: what it cannot have, it is
    // cut to. Below `PATH_MIN` it is carrying nothing, so it goes too.
    let mut widths: Vec<usize> = kept.iter().map(|&at| natural[at]).collect();
    if let Some(flex) = kept.iter().position(|&at| at == WORKSPACE) {
        let others = spread(&kept) - natural[WORKSPACE];
        let room = width.saturating_sub(others);
        if room < PATH_MIN {
            kept.remove(flex);
            widths.remove(flex);
        } else {
            widths[flex] = natural[WORKSPACE].min(room);
        }
    }
    // Whatever is still too wide loses columns from the right, down to
    // one — a table of no columns is a screen that says nothing at all.
    while kept.len() > 1 && widths.iter().sum::<usize>() + GAP * (kept.len() - 1) > width {
        kept.pop();
        widths.pop();
    }
    let others: usize =
        widths.iter().rev().skip(1).sum::<usize>() + GAP * kept.len().saturating_sub(1);
    if let Some(last) = widths.last_mut() {
        *last = (*last).min(width.saturating_sub(others));
    }

    let columns = kept
        .iter()
        .zip(&widths)
        .map(|(&at, &wide)| Column {
            title: rows
                .first()
                .and_then(|header| header.get(at))
                .cloned()
                .unwrap_or_default(),
            width: u16::try_from(wide).unwrap_or(u16::MAX),
        })
        .collect();
    let cut = |row: &Vec<String>| -> Vec<String> {
        kept.iter()
            .zip(&widths)
            .map(|(&at, &wide)| {
                let cell = row.get(at).map(String::as_str).unwrap_or_default();
                if at == WORKSPACE {
                    table::truncate_head(cell, wide)
                } else {
                    table::truncate_tail(cell, wide)
                }
            })
            .collect()
    };
    (columns, rows.iter().skip(1).map(cut).collect())
}

/// The keys the TUI understands, already decoded from the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
    Enter,
    Stop,
    New,
    /// Install a role, or keep a secret: what this tab adds.
    Add,
    Remove,
    /// Reset a box, or run the probes again: what this tab starts over.
    Reset,
    /// Name a box without starting it.
    Rename,
    /// Widen what the store would give back to what nothing references.
    Widen,
    /// Narrow the list to what matches what you type next.
    Filter,
    /// Every command this tool has, as a list to pick from.
    Palette,
    NextTab,
    /// Every key of this screen, on a screen of its own.
    Help,
    /// Take back what is up — a filter, a question — or leave.
    Cancel,
    /// The one key that answers a question. Anything else cancels it, so
    /// a stray press can never delete an agent's history.
    Yes,
    Quit,
}

/// Something the panel does without leaving the screen, and the outcome
/// comes back to it through [`Tui::acted`].
///
/// One type rather than one closure per verb: the panel hands these to a
/// single caller, so a new in-place act is a variant here and nothing else
/// has to change shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Kill this box's PID 1; its own `wormhole box` cleans up.
    Stop(u32),
    /// Take this box away: its home, and everything the agent kept in it.
    Remove(String),
    /// Take this role's name back. The directory it points at is left
    /// alone; a name is all an install is.
    Uninstall(String),
    /// Keep a value the recipes ask for, for every box.
    Keep(String, String),
    /// Forget a kept secret. The next box that asks for it asks you.
    Forget(String),
    /// Give back what the store can prove is finished with, and — when
    /// the store tab is widened — what no box here references either.
    Sweep(bool),
    /// Empty this box's home, keeping the box itself — same id, same
    /// name, same workspace, nothing the agent wrote.
    Reset(String),
    /// Give this box a name to type instead of its id.
    Rename(String, String),
}

impl Act {
    /// The question this has to be answered for, or `None` when it needs
    /// no answer — which is also the note the screen shows while it is
    /// waiting, so an act with nothing to ask puts nothing up.
    ///
    /// Stopping needs none: it ends a process the same row starts again,
    /// so a confirm there would be friction over nothing. Removing and
    /// resetting both take an agent's history, which no key press should
    /// be able to do by accident.
    fn question(&self) -> Option<String> {
        match self {
            // Naming is reversible by naming again, so it asks nothing.
            Act::Stop(_) | Act::Rename(..) | Act::Keep(..) => None,
            Act::Remove(id) => Some(format!(
                "remove box {id}? its home goes with it — history, logins, \
                 and whatever the agent installed. y confirm, any other key cancel"
            )),
            Act::Uninstall(name) => Some(format!(
                "uninstall role {name}? the name goes; the directory or the \
                 commit it points at is left alone. y confirm, any other key cancel"
            )),
            Act::Forget(name) => Some(format!(
                "forget {name}? the next box whose recipe asks for it asks you \
                 again. y confirm, any other key cancel"
            )),
            Act::Sweep(widened) => Some(format!(
                "give back everything the store can prove is finished with{}? \
                 y confirm, any other key cancel",
                if *widened {
                    ", and everything no box here references"
                } else {
                    ""
                }
            )),
            Act::Reset(id) => Some(format!(
                "reset box {id}? it keeps its id, name and workspace, and starts \
                 next time with an empty home. y confirm, any other key cancel"
            )),
        }
    }

    /// What the screen says once it has happened. Public because the
    /// commands say the same thing when they do the same work, and two
    /// sentences for one act would drift.
    pub fn done(&self) -> String {
        match self {
            Act::Stop(_) => "stopping that box".to_owned(),
            Act::Remove(id) => format!("box {id} removed"),
            Act::Reset(id) => format!("box {id} reset; its next start begins fresh"),
            Act::Rename(id, name) => format!("box {id} is now {name}"),
            Act::Uninstall(name) => format!("role {name} uninstalled"),
            Act::Keep(name, _) => format!("{name} kept for every box"),
            Act::Forget(name) => format!("{name} forgotten"),
            Act::Sweep(_) => "the store gave back what it could prove".to_owned(),
        }
    }
}

/// What the binary must do after a key: leave the panel for something
/// else, or do one thing here and stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Redraw,
    /// Leave the TUI and join this running box (`wormhole attach <id>`).
    Attach(String),
    /// there. The list is the binary's to gather: which directories still
    /// exist is not something this crate may look up.
    Workspaces(String),
    /// Leave the TUI and start another box in the current workspace.
    New,
    /// Put this text on a screen of its own, with nothing to agree to.
    /// What `?` answers with, and where a report goes.
    Read(String),
    /// Go to this tab, and gather its rows.
    Goto(Tab),
    /// Run this command line and put what it printed on a screen.
    Run(Vec<String>),
    /// Every command this tool has, as a list to pick from.
    Commands(Vec<Entry>),
    /// Gather this tab's rows and hand them back through `show`.
    Gather(Tab),
    /// Show this role's whole recipe.
    Show(String),
    /// Ask which role to install, then install it.
    Install,
    /// Ask for a secret to keep, masked.
    Keep,
    /// Ask for a name for this box, then set it.
    Name(String),
    Quit,
    /// Do this without leaving the panel, then report through
    /// [`Tui::acted`].
    Do(Act),
}

impl Tui {
    /// Swaps in a fresh scan, keeping the cursor on the box it was on.
    ///
    /// When that box is gone the cursor stays where it was on the screen,
    /// which is the row that took its place — the one behaviour an index
    /// got right, kept without the one it got wrong.
    pub fn refresh(&mut self, scan: Scan) {
        let was = self.row();
        self.boxes = scan.boxes;
        self.problems = scan.problems;
        if self.tab == Tab::Boxes && self.row().is_none() {
            let rows = self.rows();
            let at = was.unwrap_or(0).min(rows.len().saturating_sub(1));
            match rows.get(at) {
                Some(entry) => {
                    self.selected.insert(Tab::Boxes, entry.record.id.clone());
                }
                None => {
                    self.selected.remove(&Tab::Boxes);
                }
            }
        }
    }

    /// Where the cursor is in the tab that is up, as an index into what
    /// that tab is showing. Derived, never stored: an index kept beside
    /// the key is a second answer to one question, and the two can
    /// disagree.
    fn row(&self) -> Option<usize> {
        let wanted = self.key()?;
        self.keys().iter().position(|key| key == wanted)
    }

    /// The box the cursor is on. Only the box list has one; every other
    /// tab acts on a row key instead.
    fn here(&self) -> Option<&Listing> {
        let wanted = self.key()?;
        self.rows()
            .into_iter()
            .find(|entry| entry.record.id == wanted)
    }

    /// The box the cursor is on, for a caller that has to look it up.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.here().map(|entry| entry.record.id.as_str())
    }

    fn refuse(&mut self, why: &str) -> Action {
        self.note = Some(Said {
            text: why.to_owned(),
            level: Level::Refusal,
        });
        Action::Redraw
    }

    /// Enter does the one thing that row allows: join a box that is
    /// running, start one that is not. Nothing else could be meant, so
    /// nothing else has to be typed.
    pub fn update(&mut self, press: Press, viewport: Viewport) -> Action {
        // A refusal answers one key press. Cleared here, so it can never
        // go on describing a key the user has already moved past.
        self.note = None;
        // A filter being typed owns the letters. `w` is a key on this
        // screen and a character in a search, and the only thing that can
        // tell them apart is what the screen is doing with characters —
        // which is why the binary reports presses and this decides.
        if self.typing {
            return self.filtering(press);
        }
        // A question owns the whole keyboard until it is answered: `y`
        // does it, everything else — `q` included — takes it back. Taken
        // rather than read, so no key can leave the question standing.
        if let Some(act) = self.pending.take() {
            return match command(press, on::BOXES) {
                Some(Key::Yes) => Action::Do(act),
                // Saying so, because a cancelled question that redrew an
                // unchanged screen looked exactly like a key the panel
                // does not know.
                _ => self.refuse("cancelled; nothing was touched"),
            };
        }
        let Some(key) = command(press, self.tab.keys()) else {
            return Action::Redraw;
        };
        // Moving is the same on every tab: rows are rows.
        if matches!(
            key,
            Key::Up | Key::Down | Key::Top | Key::Bottom | Key::PageUp | Key::PageDown
        ) {
            let keys = self.keys();
            let at = moved(self.row().unwrap_or(0), keys.len(), key, viewport);
            self.point(keys.get(at).cloned());
            return Action::Redraw;
        }
        // What a key does to a row depends on which tab the row is in:
        // `x` takes a box away, uninstalls a role, forgets a secret and
        // gives the store back. One word each, in the hints of the tab
        // it belongs to.
        // Leaving, the filter, the tabs, the keys and the commands work
        // the same wherever you are, so they are answered once.
        if let Some(action) = self.anywhere(key) {
            return action;
        }
        if self.tab != Tab::Boxes {
            return self.on_sheet(key);
        }
        match key {
            Key::New => Action::New,
            // A box that is running is already somewhere, so joining it
            // is the one thing that row allows. A box that is not has a
            // place to start, and which place is the next question —
            // asked rather than guessed, because the guess was right
            // often enough to be invisible and wrong with no way to say
            // so.
            Key::Enter => match self.here() {
                Some(entry) if entry.running.is_some() => Action::Attach(entry.record.id.clone()),
                Some(entry) => Action::Workspaces(entry.record.id.clone()),
                None => Action::Redraw,
            },
            Key::Stop => match self.here().map(|entry| entry.running) {
                Some(Some(pid)) => Action::Do(Act::Stop(pid)),
                // Stopping an idle box would mean killing nothing — which
                // is said, not silently done.
                Some(None) => self.refuse("that box is not running; enter starts it again"),
                None => Action::Redraw,
            },
            Key::Rename => match self.here() {
                Some(entry) if entry.running.is_some() => self.refuse(
                    "that box is running; a name it started under cannot be changed under it",
                ),
                Some(entry) => Action::Name(entry.record.id.clone()),
                None => Action::Redraw,
            },
            Key::Remove => self.ask(Act::Remove),
            Key::Reset => self.ask(Act::Reset),
            // Keys the box list names on other tabs, movement handled
            // above, and the one key that is only ever an answer to a
            // question that is not up.
            _ => Action::Redraw,
        }
    }

    /// What a key does on any tab, or nothing when it is that tab's own.
    fn anywhere(&mut self, key: Key) -> Option<Action> {
        Some(match key {
            Key::Quit => Action::Quit,
            // One key for "take back what is up", and only when nothing
            // is up does it leave. A filter typed by mistake must not
            // cost the panel.
            Key::Cancel if self.filter.is_empty() => Action::Quit,
            Key::Cancel => {
                self.filter.clear();
                self.anchor();
                Action::Redraw
            }
            Key::Filter => {
                self.typing = true;
                Action::Redraw
            }
            Key::NextTab => self.go(self.tab.next()),
            Key::Help => Action::Read(keys_page(self.tab.keys())),
            Key::Palette => Action::Commands(self.commands()),
            _ => return None,
        })
    }

    /// Moves to a tab and asks for its rows.
    ///
    /// Asked for rather than gathered with the box list, because walking
    /// the store and running the host probes are seconds of work each and
    /// the box list is rescanned every second.
    fn go(&mut self, tab: Tab) -> Action {
        self.tab = tab;
        // The filter belongs to the list it was typed over. Carried to
        // the next tab it would hide rows there for a reason typed
        // somewhere else.
        self.filter.clear();
        self.typing = false;
        self.anchor();
        Action::Gather(tab)
    }

    /// What a key does on a tab that is not the box list.
    ///
    /// The rows of those tabs are a name and some cells, so the acts are
    /// the same shape as the box list's and the differences are words.
    fn on_sheet(&mut self, key: Key) -> Action {
        let tab = self.tab;
        match key {
            // Probing a host is the one act that is its own refresh.
            Key::Reset if tab == Tab::Doctor => Action::Gather(tab),
            // A view toggle, not an act: nothing on disk changes, so it
            // has nothing to report and nothing to ask.
            Key::Widen if tab == Tab::Store => {
                self.widened = !self.widened;
                Action::Gather(tab)
            }
            Key::Add => match tab {
                Tab::Roles => Action::Install,
                Tab::Secrets => Action::Keep,
                _ => Action::Redraw,
            },
            // Every row of the store is a thing on disk, and what `x`
            // gives back is what the sweep has proven, not one row.
            Key::Remove if tab == Tab::Store => {
                let widened = self.widened;
                self.put(Act::Sweep(widened))
            }
            Key::Enter | Key::Remove => {
                let Some(name) = self.key().map(str::to_owned) else {
                    return Action::Redraw;
                };
                match (key, tab) {
                    (Key::Enter, Tab::Roles) => Action::Show(name),
                    (Key::Remove, Tab::Roles) => self.put(Act::Uninstall(name)),
                    (Key::Remove, Tab::Secrets) => self.put(Act::Forget(name)),
                    _ => Action::Redraw,
                }
            }
            _ => Action::Redraw,
        }
    }

    /// Puts an act's question up, or does it when it asks nothing.
    ///
    /// A question is a note like any other. `update` clears the note on
    /// every key, which is exactly when a question has to go — so one
    /// field holds both and they cannot both be up by construction
    /// rather than by comment.
    fn put(&mut self, act: Act) -> Action {
        match act.question() {
            None => Action::Do(act),
            Some(text) => {
                self.note = Some(Said {
                    text,
                    level: Level::Question,
                });
                self.pending = Some(act);
                Action::Redraw
            }
        }
    }

    /// What a press does while a filter is being typed. Everything is a
    /// character except the few keys that end the typing.
    fn filtering(&mut self, press: Press) -> Action {
        typing(&mut self.filter, &mut self.typing, press);
        self.anchor();
        Action::Redraw
    }

    /// Puts the cursor on a row that is actually shown.
    ///
    /// A filter can hide the box the cursor was on. Leaving it there
    /// would leave every key acting on a row nobody can see.
    fn anchor(&mut self) {
        if self.row().is_none() {
            self.point(self.keys().first().cloned());
        }
    }

    /// Puts the cursor of the tab that is up on this row, or on nothing.
    fn point(&mut self, key: Option<String>) {
        match key {
            Some(key) => {
                self.selected.insert(self.tab, key);
            }
            None => {
                self.selected.remove(&self.tab);
            }
        }
    }

    /// Puts a question about the selected box on the screen, or says why
    /// there is nothing to ask.
    ///
    /// Both acts need the box idle for the same reason: its home is what
    /// they touch, and a running agent is writing to it. The claim the
    /// binary takes is what actually proves that — this only spares the
    /// user a question whose answer was always going to be refused.
    fn ask(&mut self, make: fn(String) -> Act) -> Action {
        match self.here() {
            None => Action::Redraw,
            Some(entry) if entry.running.is_some() => {
                self.refuse("that box is running; d stops it first")
            }
            Some(entry) => {
                let id = entry.record.id.clone();
                self.put(make(id))
            }
        }
    }

    /// What the stop the panel asked for came to.
    ///
    /// These are the acts that leave the user sitting on this screen, so
    /// they are the ones that have to report themselves — a signal lands
    /// when the kernel gets to it, and until then the row still reads
    /// `running`. The words stay here rather than in the binary: what the
    /// screen says is this module's decision.
    pub fn acted(&mut self, act: &Act, outcome: Result<(), String>) {
        self.note = Some(match outcome {
            Ok(()) => Said {
                text: act.done(),
                level: Level::Info,
            },
            Err(why) => Said {
                text: why,
                level: Level::Refusal,
            },
        });
    }

    /// Everything about the box under the cursor that the table has no
    /// column for — the workspaces it has served most of all, which were
    /// recorded, counted as `+N`, and shown nowhere.
    fn detail(&self, width: usize, exists: &dyn Fn(&Path) -> bool) -> Vec<Field> {
        laid_out(
            if self.tab == Tab::Boxes {
                self.box_says(exists)
            } else {
                // Every other tab's rows carry their own, gathered where
                // the row was and laid out by the same body.
                let here = self.key();
                self.sheets
                    .get(&self.tab)
                    .into_iter()
                    .flat_map(|sheet| &sheet.rows)
                    .find(|row| Some(row.key.as_str()) == here)
                    .map(|row| {
                        row.detail
                            .iter()
                            .map(|(name, value)| Says::said(name, value.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            },
            width,
        )
    }

    /// Everything about the box under the cursor that the table has no
    /// column for — the workspaces it has served most of all, which were
    /// recorded, counted as `+N`, and shown nowhere.
    fn box_says(&self, exists: &dyn Fn(&Path) -> bool) -> Vec<Says> {
        let Some(entry) = self.here() else {
            return Vec::new();
        };
        let record = &entry.record;
        let mut says = vec![
            Says::or_dash("alias", record.alias.as_deref()),
            match entry.running {
                Some(pid) => Says::said("state", format!("running (pid {pid})")).toned(Tone::Good),
                None => Says::said("state", "idle").toned(Tone::Dim),
            },
            Says::or_dash("product", record.agent.as_deref()),
            match record.source.as_deref() {
                Some(source) => Says::said("role", source),
                None => Says::said("role", "this workspace's wormhole.toml").toned(Tone::Dim),
            },
            Says::said("workspaces", ""),
        ];
        // The list the table could only say `+N` about.
        for one in crate::home::served(record, exists) {
            let mark = one.mark();
            says.push(Says {
                name: "",
                value: if mark.is_empty() {
                    one.path.display().to_string()
                } else {
                    format!("{}  {mark}", one.path.display())
                },
                path: true,
                tone: if one.gone { Tone::Bad } else { Tone::Plain },
            });
        }
        says
    }

    /// What the scan could not read, capped, with the rest counted.
    fn notices(&self) -> Vec<String> {
        if self.problems.is_empty() {
            return Vec::new();
        }
        let mut said = vec!["could not read:".to_owned()];
        said.extend(
            self.problems
                .iter()
                .take(PROBLEMS_SHOWN)
                .map(|problem| format!("  {problem}")),
        );
        let hidden = self.problems.len().saturating_sub(PROBLEMS_SHOWN);
        if hidden > 0 {
            said.push(format!("  ... and {hidden} more"));
        }
        said
    }

    /// The whole screen, fitted to this terminal.
    ///
    /// `exists` answers whether a workspace is still there. Asked of the
    /// caller because this crate does no I/O, and asked at all because a
    /// list of places to go back to that includes the ones that are gone
    /// is a list of things that will refuse.
    pub fn screen(
        &self,
        viewport: Viewport,
        now_unix: u64,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Screen {
        let viewport = viewport.or_fallback();
        let width = viewport.width();
        let shown = self.shown(now_unix);
        let mut table = vec![shown.columns.clone()];
        table.extend(shown.cells.iter().cloned());
        let (columns, cells) = fit_columns(&table, width.saturating_sub(GUTTER));
        // Spent in the order the screen owes it. The hints first: a
        // screen with no way off it is worse than a short list. Then the
        // note, which answers the key just pressed and so cannot wait for
        // the next screen.
        let mut left = viewport.height();
        let heading = self.banner();
        let heading_lines = usize::from(heading.is_some()) * 2;
        let hints = fit_hints(hints_for(self.tab.keys()), width, left);
        left -= hints.len();
        let heading = (left >= heading_lines).then_some(heading).flatten();
        left -= usize::from(heading.is_some()) * 2;
        let note = self
            .note
            .as_ref()
            .map(|note| {
                let mut lines = table::wrap(&note.text, width);
                lines.truncate(NOTE_LINES.min(left));
                Note {
                    lines,
                    level: note.level,
                }
            })
            .filter(|note| !note.lines.is_empty());
        left -= note.as_ref().map_or(0, |note| note.lines.len());

        // The list before the rest, down to a floor: the panel is a box
        // list, and a detail pane that pushed the boxes off it would have
        // taken away more than it gave.
        let natural = if cells.is_empty() { 1 } else { 1 + cells.len() };
        let floor = natural.min(LIST_FLOOR).min(left);
        left -= floor;

        let mut detail = self.detail(width, exists);
        detail.truncate(left);
        left -= detail.len();

        let summary = shown
            .summary
            .clone()
            .filter(|said| left > 0 && !said.is_empty())
            .map(|said| table::truncate_tail(&said, width));
        left -= usize::from(summary.is_some());
        let mut notices: Vec<String> = self
            .notices()
            .into_iter()
            .map(|notice| table::truncate_tail(&notice, width))
            .collect();
        notices.truncate(left);
        left -= notices.len();

        // Anything nothing else wanted goes back to the list.
        let room = floor + left;
        // From the rows already built, rather than building them again.
        let cursor = self
            .key()
            .and_then(|wanted| shown.keys.iter().position(|key| key == wanted));
        // The header and the scroll line are the list's own overhead, and
        // a list with no room for a row spends neither.
        let scrolled = cells.len() + 1 > room;
        let visible = room.saturating_sub(1 + usize::from(scrolled));
        let (rows, cursor, scroll) = listed(cells.clone(), &shown.tones, cursor, visible);

        Screen {
            heading,
            columns,
            rows,
            cursor,
            scroll,
            // A filter that hid every row must say so in its own words.
            // "No boxes yet" over a store full of them would be the
            // screen lying about what it is holding back.
            empty: (cells.is_empty() && room > 0).then(|| shown.empty.clone()),
            detail,
            summary,
            notices,
            note,
            hints,
        }
    }

    /// What the screen says it is narrowed to, if it is.
    ///
    /// Shown rather than implied: rows missing for a reason nobody can
    /// see is the same defect as a key that does nothing, and the caret
    /// is what says the letters are going into the filter rather than
    /// into the keys.
    fn banner(&self) -> Option<String> {
        let tabs: Vec<String> = Tab::ALL
            .iter()
            .map(|tab| {
                if *tab == self.tab {
                    format!("[{}]", tab.title())
                } else {
                    format!(" {} ", tab.title())
                }
            })
            .collect();
        let bar = tabs.join("");
        if self.typing {
            return Some(format!("{bar}   /{}_", self.filter));
        }
        if self.filter.is_empty() {
            return Some(bar);
        }
        Some(format!(
            "{bar}   /{}   {} of {}   esc clears",
            self.filter,
            self.keys().len(),
            self.all()
        ))
    }

    /// How many rows this tab holds before the filter.
    fn all(&self) -> usize {
        match self.tab {
            Tab::Boxes => self.boxes.len(),
            tab => self.sheets.get(&tab).map_or(0, |sheet| sheet.rows.len()),
        }
    }
}

/// The list a new box starts from, a product to run it as, or a workspace
/// to start it in. The cursor on it, nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub items: Vec<String>,
    pub selected: usize,
    heading: &'static str,
    /// What the list is narrowed to, and whether it is being typed. A
    /// role list, a workspace list and the command palette are all this
    /// screen, and all three are worth narrowing.
    filter: String,
    typing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickAction {
    Redraw,
    /// Show this item's permission preview.
    Chosen(usize),
    /// Return to the box list.
    Back,
    /// Show every key this screen answers to.
    Keys,
}

impl Picker {
    pub fn new(items: Vec<String>) -> Self {
        Self::with_heading("start a new box from:", items)
    }

    /// The same list with the cursor already on one item.
    #[must_use]
    pub fn starting_at(self, at: usize) -> Self {
        Picker {
            selected: at.min(self.items.len().saturating_sub(1)),
            ..self
        }
    }

    pub fn with_heading(heading: &'static str, items: Vec<String>) -> Self {
        Picker {
            items,
            selected: 0,
            heading,
            filter: String::new(),
            typing: false,
        }
    }

    /// Which items the filter leaves, as indexes into `items` — so what
    /// comes back from [`PickAction::Chosen`] is an index into the list
    /// the caller handed over, whatever the screen was showing.
    fn shown(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|at| matches(std::slice::from_ref(&self.items[*at]), &self.filter))
            .collect()
    }

    pub fn update(&mut self, press: Press, viewport: Viewport) -> PickAction {
        if self.typing {
            typing(&mut self.filter, &mut self.typing, press);
            self.anchor();
            return PickAction::Redraw;
        }
        let shown = self.shown();
        let at = shown.iter().position(|item| *item == self.selected);
        match command(press, on::PICK) {
            Some(
                key @ (Key::Up | Key::Down | Key::Top | Key::Bottom | Key::PageUp | Key::PageDown),
            ) => {
                let next = moved(at.unwrap_or(0), shown.len(), key, viewport);
                self.selected = shown.get(next).copied().unwrap_or(self.selected);
                PickAction::Redraw
            }
            Some(Key::Enter) if at.is_some() => PickAction::Chosen(self.selected),
            Some(Key::Quit) => PickAction::Back,
            Some(Key::Cancel) if self.filter.is_empty() => PickAction::Back,
            Some(Key::Cancel) => {
                self.filter.clear();
                self.anchor();
                PickAction::Redraw
            }
            Some(Key::Filter) => {
                self.typing = true;
                PickAction::Redraw
            }
            Some(Key::Help) => PickAction::Keys,
            // A list to pick one thing from. Nothing on it can be
            // stopped, removed, reset or moved, and a filter that matches
            // nothing has nothing to choose.
            _ => PickAction::Redraw,
        }
    }

    /// Puts the cursor on an item the filter still shows.
    fn anchor(&mut self) {
        let shown = self.shown();
        if !shown.contains(&self.selected) {
            self.selected = shown.first().copied().unwrap_or(0);
        }
    }

    /// The same screen shape the box list draws, with one column and no
    /// detail: one renderer, so a picker cannot scroll differently from
    /// the list it was opened over.
    #[must_use]
    pub fn screen(&self, viewport: Viewport) -> Screen {
        let viewport = viewport.or_fallback();
        // The cursor column the renderer draws, so an item is never cut
        // to a width the row does not have.
        let width = viewport.width().saturating_sub(GUTTER);
        let mut left = viewport.height();
        let hints = fit_hints(hints_for(on::PICK), width, left);
        left -= hints.len();
        // The heading and the blank line under it, or neither: half of
        // that pair is a blank line saying nothing. While a filter is
        // being typed the heading is the filter, because a list narrowed
        // by something invisible is a list missing rows for no reason.
        let said = if self.typing {
            format!("{}   /{}_", self.heading, self.filter)
        } else if self.filter.is_empty() {
            self.heading.to_owned()
        } else {
            format!("{}   /{}   esc clears", self.heading, self.filter)
        };
        let heading = (left >= 2).then(|| {
            left -= 2;
            table::truncate_tail(&said, width)
        });
        let items = self.shown();
        let at = items.iter().position(|item| *item == self.selected);
        let scrolled = items.len() > left;
        let visible = left.saturating_sub(usize::from(scrolled));
        let cells: Vec<Vec<String>> = items
            .iter()
            .map(|at| vec![table::truncate_tail(&self.items[*at], width)])
            .collect();
        let empty = items.is_empty();
        let (rows, cursor, scroll) = listed(cells, &[], at, visible);
        Screen {
            heading,
            columns: Screen::one_column(width),
            rows,
            cursor,
            scroll,
            empty: (empty && left > 0).then(|| {
                if self.items.is_empty() {
                    "nothing to pick".to_owned()
                } else {
                    format!("nothing matches {}", self.filter)
                }
            }),
            hints,
            ..Screen::default()
        }
    }
}

/// One installed role, as the binary read it off the disk.
///
/// The reading is the binary's — a role is a directory, a symlink or a
/// pointer at a commit — and what the row says about it is this crate's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRow {
    pub name: String,
    /// The directory it points at, or the pinned repository.
    pub from: String,
    /// Whether it can start, in the words `role list` already uses.
    pub state: String,
}

/// The roles tab: what is installed, and whether each can start.
#[must_use]
pub fn roles_sheet(roles: &[RoleRow], at: &str) -> Sheet {
    Sheet {
        columns: ["NAME", "FROM", "STATE"].map(str::to_owned).to_vec(),
        rows: roles
            .iter()
            .map(|role| SheetRow {
                key: role.name.clone(),
                cells: vec![role.name.clone(), role.from.clone(), role.state.clone()],
                // A role that cannot start is still installed, and the
                // row is where you find out one command is missing.
                tone: if role.state.is_empty() {
                    Tone::Plain
                } else {
                    Tone::Bad
                },
                detail: vec![
                    ("name", role.name.clone()),
                    ("from", role.from.clone()),
                    (
                        "state",
                        if role.state.is_empty() {
                            "ready".to_owned()
                        } else {
                            role.state.clone()
                        },
                    ),
                ],
            })
            .collect(),
        empty: format!("no roles installed in {at}"),
        summary: None,
    }
}

/// The store tab: what the data home holds and what of it can go.
///
/// The same items and verdicts `wormhole gc` reports, so the screen and
/// the command cannot disagree about what is provably dead.
#[must_use]
pub fn store_sheet(items: &[crate::gc::Item], sweep: crate::gc::Sweep) -> Sheet {
    let plan = crate::gc::plan(items, sweep);
    let (taken, kept) = crate::gc::weights(&plan);
    let mut summary = format!(
        "{} reclaimable, {} kept",
        crate::gc::Bytes(taken),
        crate::gc::Bytes(kept)
    );
    if plan.offered > 0 {
        summary.push_str(&format!(
            "   {} takes {} more that no box here references",
            cap_for(Key::Widen, on::STORE),
            crate::gc::Bytes(plan.offered)
        ));
    }
    Sheet {
        columns: ["WHAT", "SIZE", "PATH"].map(str::to_owned).to_vec(),
        rows: items
            .iter()
            .map(|item| SheetRow {
                key: item.path.display().to_string(),
                cells: vec![
                    item.verdict.word().to_owned(),
                    crate::gc::Bytes(item.bytes).to_string(),
                    item.path.display().to_string(),
                ],
                tone: if item.verdict.taken_by(sweep) {
                    Tone::Bad
                } else {
                    Tone::Dim
                },
                detail: vec![
                    ("path", item.path.display().to_string()),
                    ("size", crate::gc::Bytes(item.bytes).to_string()),
                    ("what", item.verdict.word().to_owned()),
                    ("why", item.verdict.reason().to_owned()),
                ],
            })
            .collect(),
        empty: "nothing in the data home yet".to_owned(),
        summary: Some(summary),
    }
}

/// The secrets tab. Names only: a value on the screen has been handed to
/// whoever is looking at it.
#[must_use]
pub fn secrets_sheet(names: &[String]) -> Sheet {
    Sheet {
        columns: vec!["NAME".to_owned()],
        rows: names
            .iter()
            .map(|name| SheetRow {
                key: name.clone(),
                cells: vec![name.clone()],
                tone: Tone::Plain,
                detail: vec![
                    ("name", name.clone()),
                    ("value", "kept, never shown".to_owned()),
                    ("used", "by every box whose recipe asks for it".to_owned()),
                ],
            })
            .collect(),
        empty: "no secrets kept".to_owned(),
        summary: None,
    }
}

/// The host tab: what `wormhole doctor` probed, and its verdict.
#[must_use]
pub fn doctor_sheet(verdict: &crate::doctor::Verdict) -> Sheet {
    use crate::doctor::{Outcome, Status};
    Sheet {
        columns: ["PROBE", "RESULT"].map(str::to_owned).to_vec(),
        rows: verdict
            .results
            .iter()
            .map(|result| {
                let (word, tone, detail) = match &result.outcome {
                    Outcome::Pass => ("ok", Tone::Good, String::new()),
                    Outcome::Fail(why) => ("FAIL", Tone::Bad, why.clone()),
                    Outcome::Info(said) => ("info", Tone::Dim, said.clone()),
                };
                SheetRow {
                    key: result.probe.name().to_owned(),
                    cells: vec![result.probe.name().to_owned(), word.to_owned()],
                    tone,
                    detail: vec![
                        ("probe", result.probe.name().to_owned()),
                        ("result", word.to_owned()),
                        (
                            "detail",
                            if detail.is_empty() {
                                "-".to_owned()
                            } else {
                                detail
                            },
                        ),
                    ],
                }
            })
            .collect(),
        empty: "nothing probed yet".to_owned(),
        summary: Some(
            match verdict.status {
                Status::Ok => "verdict: ok",
                Status::Unusable => "verdict: this host cannot run wormhole",
            }
            .to_owned(),
        ),
    }
}

/// One line of the command palette: what it is, the command line it is,
/// and what pressing enter does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub does: String,
    /// The exact command line this runs. Shown, because a surface that
    /// did the tool's work in its own words would become a second way to
    /// describe the tool, and the two would drift.
    pub command: String,
    pub action: Action,
}

impl Entry {
    fn new(does: &str, command: &str, action: Action) -> Entry {
        Entry {
            does: does.to_owned(),
            command: format!("wormhole {command}"),
            action,
        }
    }

    /// The row the palette draws.
    #[must_use]
    pub fn line(&self, width: usize) -> String {
        let room = width.saturating_sub(self.does.len() + GAP);
        format!(
            "{:label$}{}",
            self.does,
            table::truncate_tail(&self.command, room),
            label = self.does.len() + GAP
        )
    }
}

impl Tui {
    /// Every command this tool has, bound to what the cursor is on.
    ///
    /// The gap this closes: a panel with one key per verb can only ever
    /// have as many verbs as there are letters, and the rest of the tool
    /// stays on the command line. This is the rest of the tool.
    #[must_use]
    pub fn commands(&self) -> Vec<Entry> {
        let mut said = Vec::new();
        let box_here = self.here();
        let id = box_here.map(|entry| entry.record.id.clone());
        let running = box_here.and_then(|entry| entry.running);
        let row = self.key().map(str::to_owned);

        said.push(Entry::new("start a box here", "box --new", Action::New));
        if let Some(id) = &id {
            if let Some(pid) = running {
                said.push(Entry::new(
                    "join this box",
                    &format!("attach {id}"),
                    Action::Attach(id.clone()),
                ));
                said.push(Entry::new(
                    "stop this box",
                    &format!("stop {id}"),
                    Action::Do(Act::Stop(pid)),
                ));
            } else {
                said.push(Entry::new(
                    "start this box again",
                    &format!("box --id {id}"),
                    Action::Workspaces(id.clone()),
                ));
                said.push(Entry::new(
                    "name this box",
                    &format!("rename {id} NAME"),
                    Action::Name(id.clone()),
                ));
                said.push(Entry::new(
                    "empty this box's home",
                    &format!("reset {id}"),
                    Action::Do(Act::Reset(id.clone())),
                ));
                said.push(Entry::new(
                    "take this box away",
                    &format!("remove {id}"),
                    Action::Do(Act::Remove(id.clone())),
                ));
            }
        }
        said.push(Entry::new(
            "every box",
            "ps --all",
            Action::Goto(Tab::Boxes),
        ));

        said.push(Entry::new(
            "installed roles",
            "role list",
            Action::Goto(Tab::Roles),
        ));
        said.push(Entry::new(
            "install a role",
            "role add DIR",
            Action::Install,
        ));
        if self.tab == Tab::Roles
            && let Some(name) = &row
        {
            said.push(Entry::new(
                "this role's whole recipe",
                &format!("role show {name}"),
                Action::Show(name.clone()),
            ));
            said.push(Entry::new(
                "uninstall this role",
                &format!("role remove {name}"),
                Action::Do(Act::Uninstall(name.clone())),
            ));
        }

        said.push(Entry::new(
            "what the store holds",
            "gc",
            Action::Goto(Tab::Store),
        ));
        said.push(Entry::new(
            "give back what is proven dead",
            "gc --delete",
            Action::Do(Act::Sweep(self.widened)),
        ));
        said.push(Entry::new(
            "kept secrets",
            "secret list",
            Action::Goto(Tab::Secrets),
        ));
        said.push(Entry::new("keep a secret", "secret set NAME", Action::Keep));
        if self.tab == Tab::Secrets
            && let Some(name) = &row
        {
            said.push(Entry::new(
                "forget this secret",
                &format!("secret remove {name}"),
                Action::Do(Act::Forget(name.clone())),
            ));
        }

        said.push(Entry::new(
            "can this host run boxes",
            "doctor",
            Action::Goto(Tab::Doctor),
        ));
        // The rest print and stop, so they are run and read rather than
        // acted on. Run as themselves, so what the line says is what
        // happens.
        for (does, command) in [
            ("write a wormhole.toml here", "init"),
            ("build this recipe's image now", "build"),
        ] {
            said.push(Entry::new(
                does,
                command,
                Action::Run(vec![command.to_owned()]),
            ));
        }
        // These two are text this crate already holds, so a child
        // process to obtain them would be a fork to read a constant.
        said.push(Entry::new(
            "the whole tool on one page",
            "help",
            Action::Read(crate::help::page()),
        ));
        said.push(Entry::new(
            "which wormhole this is",
            "version",
            Action::Read(format!("wormhole {}\n", crate::VERSION)),
        ));
        said.push(Entry::new(
            "the keys of this screen",
            "",
            Action::Read(keys_page(self.tab.keys())),
        ));
        said
    }

    /// Moves to a tab from somewhere other than the tab key.
    pub fn goto(&mut self, tab: Tab) -> Action {
        self.go(tab)
    }
}

/// What a start can be told, as a screen.
///
/// The gap this closes: the panel's `n` built exactly `--new [--role R]
/// [--run P]` and nothing else, so every flag that changes what a start
/// *is* — the name, the login, a variable, a command instead of the
/// agent — was reachable only from a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    fields: Vec<Filled>,
    selected: usize,
    editing: Option<Ask>,
}

/// One line of a form: what it is called, what it is set to, and what it
/// becomes on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Filled {
    label: &'static str,
    flag: &'static str,
    value: String,
    /// The values this field may take. Empty means anything typed.
    /// The values this field cycles through, or empty for anything
    /// typed. Borrowed, because a field with a short list of answers has
    /// the same short list every time it is built.
    cycle: &'static [&'static str],
    /// What an empty value means, for the line that says so.
    absent: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    Redraw,
    /// Start a box with these arguments.
    Start(Vec<String>),
    Back,
}

/// What the login field offers, in the manifest's own words.
const LOGINS: &[&str] = &["", "none", "copy", "share"];

impl Form {
    /// A form for a start from `role`, running `product`.
    ///
    /// Both are already chosen by the time this is up — the role list and
    /// the product list are how you got here — so they are shown and not
    /// asked again.
    #[must_use]
    pub fn new(role: Option<&str>, product: Option<&str>) -> Form {
        Form {
            fields: vec![
                Filled {
                    label: "recipe",
                    flag: "--role",
                    value: role.unwrap_or_default().to_owned(),
                    cycle: &[],
                    absent: "this workspace's wormhole.toml",
                },
                Filled {
                    label: "product",
                    flag: "--run",
                    value: product.unwrap_or_default().to_owned(),
                    cycle: &[],
                    absent: "the recipe's own",
                },
                Filled {
                    label: "name",
                    flag: "--as",
                    value: String::new(),
                    cycle: &[],
                    absent: "none; the id names it",
                },
                Filled {
                    label: "login",
                    flag: "--credentials",
                    value: String::new(),
                    cycle: LOGINS,
                    absent: "what the recipe says",
                },
                Filled {
                    label: "variable",
                    flag: "--env",
                    value: String::new(),
                    cycle: &[],
                    absent: "only what the recipe declares",
                },
                Filled {
                    label: "instead of the agent",
                    flag: "--",
                    value: String::new(),
                    cycle: &[],
                    absent: "nothing; the agent runs",
                },
            ],
            selected: 0,
            editing: None,
        }
    }

    /// The command line this form is. What the screen shows and what the
    /// start is given are one string, so the screen cannot promise
    /// something the start does not do.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec!["--new".to_owned()];
        for field in self.fields.iter().filter(|field| !field.value.is_empty()) {
            argv.push(field.flag.to_owned());
            // A command is the rest of the line, not one argument.
            if field.flag == "--" {
                argv.extend(field.value.split_whitespace().map(str::to_owned));
            } else {
                argv.push(field.value.clone());
            }
        }
        argv
    }

    /// What the line would read as, for the screen that shows it.
    #[must_use]
    pub fn command(&self) -> String {
        format!("wormhole box {}", self.argv().join(" "))
    }

    pub fn update(&mut self, press: Press, viewport: Viewport) -> FormAction {
        if let Some(asking) = &mut self.editing {
            match asking.update(press) {
                AskAction::Redraw => return FormAction::Redraw,
                AskAction::Back => self.editing = None,
                AskAction::Given(value) => {
                    self.fields[self.selected].value = value;
                    self.editing = None;
                }
            }
            return FormAction::Redraw;
        }
        match command(press, on::PICK) {
            Some(Key::Quit | Key::Cancel) => FormAction::Back,
            Some(key @ (Key::Up | Key::Down | Key::Top | Key::Bottom)) => {
                self.selected = moved(self.selected, self.fields.len() + 1, key, viewport);
                FormAction::Redraw
            }
            // The row under the fields is the line itself, and enter on
            // it is the start.
            Some(Key::Enter) if self.selected == self.fields.len() => {
                FormAction::Start(self.argv())
            }
            Some(Key::Enter) => {
                let field = &self.fields[self.selected];
                if field.cycle.is_empty() {
                    self.editing = Some(
                        Ask::new(format!("{} {}:", field.flag, field.label))
                            .filled(field.value.clone()),
                    );
                } else {
                    // A field with a short list of answers cycles rather
                    // than asking: three keystrokes to type `share` when
                    // there are only three answers is friction.
                    let at = field.cycle.iter().position(|one| *one == field.value);
                    let next = field.cycle[(at.unwrap_or(0) + 1) % field.cycle.len()];
                    self.fields[self.selected].value = next.to_owned();
                }
                FormAction::Redraw
            }
            _ => FormAction::Redraw,
        }
    }

    #[must_use]
    pub fn screen(&self, viewport: Viewport) -> Screen {
        if let Some(asking) = &self.editing {
            return asking.screen(viewport);
        }
        let viewport = viewport.or_fallback();
        let width = viewport.width().saturating_sub(GUTTER);
        let label = self
            .fields
            .iter()
            .map(|field| field.label.len())
            .max()
            .unwrap_or(0)
            + GAP;
        let mut rows: Vec<Row> = self
            .fields
            .iter()
            .map(|field| Row {
                cells: vec![table::truncate_tail(
                    &format!(
                        "{:label$}{}",
                        field.label,
                        if field.value.is_empty() {
                            field.absent
                        } else {
                            &field.value
                        }
                    ),
                    width,
                )],
                tone: if field.value.is_empty() {
                    Tone::Dim
                } else {
                    Tone::Plain
                },
            })
            .collect();
        rows.push(Row {
            cells: vec![table::truncate_tail(&self.command(), width)],
            tone: Tone::Good,
        });
        Screen {
            heading: Some("start a box with:".to_owned()),
            columns: Screen::one_column(width),
            rows,
            cursor: Some(self.selected),
            hints: fit_hints(
                vec![
                    Hint {
                        cap: "enter",
                        does: "set, or start on the last line",
                    },
                    Hint {
                        cap: "q",
                        does: "back",
                    },
                ],
                width,
                HINT_LINES,
            ),
            ..Screen::default()
        }
    }
}

/// One line of text being typed: a name for a box, a role to install, a
/// secret to keep.
///
/// Its own screen rather than a field on another, because what it is for
/// is always a question that has just been asked, and the answer is the
/// only thing the keyboard is doing until it is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub label: String,
    pub value: String,
    /// Drawn as dots. A secret that appears on the screen has been
    /// handed to whoever is looking at it.
    pub masked: bool,
    /// Why the value as typed cannot be used, or nothing.
    refusal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskAction {
    Redraw,
    Given(String),
    Back,
}

impl Ask {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Ask {
        Ask {
            label: label.into(),
            value: String::new(),
            masked: false,
            refusal: None,
        }
    }

    #[must_use]
    pub fn masked(self) -> Ask {
        Ask {
            masked: true,
            ..self
        }
    }

    /// Starts with something already in it, for a question that has an
    /// answer to change rather than one to give.
    #[must_use]
    pub fn filled(self, value: impl Into<String>) -> Ask {
        Ask {
            value: value.into(),
            ..self
        }
    }

    /// Says why the answer just given cannot be used, and keeps it on the
    /// screen to be edited — retyping a name because one character was
    /// taken is the kind of friction that makes people use the CLI.
    pub fn refuse(&mut self, why: impl Into<String>) {
        self.refusal = Some(why.into());
    }

    pub fn update(&mut self, press: Press) -> AskAction {
        self.refusal = None;
        match press {
            // An empty answer is no answer, and enter on one would look
            // like it had been given.
            Press::Enter if !self.value.is_empty() => AskAction::Given(self.value.clone()),
            Press::Escape => AskAction::Back,
            Press::Backspace => {
                self.value.pop();
                AskAction::Redraw
            }
            Press::Typed(c) => {
                self.value.push(c);
                AskAction::Redraw
            }
            _ => AskAction::Redraw,
        }
    }

    #[must_use]
    pub fn screen(&self, viewport: Viewport) -> Screen {
        let viewport = viewport.or_fallback();
        let width = viewport.width().saturating_sub(GUTTER);
        let shown = if self.masked {
            "*".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        Screen {
            heading: Some(table::truncate_tail(&self.label, width)),
            columns: Screen::one_column(width),
            rows: vec![Row {
                // The caret, so a screen with nothing typed yet still
                // reads as a screen that is waiting for typing.
                cells: vec![table::truncate_head(&format!("{shown}_"), width)],
                tone: Tone::Plain,
            }],
            cursor: Some(0),
            note: self.refusal.as_ref().map(|why| Note {
                lines: table::wrap(why, viewport.width()),
                level: Level::Refusal,
            }),
            hints: fit_hints(
                vec![
                    Hint {
                        cap: "enter",
                        does: "give it",
                    },
                    Hint {
                        cap: "esc",
                        does: "back",
                    },
                ],
                width,
                HINT_LINES,
            ),
            ..Screen::default()
        }
    }
}

/// Everything the box will see, on one screen, before anything starts.
/// What is absent reads as absent — a skipped line would hide exactly
/// what the user came to check.
pub fn preview(manifest: &Manifest, source: &str, image_ready: bool) -> String {
    let mut text = format!("manifest: {source}\n");
    if let Some(name) = &manifest.name {
        text.push_str(&format!("name: {name}\n"));
    }
    match manifest.agent.run.as_slice() {
        [] => text.push_str("agent: none\n"),
        names => {
            text.push_str(&format!("agent: {}", names.join(", ")));
            if names.len() == 1
                && let Some(model) = &manifest.agent.model
            {
                text.push_str(&format!(" (model {model})"));
            }
            text.push('\n');
        }
    }
    text.push_str("\nthe box will see:\n");
    text.push_str("  workspace (read-write)\n");
    if manifest.access.grants.is_empty() {
        text.push_str("  grants: none\n");
    } else {
        for grant in &manifest.access.grants {
            text.push_str(&format!("  {grant}\n"));
        }
    }
    text.push_str(&format!(
        "  host CA bundle: {}\n",
        if manifest.access.host_ca {
            "trusted"
        } else {
            "not shared"
        }
    ));
    match manifest.access.dns {
        Some(dns) => text.push_str(&format!("  dns: {dns}\n")),
        None => text.push_str("  dns: the host's resolver\n"),
    }
    // What the agent logs in with is the one host secret a box can be
    // handed; a preview that hid it would understate exactly the thing
    // this screen exists to show.
    text.push_str(match manifest.access.credentials {
        crate::manifest::Credentials::None => "  credentials: none — /login in the box\n",
        crate::manifest::Credentials::Copy => {
            "  credentials: copy — the host's login, copied once\n"
        }
        crate::manifest::Credentials::Share => {
            "  credentials: share — the host's login file, bound read-write\n"
        }
    });
    text.push_str(match manifest.agent.resume {
        crate::manifest::Resume::Here => "  home: kept per box; each workspace starts its own\n",
        crate::manifest::Resume::Anywhere => {
            "  home: one for every workspace; each sees what the others left\n"
        }
    });
    if manifest.env.is_empty() {
        text.push_str("\nenv: none\n");
    } else {
        text.push_str("\nenv:\n");
        for (name, declared) in &manifest.env {
            match &declared.fixed {
                Some(fixed) => text.push_str(&format!("  {name} = {fixed}\n")),
                None if declared.default.is_empty() => {
                    text.push_str(&format!("  {name} (host value, empty default)\n"));
                }
                None => text.push_str(&format!(
                    "  {name} (host value, default {})\n",
                    declared.default
                )),
            }
        }
    }
    text.push('\n');
    match &manifest.agent.preflight {
        Some(hook) => text.push_str(&format!("preflight: {hook}\n")),
        None => text.push_str("preflight: none\n"),
    }
    match &manifest.agent.instructions {
        Some(file) => text.push_str(&format!("instructions: {file}\n")),
        None => text.push_str("instructions: built-in only\n"),
    }
    match &manifest.agent.rules {
        Some(dir) => text.push_str(&format!("rules: {dir}\n")),
        None => text.push_str("rules: none\n"),
    }
    text.push_str(&format!(
        "image: {}\n",
        if image_ready {
            "ready"
        } else {
            "not built — will build first"
        }
    ));
    // What the image is made of, last because it is the longest and the
    // only part that runs commands. A preview that showed the grants and
    // hid these lines would be the shape of diligence without its
    // substance — for a role somebody else wrote, this is the screen.
    text.push_str(&format!(
        "\nbuilt from:\n  {}\n  sha256 {}\n",
        manifest.image.base, manifest.image.base_sha256
    ));
    if manifest.image.packages.is_empty() {
        text.push_str("  packages: none\n");
    } else {
        text.push_str(&format!(
            "  packages: {}\n",
            manifest.image.packages.join(" ")
        ));
    }
    // A build line that reads `tar -xzf /tmp/rtk.tar.gz` says nothing
    // about where those bytes came from, so the URL and the digest are
    // named beside the command that consumes them.
    for artifact in &manifest.image.artifacts {
        text.push_str(&format!(
            "  fetched: {}\n    sha256 {}\n    into {}\n",
            artifact.url, artifact.sha256, artifact.into
        ));
    }
    if manifest.image.build.is_empty() {
        text.push_str("  build: nothing runs\n");
    } else {
        text.push_str("  build:\n");
        for line in &manifest.image.build {
            text.push_str(&format!("    {line}\n"));
        }
    }
    text
}

/// What a start in a workspace the box has never served has to be read
/// before it happens: the recipe, and the tree it is about to be pointed
/// at. Widening a box's reach is what `wormhole box --id X` does when it
/// is typed in a new tree, and a keystroke must not do it more quietly.
pub fn widening(id: &str, here: &Path) -> String {
    format!(
        "box {id} has never run in {}.\n\
         Starting it here points its kept home — its history, its logins, \
         whatever it installed — at this tree, and adds this tree to the \
         list it can be resumed in.\n",
        here.display()
    )
}

/// A block of text as much of it as fits, and where in it that was.
///
/// The permission preview is forty lines and more, and until it was
/// paged the lines past the bottom of the terminal were drawn off the
/// screen — including, for a role somebody else wrote, the `[image]
/// build` lines that are the whole reason the screen exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub lines: Vec<String>,
    pub scroll: Option<Scroll>,
    pub hints: Vec<Vec<Hint>>,
}

/// What a block of text is on the screen for: something to agree to, or
/// something to have read. The keys differ, and a screen that offered
/// `enter start` over a refusal would be offering a key that does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Confirm,
    Message,
}

impl Reading {
    /// Which bindings this kind of page answers to. Here rather than in
    /// the binary, so the two cannot disagree about what `enter` does.
    #[must_use]
    pub fn keys(self) -> u8 {
        match self {
            Reading::Confirm => on::CONFIRM,
            Reading::Message => on::MESSAGE,
        }
    }
}

/// How many lines of `text` a terminal shows, starting at `at`.
#[must_use]
pub fn page(text: &str, viewport: Viewport, at: usize, reading: Reading) -> Page {
    let viewport = viewport.or_fallback();
    let wrapped = paged(text, viewport);
    let room = viewport.height().saturating_sub(1); // the hints
    let first = at.min(wrapped.len().saturating_sub(room));
    let shown = wrapped.len().min(first + room);
    Page {
        lines: wrapped[first.min(shown)..shown].to_vec(),
        scroll: (shown - first < wrapped.len()).then(|| Scroll {
            first,
            shown: shown - first,
            total: wrapped.len(),
        }),
        hints: fit_hints(
            hints_for(match reading {
                Reading::Confirm => on::CONFIRM,
                Reading::Message => on::MESSAGE,
            }),
            viewport.width(),
            HINT_LINES,
        ),
    }
}

/// Where a scroll key moves a reader in `text`.
#[must_use]
pub fn scroll_to(text: &str, viewport: Viewport, at: usize, press: Press) -> usize {
    let viewport = viewport.or_fallback();
    let room = viewport.height().saturating_sub(1);
    let last = paged(text, viewport).len().saturating_sub(room);
    // A screenful, less the line that keeps a reader's place.
    let page = room.saturating_sub(1).max(1);
    match command(press, on::PAGES) {
        Some(key) => step(at, last, page, key),
        None => at,
    }
}

/// `text` as lines that fit the terminal, keeping its own line breaks and
/// reflowing only what is too wide for one.
///
/// A line that already fits is passed through untouched, indentation and
/// all. The preview says what is inside a recipe by indenting it — a
/// build line under `build:`, a digest under the URL it belongs to — so a
/// wrap that reflowed every line would flatten the one thing that says
/// which line belongs to which.
fn paged(text: &str, viewport: Viewport) -> Vec<String> {
    text.lines()
        .flat_map(|line| {
            if table::display_width(line) <= viewport.width() {
                return vec![line.to_owned()];
            }
            table::wrap(line, viewport.width())
        })
        .collect()
}

/// What the product list is choosing. One string, so the panel's `n` and
/// `wormhole box` cannot ask the same question in two wordings.
pub const RUN_HEADING: &str = "run as:";

/// What the workspace list is choosing.
pub const WHERE_HEADING: &str = "start this box in:";

/// What the command palette is choosing.
pub const COMMANDS_HEADING: &str = "every command, on what the cursor is on:";

/// Products a start should offer a pick for. `None` when there is
/// nothing to choose: no agent, or only one.
#[must_use]
pub fn pick_run(manifest: &Manifest) -> Option<&[String]> {
    let names = manifest.agent.run.as_slice();
    (names.len() > 1).then_some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn manifest(extra: &str) -> crate::manifest::Manifest {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let version = crate::manifest::VERSION;
        crate::manifest::parse(&format!(
            "version = {version}\n{extra}[image]\nbase = \"https://x.test/r.tar.gz\"\nbase_sha256 = \"{digest}\"\n"
        ))
        .expect("valid manifest")
    }

    /// A terminal wide and tall enough that nothing has to be fitted, so
    /// a test about what a key does is not also a test about geometry.
    const ROOMY: Viewport = Viewport {
        width: 200,
        height: 60,
    };

    fn there(_: &Path) -> bool {
        true
    }

    fn screen_of(tui: &Tui) -> Screen {
        tui.screen(ROOMY, 0, &there)
    }

    fn note_of(tui: &Tui) -> String {
        screen_of(tui)
            .note
            .map(|note| note.text())
            .unwrap_or_default()
    }

    /// Every cell of the visible table, for the assertions that do not
    /// care which column something landed in.
    fn text_of(screen: &Screen) -> String {
        let mut said = screen.heading.clone().unwrap_or_default();
        said.push('\n');
        said.push_str(&screen.empty.clone().unwrap_or_default());
        for row in &screen.rows {
            said.push_str(&row.cells.join(" "));
            said.push('\n');
        }
        for field in &screen.detail {
            said.push_str(&format!("{}\n", field.text));
        }
        for notice in &screen.notices {
            said.push_str(notice);
            said.push('\n');
        }
        if let Some(note) = &screen.note {
            said.push_str(&note.text());
            said.push('\n');
        }
        for line in &screen.hints {
            for hint in line {
                said.push_str(&format!("{} {}  ", hint.cap, hint.does));
            }
        }
        said
    }

    #[test]
    fn the_picker_moves_chooses_and_goes_back() {
        let mut picker = Picker::new(vec!["workspace".to_owned(), "role alphaca".to_owned()]);
        assert_eq!(picker.update(Press::Typed('j'), ROOMY), PickAction::Redraw);
        assert_eq!(picker.update(Press::Enter, ROOMY), PickAction::Chosen(1));
        assert_eq!(picker.update(Press::Typed('k'), ROOMY), PickAction::Redraw);
        assert_eq!(picker.update(Press::Enter, ROOMY), PickAction::Chosen(0));
        assert_eq!(picker.update(Press::Typed('q'), ROOMY), PickAction::Back);
    }

    #[test]
    fn the_picker_cursor_stays_inside_the_list() {
        let mut picker = Picker::new(vec!["only".to_owned()]);
        picker.update(Press::Typed('j'), ROOMY);
        picker.update(Press::Typed('j'), ROOMY);
        assert_eq!(picker.update(Press::Enter, ROOMY), PickAction::Chosen(0));
    }

    #[test]
    fn the_picker_screen_marks_the_selection_and_hints() {
        let mut picker = Picker::new(vec!["workspace".to_owned(), "role alphaca".to_owned()]);
        picker.update(Press::Typed('j'), ROOMY);
        let screen = picker.screen(ROOMY);
        assert_eq!(screen.heading.as_deref(), Some("start a new box from:"));
        assert_eq!(screen.rows[0].cells[0], "workspace");
        assert_eq!(screen.rows[1].cells[0], "role alphaca");
        assert_eq!(screen.cursor, Some(1));
        assert!(text_of(&screen).contains("enter choose"), "{screen:?}");
    }

    /// A picker with more items than the terminal has lines must show the
    /// one the cursor is on. A role list on a short terminal is the case.
    #[test]
    fn a_picker_longer_than_the_terminal_shows_the_row_the_cursor_is_on() {
        let mut picker = Picker::new((0..40).map(|n| format!("role {n}")).collect());
        for _ in 0..30 {
            picker.update(Press::Typed('j'), ROOMY);
        }
        let screen = picker.screen(Viewport {
            width: 80,
            height: 10,
        });
        let at = screen.cursor.expect("a cursor");
        assert_eq!(screen.rows[at].cells[0], "role 30");
        assert!(
            screen.rows.len() <= 7,
            "{} rows in 10 lines",
            screen.rows.len()
        );
        assert_eq!(screen.scroll.map(|s| s.total), Some(40));
    }

    /// The preview is the whole point: everything the box will see, on one
    /// screen, before anything starts.
    #[test]
    fn the_preview_names_everything_the_box_will_see() {
        let m = manifest(concat!(
            "name = \"Alphaca\"\n",
            "[agent]\n",
            "run = \"claude\"\n",
            "model = \"claude-fable-5\"\n",
            "instructions = \"ROLE.md\"\n",
            "preflight = \"hooks/preflight.sh\"\n",
            "rules = \"rules\"\n",
            "[access]\n",
            "grants = [\"~/some/path\"]\n",
            "dns = \"1.1.1.1\"\n",
            "host_ca = true\n",
            "credentials = \"copy\"\n",
            "[env.SHELL]\nfixed = \"/bin/bash\"\n",
            "[env.ANTHROPIC_API_KEY]\ndefault = \"\"\n",
        ));
        let view = preview(&m, "role alphaca", true);
        for expected in [
            "role alphaca",
            "Alphaca",
            "claude",
            "claude-fable-5",
            "workspace (read-write)",
            "~/some/path",
            "host CA bundle: trusted",
            "dns: 1.1.1.1",
            "credentials: copy — the host's login, copied once",
            "SHELL = /bin/bash",
            "ANTHROPIC_API_KEY (host value, empty default)",
            "preflight: hooks/preflight.sh",
            "instructions: ROLE.md",
            "rules: rules",
            "image: ready",
            "home: kept per box; each workspace starts its own",
        ] {
            assert!(view.contains(expected), "missing {expected:?} in:\n{view}");
        }
    }

    /// A shared home is every project's history in reach of every other,
    /// so the screen that asks says so.
    #[test]
    fn the_preview_says_when_one_home_serves_every_workspace() {
        let m = manifest("[agent]\nrun = \"claude\"\nresume = \"anywhere\"\n");
        let view = preview(&m, "role alphaca", true);
        assert!(
            view.contains("home: one for every workspace; each sees what the others left"),
            "{view}"
        );
    }

    #[test]
    fn a_role_with_one_product_skips_the_picker() {
        assert_eq!(pick_run(&manifest("[agent]\nrun = \"claude\"\n")), None);
        assert_eq!(pick_run(&manifest("")), None);
    }

    #[test]
    fn a_role_with_several_products_is_a_list_to_pick_from() {
        let m = manifest("[agent]\nrun = [\"claude\", \"codex\", \"grok\"]\n");
        assert_eq!(
            pick_run(&m).map(<[String]>::to_vec),
            Some(vec![
                "claude".to_owned(),
                "codex".to_owned(),
                "grok".to_owned()
            ])
        );
    }

    #[test]
    fn a_product_picker_says_what_it_is_choosing() {
        let picker =
            Picker::with_heading(RUN_HEADING, vec!["claude".to_owned(), "grok".to_owned()]);
        let screen = picker.screen(ROOMY);
        assert_eq!(screen.heading.as_deref(), Some("run as:"));
        assert_eq!(screen.rows[0].cells[0], "claude");
        assert_eq!(screen.cursor, Some(0));
    }

    /// Pointing a box's kept home at a tree it has never seen is what
    /// `wormhole box --id X` does when it is typed in a new tree. A key
    /// that did it without saying so would be the quieter path to the
    /// louder thing.
    #[test]
    fn widening_a_box_says_which_tree_and_what_it_means() {
        let said = widening("a3f9c1e40b2d", Path::new("/home/me/other"));
        assert!(said.contains("a3f9c1e40b2d"), "{said}");
        assert!(said.contains("/home/me/other"), "{said}");
        assert!(said.contains("never run"), "{said}");
    }

    #[test]
    fn the_preview_lists_every_product_a_role_may_run() {
        let m = manifest("[agent]\nrun = [\"claude\", \"codex\", \"grok\"]\n");
        let view = preview(&m, "role alphaca", false);
        assert!(view.contains("agent: claude, codex, grok"), "{view}");
        assert!(!view.contains("model"), "{view}");
    }

    /// The preview is what stands between a role somebody else wrote and
    /// that role's shell running on this machine. `[image] build` is the
    /// part that executes, so a preview that showed grants and hid the
    /// build lines would look like diligence while omitting the point.
    #[test]
    fn the_preview_shows_the_shell_the_image_is_built_by() {
        let m = crate::manifest::parse(&format!(
            concat!(
                "version = {version}\n",
                "[agent]\nrun = \"claude\"\n",
                "[image]\n",
                "base = \"https://x.test/r.tar.gz\"\n",
                "base_sha256 = \"{digest}\"\n",
                "packages = [\"git\", \"nodejs\"]\n",
                "build = [\"wget -qO /tmp/x https://x.test/x\", \"sh /tmp/x\"]\n",
            ),
            version = crate::manifest::VERSION,
            digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .expect("valid manifest");
        let view = preview(&m, "role stranger", false);
        for expected in [
            "https://x.test/r.tar.gz",
            "0123456789abcdef",
            "git",
            "nodejs",
            "wget -qO /tmp/x https://x.test/x",
            "sh /tmp/x",
        ] {
            assert!(view.contains(expected), "missing {expected:?} in:\n{view}");
        }
    }

    /// What is absent must read as absent, never be omitted — a preview
    /// that skips a line hides exactly what the user came to check.
    #[test]
    fn the_preview_says_what_the_box_does_not_get() {
        let view = preview(&manifest(""), "workspace", false);
        for expected in [
            "grants: none",
            "host CA bundle: not shared",
            "dns: the host's resolver",
            "credentials: none — /login in the box",
            "env: none",
            "preflight: none",
            "rules: none",
            "image: not built",
            "packages: none",
            "build: nothing runs",
        ] {
            assert!(view.contains(expected), "missing {expected:?} in:\n{view}");
        }
    }

    /// A build line that reads `tar -xzf /tmp/rtk.tar.gz` says nothing
    /// about where those bytes came from. Hiding the URL and the digest
    /// while showing the command that consumes them would be the shape of
    /// diligence without its substance — this is the screen that stands
    /// between a role somebody else wrote and this machine.
    #[test]
    fn the_preview_names_every_artifact_the_build_is_handed() {
        let manifest = crate::manifest::parse(&format!(
            "version = {}\n[image]\nbase = \"https://x.test/r.tar.gz\"\n\
             base_sha256 = \"{}\"\nbuild = [\"tar -xzf /tmp/rtk.tar.gz\"]\n\
             [[image.artifact]]\nurl = \"https://x.test/rtk.tar.gz\"\n\
             sha256 = \"{}\"\ninto = \"/tmp/rtk.tar.gz\"\n",
            crate::manifest::VERSION,
            "a".repeat(64),
            "b".repeat(64)
        ))
        .expect("valid");
        let view = preview(&manifest, "role at /r", false);
        assert!(view.contains("https://x.test/rtk.tar.gz"), "{view}");
        assert!(view.contains(&"b".repeat(64)), "{view}");
        assert!(view.contains("/tmp/rtk.tar.gz"), "{view}");
    }

    /// A scan from pids: `Some` is a running box, `None` an idle one.
    fn scan(pids: &[Option<u32>]) -> crate::home::Scan {
        scan_of(&pids.iter().map(|pid| (*pid, 0)).collect::<Vec<_>>())
    }

    /// The same, where each box also has a time it last ran — which is
    /// what the list is sorted by, and so what can move a row.
    fn scan_of(boxes: &[(Option<u32>, u64)]) -> crate::home::Scan {
        crate::home::Scan {
            boxes: boxes
                .iter()
                .enumerate()
                .map(|(n, (pid, started))| Listing {
                    record: crate::home::Record {
                        key: format!("w-{n:012x}"),
                        id: format!("{n:012x}"),
                        workspace: PathBuf::from("/w"),
                        earlier: Vec::new(),
                        role: None,
                        source: None,
                        alias: None,
                        name: None,
                        agent: None,
                        created_unix: 0,
                        started_unix: *started,
                    },
                    running: *pid,
                })
                .collect(),
            problems: Vec::new(),
        }
    }

    /// A box list from pids: `Some` is a running box, `None` an idle one.
    fn boxes(pids: &[Option<u32>]) -> Tui {
        let mut tui = Tui::default();
        tui.refresh(scan(pids));
        tui
    }

    fn running(pids: &[u32]) -> Tui {
        boxes(&pids.iter().copied().map(Some).collect::<Vec<_>>())
    }

    #[test]
    fn the_cursor_stays_inside_the_list() {
        let mut tui = running(&[1, 2]);
        assert_eq!(tui.update(Press::Typed('k'), ROOMY), Action::Redraw);
        assert_eq!(tui.selected(), Some("000000000000"));
        tui.update(Press::Typed('j'), ROOMY);
        tui.update(Press::Typed('j'), ROOMY);
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(tui.selected(), Some("000000000001"));
    }

    /// The bug this pins: the cursor was an index into a list sorted by
    /// when each box last ran, rescanned once a second. A box started in
    /// another terminal re-sorted the list under the cursor, and `d` —
    /// which does not ask — then stopped whichever box had taken the row.
    #[test]
    fn a_refresh_that_reorders_the_list_keeps_the_cursor_on_the_same_box() {
        let mut tui = Tui::default();
        tui.refresh(scan_of(&[(Some(10), 100), (Some(20), 50)]));
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(tui.selected(), Some("000000000001"));
        // Box 0 runs again, so it sorts ahead and the rows swap.
        tui.refresh(scan_of(&[(Some(10), 300), (Some(20), 50)]));
        assert_eq!(tui.selected(), Some("000000000001"));
        assert_eq!(
            tui.update(Press::Typed('d'), ROOMY),
            Action::Do(Act::Stop(20))
        );
    }

    /// When the box under the cursor is gone there is no id to hold on
    /// to, so the cursor holds its place on the screen instead.
    #[test]
    fn a_refresh_that_drops_the_selected_box_leaves_the_cursor_where_it_was() {
        let mut tui = running(&[10, 20, 30]);
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(tui.selected(), Some("000000000001"));
        let mut left = scan(&[Some(10), Some(20), Some(30)]);
        left.boxes.remove(1);
        tui.refresh(left);
        assert_eq!(tui.selected(), Some("000000000002"));
    }

    #[test]
    fn enter_attaches_and_d_stops_the_selected_box() {
        let mut tui = running(&[10, 20]);
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(
            tui.update(Press::Enter, ROOMY),
            Action::Attach("000000000001".to_owned())
        );
        assert_eq!(
            tui.update(Press::Typed('d'), ROOMY),
            Action::Do(Act::Stop(20))
        );
    }

    /// Enter does the one thing the row allows. On an idle box that is
    /// starting it again — the whole reason its home was kept.
    #[test]
    fn enter_on_an_idle_box_resumes_it_and_d_kills_nothing() {
        let mut tui = boxes(&[Some(10), None]);
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(
            tui.update(Press::Enter, ROOMY),
            Action::Workspaces("000000000001".to_owned())
        );
        assert_eq!(tui.update(Press::Typed('d'), ROOMY), Action::Redraw);
    }

    /// The gap this closes: a box's other workspaces were recorded,
    /// shown as `+N`, and reachable from nowhere — and the panel picked
    /// one for you, which was right often enough to be invisible and
    /// wrong with no way to say so. Enter asks.
    #[test]
    fn enter_on_an_idle_box_asks_which_workspace_to_start_it_in() {
        let mut tui = boxes(&[None]);
        assert_eq!(
            tui.update(Press::Enter, ROOMY),
            Action::Workspaces("000000000000".to_owned())
        );
    }

    #[test]
    fn an_empty_list_only_redraws_on_enter_stop_and_workspace() {
        let mut tui = running(&[]);
        assert_eq!(tui.update(Press::Enter, ROOMY), Action::Redraw);
        assert_eq!(tui.update(Press::Typed('d'), ROOMY), Action::Redraw);
        assert_eq!(tui.update(Press::Typed('w'), ROOMY), Action::Redraw);
        assert_eq!(tui.update(Press::Typed('q'), ROOMY), Action::Quit);
    }

    /// The bug this pins: `d` on an idle box did the same thing as a key
    /// the panel does not know — nothing, drawn identically. Most rows in
    /// a panel are idle, so `d` read as broken.
    #[test]
    fn d_on_an_idle_box_says_why_rather_than_nothing() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Press::Typed('d'), ROOMY), Action::Redraw);
        assert!(note_of(&tui).contains("not running"), "{}", note_of(&tui));
    }

    /// The words a stop reports itself with live here, not in the panel,
    /// so both outcomes are pinned where every other screen decision is.
    #[test]
    fn a_stop_says_what_it_came_to() {
        let mut tui = boxes(&[Some(7)]);
        tui.acted(&Act::Stop(7), Ok(()));
        assert!(note_of(&tui).contains("stopping"), "{}", note_of(&tui));
        assert_eq!(screen_of(&tui).note.map(|n| n.level), Some(Level::Info));
        tui.acted(
            &Act::Stop(7),
            Err("cannot stop the box's init 7: EPERM".to_owned()),
        );
        assert!(note_of(&tui).contains("EPERM"), "{}", note_of(&tui));
        assert_eq!(screen_of(&tui).note.map(|n| n.level), Some(Level::Refusal));
    }

    /// The gap this closes: the panel listed every box on this host and
    /// was the one surface from which none of them could be cleared up.
    #[test]
    fn x_asks_before_it_removes_and_y_is_what_answers() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Press::Typed('x'), ROOMY), Action::Redraw);
        assert!(note_of(&tui).contains("remove box 000000000000"));
        assert!(note_of(&tui).contains("y confirm"));
        assert_eq!(screen_of(&tui).note.map(|n| n.level), Some(Level::Question));
        assert_eq!(
            tui.update(Press::Typed('y'), ROOMY),
            Action::Do(Act::Remove("000000000000".to_owned()))
        );
    }

    #[test]
    fn r_asks_before_it_empties_a_box_and_says_what_it_keeps() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Press::Typed('r'), ROOMY), Action::Redraw);
        assert!(note_of(&tui).contains("reset box 000000000000"));
        assert!(note_of(&tui).contains("keeps its id"));
        assert_eq!(
            tui.update(Press::Typed('y'), ROOMY),
            Action::Do(Act::Reset("000000000000".to_owned()))
        );
    }

    /// A question about deleting an agent's whole history must not be
    /// answerable by a stray keystroke — and a cancel must say so, or it
    /// looks exactly like a key the panel does not know.
    #[test]
    fn any_key_but_y_cancels_the_question_and_says_that_it_did() {
        let mut tui = boxes(&[None]);
        tui.update(Press::Typed('x'), ROOMY);
        assert_eq!(tui.update(Press::Typed('j'), ROOMY), Action::Redraw);
        assert!(!note_of(&tui).contains("remove box"), "{}", note_of(&tui));
        assert!(note_of(&tui).contains("cancelled"), "{}", note_of(&tui));
        // The question is gone, so `y` is now a key with nothing to answer.
        assert_eq!(tui.update(Press::Typed('y'), ROOMY), Action::Redraw);
    }

    /// `q` is the reflex for "no". Taking the whole panel down on it
    /// would be the one answer nobody meant.
    #[test]
    fn q_answers_the_question_rather_than_leaving_the_panel() {
        let mut tui = boxes(&[None]);
        tui.update(Press::Typed('x'), ROOMY);
        assert_eq!(tui.update(Press::Typed('q'), ROOMY), Action::Redraw);
        assert_eq!(tui.update(Press::Typed('q'), ROOMY), Action::Quit);
    }

    /// Both acts touch the home a running agent is writing to. The claim
    /// the binary takes is the real proof; refusing here is what spares
    /// the user a question that was always going to be refused.
    #[test]
    fn a_running_box_is_neither_removed_nor_reset_and_the_screen_says_why() {
        for key in [Press::Typed('x'), Press::Typed('r')] {
            let mut tui = boxes(&[Some(7)]);
            assert_eq!(tui.update(key, ROOMY), Action::Redraw);
            assert!(note_of(&tui).contains("running"), "{}", note_of(&tui));
            assert!(note_of(&tui).contains("d stops it first"));
            // Nothing was asked, so nothing can be confirmed.
            assert_eq!(tui.update(Press::Typed('y'), ROOMY), Action::Redraw);
        }
    }

    #[test]
    fn an_empty_list_has_nothing_to_remove_or_reset() {
        let mut tui = running(&[]);
        assert_eq!(tui.update(Press::Typed('x'), ROOMY), Action::Redraw);
        assert_eq!(tui.update(Press::Typed('r'), ROOMY), Action::Redraw);
        assert!(!note_of(&tui).contains("remove box"));
    }

    #[test]
    fn a_removal_and_a_reset_say_what_they_came_to() {
        let mut tui = boxes(&[None]);
        tui.acted(&Act::Remove("000000000000".to_owned()), Ok(()));
        assert!(note_of(&tui).contains("removed"), "{}", note_of(&tui));
        tui.acted(&Act::Reset("000000000000".to_owned()), Ok(()));
        assert!(note_of(&tui).contains("reset"), "{}", note_of(&tui));
        tui.acted(
            &Act::Remove("000000000000".to_owned()),
            Err("box 000000000000 is running".to_owned()),
        );
        assert!(note_of(&tui).contains("is running"), "{}", note_of(&tui));
    }

    /// The hints are the list of what can be done here, so a terminal too
    /// narrow for them wraps them, and one too narrow for that keeps
    /// every keycap and loses the words. A key dropped off the end is
    /// gone with nothing saying it was there.
    #[test]
    fn hints_too_wide_for_the_terminal_never_lose_a_key() {
        let tui = boxes(&[None]);
        let every = hints_for(on::BOXES).len();
        // From the narrowest terminal that can hold every keycap on two
        // lines. Narrower than that nothing can hold them, and what the
        // screen owes then is only that it still fits, which the height
        // test proves at every width down to one.
        for width in [24u16, 40, 60, 79, 200] {
            let screen = tui.screen(Viewport { width, height: 20 }, 0, &there);
            let shown: Vec<&Hint> = screen.hints.iter().flatten().collect();
            assert_eq!(shown.len(), every, "{width} dropped a key: {shown:?}");
            assert!(screen.hints.len() <= HINT_LINES, "{width}: {shown:?}");
            for line in &screen.hints {
                let drawn: usize = line
                    .iter()
                    .map(|hint| {
                        hint.cap.len() + usize::from(!hint.does.is_empty()) + hint.does.len()
                    })
                    .sum::<usize>()
                    + GAP * line.len().saturating_sub(1);
                // One hint wider than the whole terminal gets a line of
                // its own and overflows it; anything else must fit.
                assert!(
                    drawn <= usize::from(width) || line.len() == 1,
                    "{width}: {drawn} drawn on one line"
                );
            }
        }
    }

    /// A key that is not on the screen is a key nobody presses — and a
    /// keycap on the screen that decodes to nothing is a screen that
    /// lies. Both come from one table now, so neither can happen; this
    /// pins that the table is what both read.
    #[test]
    fn every_keycap_a_screen_shows_is_a_key_that_screen_answers_to() {
        for screen in [
            on::BOXES,
            on::ROLES,
            on::STORE,
            on::SECRETS,
            on::DOCTOR,
            on::PICK,
            on::CONFIRM,
            on::MESSAGE,
        ] {
            for hint in hints_for(screen) {
                let press = BINDINGS
                    .iter()
                    .find(|binding| binding.cap == hint.cap && binding.on & screen != 0)
                    .map(|binding| binding.press)
                    .unwrap_or_else(|| panic!("{hint:?} is on no binding"));
                assert!(
                    command(press, screen).is_some(),
                    "{hint:?} decodes to nothing on {screen:b}"
                );
            }
            // And `?` reaches every one of them, quiet keys included.
            let page = keys_page(screen);
            for binding in BINDINGS.iter().filter(|b| b.on & screen != 0) {
                assert!(
                    page.contains(binding.does),
                    "{page}\nmisses {}",
                    binding.cap
                );
            }
        }
    }

    /// The confirm keycap is named on the question, not in the hints: a
    /// key that only ever answers something has nothing to offer when
    /// nothing was asked.
    #[test]
    fn the_key_that_answers_a_question_is_named_on_the_question() {
        let mut asking = boxes(&[None]);
        asking.update(Press::Typed('x'), ROOMY);
        assert!(note_of(&asking).contains("y confirm"));
        assert_eq!(command(Press::Typed('y'), on::BOXES), Some(Key::Yes));
        assert!(
            !hints_for(on::BOXES).iter().any(|hint| hint.cap == "y"),
            "the hints offer a key with nothing to answer"
        );
    }
    /// A character the panel does not bind must stay unbound, or a stray
    /// keystroke starts meaning something.
    #[test]
    fn a_character_the_panel_does_not_bind_decodes_to_nothing() {
        assert_eq!(command(Press::Typed('z'), on::BOXES), None);
        assert_eq!(command(Press::Typed('Y'), on::BOXES), None);
        assert_eq!(command(Press::Typed('W'), on::BOXES), None);
        // A key of one screen is not a key of another.
        assert_eq!(command(Press::Typed('w'), on::ROLES), None);
        assert_eq!(command(Press::Typed('n'), on::PICK), None);
    }

    /// A refusal answers one key press. Left on the screen it would go on
    /// describing a key the user has since moved past.
    #[test]
    fn a_refusal_is_cleared_by_the_next_key() {
        let mut tui = boxes(&[None, Some(7)]);
        tui.update(Press::Typed('d'), ROOMY);
        assert!(note_of(&tui).contains("not running"));
        tui.update(Press::Typed('j'), ROOMY);
        assert!(note_of(&tui).is_empty(), "{}", note_of(&tui));
    }

    /// Typing into `tui` as a person would: one press per character.
    fn typed(tui: &mut Tui, text: &str) {
        for c in text.chars() {
            tui.update(Press::Typed(c), ROOMY);
        }
    }

    /// A named box to filter for, so the filter has something to match
    /// on besides an id.
    fn named(pids: &[Option<u32>], names: &[&str]) -> Tui {
        let mut tui = boxes(pids);
        for (entry, name) in tui.boxes.iter_mut().zip(names) {
            entry.record.alias = Some((*name).to_owned());
        }
        tui
    }

    /// The gap this closes: a host with thirty boxes across ten trees is
    /// what this tool is for, and the only way to find one was to read.
    #[test]
    fn a_filter_narrows_the_list_to_what_matches() {
        let mut tui = named(&[None, None, None], &["api", "web", "api-old"]);
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "api");
        tui.update(Press::Enter, ROOMY);

        let screen = screen_of(&tui);
        let said = text_of(&screen);
        assert!(said.contains("api"), "{said}");
        assert!(!said.contains("web"), "{said}");
        assert_eq!(screen.rows.len(), 2, "{said}");
        // Rows missing for a reason nobody can see is the same defect as
        // a key that does nothing, so the screen says what it is hiding.
        assert!(
            screen
                .heading
                .as_deref()
                .is_some_and(|said| said.contains("/api") && said.contains("2 of 3")),
            "{:?}",
            screen.heading
        );
    }

    /// While a filter is being typed every letter is a letter. `w` is the
    /// workspace key and `q` leaves the panel, and a search for `qw` must
    /// be a search rather than two commands.
    #[test]
    fn a_letter_typed_into_a_filter_is_not_the_key_it_spells() {
        let mut tui = named(&[None], &["qwerty"]);
        tui.update(Press::Typed('/'), ROOMY);
        for c in "qw".chars() {
            assert_eq!(tui.update(Press::Typed(c), ROOMY), Action::Redraw);
        }
        assert!(
            screen_of(&tui)
                .heading
                .as_deref()
                .is_some_and(|said| said.contains("/qw")),
            "{:?}",
            screen_of(&tui).heading
        );
        // And once it is kept, they are keys again.
        tui.update(Press::Enter, ROOMY);
        assert_eq!(tui.update(Press::Typed('q'), ROOMY), Action::Quit);
    }

    /// A filter can hide the row the cursor was on. Leaving it there
    /// would leave every key acting on a box nobody can see.
    #[test]
    fn a_filter_that_hides_the_cursor_moves_it_to_a_row_that_is_shown() {
        let mut tui = named(&[Some(7), None], &["web", "api"]);
        assert_eq!(tui.selected(), Some("000000000000"));
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "api");
        assert_eq!(tui.selected(), Some("000000000001"));
        // And the keys act on it, not on the row that is hidden.
        tui.update(Press::Enter, ROOMY);
        assert_eq!(
            tui.update(Press::Enter, ROOMY),
            Action::Workspaces("000000000001".to_owned())
        );
    }

    /// A filter that matches nothing must say so in its own words. "No
    /// boxes yet" over a store full of them would be the screen lying
    /// about what it is holding back.
    #[test]
    fn a_filter_that_matches_nothing_says_so_and_not_that_there_are_none() {
        let mut tui = named(&[None], &["api"]);
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "zzz");
        let screen = screen_of(&tui);
        assert_eq!(screen.empty.as_deref(), Some("no box matches zzz"));
        assert_eq!(screen.cursor, None);
        // Nothing is selected, so nothing can be acted on by mistake.
        assert_eq!(tui.update(Press::Escape, ROOMY), Action::Redraw);
        assert_eq!(screen_of(&tui).empty, None);
        // And the tab bar is still there, because it always is.
        assert!(
            screen_of(&tui)
                .heading
                .unwrap_or_default()
                .contains("boxes")
        );
    }

    /// One key for "take back what is up", and only when nothing is up
    /// does it leave. A filter typed by mistake must not cost the panel.
    #[test]
    fn escape_clears_a_filter_before_it_leaves() {
        let mut tui = named(&[None], &["api"]);
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "api");
        tui.update(Press::Enter, ROOMY);
        assert_eq!(tui.update(Press::Escape, ROOMY), Action::Redraw);
        assert!(
            !screen_of(&tui).heading.unwrap_or_default().contains("/api"),
            "the filter is still on the screen"
        );
        assert_eq!(tui.update(Press::Escape, ROOMY), Action::Quit);
    }

    /// Backspace while typing, so a mistyped filter is one key to fix
    /// rather than an escape and a retype.
    #[test]
    fn backspace_takes_back_one_character_of_a_filter() {
        let mut tui = named(&[None, None], &["api", "web"]);
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "apx");
        assert_eq!(screen_of(&tui).rows.len(), 0);
        tui.update(Press::Backspace, ROOMY);
        assert_eq!(screen_of(&tui).rows.len(), 1);
    }

    /// A list longer than one screen needs more than one row at a time.
    #[test]
    fn g_and_pages_move_the_cursor_further_than_one_row() {
        let mut tui = boxes(&[None; 40]);
        let small = Viewport {
            width: 100,
            height: 14,
        };
        tui.update(Press::Typed('G'), small);
        assert_eq!(tui.selected(), Some("000000000027"));
        tui.update(Press::Typed('g'), small);
        assert_eq!(tui.selected(), Some("000000000000"));
        tui.update(Press::PageDown, small);
        let after = tui.selected().expect("a row").to_owned();
        assert_ne!(after, "000000000000");
        assert_ne!(after, "000000000001");
        tui.update(Press::PageUp, small);
        assert_eq!(tui.selected(), Some("000000000000"));
        // Home and End are the same two keys under other names.
        tui.update(Press::End, small);
        assert_eq!(tui.selected(), Some("000000000027"));
        tui.update(Press::Home, small);
        assert_eq!(tui.selected(), Some("000000000000"));
    }

    /// The gap this closes: one hint line was the whole of what the panel
    /// told anybody about itself.
    #[test]
    fn question_mark_shows_every_key_of_this_screen() {
        let mut tui = boxes(&[None]);
        let Action::Read(page) = tui.update(Press::Typed('?'), ROOMY) else {
            panic!("? did not answer with a screen");
        };
        for expected in ["rename", "filter", "a screen down", "first"] {
            assert!(page.contains(expected), "{page}\nmisses {expected}");
        }
    }

    /// A sheet of `rows`, keyed by their first cell.
    fn sheet(columns: &[&str], rows: &[&str]) -> Sheet {
        Sheet {
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|row| SheetRow {
                    key: (*row).to_owned(),
                    cells: vec![(*row).to_owned()],
                    tone: Tone::Plain,
                    detail: vec![("about", (*row).to_owned())],
                })
                .collect(),
            empty: "nothing here".to_owned(),
            summary: Some("a summary".to_owned()),
        }
    }

    /// The gap this closes: the panel reached boxes and nothing else.
    /// Roles, the store, secrets and the host had no screen at all.
    #[test]
    fn tab_moves_between_the_tabs_and_asks_for_the_rows_of_each() {
        let mut tui = boxes(&[None]);
        for expected in [
            Tab::Roles,
            Tab::Store,
            Tab::Secrets,
            Tab::Doctor,
            Tab::Boxes,
        ] {
            assert_eq!(tui.update(Press::Tab, ROOMY), Action::Gather(expected));
            assert_eq!(tui.tab(), expected);
        }
        // And the bar says which one is up.
        tui.update(Press::Tab, ROOMY);
        let heading = screen_of(&tui).heading.unwrap_or_default();
        assert!(heading.contains("[roles]"), "{heading}");
        assert!(heading.contains(" boxes "), "{heading}");
    }

    /// Coming back to a tab comes back to the row you left it on.
    #[test]
    fn every_tab_keeps_its_own_cursor() {
        let mut tui = boxes(&[None, None]);
        tui.update(Press::Typed('j'), ROOMY);
        assert_eq!(tui.selected(), Some("000000000001"));

        tui.update(Press::Tab, ROOMY);
        tui.show(Tab::Roles, sheet(&["NAME"], &["alphaca", "java"]));
        tui.update(Press::Typed('j'), ROOMY);
        let screen = tui.screen(ROOMY, 0, &there);
        assert_eq!(
            screen.rows[screen.cursor.expect("a cursor")].cells[0],
            "java"
        );

        // Round the tabs and back to the boxes.
        for _ in 0..4 {
            tui.update(Press::Tab, ROOMY);
        }
        assert_eq!(tui.tab(), Tab::Boxes);
        assert_eq!(tui.selected(), Some("000000000001"));
    }

    /// `x` takes a box away, uninstalls a role and forgets a secret. One
    /// key, one meaning per tab, and the hints of each say which.
    #[test]
    fn a_key_does_what_the_tab_it_is_pressed_on_says() {
        let mut tui = boxes(&[None]);
        tui.update(Press::Tab, ROOMY);
        tui.show(Tab::Roles, sheet(&["NAME"], &["alphaca"]));
        assert_eq!(tui.update(Press::Typed('x'), ROOMY), Action::Redraw);
        assert!(
            note_of(&tui).contains("uninstall role alphaca"),
            "{}",
            note_of(&tui)
        );
        assert_eq!(
            tui.update(Press::Typed('y'), ROOMY),
            Action::Do(Act::Uninstall("alphaca".to_owned()))
        );

        // The same key, three tabs along.
        tui.update(Press::Tab, ROOMY);
        tui.update(Press::Tab, ROOMY);
        tui.show(Tab::Secrets, sheet(&["NAME"], &["GITHUB_TOKEN"]));
        assert_eq!(tui.update(Press::Typed('x'), ROOMY), Action::Redraw);
        assert!(
            note_of(&tui).contains("forget GITHUB_TOKEN"),
            "{}",
            note_of(&tui)
        );
        assert_eq!(
            tui.update(Press::Typed('y'), ROOMY),
            Action::Do(Act::Forget("GITHUB_TOKEN".to_owned()))
        );
    }

    /// The hints of a tab are that tab's keys, and no other tab's.
    #[test]
    fn each_tab_hints_its_own_keys_and_no_others() {
        let mut tui = boxes(&[None]);
        assert!(text_of(&screen_of(&tui)).contains("d stop"));
        tui.update(Press::Tab, ROOMY);
        let said = text_of(&screen_of(&tui));
        assert!(said.contains("a install"), "{said}");
        assert!(!said.contains("d stop"), "{said}");
        assert!(!said.contains("d stop"), "{said}");
    }

    /// A tab's rows and its detail come from the same place, so the pane
    /// under the list is about the row the cursor is on.
    #[test]
    fn a_tabs_detail_follows_its_cursor_and_its_summary_is_shown() {
        let mut tui = boxes(&[None]);
        tui.update(Press::Tab, ROOMY);
        tui.show(Tab::Roles, sheet(&["NAME"], &["alphaca", "java"]));
        let screen = tui.screen(ROOMY, 0, &there);
        assert!(
            screen.detail.iter().any(|f| f.text.contains("alphaca")),
            "{:?}",
            screen.detail
        );
        assert_eq!(screen.summary.as_deref(), Some("a summary"));
        tui.update(Press::Typed('j'), ROOMY);
        assert!(
            tui.screen(ROOMY, 0, &there)
                .detail
                .iter()
                .any(|f| f.text.contains("java"))
        );
    }

    /// A filter belongs to the list it was typed over. Carried along it
    /// would hide rows on the next tab for a reason typed on this one.
    #[test]
    fn moving_to_another_tab_clears_the_filter() {
        let mut tui = named(&[None, None], &["api", "web"]);
        tui.update(Press::Typed('/'), ROOMY);
        typed(&mut tui, "api");
        tui.update(Press::Enter, ROOMY);
        assert_eq!(screen_of(&tui).rows.len(), 1);
        tui.update(Press::Tab, ROOMY);
        tui.show(Tab::Roles, sheet(&["NAME"], &["alphaca", "java"]));
        assert_eq!(tui.screen(ROOMY, 0, &there).rows.len(), 2);
    }

    /// Every tab's rows come from the same body its command prints from.
    #[test]
    fn the_store_and_host_tabs_render_what_their_commands_report() {
        let items = [crate::gc::Item {
            path: std::path::PathBuf::from("/data/images/abc"),
            bytes: 2_000_000,
            verdict: crate::gc::Verdict::Unreferenced("no box names it".to_owned()),
        }];
        let sweep = crate::gc::Sweep {
            delete: false,
            unreferenced: false,
        };
        let store = store_sheet(&items, sweep);
        assert_eq!(store.rows[0].key, "/data/images/abc");
        assert!(store.rows[0].cells[1].contains('M'), "{:?}", store.rows[0]);
        // The offer `gc` makes in its report is the offer the tab makes.
        assert!(
            store
                .summary
                .as_deref()
                .is_some_and(|said| said.contains("u takes")),
            "{:?}",
            store.summary
        );

        let verdict = crate::doctor::evaluate(vec![crate::doctor::ProbeResult {
            probe: crate::doctor::Probe::UserNamespaces,
            outcome: crate::doctor::Outcome::Fail("blocked".to_owned()),
        }]);
        let host = doctor_sheet(&verdict);
        assert_eq!(host.rows[0].cells[1], "FAIL");
        assert_eq!(host.rows[0].tone, Tone::Bad);
        assert!(
            host.rows[0]
                .detail
                .iter()
                .any(|(_, value)| value.contains("blocked"))
        );
        assert_eq!(
            host.summary.as_deref(),
            Some("verdict: this host cannot run wormhole")
        );
    }

    /// The gap this closes: `n` built `--new [--role R] [--run P]` and
    /// nothing else, so every flag that changes what a start *is* was
    /// reachable only from a command line.
    ///
    /// The proof is a round trip: what the form shows is parsed back by
    /// the very parser `wormhole box` uses, so a screen cannot promise an
    /// argument the start would refuse.
    #[test]
    fn what_the_form_shows_is_what_a_start_is_given() {
        let mut form = Form::new(Some("alphaca"), Some("grok"));
        // name, login, variable, command — down to each and set it.
        form.update(Press::Typed('j'), ROOMY);
        form.update(Press::Typed('j'), ROOMY);
        form.update(Press::Enter, ROOMY);
        for c in "api".chars() {
            form.update(Press::Typed(c), ROOMY);
        }
        form.update(Press::Enter, ROOMY);
        // The login cycles rather than being typed.
        form.update(Press::Typed('j'), ROOMY);
        form.update(Press::Enter, ROOMY);
        form.update(Press::Enter, ROOMY);

        let argv = form.argv();
        assert_eq!(
            argv,
            [
                "--new",
                "--role",
                "alphaca",
                "--run",
                "grok",
                "--as",
                "api",
                "--credentials",
                "copy",
            ]
        );
        assert!(form.command().starts_with("wormhole box --new"));

        // And the start's own parser accepts exactly that.
        let parsed = crate::run::parse_box_args(&argv).expect("a start would take this");
        assert!(parsed.new);
        assert_eq!(parsed.role.as_deref(), Some("alphaca"));
        assert_eq!(parsed.run.as_deref(), Some("grok"));
        assert_eq!(parsed.alias.as_deref(), Some("api"));
        assert_eq!(parsed.credentials, Some(crate::manifest::Credentials::Copy));

        // Enter on the last line is the start.
        form.update(Press::Typed('G'), ROOMY);
        assert_eq!(form.update(Press::Enter, ROOMY), FormAction::Start(argv));
    }

    /// A command replaces the agent, and it is the rest of the line
    /// rather than one argument.
    #[test]
    fn a_command_in_the_form_is_the_rest_of_the_line() {
        let mut form = Form::new(None, None);
        form.update(Press::Typed('G'), ROOMY);
        form.update(Press::Typed('k'), ROOMY);
        form.update(Press::Enter, ROOMY);
        for c in "sh -c ls".chars() {
            form.update(Press::Typed(c), ROOMY);
        }
        form.update(Press::Enter, ROOMY);
        assert_eq!(form.argv(), ["--new", "--", "sh", "-c", "ls"]);
        let parsed = crate::run::parse_box_args(&form.argv()).expect("a start would take this");
        assert_eq!(
            parsed.command,
            Some(vec!["sh".to_owned(), "-c".to_owned(), "ls".to_owned()])
        );
    }

    /// What is absent reads as absent, here as on the preview.
    #[test]
    fn an_unset_field_says_what_leaving_it_unset_means() {
        let form = Form::new(None, None);
        let said = text_of(&form.screen(ROOMY));
        assert!(said.contains("this workspace's wormhole.toml"), "{said}");
        assert!(said.contains("nothing; the agent runs"), "{said}");
        assert_eq!(form.argv(), ["--new"]);
    }

    /// A new box needs no selection: it starts in the current workspace.
    #[test]
    fn n_starts_a_new_box_with_or_without_a_selection() {
        assert_eq!(running(&[]).update(Press::Typed('n'), ROOMY), Action::New);
        assert_eq!(running(&[1]).update(Press::Typed('n'), ROOMY), Action::New);
    }

    #[test]
    fn a_refresh_that_empties_the_list_leaves_nothing_selected() {
        let mut tui = running(&[1, 2, 3]);
        tui.update(Press::Typed('j'), ROOMY);
        tui.update(Press::Typed('j'), ROOMY);
        tui.refresh(crate::home::Scan::default());
        assert_eq!(tui.selected(), None);
    }

    /// What the scan could not read is drawn on the panel, not printed at
    /// it: the panel owns the terminal, and a reader that writes there
    /// draws over the screen every time the list is refreshed.
    #[test]
    fn what_the_scan_cannot_read_is_drawn_on_the_panel() {
        let mut tui = Tui::default();
        tui.refresh(crate::home::Scan {
            problems: vec!["/data/homes/proj-1 holds no box record".to_owned()],
            ..scan(&[Some(10)])
        });
        let screen = screen_of(&tui);
        assert_eq!(screen.notices[0], "could not read:");
        assert_eq!(
            screen.notices[1],
            "  /data/homes/proj-1 holds no box record"
        );
    }

    #[test]
    fn a_scan_with_nothing_wrong_says_nothing() {
        assert!(screen_of(&running(&[10])).notices.is_empty());
    }

    /// A store full of unreadable homes must not push the box list off
    /// the screen. What is left out is counted rather than dropped.
    #[test]
    fn a_pile_of_problems_is_capped_and_the_rest_counted() {
        let mut tui = Tui::default();
        tui.refresh(crate::home::Scan {
            boxes: Vec::new(),
            problems: (0..PROBLEMS_SHOWN + 3)
                .map(|n| format!("home {n}"))
                .collect(),
        });
        let said = text_of(&screen_of(&tui));
        assert!(
            said.contains(&format!("home {}", PROBLEMS_SHOWN - 1)),
            "{said}"
        );
        assert!(
            !said.contains(&format!("home {PROBLEMS_SHOWN}\n")),
            "{said}"
        );
        assert!(said.contains("... and 3 more"), "{said}");
    }

    #[test]
    fn the_screen_marks_the_selected_row_and_shows_the_hints() {
        let mut tui = running(&[10, 20]);
        tui.update(Press::Typed('j'), ROOMY);
        let screen = screen_of(&tui);
        assert_eq!(screen.columns[0].title, "ID");
        assert_eq!(screen.cursor, Some(1));
        assert!(text_of(&screen).contains("enter join, or start"));
    }

    #[test]
    fn an_empty_screen_says_no_boxes_without_a_cursor() {
        let screen = screen_of(&running(&[]));
        assert_eq!(screen.empty.as_deref(), Some("no boxes yet"));
        assert_eq!(screen.cursor, None);
        assert!(screen.rows.is_empty());
        assert!(screen.detail.is_empty());
    }

    /// The defect this closes: the box table is 135 columns wide on a
    /// real store, and the screen had no idea how wide the terminal was.
    /// Every row wrapped, and the cursor then marked half a row.
    #[test]
    fn no_row_is_ever_wider_than_the_terminal() {
        let mut tui = running(&[10, 20]);
        tui.boxes[0].record.workspace =
            PathBuf::from("/home/nabor/work/very/deeply/nested/monorepo/services/checkout");
        tui.boxes[0].record.role = Some("alphaca-java".to_owned());
        for width in [20u16, 40, 60, 80, 120, 200] {
            let screen = tui.screen(Viewport { width, height: 30 }, 0, &there);
            // The cursor marker's gutter counts: a fitting that did not
            // know about it overflows by two columns on every row.
            let drawn: usize = GUTTER
                + screen
                    .columns
                    .iter()
                    .map(|c| usize::from(c.width))
                    .sum::<usize>()
                + GAP * screen.columns.len().saturating_sub(1);
            assert!(drawn <= usize::from(width), "{width}: {drawn} drawn");
            for row in &screen.rows {
                for (cell, column) in row.cells.iter().zip(&screen.columns) {
                    assert!(
                        table::display_width(cell) <= usize::from(column.width),
                        "{width}: {cell:?} in a column {} wide",
                        column.width
                    );
                }
            }
        }
    }

    /// A narrow terminal drops columns rather than squeezing all of them:
    /// eight columns cut to four characters each fits and says nothing.
    /// The id, the state and the path are what survive.
    #[test]
    fn a_narrow_terminal_keeps_the_columns_that_identify_a_box() {
        let screen = running(&[10]).screen(
            Viewport {
                width: 46,
                height: 20,
            },
            0,
            &there,
        );
        let titles: Vec<&str> = screen.columns.iter().map(|c| c.title.as_str()).collect();
        assert!(titles.contains(&"ID"), "{titles:?}");
        assert!(titles.contains(&"STATE"), "{titles:?}");
        assert!(titles.contains(&"WORKSPACE"), "{titles:?}");
        assert!(!titles.contains(&"NAME"), "{titles:?}");
    }

    /// A column of nothing but dashes costs the same width as one that
    /// says something, so it goes first however the order reads.
    #[test]
    fn a_column_that_says_nothing_is_dropped_before_one_that_does() {
        let mut tui = running(&[10, 20]);
        // Named boxes, with no alias between them.
        for entry in &mut tui.boxes {
            entry.record.name = Some("Alphaca".to_owned());
            entry.record.role = Some("alphaca".to_owned());
        }
        let screen = tui.screen(
            Viewport {
                width: 58,
                height: 20,
            },
            0,
            &there,
        );
        let titles: Vec<&str> = screen.columns.iter().map(|c| c.title.as_str()).collect();
        assert!(!titles.contains(&"ALIAS"), "{titles:?}");
        assert!(titles.contains(&"ROLE"), "{titles:?}");
    }

    /// The screen must fit the terminal it was asked for, whatever is on
    /// it: a line over the bottom is a line nobody reads, and on a
    /// wrapping terminal it is the hints that go.
    ///
    /// Every size, not a chosen one: the narrow widths are where the note
    /// wraps into more lines than were budgeted for it, which is exactly
    /// the arithmetic a chosen size would have missed.
    #[test]
    fn a_screen_never_asks_for_more_lines_than_the_terminal_has() {
        let mut tui = Tui::default();
        tui.refresh(crate::home::Scan {
            boxes: scan(&[None; 30]).boxes,
            problems: (0..9).map(|n| format!("home {n}")).collect(),
        });
        tui.update(Press::Typed('x'), ROOMY);
        for width in [1u16, 8, 20, 46, 80, 200] {
            for height in 1..40u16 {
                let screen = tui.screen(Viewport { width, height }, 0, &there);
                assert!(
                    screen.lines() <= usize::from(height),
                    "{width}x{height}: screen wants {} lines",
                    screen.lines()
                );
            }
        }
    }

    /// The same of every other screen the panel draws. A picker that
    /// overflowed would scroll its own heading off the top.
    #[test]
    fn a_picker_never_asks_for_more_lines_than_the_terminal_has() {
        for items in [0usize, 1, 40] {
            let picker = Picker::new((0..items).map(|n| format!("role {n}")).collect());
            for width in [1u16, 8, 80] {
                for height in 1..20u16 {
                    let screen = picker.screen(Viewport { width, height });
                    assert!(
                        screen.lines() <= usize::from(height),
                        "{items} items at {width}x{height}: screen wants {} lines",
                        screen.lines()
                    );
                }
            }
        }
    }

    /// A list longer than the terminal must still show the row the cursor
    /// is on, or moving down walks the cursor off the screen.
    #[test]
    fn a_list_longer_than_the_terminal_shows_the_row_the_cursor_is_on() {
        let mut tui = boxes(&[None; 40]);
        let small = Viewport {
            width: 100,
            height: 12,
        };
        for expected in 0..40usize {
            let screen = tui.screen(small, 0, &there);
            let at = screen.cursor.expect("a cursor");
            assert_eq!(screen.rows[at].cells[0], format!("{expected:012x}"));
            tui.update(Press::Typed('j'), ROOMY);
        }
        let screen = tui.screen(small, 0, &there);
        let scroll = screen.scroll.expect("a scroll line");
        assert_eq!(scroll.total, 40);
        assert_eq!(
            scroll.label(),
            format!("{}-40 of 40", 40 - scroll.shown + 1)
        );
    }

    /// The defect this closes: the permission preview is forty lines and
    /// the screen drew all of them, so on any ordinary terminal the
    /// `[image] build` lines — the part that executes — were below the
    /// bottom and could not be reached.
    #[test]
    fn a_preview_longer_than_the_terminal_can_be_scrolled_to_its_end() {
        let text = preview(
            &manifest("[agent]\nrun = \"claude\"\n"),
            "role stranger",
            false,
        );
        let small = Viewport {
            width: 80,
            height: 10,
        };
        let top = page(&text, small, 0, Reading::Confirm);
        assert_eq!(top.lines.len(), 9);
        assert!(top.lines[0].contains("role stranger"), "{:?}", top.lines);
        let scroll = top.scroll.expect("more to read");
        assert!(scroll.total > 9, "{scroll:?}");

        let mut at = 0;
        for _ in 0..scroll.total {
            at = scroll_to(&text, small, at, Press::Typed('j'));
        }
        let end = page(&text, small, at, Reading::Confirm);
        assert!(
            end.lines
                .iter()
                .any(|line| line.contains("build:") || line.contains("build: nothing runs")),
            "{:?}",
            end.lines
        );
        // Scrolling past the end is not a way to reach an empty screen.
        assert_eq!(end.lines.len(), 9);
        assert_eq!(scroll_to(&text, small, at, Press::Typed('j')), at);
        assert_eq!(scroll_to(&text, small, 0, Press::Typed('k')), 0);
    }

    /// The preview says which line belongs to which by indenting it. A
    /// pager that reflowed every line would flatten exactly that.
    #[test]
    fn a_page_keeps_the_indentation_the_preview_reads_by() {
        let text = crate::manifest::parse(&format!(
            "version = {}\n[image]\nbase = \"https://x.test/r.tar.gz\"\n\
             base_sha256 = \"{}\"\nbuild = [\"echo built-by-a-stranger\"]\n",
            crate::manifest::VERSION,
            "a".repeat(64),
        ))
        .map(|m| preview(&m, "role stranger", false))
        .expect("valid");
        let all: Vec<String> = (0..40)
            .flat_map(|at| {
                page(
                    &text,
                    Viewport {
                        width: 80,
                        height: 12,
                    },
                    at,
                    Reading::Confirm,
                )
                .lines
            })
            .collect();
        assert!(
            all.iter()
                .any(|line| line == "    echo built-by-a-stranger"),
            "{all:?}"
        );
        assert!(all.iter().any(|line| line.is_empty()), "{all:?}");
    }

    /// A page must fit the terminal it was asked for, like every other
    /// screen here.
    #[test]
    fn a_page_never_asks_for_more_lines_than_the_terminal_has() {
        let text = preview(
            &manifest("[agent]\nrun = \"claude\"\n"),
            "role stranger",
            false,
        );
        for width in [1u16, 20, 80] {
            for height in 1..20u16 {
                let viewport = Viewport { width, height };
                let shown = page(&text, viewport, 3, Reading::Confirm);
                assert!(
                    shown.lines.len() < usize::from(height),
                    "{width}x{height}: {} lines plus the hints",
                    shown.lines.len()
                );
            }
        }
    }

    /// Everything the table has no column for, on the box the cursor is
    /// on — the workspaces most of all.
    #[test]
    fn the_detail_names_every_workspace_the_selected_box_has_served() {
        let mut tui = boxes(&[None]);
        tui.boxes[0].record.workspace = PathBuf::from("/w/a");
        tui.boxes[0].record.earlier = vec![PathBuf::from("/w/b"), PathBuf::from("/w/gone")];
        let screen = tui.screen(ROOMY, 0, &|path| path != Path::new("/w/gone"));
        let said = text_of(&screen);
        assert!(said.contains("/w/a  (last used)"), "{said}");
        assert!(said.contains("/w/b"), "{said}");
        assert!(said.contains("/w/gone  (gone)"), "{said}");
        assert!(
            screen
                .detail
                .iter()
                .any(|f| f.text.contains("/w/gone") && f.tone == Tone::Bad),
            "{screen:?}"
        );
    }

    /// A box with no selection has nothing to detail, and a detail pane
    /// of stale fields would be worse than none.
    #[test]
    fn the_detail_follows_the_cursor() {
        let mut tui = boxes(&[Some(7), None]);
        assert!(text_of(&screen_of(&tui)).contains("running (pid 7)"));
        tui.update(Press::Typed('j'), ROOMY);
        assert!(text_of(&screen_of(&tui)).contains("idle"));
    }
}
