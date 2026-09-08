//! The control surface's decisions: what a key does to the box list and
//! what the screen shows. Terminal I/O lives in the binary.

use crate::home::{Listing, Scan};
use crate::manifest::Manifest;

/// The box list and the cursor on it. Every box this host keeps, not only
/// the running ones: an idle box is the thing you go back to, and one the
/// control surface cannot show is one you cannot reach from it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tui {
    pub boxes: Vec<Listing>,
    /// What the last scan could not read, shown on the screen the panel
    /// draws. Nothing prints these while the panel is up.
    pub problems: Vec<String>,
    pub selected: usize,
    /// What the last key could not do here, shown under the hints and
    /// cleared by the next key.
    ///
    /// Without it a key the panel understands but cannot act on — `d` on
    /// a box that is not running — redraws an unchanged screen, which is
    /// exactly what a key it does not understand already does. Two
    /// different answers must not look the same.
    note: Option<String>,
    /// An act waiting to be answered for. Held here rather than on a
    /// screen of its own so the question, the keys that answer it and the
    /// refusals around it are all one testable state machine — the panel
    /// draws what this says and decides nothing.
    pending: Option<Act>,
}

/// How many unreadable things the panel names before it stops listing
/// them. A store full of broken homes must not push the box list off the
/// screen, and the count says how much was left out.
const PROBLEMS_SHOWN: usize = 5;

/// One cursor on a list of `len` rows: where every screen's Up and Down
/// land, so no screen can drift on edge behavior.
fn step(selected: usize, len: usize, key: Key) -> usize {
    match key {
        Key::Up => selected.saturating_sub(1),
        Key::Down => (selected + 1).min(len.saturating_sub(1)),
        _ => selected,
    }
}

/// The cursor column every list screen draws.
fn cursor(selected: bool) -> &'static str {
    if selected { "> " } else { "  " }
}

/// The keys the TUI understands, already decoded from the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Stop,
    New,
    Remove,
    Reset,
    /// The one key that answers a question. Anything else cancels it, so
    /// a stray press can never delete an agent's history.
    Yes,
    Quit,
}

impl Key {
    /// Which key a character is, if it is one.
    ///
    /// The binding lives here, beside the hints that name it: a keycap
    /// printed in one crate and decoded in another is a hint that can
    /// start lying with every test still green. The binary keeps only
    /// what is genuinely its own — arrows, Enter, Escape, `Ctrl-C`.
    pub fn from_char(typed: char) -> Option<Key> {
        Some(match typed {
            'k' => Key::Up,
            'j' => Key::Down,
            'd' => Key::Stop,
            'n' => Key::New,
            'x' => Key::Remove,
            'r' => Key::Reset,
            'y' => Key::Yes,
            'q' => Key::Quit,
            _ => return None,
        })
    }
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
    /// Empty this box's home, keeping the box itself — same id, same
    /// name, same workspace, nothing the agent wrote.
    Reset(String),
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
            Act::Stop(_) => None,
            Act::Remove(id) => Some(format!(
                "remove box {id}? its home goes with it — history, logins, \
                 and whatever the agent installed. y confirm, any other key cancel"
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
    /// Leave the TUI and start this idle box again, in its own workspace,
    /// with the home and history it kept.
    Resume(String),
    /// Leave the TUI and start another box in the current workspace.
    New,
    Quit,
    /// Do this without leaving the panel, then report through
    /// [`Tui::acted`].
    Do(Act),
}

impl Tui {
    /// Swaps in a fresh scan, keeping the cursor on a valid row.
    pub fn refresh(&mut self, scan: Scan) {
        self.boxes = scan.boxes;
        self.problems = scan.problems;
        self.selected = self.selected.min(self.boxes.len().saturating_sub(1));
    }

    /// Enter does the one thing that row allows: join a box that is
    /// running, start one that is not. Nothing else could be meant, so
    /// nothing else has to be typed.
    pub fn update(&mut self, key: Key) -> Action {
        // A refusal answers one key press. Cleared here, so it can never
        // go on describing a key the user has already moved past.
        self.note = None;
        // A question owns the whole keyboard until it is answered: `y`
        // does it, everything else — `q` included — takes it back. Taken
        // rather than read, so no key can leave the question standing.
        if let Some(act) = self.pending.take() {
            return match key {
                Key::Yes => Action::Do(act),
                _ => Action::Redraw,
            };
        }
        match key {
            Key::Quit => Action::Quit,
            Key::New => Action::New,
            Key::Up | Key::Down => {
                self.selected = step(self.selected, self.boxes.len(), key);
                Action::Redraw
            }
            // The row is read inside the arms that need it, so a cursor
            // move costs no copy of an id it never looks at.
            Key::Enter => match self.boxes.get(self.selected) {
                Some(entry) if entry.running.is_some() => Action::Attach(entry.record.id.clone()),
                Some(entry) => Action::Resume(entry.record.id.clone()),
                None => Action::Redraw,
            },
            Key::Stop => match self.boxes.get(self.selected).map(|entry| entry.running) {
                Some(Some(pid)) => Action::Do(Act::Stop(pid)),
                // Stopping an idle box would mean killing nothing — which
                // is said, not silently done.
                Some(None) => {
                    self.note = Some("that box is not running; enter starts it again".to_owned());
                    Action::Redraw
                }
                None => Action::Redraw,
            },
            Key::Remove => self.ask(Act::Remove),
            Key::Reset => self.ask(Act::Reset),
            // Only ever an answer, and there was no question: the same
            // nothing any key the panel does not know already does.
            Key::Yes => Action::Redraw,
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
        match self.boxes.get(self.selected) {
            None => Action::Redraw,
            Some(entry) if entry.running.is_some() => {
                self.note = Some("that box is running; d stops it first".to_owned());
                Action::Redraw
            }
            Some(entry) => {
                let act = make(entry.record.id.clone());
                // A question is a note like any other. `update` clears the
                // note on every key, which is exactly when a question has
                // to go — so one field holds both and they cannot both be
                // up by construction rather than by comment.
                self.note = act.question();
                self.pending = Some(act);
                Action::Redraw
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
            Ok(()) => act.done(),
            Err(why) => why,
        });
    }

    /// The whole screen: the box list with a cursor column, what the scan
    /// could not read, then the key hints.
    pub fn view(&self, now_unix: u64) -> String {
        let mut screen = String::new();
        for (row, line) in crate::home::list(&self.boxes, now_unix).lines().enumerate() {
            screen.push_str(cursor(!self.boxes.is_empty() && row == self.selected + 1));
            screen.push_str(line);
            screen.push('\n');
        }
        if !self.problems.is_empty() {
            screen.push_str("\ncould not read:\n");
            for problem in self.problems.iter().take(PROBLEMS_SHOWN) {
                screen.push_str(&format!("  {problem}\n"));
            }
            let hidden = self.problems.len().saturating_sub(PROBLEMS_SHOWN);
            if hidden > 0 {
                screen.push_str(&format!("  ... and {hidden} more\n"));
            }
        }
        screen
            .push_str("\nenter attach or resume   n new   d stop   x remove   r reset   q quit\n");
        // Last, so a line appearing and going never moves the hints.
        if let Some(note) = &self.note {
            screen.push_str(note);
            screen.push('\n');
        }
        screen
    }
}

/// The list a new box starts from: the workspace's own manifest and every
/// installed role. The cursor on it, nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub items: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickAction {
    Redraw,
    /// Show this item's permission preview.
    Chosen(usize),
    /// Return to the box list.
    Back,
}

impl Picker {
    pub fn new(items: Vec<String>) -> Self {
        Picker { items, selected: 0 }
    }

    pub fn update(&mut self, key: Key) -> PickAction {
        match key {
            Key::Up | Key::Down => {
                self.selected = step(self.selected, self.items.len(), key);
                PickAction::Redraw
            }
            Key::Enter => PickAction::Chosen(self.selected),
            Key::Quit => PickAction::Back,
            // This screen picks what a new box starts from. Nothing is
            // selected that could be stopped, removed or reset.
            Key::Stop | Key::New | Key::Remove | Key::Reset | Key::Yes => PickAction::Redraw,
        }
    }

    pub fn view(&self) -> String {
        let mut screen = String::from("start a new box from:\n\n");
        for (row, item) in self.items.iter().enumerate() {
            screen.push_str(cursor(row == self.selected));
            screen.push_str(item);
            screen.push('\n');
        }
        screen.push_str("\nenter preview   q back\n");
        screen
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
    match &manifest.agent.run {
        Some(agent) => {
            text.push_str(&format!("agent: {agent}"));
            if let Some(model) = &manifest.agent.model {
                text.push_str(&format!(" (model {model})"));
            }
            text.push('\n');
        }
        None => text.push_str("agent: none\n"),
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

/// The keys the preview screen offers, added by whoever is drawing a
/// screen. `role add` and `role show` print the same recipe to a stdout
/// with nobody at it, where offering a key to press would be a lie.
pub const PREVIEW_HINTS: &str = "\nenter start   q back\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> crate::manifest::Manifest {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let version = crate::manifest::VERSION;
        crate::manifest::parse(&format!(
            "version = {version}\n{extra}[image]\nbase = \"https://x.test/r.tar.gz\"\nbase_sha256 = \"{digest}\"\n"
        ))
        .expect("valid manifest")
    }

    #[test]
    fn the_picker_moves_chooses_and_goes_back() {
        let mut picker = Picker::new(vec!["workspace".to_owned(), "role alphaca".to_owned()]);
        assert_eq!(picker.update(Key::Down), PickAction::Redraw);
        assert_eq!(picker.update(Key::Enter), PickAction::Chosen(1));
        assert_eq!(picker.update(Key::Up), PickAction::Redraw);
        assert_eq!(picker.update(Key::Enter), PickAction::Chosen(0));
        assert_eq!(picker.update(Key::Quit), PickAction::Back);
    }

    #[test]
    fn the_picker_cursor_stays_inside_the_list() {
        let mut picker = Picker::new(vec!["only".to_owned()]);
        picker.update(Key::Down);
        picker.update(Key::Down);
        assert_eq!(picker.update(Key::Enter), PickAction::Chosen(0));
    }

    #[test]
    fn the_picker_view_marks_the_selection_and_hints() {
        let mut picker = Picker::new(vec!["workspace".to_owned(), "role alphaca".to_owned()]);
        picker.update(Key::Down);
        let view = picker.view();
        assert!(view.contains("  workspace"), "{view}");
        assert!(view.contains("> role alphaca"), "{view}");
        assert!(view.contains("enter preview"), "{view}");
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
            "image: ready",
        ] {
            assert!(view.contains(expected), "missing {expected:?} in:\n{view}");
        }
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
        crate::home::Scan {
            boxes: pids
                .iter()
                .enumerate()
                .map(|(n, pid)| Listing {
                    record: crate::home::Record {
                        id: format!("{n:012x}"),
                        workspace: std::path::PathBuf::from("/w"),
                        role: None,
                        source: None,
                        alias: None,
                        name: None,
                        agent: None,
                        created_unix: 0,
                        started_unix: 0,
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
        assert_eq!(tui.update(Key::Up), Action::Redraw);
        assert_eq!(tui.selected, 0);
        tui.update(Key::Down);
        tui.update(Key::Down);
        tui.update(Key::Down);
        assert_eq!(tui.selected, 1);
    }

    #[test]
    fn enter_attaches_and_d_stops_the_selected_box() {
        let mut tui = running(&[10, 20]);
        tui.update(Key::Down);
        assert_eq!(
            tui.update(Key::Enter),
            Action::Attach("000000000001".to_owned())
        );
        assert_eq!(tui.update(Key::Stop), Action::Do(Act::Stop(20)));
    }

    /// Enter does the one thing the row allows. On an idle box that is
    /// starting it again — the whole reason its home was kept.
    #[test]
    fn enter_on_an_idle_box_resumes_it_and_d_kills_nothing() {
        let mut tui = boxes(&[Some(10), None]);
        tui.update(Key::Down);
        assert_eq!(
            tui.update(Key::Enter),
            Action::Resume("000000000001".to_owned())
        );
        assert_eq!(tui.update(Key::Stop), Action::Redraw);
    }

    #[test]
    fn an_empty_list_only_redraws_on_enter_and_stop() {
        let mut tui = running(&[]);
        assert_eq!(tui.update(Key::Enter), Action::Redraw);
        assert_eq!(tui.update(Key::Stop), Action::Redraw);
        assert_eq!(tui.update(Key::Quit), Action::Quit);
    }

    /// The bug this pins: `d` on an idle box did the same thing as a key
    /// the panel does not know — nothing, drawn identically. Most rows in
    /// a panel are idle, so `d` read as broken.
    #[test]
    fn d_on_an_idle_box_says_why_rather_than_nothing() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Key::Stop), Action::Redraw);
        let screen = tui.view(0);
        assert!(screen.contains("not running"), "{screen}");
    }

    /// The words a stop reports itself with live here, not in the panel,
    /// so both outcomes are pinned where every other screen decision is.
    #[test]
    fn a_stop_says_what_it_came_to() {
        let mut tui = boxes(&[Some(7)]);
        tui.acted(&Act::Stop(7), Ok(()));
        assert!(tui.view(0).contains("stopping"), "{}", tui.view(0));
        tui.acted(
            &Act::Stop(7),
            Err("cannot stop the box's init 7: EPERM".to_owned()),
        );
        assert!(tui.view(0).contains("EPERM"), "{}", tui.view(0));
    }

    /// The gap this closes: the panel listed every box on this host and
    /// was the one surface from which none of them could be cleared up.
    #[test]
    fn x_asks_before_it_removes_and_y_is_what_answers() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Key::Remove), Action::Redraw);
        let asked = tui.view(0);
        assert!(asked.contains("remove box 000000000000"), "{asked}");
        assert!(asked.contains("y confirm"), "{asked}");
        assert_eq!(
            tui.update(Key::Yes),
            Action::Do(Act::Remove("000000000000".to_owned()))
        );
    }

    #[test]
    fn r_asks_before_it_empties_a_box_and_says_what_it_keeps() {
        let mut tui = boxes(&[None]);
        assert_eq!(tui.update(Key::Reset), Action::Redraw);
        let asked = tui.view(0);
        assert!(asked.contains("reset box 000000000000"), "{asked}");
        assert!(asked.contains("keeps its id"), "{asked}");
        assert_eq!(
            tui.update(Key::Yes),
            Action::Do(Act::Reset("000000000000".to_owned()))
        );
    }

    /// A question about deleting an agent's whole history must not be
    /// answerable by a stray keystroke.
    #[test]
    fn any_key_but_y_cancels_the_question_and_leaves_the_box_alone() {
        let mut tui = boxes(&[None]);
        tui.update(Key::Remove);
        assert_eq!(tui.update(Key::Down), Action::Redraw);
        assert!(!tui.view(0).contains("remove box"), "{}", tui.view(0));
        // The question is gone, so `y` is now a key with nothing to answer.
        assert_eq!(tui.update(Key::Yes), Action::Redraw);
    }

    /// `q` is the reflex for "no". Taking the whole panel down on it
    /// would be the one answer nobody meant.
    #[test]
    fn q_answers_the_question_rather_than_leaving_the_panel() {
        let mut tui = boxes(&[None]);
        tui.update(Key::Remove);
        assert_eq!(tui.update(Key::Quit), Action::Redraw);
        assert_eq!(tui.update(Key::Quit), Action::Quit);
    }

    /// Both acts touch the home a running agent is writing to. The claim
    /// the binary takes is the real proof; refusing here is what spares
    /// the user a question that was always going to be refused.
    #[test]
    fn a_running_box_is_neither_removed_nor_reset_and_the_screen_says_why() {
        for key in [Key::Remove, Key::Reset] {
            let mut tui = boxes(&[Some(7)]);
            assert_eq!(tui.update(key), Action::Redraw);
            let screen = tui.view(0);
            assert!(screen.contains("running"), "{screen}");
            assert!(screen.contains("d stops it first"), "{screen}");
            // Nothing was asked, so nothing can be confirmed.
            assert_eq!(tui.update(Key::Yes), Action::Redraw);
        }
    }

    #[test]
    fn an_empty_list_has_nothing_to_remove_or_reset() {
        let mut tui = running(&[]);
        assert_eq!(tui.update(Key::Remove), Action::Redraw);
        assert_eq!(tui.update(Key::Reset), Action::Redraw);
        assert!(!tui.view(0).contains("remove box"), "{}", tui.view(0));
    }

    #[test]
    fn a_removal_and_a_reset_say_what_they_came_to() {
        let mut tui = boxes(&[None]);
        tui.acted(&Act::Remove("000000000000".to_owned()), Ok(()));
        assert!(tui.view(0).contains("removed"), "{}", tui.view(0));
        tui.acted(&Act::Reset("000000000000".to_owned()), Ok(()));
        assert!(tui.view(0).contains("reset"), "{}", tui.view(0));
        tui.acted(
            &Act::Remove("000000000000".to_owned()),
            Err("box 000000000000 is running".to_owned()),
        );
        assert!(tui.view(0).contains("is running"), "{}", tui.view(0));
    }

    /// A key that is not on the screen is a key nobody presses — and a
    /// keycap on the screen that decodes to nothing is a screen that
    /// lies. Both halves are asserted against the same binding table.
    #[test]
    fn every_keycap_the_hints_show_is_a_key_the_panel_decodes() {
        let screen = boxes(&[None]).view(0);
        for (cap, key) in [
            ('n', Key::New),
            ('d', Key::Stop),
            ('x', Key::Remove),
            ('r', Key::Reset),
            ('q', Key::Quit),
        ] {
            assert!(screen.contains(cap), "no {cap:?} in the hints:\n{screen}");
            assert_eq!(Key::from_char(cap), Some(key), "{cap:?}");
        }
        assert!(screen.contains("enter"), "{screen}");
        // The confirm keycap is named on the question, not in the hints.
        let mut asking = boxes(&[None]);
        asking.update(Key::Remove);
        assert!(asking.view(0).contains("y confirm"), "{}", asking.view(0));
        assert_eq!(Key::from_char('y'), Some(Key::Yes));
    }

    /// A character the panel does not bind must stay unbound, or a stray
    /// keystroke starts meaning something.
    #[test]
    fn a_character_the_panel_does_not_bind_decodes_to_nothing() {
        assert_eq!(Key::from_char('z'), None);
        assert_eq!(Key::from_char('Y'), None);
    }

    /// A refusal answers one key press. Left on the screen it would go on
    /// describing a key the user has since moved past.
    #[test]
    fn a_refusal_is_cleared_by_the_next_key() {
        let mut tui = boxes(&[None, Some(7)]);
        tui.update(Key::Stop);
        assert!(tui.view(0).contains("not running"));
        tui.update(Key::Down);
        assert!(!tui.view(0).contains("not running"), "{}", tui.view(0));
    }

    /// A new box needs no selection: it starts in the current workspace.
    #[test]
    fn n_starts_a_new_box_with_or_without_a_selection() {
        assert_eq!(running(&[]).update(Key::New), Action::New);
        assert_eq!(running(&[1]).update(Key::New), Action::New);
    }

    #[test]
    fn a_refresh_that_shrinks_the_list_pulls_the_cursor_back() {
        let mut tui = running(&[1, 2, 3]);
        tui.update(Key::Down);
        tui.update(Key::Down);
        tui.refresh(scan(&[Some(1)]));
        assert_eq!(tui.selected, 0);
        tui.refresh(crate::home::Scan::default());
        assert_eq!(tui.selected, 0);
    }

    /// What the scan could not read is drawn on the panel, not printed at
    /// it: the panel owns the terminal, and a reader that writes there
    /// draws over the screen every time the list is refreshed.
    #[test]
    fn what_the_scan_could_not_read_is_drawn_on_the_panel() {
        let mut tui = Tui::default();
        tui.refresh(crate::home::Scan {
            problems: vec!["/data/homes/proj-1 holds no box record".to_owned()],
            ..scan(&[Some(10)])
        });
        let view = tui.view(0);
        assert!(view.contains("could not read:"), "{view}");
        assert!(
            view.contains("  /data/homes/proj-1 holds no box record"),
            "{view}"
        );
        // The hints stay last: a warning must not push them off the end.
        assert!(
            view.lines().last().is_some_and(|l| l.contains("q quit")),
            "{view}"
        );
    }

    #[test]
    fn a_scan_with_nothing_wrong_says_nothing() {
        let view = running(&[10]).view(0);
        assert!(!view.contains("could not read"), "{view}");
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
        let view = tui.view(0);
        assert!(
            view.contains(&format!("home {}", PROBLEMS_SHOWN - 1)),
            "{view}"
        );
        assert!(!view.contains(&format!("home {PROBLEMS_SHOWN}")), "{view}");
        assert!(view.contains("... and 3 more"), "{view}");
    }

    #[test]
    fn the_view_marks_the_selected_row_and_shows_the_hints() {
        let mut tui = running(&[10, 20]);
        tui.update(Key::Down);
        let view = tui.view(0);
        let lines: Vec<&str> = view.lines().collect();
        assert!(lines[0].starts_with("  ID"), "{view}");
        assert!(lines[1].starts_with("  "), "{view}");
        assert!(lines[2].starts_with("> "), "{view}");
        assert!(view.contains("enter attach"), "{view}");
    }

    #[test]
    fn an_empty_view_says_no_boxes_without_a_cursor() {
        let view = running(&[]).view(0);
        assert!(view.contains("no boxes yet"), "{view}");
        assert!(!view.contains("> "), "{view}");
    }
}
