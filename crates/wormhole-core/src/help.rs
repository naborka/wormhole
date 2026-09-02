//! `wormhole help`: the whole tool on one page.
//!
//! Written to be read once and then used — by a person who has never seen
//! this tool, and by an agent that has one screenful of context to spare.
//! So: every command, the manifest that drives them, how to make a role
//! and how to move the agent's version, and nothing else.
//!
//! The command table here is the only list of commands wormhole has. The
//! page renders from it and so does the one-line usage a refusal prints,
//! and a test pins it against the dispatch in the binary — a help page
//! that has drifted from the tool is worse than none, because it is
//! believed.

/// One command, as the help lists it. `name` is the word typed after
/// `wormhole`; `args` is what may follow it.
pub struct Line {
    pub name: &'static str,
    pub args: &'static str,
    pub blurb: &'static str,
}

/// The first three things anybody runs, in the order they run them.
const START: &[Line] = &[
    Line {
        name: "doctor",
        args: "",
        blurb: "can this host run boxes? run this first",
    },
    Line {
        name: "init",
        args: "",
        blurb: "write a wormhole.toml here; never over one already there",
    },
    Line {
        name: "box",
        args: "",
        blurb: "run the agent in this folder",
    },
];

const BOXES: &[Line] = &[
    Line {
        name: "box",
        args: "[--new] [--as NAME]",
        blurb: "start one; bare `box` resumes the last free one",
    },
    Line {
        name: "box",
        args: "--id ID|NAME",
        blurb: "that exact box, whenever",
    },
    Line {
        name: "box",
        args: "--role NAME|DIR|REF",
        blurb: "use a role instead of ./wormhole.toml",
    },
    Line {
        name: "box",
        args: "-- CMD...",
        blurb: "your command in the box instead of the agent",
    },
    Line {
        name: "ps",
        args: "[--all]",
        blurb: "what is running; --all adds every idle box too",
    },
    Line {
        name: "attach",
        args: "ID|NAME [-- CMD...]",
        blurb: "a second terminal into a running box",
    },
    Line {
        name: "stop",
        args: "ID|NAME",
        blurb: "end it; the home is kept for the next start",
    },
    Line {
        name: "rename",
        args: "ID|NAME NEW",
        blurb: "name it, without starting it",
    },
    Line {
        name: "reset",
        args: "ID|NAME",
        blurb: "keep the box, empty its home",
    },
    Line {
        name: "remove",
        args: "ID|NAME...",
        blurb: "take boxes away for good",
    },
];

const ROLES: &[Line] = &[
    Line {
        name: "role add",
        args: "DIR [--as NAME]",
        blurb: "install a role you wrote; symlinked, not copied",
    },
    Line {
        name: "role add",
        args: "URL@SHA [--as NAME]",
        blurb: "one from git, pinned; shows the recipe and asks",
    },
    Line {
        name: "role list",
        args: "",
        blurb: "what is installed, and whether it can start",
    },
    Line {
        name: "role show",
        args: "NAME|DIR",
        blurb: "the whole recipe, without installing it",
    },
    Line {
        name: "role remove",
        args: "NAME",
        blurb: "take the name back; your directory is left alone",
    },
];

const REST: &[Line] = &[
    Line {
        name: "build",
        args: "[--role NAME|DIR|REF]",
        blurb: "build the image now, not at the next start",
    },
    Line {
        name: "gc",
        args: "[--delete [--unreferenced]]",
        blurb: "what the store holds, and what of it can go",
    },
    Line {
        name: "usage",
        args: "",
        blurb: "what is left of the account's limits",
    },
    Line {
        name: "broker",
        args: "",
        blurb: "the host-side proxy that holds the credential",
    },
    Line {
        name: "run",
        args: "[--grant PATH]... -- CMD...",
        blurb: "the boundary on its own, with no manifest",
    },
    Line {
        name: "tui",
        args: "",
        blurb: "the panel; same as bare `wormhole`",
    },
    Line {
        name: "help",
        args: "",
        blurb: "this page",
    },
];

/// Every command wormhole answers to. The drift test compares this
/// against the binary's own dispatch.
pub fn commands() -> impl Iterator<Item = &'static str> {
    [START, BOXES, ROLES, REST]
        .into_iter()
        .flatten()
        .map(|line| line.name)
}

/// The shape of a command line, for a refusal that had no more specific
/// one to give. Not the whole list: a wall of text under an error hides
/// the error, which is the thing the reader came for.
pub const USAGE: &str = "usage: wormhole <command> [args]";

/// The last line of every refusal. One place to go, named the same way
/// every time, so nobody has to guess what the tool is called next.
pub const MORE: &str = "run `wormhole help` for the whole tool on one page";

/// The masthead, and the two things every reader needs before the rest
/// means anything.
const HEAD: &str = r#"wormhole — run a coding agent in a box that sees one folder of yours and
nothing else of your machine. No daemon, no root, no container runtime.

USAGE
  wormhole                      the panel: every box, pick one
  wormhole COMMAND [args]

FIRST RUN
"#;

const WHAT_A_BOX_IS: &str = r#"
A BOX
  Its own $HOME, kept between runs: history, logins, whatever the agent
  installed. Its root filesystem is a fresh copy of the image and is
  deleted when the box exits, so nothing outside $HOME survives.
  One folder holds as many boxes as you make. Name one with `--as`; after
  that its 12-hex id and that name both work, everywhere a box is taken.

"#;

const AFTER_BOXES: &str = r#"
  reset keeps the box and empties it. remove takes it away. Both need the
  box stopped, and both are refused while it runs.

PANEL   (bare `wormhole`)
  enter join or start · n new · d stop · x remove · r reset · q quit
  x and r ask first. y is the only key that answers; anything else cancels.

MANIFEST
  ./wormhole.toml is the whole recipe. Nothing reaches the box unless this
  file names it. Change any [image] key and the next start builds a new
  image; change anything else and it does not.

  version = 1

  [image]
  base = "https://.../rootfs.tar.gz"
  base_sha256 = "<64 hex>"          # checked before anything is extracted
  packages = ["git", "nodejs"]      # apk add, once, when the image is built
  build = ["npm i -g some-tool"]    # shell lines, in order, as root

  [[image.artifact]]                # a file fetched on the HOST and proved
  url = "https://.../tool.tgz"      # by its digest, then handed to the
  sha256 = "<64 hex>"               # build read-only. The box opens no
  into = "/tmp/tool.tgz"            # connection for it

  [agent]
  run = "claude"
  model = "claude-opus-5"
  instructions = "ROLE.md"          # added to the built-in instructions
  preflight = "hooks/setup.sh"      # runs in the box before the agent

  [access]                          # baseline is empty: this folder only
  grants = ["~/.ssh"]               # host paths the box may see
  dns = "1.1.1.1"                   # without it the box has no resolver
  host_ca = true                    # trust what the host trusts
  network = "none"                  # no route off the machine at all
  broker = true                     # reach the API via the host, so the
                                    # box holds no credential of its own

  [env.SOME_VAR]
  fixed = "1"                       # the box cannot override this
  # default = ""                    # host's value wins when it has one

  [runtime]
  rootfs = "copy"                   # or "readonly": no copy, faster, and
                                    # no installing at run time
  snapshot = true                   # say what the agent changed, on exit

MAKE A ROLE
  A role is the same manifest, kept somewhere that is not the tree you are
  standing in, so one recipe serves any folder.

  mkdir -p myrole/hooks
  # myrole/wormhole.toml    the manifest above
  # myrole/ROLE.md          extra instructions for the agent   (optional)
  # myrole/hooks/setup.sh   runs in the box before the agent   (optional)
  wormhole role add ./myrole --as mine
  wormhole box --role mine

  A role is identified by where it comes from, never by what you typed, so
  every spelling of one role gets you the same box back.

"#;

const UPDATE: &str = r#"
MOVE THE AGENT'S VERSION
  The version is pinned in the manifest, so moving it is an edit and a
  rebuild. A box looks its image up on every start and keeps its home, so
  nothing is lost and there is nothing to log into again.

  1. edit the [[image.artifact]] url and sha256 to the new version
  2. wormhole build                        # or let the next start do it
  3. wormhole stop ID                      # it keeps its old root until
     wormhole box --id ID                  # this, which applies the change
  4. wormhole gc --delete --unreferenced   # give the old image back

  An agent that updates itself inside the box writes into a filesystem
  that is deleted on exit, so that update never survives a restart. Turn
  it off — [env.DISABLE_UPDATES] with fixed = "1" — and let the pin decide.

MORE
"#;

const TAIL: &str = r#"
DISK
  wormhole gc removes only what it can prove.
  dead          finished with               --delete takes it
  unreferenced  no box here starts from it  --delete --unreferenced
  unproven      a recipe could not be read  nothing takes it
  live          in use                      nothing takes it

WHERE THINGS ARE
  ./wormhole.toml                   this folder's recipe
  ~/.config/wormhole/roles/         installed roles
  ~/.local/share/wormhole/homes/    one kept $HOME per box
  ~/.local/share/wormhole/images/   built images, by recipe digest

EXIT
  0 did it   ·   1 refused, and said why   ·   2 the command was wrong

Full guide: https://naborka.github.io/wormhole
"#;

/// The page. Prose in raw strings, so what is written is what prints;
/// command lists rendered from the one table above, so they cannot drift.
pub fn page() -> String {
    let mut out = String::from(HEAD);
    section(&mut out, START);
    out.push_str(WHAT_A_BOX_IS);
    section(&mut out, BOXES);
    out.push_str(AFTER_BOXES);
    section(&mut out, ROLES);
    out.push_str(UPDATE);
    section(&mut out, REST);
    out.push_str(TAIL);
    out
}

/// One block of commands, with the blurbs lined up under each other.
fn section(out: &mut String, lines: &[Line]) {
    let left = |line: &Line| {
        if line.args.is_empty() {
            line.name.to_owned()
        } else {
            format!("{} {}", line.name, line.args)
        }
    };
    let width = lines.iter().map(|line| left(line).chars().count()).max();
    let Some(width) = width else { return };
    for line in lines {
        let spelled = left(line);
        let pad = width - spelled.chars().count();
        out.push_str(&format!("  {spelled}{:pad$}  {}\n", "", line.blurb));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An agent reads this once and then acts on it, so what it costs to
    /// read matters as much as what it says. Wide enough for a terminal
    /// nobody has resized, short enough to keep.
    #[test]
    fn the_page_fits_a_terminal_and_a_context_window() {
        let page = page();
        let lines: Vec<&str> = page.lines().collect();
        assert!(lines.len() < 140, "{} lines is too long", lines.len());
        for line in &lines {
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line:?}",
                line.chars().count()
            );
        }
    }

    /// Every command is spelled the way it is typed, so an agent can lift
    /// a line out of the page and run it.
    #[test]
    fn the_page_names_every_command_it_lists() {
        let page = page();
        for command in commands() {
            assert!(page.contains(command), "{command} is not on the page");
        }
    }

    /// The things somebody comes to this page to find out. Named here so
    /// dropping one is a failing test rather than a quiet regression.
    #[test]
    fn the_page_answers_what_people_arrive_asking() {
        let page = page();
        for topic in [
            "wormhole.toml",
            "[[image.artifact]]",
            "role add",
            "DISABLE_UPDATES",
            "gc --delete --unreferenced",
            "~/.local/share/wormhole/homes/",
            "unreferenced",
        ] {
            assert!(page.contains(topic), "the page never mentions {topic:?}");
        }
    }

    /// A wall of text under an error hides the error. One line of shape,
    /// one line saying where the rest is.
    #[test]
    fn a_refusal_points_at_the_page_instead_of_reprinting_it() {
        assert_eq!(USAGE.lines().count(), 1, "{USAGE}");
        assert_eq!(MORE.lines().count(), 1, "{MORE}");
        assert!(MORE.contains("wormhole help"), "{MORE}");
        // The prefix `usage()` keys off to tell a parser's answer from a
        // sentence. Pinned here, beside the string it is checked against.
        assert!(USAGE.starts_with("usage:"), "{USAGE}");
    }

    /// The blurbs line up, which is most of what makes a list scannable.
    #[test]
    fn a_section_lines_its_blurbs_up() {
        let mut out = String::new();
        section(
            &mut out,
            &[
                Line {
                    name: "ps",
                    args: "",
                    blurb: "short",
                },
                Line {
                    name: "attach",
                    args: "ID|NAME",
                    blurb: "long",
                },
            ],
        );
        let column = |blurb: &str| {
            out.lines()
                .find(|line| line.ends_with(blurb))
                .map(|line| line.len() - blurb.len())
                .expect("the row")
        };
        assert_eq!(column("short"), column("long"), "{out}");
    }
}
