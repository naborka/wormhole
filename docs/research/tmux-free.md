# A wormhole that never touches tmux

Primary-source research into taking every tmux rule out of wormhole while a
phone running Moshi keeps everything it needs. Checked 2026-10-19 against:
wormhole at 1e11443 plus the uncommitted tmux layer on top of it; tmux 3.5a,
run (Alpine 3.22 packages `tmux-3.5a-r1`, `libevent-2.1.13-r0`,
`tmux-doc-3.5a-r1`) and read (tag `3.5a`, commit 549c35b06165), and current
tmux for what changed since (tag `3.8`, commit 72c20dc22c24, and master
e5a694618bdb); Moshi 4.0.3 (App Store, released 2026-10-17; Google Play,
updated 2026-10-18), its docs at `getmoshi.app`, its App Store version
history and its own setup skill (`rjyo/moshi-skill`); Claude Code 2.1.303,
the binary in this box (a Bun-compiled ELF, read at byte offsets written
`@N` below) and its docs at `code.claude.com`; Codex `rust-v0.163.0`
(commit 83086188ae06) and its docs at `learn.chatgpt.com`; Linux
`drivers/tty/tty_jobctrl.c` at master c03c1a14507d. The earlier run of Moshi
4.0.2 against real boxes is in [remote-control.md](remote-control.md).
"Inference" marks reasoning of my own rather than a quote or a run. "ADR
0006" below is its first version, which hosted boxes in tmux sessions; what
this research led to replaced it under the same number.

The question, from the owner: as few interactions as possible between
wormhole and tmux, fully transparent. wormhole never creates or resumes a
tmux session by itself; best of all, no tmux dependency at all. The agent in
a box still works smoothly inside tmux, and Moshi gets everything it needs
from the user's own tmux sessions and windows (tabs). No rule like one box
per tmux session. The trigger: in Moshi, a new tab (a tmux window) in an
existing session, `wormhole`, Enter on an idle box; wormhole made a new tmux
session and switched the client there, where the box was expected to run in
that tab.

**Answer up front.** Possible, and Moshi keeps working. wormhole needs no
tmux code: a box already keeps the terminal it was started in, so a box
started in a tmux window is that window's process, and everything Moshi uses
without moshi-hook (its session picker, tmux's windows as tabs, a push from a
host `alert-bell` hook, deep links by session and window) comes from the
user's tmux. No Moshi feature needs one agent per session, a session named
after a box, or anything from wormhole. What goes: persistence for a box
started outside any multiplexer, `attach` reaching the agent already
running, the panel jumping to a running box, a session named after a box.
Each is the multiplexer's job or needs a decision, not tmux code in
wormhole. What stays: the box keeps the caller's PTY; `TERM` and its
description cross and `TMUX` never does; Claude Code is told to ring the
bell; the box's own `/tmp` and `/run`; never granting a socket directory.

---

## What the evidence says

1. **All of wormhole's tmux code is uncommitted and comes out cleanly.**
   Three new files: `crates/wormhole/src/tmux.rs` (101 lines),
   `crates/wormhole-core/src/terminal.rs` (549 lines, 23 tests),
   `crates/wormhole-core/src/config.rs` (66 lines, 3 tests); one new test
   file, `crates/wormhole/tests/tmux.rs` (793 lines, 28 tests on a recording
   fake `tmux`); and hunks in tracked files. Every hunk of `main.rs` is tmux
   (`git diff -U0`: lines 10, 18, 74, 239-251, 377-389, 448, 1082,
   1091-1268, 1851-1858). The 31 modified files also carry unrelated
   changes, listed under "In the same files, not tmux".
2. **Without it, wormhole already does what the owner expected.** At
   1e11443 the panel runs every session as its own child on the panel's
   terminal and draws itself again when it ends: `panel::Pick::Attach` calls
   `session(["attach", id])` (HEAD `main.rs:1052`), an idle box starts
   through `session(["box", "--id", id], Some(workspace))` (HEAD
   `main.rs:1206-1211`), and `session` spawns `wormhole <argv>` and waits
   (HEAD `main.rs:1064-1086`). The uncommitted code keeps exactly that
   path for herdr, zellij and a plain terminal (`session`,
   `main.rs:1093-1100`) and diverts only when `TMUX` is set. Enter on an
   idle box in a Moshi tab then starts the box in that tab.
3. **A box keeps the caller's terminal, so tmux sees the agent directly.**
   Nothing in `crates/wormhole/src` calls `setsid`, `setpgid`, `tcsetpgrp`
   or `TIOCSCTTY` (`rg`, no match). The agent writes to the pane's own PTY:
   BEL, OSC 0 and 2, focus reports and key sequences cross untouched. herdr
   already read a real boxed agent through the pane's process group
   (remote-control.md, finding 2).
4. **What crosses into a box from a tmux pane: `TERM_PROGRAM=tmux`, never
   `TMUX`.** `TERM`, `COLORTERM`, `TERM_PROGRAM` and `TERM_PROGRAM_VERSION`
   are declared for every box and take the host's values
   (`crates/wormhole-core/src/terminfo.rs:20-25`, `manifest.rs:1180-1185`);
   `TERM` only when its compiled description could be copied into the box
   home, else `xterm-256color` (`crates/wormhole/src/terminfo.rs:24-29`).
   Nothing undeclared reaches a box (`manifest.rs:1165-1168`). tmux gives
   every pane `TERM` from `default-terminal`, `TERM_PROGRAM=tmux`,
   `TERM_PROGRAM_VERSION` and `TMUX` (`environ.c:260-265, 277-278`) and
   `TMUX_PANE` (`spawn.c:313`), and no `COLORTERM` (run below). So a boxed
   agent sees `TERM=tmux-256color`, `TERM_PROGRAM=tmux` and no `TMUX`.
5. **Moshi's picker lists sessions, never windows.** "The connect-time
   session picker runs one shell command over your SSH connection, before
   any terminal exists. It never talks to moshi-hook", namely
   `sh -lc 'command -v tmux; tmux list-sessions'`
   ([debug-multiplexer-chooser](https://getmoshi.app/docs/debug-multiplexer-chooser)).
   Its own comparison: "Moshi picker granularity: Session, then find your
   window"; "tmux attach drops you in the session; you swipe or tap your way
   to the right window"
   ([tmux-or-herdr](https://getmoshi.app/guides/tmux-or-herdr)). Run below:
   a session of three windows prints as one line, `work: 3 windows`.
6. **Moshi's tabs are tmux's windows, driven by tmux's own keys.** Its tmux
   panel "Sends Ctrl+B (the tmux prefix) followed by the key": `c` "Create
   new window", `n`, `p`, `d`, `1-9, 0` "Switch to window by number"; a
   number row: "Tap 3 to jump to window 3"
   ([keyboard guide](https://getmoshi.app/articles/moshi-keyboard-guide)).
   None of the 90 pages in its sitemap mentions control mode or `-CC`. The
   window tree (Sidebar, formerly Jump To) "asks moshi-hook on the host for
   a live snapshot"
   ([jump-to](https://getmoshi.app/docs/jump-to)), and the swipe between
   windows needs moshi-hook's "Live multiplexer detection"
   ([hooks](https://getmoshi.app/docs/hooks)). With moshi-hook off, as a
   wormhole host keeps it (remote-control.md, finding 6), the tab list a
   phone sees is tmux's own status line. Moshi turns tmux's mouse on at
   attach ("Opt-in; Moshi enables it at attach", tmux-or-herdr) and "already
   clears `status-right` on sessions it starts"
   ([moshi-skill](https://github.com/rjyo/moshi-skill/blob/main/skills/moshi-best-practices/SKILL.md),
   line 243); 3.15.0 added "Turn off global tmux option changes in Settings,
   Advanced, tmux" (App Store history).
7. **Deep links name a session and at most windows 0 to 9.** "For tmux,
   session is the tmux session name. The optional window is a single-digit
   window index from 0 through 9; Moshi resumes the matching active card and
   sends the configured tmux prefix followed by that digit." A pane id
   works only through moshi-hook: "Pane focus requires a current moshi-hook
   daemon on the host". Links "only resume cards that are already active or
   minimized"; with none matching, "Moshi shows an error instead of opening
   a new connection" ([notifications](https://getmoshi.app/docs/notifications)).
   This corrects remote-control.md's "Deep links" line, which lists
   `window=<n>&pane=<id>` without either limit.
8. **A push needs only the host's tmux, with its defaults.** The webhook
   takes `token`, `title`, `message` and `data: {"type": "url", "url":
   "moshi://..."}` (notifications). Once the user sets an `alert-bell`
   hook, tmux runs it for a bell in any window of any session, attached or
   not, with no other setting: `monitor-bell` defaults on, `bell-action` to
   `any` (`options-table.c:1020, 479`; `alerts.c:183-215`). In the hook, `#{session_name}` and
   `#{window_index}` are the bell's; `#{pane_id}` is the window's *active*
   pane, not the one that rang (`cmd-find.c:744`), and `#{hook_pane}` is
   empty, because the bell is announced per window (`notify.c:212, 275-281`).
   Current tmux is the same (master `cmd-find.c:764`, `events.c:187-199`).
   Run below, all of it.
9. **Moshi labels a card with the outer terminal's title.** "Moshi reads the
   terminal title to label a session, the same way it picks up tmux's
   set-titles" ([herdr guide](https://getmoshi.app/guides/herdr)). tmux
   writes that title only with `set-titles on`, off by default
   (`options-table.c:668`), from `set-titles-string`, by default
   `#S:#I:#W - "#T" #{session_alerts}` (`:675`). Moshi's own setup skill
   recommends `set-titles on` with `"#I: #W"`, `base-index 1`, one session
   per project directory, and windows `agent`, `review`, `tests`, `servers`,
   `misc` (moshi-skill lines 186-188, 208, 219-225).
10. **No Moshi feature needs one agent per session, a session named after a
    box, or anything from wormhole.** Findings 5 to 9: sessions are picked,
    windows are tmux's, pushes and labels come from the host's tmux, links
    take any session name. Moshi's own advice is the opposite of a session
    per agent: "Keep one agent per window when possible"
    ([tmux](https://getmoshi.app/docs/tmux)); "For most people the answer
    is one host, one multiplexer, one window (or tab) per agent", "Give
    each agent its own window or tab, named for the task", with `tmux
    new-window -n tests` as the example ([fleet](https://getmoshi.app/guides/fleet)).
    That is the tmux-free layout exactly: a box per window, the window
    named by the user. The only session naming Moshi does is its own
    launcher: `moshi DIR` "names the tmux session after the directory
    basename" ([moshi-cli](https://getmoshi.app/docs/moshi-cli)).
11. **tmux drops desktop notifications and passes the bell.** OSC 9 and OSC
    777 fall into the `default` case and are dropped (`input.c:2391-2447`);
    current tmux handles only the OSC 9;4 progress bar (master `input.c`,
    `input_osc_9`). `allow-passthrough` is off by default
    (`options-table.c:881-883`). A bell goes on to every attached client of
    the session that is not in control mode (`alerts.c:310-314`). Run below.
12. **Claude Code in a box under host tmux rings only because wormhole asks
    it to.** It names the terminal from `TERM_PROGRAM` before it looks at
    `TMUX` (`@227842611`), so a box reads as terminal `tmux`. With the
    default `preferredNotifChannel = "auto"` it sends a desktop notification
    "in iTerm2, Ghostty, and Kitty ... and does nothing elsewhere"
    ([settings-reference](https://code.claude.com/docs/en/settings-reference#preferrednotifchannel));
    the code falls to `no_method_available` (`@247642513`, `@247642730`).
    wormhole writes `terminal_bell` where unset (`manifest.rs:511-512`), and
    `"terminal_bell"` "rings the bell character in any terminal". It wraps
    escapes in tmux passthrough only when `TMUX` is set (`@235521603`,
    `@235522560`), so a box never wraps. Being away means not typing:
    `permission_prompt` after "about six seconds", `idle_prompt` when "Claude
    finished responding about 60 seconds ago and you haven't typed since"
    ([hooks](https://code.claude.com/docs/en/hooks#notification)); no focus
    is needed.
13. **Codex in a box under host tmux rings the bell, but only when it
    believes it is unfocused.** Its detection skips `TERM_PROGRAM=tmux`
    (`codex-rs/terminal-detection/src/lib.rs:257-263`), and
    `TERM=tmux-256color` names no terminal (`:124-130`), so `auto` picks BEL
    (`codex-rs/tui/src/notifications/mod.rs:20-32, 53-62`). It wraps OSC 9
    only under `TMUX` (`notifications/osc9.rs:50`). The default condition is
    "Emit TUI notifications only while the terminal is unfocused"
    (`codex-rs/config/src/types.rs:742-748`); focus starts true
    (`codex-rs/tui/src/tui.rs:752`), Codex asks for DEC 1004 focus reports
    (`tui.rs:262`) and checks the flag before it notifies (`tui.rs:117-122,
    1018-1022`).
14. **Without `focus-events`, a pane in a tab you leave never hears
    focus-out.** Off by default (`options-table.c:328`). Off, tmux neither
    asks the terminal for focus reports (`tty.c:505-506`) nor reports a
    window or pane switch (`session.c:501-505`, `window.c:533-536`); it
    reports focus only for the session's current window, on attach, detach
    and the terminal's own reports (`server-client.c:381-387`,
    `tty-keys.c:876-884`). Run below: a pane whose window was not current
    when the client detached heard focus-in at attach and nothing after.
    Inference: with defaults, a Codex box in a tab you switched away from
    keeps believing it is focused, and does not ring while you are in other
    tabs, attached or detached.
    remote.md's "Codex rings by default inside tmux when its terminal is
    unfocused" holds only with `focus-events on`, or for the tab still
    current when the last client detaches.
15. **`TMUX` must keep not crossing.** A tmux command from a process whose
    `TMUX` names a socket it cannot reach fails: `error connecting to
    /nonexistent/tmux-1000/default (No such file or directory)` (run below).
    Claude Code's agent teams would split panes "when you're already running
    inside a tmux session" ([agent-teams](https://code.claude.com/docs/en/agent-teams));
    its pane code throws "Could not determine current tmux pane/window"
    (`@263979563`). Codex runs `tmux` only when `TMUX` or `TMUX_PANE` is set
    (`terminal-detection/src/lib.rs:333-338`). Inference: were the socket
    reachable, a teammate's `split-window` would start a process in a host
    pane, outside the box.
16. **Every box's tab is named `wormhole` by default.** `automatic-rename` is
    on with `#{pane_current_command}` (`options-table.c:916-923`), which is
    `argv[0]` of the pane's foreground process group leader, read from host
    `/proc` (`osdep-linux.c:30-61`). The kernel hands tmux that group's
    number in tmux's own PID namespace (`tiocgpgrp`, `pid_vnr`,
    `tty_jobctrl.c:466-480`), and the leader is the host's `wormhole`
    (finding 3), so a box's PID 1 in its own namespace changes nothing. Run
    below with a stand-in: `name=wormhole`. `allow-rename` is off
    (`options-table.c:892`), so `ESC k ... ESC \` renames nothing;
    `allow-set-title` is on (`:900`), so OSC 0 and 2 set `#{pane_title}`
    (`input.c:2392-2401`). A user's format renames the tab with no help from
    wormhole: `#{b:pane_current_path}` gives the project directory,
    `#{pane_title}` the agent's title (run below).
17. **Claude Code's interleaving warning is about resuming one session
    twice.** "If you resume the same session in two terminals without
    forking, messages from both interleave into one transcript"
    ([sessions](https://code.claude.com/docs/en/sessions), line 211 of its
    Markdown). `attach` without a pane runs `claude
    --dangerously-skip-permissions` (`manifest.rs:560`): a new session on the
    same home, not the running one. Concurrent sessions sharing
    `~/.claude.json` were fixed in 2.1.259: "Fixed concurrent sessions
    silently reverting each other's `~/.claude.json` changes" (CHANGELOG).
    ADR 0006's "interleaving one transcript" overstates the cost.
18. **Two clients on one session share its current window.** Run below: the
    phone's `prefix n` moved the desktop client too. A session group keeps
    them apart: "The current and previous window and any session options
    remain independent" (tmux(1), `new-session -t`). One bell in a window
    two grouped sessions share runs the hook once per session.
19. **Two tmux settings that phone guides recommend open a box up.**
    `set-clipboard on`, which Moshi's [clipboard](https://getmoshi.app/docs/clipboard)
    page asks for, makes tmux answer an OSC 52 query from any pane with the
    newest paste buffer (`input.c:2937-2944`). Run below: a pane read
    `copied-in-another-pane`. tmux 3.7 added `get-clipboard`, whose default
    `buffer` answers the same way (tag `3.7` and `3.8` `options-table.c`,
    `.default_num = 1`; tmux(1) 3.8: "if buffer, tmux responds with the
    newest paste buffer"); `request` and `both` would ask the terminal of
    "the most recently used client", a phone's when it was last used, for
    its own clipboard (whether Moshi answers: not run). `allow-passthrough
    on`, in remote.md's
    `tmux.conf`, hands the client terminal whatever a pane wraps (run below:
    a wrapped OSC 9 reached the client bare). A boxed agent never wraps
    (finding 12, 13), so it gains nothing it needs; a hostile one gains a
    path past tmux to the phone's terminal (Inference).
20. **The security rules stand with zero tmux code.** A box has its own
    `/tmp`, `/run` and `/dev/shm` (`mount_plan.rs:457-462`, committed), which
    keeps every multiplexer socket out of reach. "Never grant `/tmp`,
    `$XDG_RUNTIME_DIR`, `~/.config/herdr` or a tmux socket directory"
    (`docs/src/guide/remote.md:169-174`) is prose only: grant validation
    refuses relative, `..`, symlinked and workspace-overlapping paths and
    nothing else (`mount_plan.rs:560-580`). moshi-hook and Tailscale SSH stay
    off (`docs/src/guide/access.md:109-114`; remote-control.md, findings 6
    and 7). None of these is a tmux feature; each is about what a box can
    reach on the host.

---

## Run, not read

tmux 3.5a from Alpine 3.22's packages, unpacked into a scratch directory,
run with no configuration (`-f /dev/null`) on a private socket under a
scratch `TMUX_TMPDIR`; real clients on a pseudo-terminal (`pty.fork()`
running `tmux attach`). Small scripts stood in for an agent.

- **Defaults** (`show-options`): `monitor-bell on`, `bell-action any`,
  `visual-bell off`, `allow-rename off`, `allow-set-title on`,
  `allow-passthrough off`, `automatic-rename on`, `automatic-rename-format
  "#{?pane_in_mode,[tmux],#{pane_current_command}}#{?pane_dead,[dead],}"`,
  `set-titles off`, `set-clipboard external`, `extended-keys off`,
  `extended-keys-format xterm`, `focus-events off`, `detach-on-destroy on`,
  `default-terminal tmux-256color`, `terminal-features[0]
  xterm*:clipboard:ccolour:cstyle:focus:title`.
- **A pane's environment**, on a server started with no `COLORTERM`:
  `TERM=tmux-256color`, `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.5a`,
  `TMUX=<socket>,<pid>,0`, `TMUX_PANE=%1`; no `COLORTERM`.
- **A bell, no client attached.** A child of the shell in pane `%3`, the
  non-active pane of window 2, ran `printf '\a'`. The hook ran with
  `session=work window=2 pane=%2 hook_pane= hook_window=@2`: the window's
  active pane, not the one that rang. Window 2's bell flag was set.
- **A bell, client attached.** From the current pane: hook with
  `window=0 pane=%0`, and the client's terminal got the BEL byte. From the
  non-active pane of another window: hook with `window=2 pane=%2`, and the
  client still got the BEL.
- **Titles and names.** OSC 2 set `pane_title` to `box api` and left the
  window name `sh`. `ESC k RENAMED ESC \` changed nothing until
  `allow-rename on`; then the name was `RENAMED` and `automatic-rename`
  turned off for that window. With `allow-set-title off`, OSC 2 changed
  nothing.
- **A box's stand-in.** A job leader whose `argv[0]` is `wormhole`, with a
  child in its process group that set the title and rang: `cmd=wormhole
  name=wormhole title=agent-set-title`, and the hook ran for window 0.
  `automatic-rename-format '#{b:pane_current_path}'` named the tab
  `work-api`, the leader's directory; `'#{pane_title}'` named it
  `agent-set-title`. A name is recomputed only after the pane draws
  (`names.c:54-93`, `PANE_CHANGED`), which an agent does all the time.
- **Moshi's probe**, `sh -lc 'command -v tmux; tmux list-sessions'`, with
  a scratch `$HOME` whose `.profile` put the unpacked tmux on `PATH` (a
  login shell here resets `PATH`), and sessions `work` (three windows) and
  `api`: `api: 1 windows (created ...)`,
  `work: 3 windows (created ...)`. `list-windows -a` shows what a phone does
  not get from it: `work:0: wormhole!*`, `work:1: RENAMED`, `work:2: sh!
  (2 panes)`.
- **An unreachable `TMUX`.** `TMUX=/nonexistent/tmux-1000/default,4242,0
  tmux split-window`, and `new-window`: exit 1, `error connecting to
  /nonexistent/tmux-1000/default (No such file or directory)`.
- **Notifications through tmux.** OSC 9, OSC 777 and a passthrough-wrapped
  OSC 9 from the current pane: none reached the client. With
  `allow-passthrough on`, the wrapped one arrived as `\e]9;MARK-PASS\a`.
- **Clipboard.** OSC 52 from a pane under `set-clipboard external`: not
  passed on, no buffer. Under `on`: passed to the client as
  `\e]52;c;<base64>\a`, and a buffer made. A pane's OSC 52 *query* under
  `external`: no answer. Under `on`, after `set-buffer
  copied-in-another-pane`: the reply `\e]52;;Y29waWVkLWluLWFub3RoZXItcGFuZQ==\a`,
  which decodes to `copied-in-another-pane`.
- **The terminal title** with `set-titles on`: the client got
  `\e]0;work:0:sh - "agent-title" 2!\a`.
- **Focus**, to a pane program that asked for reports (DEC 1004). With
  `focus-events off`: focus-in at attach; nothing when the client switched
  windows away and back; focus-out at detach; tmux never asked the client
  terminal for focus reports. Run again with the client left on the other
  window: focus-in at attach, then nothing at all, through the switch, a
  focus-out and focus-in typed into the client, and the detach. With `on`:
  focus-out and focus-in on each window switch as well, and tmux asked the
  client terminal (`\e[?1004h`, sent after its 5 s query timeout,
  `tty.c:90`, since this pseudo-terminal answers no queries).
- **Two clients.** Both on session `work`: the phone's `prefix n` moved both
  to window 1. The phone on a grouped session (`new-session -t work -s
  phone`): the desktop stayed on window 0, the phone moved to 1. A bell in
  window 1 then ran the hook twice, `session=work` and `session=phone`, both
  `window_id=@1`.
- **A pane's terminal name.** `tty` in window 1 printed `/dev/pts/1`, the
  `#{pane_tty}` tmux lists for `work:1`.

Not run: a real box (this box has no user namespaces); Claude Code or Codex
inside a tmux pane; Moshi itself (no emulator in this pass); a push through
Moshi's service; whether Moshi's terminal sends focus reports, or does
anything with a BEL; whether a Moshi card follows a `switch-client`.

---

## Inventory: every tmux touchpoint

"Goes" means it has no reason to exist once wormhole stops talking to tmux.
Line numbers are the working tree's.

### Code

| Where | What it does | Why (source) | If removed | Replaced by |
|---|---|---|---|---|
| `crates/wormhole/src/tmux.rs` (new) | Runs `tmux` (`ask`, `run`, `exec`), starts a session from a handoff (`start`, `hand_off`), renames one (`rename`), writes owner-only handoff files | ADR 0006, "a tmux session of its own" | Nothing runs tmux | nothing |
| `crates/wormhole-core/src/terminal.rs` (new) | `pane()` parses `TMUX` and `TMUX_PANE`; argv for `new-session`, `switch-client`, `attach-session`, `rename-session`, `has-session`, `display-message`, `set-option detach-on-destroy`; `session_name`; `inside_multiplexer`; `Handoff`; `adopted` with `PANE_VARS`; `;` escaping | ADR 0006; remote-control.md finding 11 (16 KB cap, `;` rule) | No pane record, no handoff | nothing |
| `crates/wormhole-core/src/config.rs` (new), `paths.rs:174` `config_file`, `main.rs:1202-1211` `host_config` | `~/.config/wormhole/config.toml`, its only key `terminal = "auto" \| "here" \| "tmux"` | ADR 0006: the default trades "close the terminal, stop the agent" for "every box reachable" | `here` is the only behavior, as at HEAD; an existing `config.toml` is no longer read | nothing |
| `main.rs:239-251`, `1160-1200` `host_box`, `1213` `on_a_terminal` | An agent start on a terminal outside any multiplexer goes to a new session on the default tmux server, this terminal attached; the exit status comes back through a status file; a detach prints `detached: ...` | ADR 0006; remote.md "On the host, once" | A box lives and dies with its terminal (HEAD) | starting boxes in a tmux window: the user's tmux |
| `main.rs:74`, `1220-1254` `hosted`, `paths.rs:221` `handoff_dir`, `run_session`'s `env` parameter (`1104-1141`) | `wormhole __hosted <file>`: reads and deletes the handoff, keeps the pane's own terminal variables, runs, leaves the status | ADR 0006, second paragraph | Nothing to run | nothing |
| `main.rs:1093-1100` `session`, `1144-1154` `host_session` | Inside tmux the panel hands every session to a new detached session and `switch-client`s there: the trigger | ADR 0006; tests `tmux.rs:270`, `:337` | The panel runs the box in its own pane, as at HEAD and inside herdr or zellij today | the user's tabs |
| `main.rs:377-389` | Records the pane when stdin is a terminal; renames the session after the box when `WORMHOLE_HOSTED` is set | ADR 0006: "so a phone's session list ... say[s] which box it is" | No pane, no rename | the user's session names; tab names from formats or titles (below) |
| `registry.rs:42-44`, `main.rs:448` | `Entry.tmux`: the pane in the box's registry entry | ADR 0006 | Gone; an entry written with `[tmux]` still parses, since `Entry` does not deny unknown fields (`registry.rs:17`) | nothing, or a terminal name (see "What is lost") |
| `main.rs:1082`, `1256-1269` `join` | Panel Enter on a running box tmux holds runs `attach` in place | ADR 0006 | HEAD's `session(["attach", id])` | the user's tmux navigation |
| `main.rs:1851-1858` | `attach` with no command `exec`s `tmux switch-client` or `attach-session` to the pane; refuses from another server | ADR 0006: "the agent already running, never a second one" | `attach` runs a command in the box, as at HEAD | a decision (see "What is lost") |
| `doctor/mod.rs:21, 34, 47`, `doctor/outcome.rs:121-133`, `probes.rs:35` | Probe 9, `tmux`: pass, or "not found; ... out of a phone's reach" | STATUS `doctor` row | 8 probes again | nothing |
| `help.rs:80`, `:316` | `attach`: "back to its agent: its tmux pane, if it has one"; the `config.toml` line | ADR 0006 | HEAD's "a second terminal into a running box" | nothing |
| `lib.rs:5`, `:21` | Exports `config`, `terminal` | | | |
| `manifest.rs:511-512`, `seed.rs:273-287` | Claude boxes get `preferredNotifChannel = "terminal_bell"` where unset | `manifest.md`: "the bell is what lets tmux on the host push a box" | **Keep.** Without it Claude sends nothing in a terminal it does not name, tmux included (finding 12) | the comment should say "any terminal or multiplexer", not tmux |
| `panel.rs:310` | `pause`: "any key to go back" instead of "for the panel" | `__hosted`'s pane has no panel to go back to | Either wording is right without hosting | nothing |
| `probes.rs:116-124` `which` | Now `pub(crate)` for `main.rs:244`, and checks the exec bit | added for the tmux probe | **Keep the exec-bit check**: it is right for the `fuse-overlayfs` probe too (`probes.rs:27`); `pub(crate)` goes | |

### Tests

| Where | What | If removed |
|---|---|---|
| `crates/wormhole/tests/tmux.rs` (new, 28 tests) | Every path above against a recording fake `tmux`, including `:270` and `:337` that lock the trigger in | Goes whole |
| `terminal.rs` (23 unit tests), `config.rs` (3), `registry.rs:99-112`, `doctor/outcome.rs:143-151`, `seed.rs:273-287` | Unit tests of the above | Go with their code; the seed test stays |
| `tests/common/mod.rs:331` `keep_boxes_here`, `tests/panel.rs:148`, `tests/product.rs:40` and their `XDG_CONFIG_HOME` | Keep test boxes off the developer's tmux, which the `auto` default would use | Go |
| `home.rs:1629`, `tests/kernel.rs:478, 534`, `lifecycle.rs:310`, `ps.rs:48`, `stop.rs:52` | `tmux: None` in `Entry` fixtures | Go |

### Docs

| Where | State | What it becomes |
|---|---|---|
| `docs/adr/0006-a-box-terminal-lives-in-tmux.md` (new) | The decision this reverses | Never committed: rewrite it, or mark it superseded as 0002 and 0003 are, and record the reverse decision |
| `docs/research/remote-control.md` (new) | Research record | Keep; its deep-link line needs finding 7's limits |
| `docs/src/guide/remote.md` (new) | Setup for phones built on hosting | Rewrite: the user's tmux, a box per window, the hook with session and window, the cautions of findings 14 and 19; keep "Never grant" and the moshi-hook and Tailscale SSH warnings |
| `docs/src/SUMMARY.md` | Links the guide | Keep |
| `docs/src/guide/access.md:109-114` | moshi-hook and Tailscale SSH as ways out of every box | Keep: security, not tmux |
| `docs/src/guide/boxes.md:322-337` | `attach` reaches a tmux pane | Rewrite with the `attach` decision |
| `docs/src/guide/manifest.md:164, 174-177` | The bell seed | Keep; "tmux on the host" becomes "the multiplexer or terminal" |
| `CHANGELOG.md`, "Every box from a phone" | Hosting | Rewrite |
| `CONCEPT.md:368` | "Detach and reattach shipped without it" | Rewrite: detach and reattach are the multiplexer's |
| `CONTEXT.md:30` | Session, with the tmux clause | HEAD's text |
| `STATUS.md` | `attach`, panel and `doctor` rows, N18, two known gaps | Rewrite those; keep the read-only root block |

### In the same files, not tmux

- The read-only root fix: `mount_plan.rs` (`MountOp::Mirror`, `lend`, five
  tests) and `boundary.rs` (`mirror`, `bind_into`); CHANGELOG "Fixed: a
  read-only root could not start a box"; STATUS "What running real boxes
  for remote control on 2026-10-16 found".
- `tar --no-same-owner`: `roles/alphaca/wormhole.toml:85` and the test
  `every_build_step_that_extracts_keeps_no_owner`, `tests/manifests.rs`.
- `key: None` in the `Entry` fixtures of `tests/kernel.rs:470, 526`.
- `help.rs`: one blank line dropped before "Full guide".
- `probes.rs`: the exec-bit check in `which`.

### Security rules, not tmux features

They hold with zero tmux code, because they are about what a box can reach
on the host, and the user's tmux, herdr and zellij are still there.

- A box's own `/tmp`, `/run` and `/dev/shm` (`mount_plan.rs:457-462`).
- Never grant `/tmp`, `$XDG_RUNTIME_DIR`, `~/.config/herdr` or a tmux
  socket directory (`remote.md:169-174`). Docs only today; no code refuses
  such a grant (finding 20).
- No moshi-hook, no Tailscale SSH on a host that runs boxes (`access.md:109-114`).
- `TMUX` and `TMUX_PANE` stay undeclared (finding 4, 15).
- New here: leave `allow-passthrough` off and `set-clipboard` at `external`
  where boxes run, or, on tmux 3.7 and later, set `get-clipboard off` before
  turning `set-clipboard on` (finding 19).

---

## The agent in a host tmux pane

What a box sees today: `TERM=tmux-256color`, `TERM_PROGRAM=tmux`, no
`TMUX`, no display. Claude Code 2.1.303 and Codex 0.163.0.

| Feature | Claude Code in a box | Codex in a box | Host `tmux.conf` | Note |
|---|---|---|---|---|
| A ring for a push | BEL, from wormhole's seed (finding 12) | BEL, only when it believes it is unfocused (finding 13) | defaults suffice for the hook; `focus-events on` for Codex | or `notification_condition = "always"` in a Codex box |
| Desktop notification (OSC 9, 777, 99) | none sent | none sent | none | tmux drops them anyway (finding 11) |
| Passthrough wrapping | never: needs `TMUX` (`@235522560`) | never: needs `TMUX` (`osc9.rs:50`) | `allow-passthrough` buys a box nothing | off is safer (finding 19) |
| Terminal title | OSC 0 (`@261201551`): U+2733 when idle, U+25D0 and U+25D1 alternating every 960 ms while busy and focused; static only under a multiplexer it detects by `TMUX` (`@261201678`), so it animates in a box | OSC 0 (`terminal_title.rs:108`) from `activity`, `thread-name`, `project-name` (`status_surfaces.rs:29-30`); the docs say `["spinner", "project"]` | `allow-set-title on` (default); `set-titles on` for Moshi's label | off: `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1` ([env-vars](https://code.claude.com/docs/en/env-vars)); `[tui] terminal_title = []` ([config sample](https://learn.chatgpt.com/docs/config-file/config-sample)) |
| Shift+Enter | extended keys asked for, since terminal `tmux` is on its list (`@243073500`) | modifyOtherKeys asked for only when `TMUX` or `TMUX_PANE` is set and tmux reports `csi-u` (`codex-rs/tui/src/tui/keyboard_modes.rs:261-280`), so never in a box | `extended-keys on`, `terminal-features 'xterm*:extkeys'` ([terminal-config](https://code.claude.com/docs/en/terminal-config#configure-tmux)) | Codex in a box: not run. Inference: only `extended-keys always` with `extended-keys-format csi-u` would send it Shift+Enter unasked, and tmux's CHANGES name no kitty keyboard protocol; its Ctrl+J is a newline anyway |
| Copy to the phone | bare OSC 52: no display tool, and `load-buffer` only under `TMUX` (`@235524802`) | bare OSC 52 after the native copy fails; `load-buffer` only under `TMUX` (`clipboard_copy.rs:192-208, 254-256`) | `set-clipboard on` | with finding 19's read risk |
| Image paste | `wl-paste` or `xclip` against a display (`@116400352`) | a display (arboard) | none | fails in a box under any terminal |
| Focus | asks for DEC 1004 (`@242149018`); focus stops the title animation (`@243109128`) and gates the recap, which waits until "the terminal is unfocused" ([interactive-mode](https://code.claude.com/docs/en/interactive-mode)) | asks for DEC 1004; gates notifications (finding 13) | `focus-events on` | finding 14 |
| Agent teams | in-process; split panes need `TMUX` | none | none | never let `TMUX` cross (finding 15) |
| Colours | no 256-colour clamp, which applies "when `$TMUX` is set" (env-vars, `CLAUDE_CODE_TMUX_TRUECOLOR`) | | `terminal-features` RGB for the client | |
| Status (OSC 7501) | asks the terminal for support first ([terminal-config](https://code.claude.com/docs/en/terminal-config#see-session-status-in-your-terminal)) | sent whenever stdout is a terminal (`codex-rs/tui/src/terminal_program_status.rs:201-215`) | none: tmux 3.5a drops it (`input.c:2444-2446`) | not a Moshi feature |

Inference on the seed: a box run directly in Ghostty, kitty or iTerm2 also
rings the bell instead of the desktop notification Claude's `auto` would
send. The bell is the one signal every terminal and multiplexer passes, and
a role or the box's own settings can still choose otherwise.

---

## Moshi against a tmux-free wormhole

| Moshi feature | What it uses | From wormhole | Works |
|---|---|---|---|
| Session picker | `sh -lc 'command -v tmux; tmux list-sessions'` | nothing | yes: the user's sessions; a box is a window in one |
| Tabs | tmux keys from its tmux panel (`c`, `n`, `p`, `0-9`), tmux's status line, touch as mouse | nothing | yes |
| Window tree (Sidebar), swipe, agent status, inbox, approvals, Chat View | moshi-hook | nothing | no, as today: moshi-hook stays off a wormhole host |
| Push | the webhook, from a host `alert-bell` hook | the agent must ring: the Claude seed | yes; Codex needs finding 14 |
| Deep link | session name, a window 0 to 9 | nothing | session and window; pane only through moshi-hook |
| Card label | the outer terminal title, from `set-titles` | nothing | yes, with `set-titles on` |
| Clipboard | OSC 52 through `set-clipboard on` | nothing | yes, with finding 19 |
| Image paste | SCP into the host's `~/.moshi/uploads/`, then the path typed ([image-paste](https://getmoshi.app/docs/image-paste)) | the path would have to exist in the box | no: the box's `$HOME` is its own kept home, so the path names a file the box lacks (Inference); true under any design |

Inference: a Moshi link resumes a card by its session (finding 7). With
wormhole's current `switch-client`, the client can leave the session its
card was opened on, and whether the card follows is undocumented. In the
tmux-free design the client stays in its session and only windows change,
so a link by session keeps matching.

---

## Which tab holds which box

Ranked from most transparent to least.

| Way | Who sets it | Shows on a phone | Verdict |
|---|---|---|---|
| tmux defaults alone | nobody | every box tab is `wormhole` in the status line (finding 16) | names nothing |
| The user names the window | the user, once: `tmux new-window -n api`, or `prefix ,` on an open tab; a given name turns `automatic-rename` off for that window (tmux(1), `automatic-rename`) | the status line | Moshi's own advice (finding 10); nothing in wormhole or the agent changes it, since `allow-rename` is off |
| The agent's own title | the agent: Claude's session title, or the name from `--name` or `/rename` ([settings-reference](https://code.claude.com/docs/en/settings-reference#terminaltitlefromrename)); Codex's project name and thread | the card label with `set-titles on` (current window only); the status line with `#T` in a format | names the work, not the box; no wormhole code. Inference: wormhole starting Claude with `--name <alias>` would put the box's alias in the title through the agent, no tmux call; Claude only, and every session of that box gets the same name |
| The user's tmux formats | the user's `tmux.conf`: `automatic-rename-format` with `#{b:pane_current_path}` or `#{pane_title}` | the status line | names the project directory or the work; for a box started from the panel the directory is the panel's (Inference) |
| One title from wormhole at box start | wormhole writes OSC 2 once: a terminal protocol, not a tmux call | until the agent's first title, which overwrites it | holds only if wormhole also turns the agents' titles off, losing Codex's "Action Required" title (Inference); a poor trade |
| Sessions or windows named by wormhole | `rename-session`, as today | the picker | excluded by the question |

---

## What is lost, and whose job it is

| Loss | Real loss or the multiplexer's job | What replaces it |
|---|---|---|
| A box started outside any multiplexer dies with its terminal and no phone sees it (ADR 0006, first paragraph) | the multiplexer's job: persistence is what tmux, herdr and zellij are for | start boxes in a tmux window; one line in the guide. Inference: the doctor line and the "no tmux" warning go too, so nothing reminds a user who forgets |
| `attach` with no command cannot reach the running agent | real for `attach`; reaching the pane is the multiplexer's job | a decision, below |
| Panel Enter on a running box cannot jump to it | the multiplexer's job | the same decision as `attach` |
| No session named after a box, for the picker and the push link | not a loss: Moshi picks sessions and the box is a window in one; the link carries `session` and `window` (finding 7, 8) | the hook's `#{session_name}` and `#{window_index}` |
| Exit status handed back, "detached" message | exists only because hosting moved the box away; at HEAD the start is the box and returns its status | nothing needed |
| The panel never waits on a box | real: in the tab the box runs in, the panel waits until it ends | another tab for the next box, which is Moshi's own "one agent per window" |

**The `attach` decision.** At HEAD, `attach` with no command runs a second
agent session in the box (`manifest.rs:884-889`, `:560`), which is not the
running agent (finding 17). Options, by correctness:

- **Keep it, and say so.** The help line returns to "a second terminal into
  a running box", and the docs say it starts a new agent session. Honest;
  two agents then share one home and one workspace.
- **A shell by default.** "A second terminal into the box" with no surprise;
  `-- claude` for a second agent. The running agent stays one tab away.
- **Refuse while the agent runs, and say where it runs.** Needs a locator.
  Inference: the box's terminal name (`ttyname` of stdin on the host side,
  `/dev/pts/7`) is one every multiplexer shares and needs no tmux call;
  tmux lists it as `#{pane_tty}` (run above), so `wormhole ps` could print
  it and the user's own binding finds the pane. Recording it is a new field,
  not a tmux interaction.

Inference: the reason HEAD gave for the agent default, "attaching means
getting back to the agent" (STATUS), is false without a pane. A shell
default, or a refusal that names the terminal, keeps `attach` from starting
an agent nobody asked for. Which one is the owner's call.

---

## Verdict

| Question | Answer |
|---|---|
| A wormhole with no tmux code? | Yes. Take out the layer in the inventory; HEAD's terminal handling is the tmux-free one |
| The agent smooth in tmux? | Yes, with host `tmux.conf` lines, not wormhole code: `extended-keys`, `focus-events`, `set-titles`; Claude needs wormhole's bell seed |
| Moshi picker | Yes: lists the user's sessions |
| Moshi tabs | Yes: tmux windows by tmux's keys and status line |
| Pushes | Yes: host `alert-bell` hook, session and window |
| Deep links | Session, and window 0 to 9; never pane without moshi-hook |

What must stay true in wormhole:

- The box keeps the caller's PTY: no new session, no process group of its
  own, no PTY in between, so BEL, titles, focus and keys reach tmux as the
  agent wrote them (finding 3).
- `TERM` crosses with its compiled description carried into the box home,
  `tmux-256color` included (finding 4); `TERM_PROGRAM` crosses.
- `TMUX` and `TMUX_PANE` never cross (finding 15).
- Claude boxes get `preferredNotifChannel = "terminal_bell"` where unset
  (finding 12).
- The box's own `/tmp`, `/run`, `/dev/shm`; never granting a socket
  directory; moshi-hook and Tailscale SSH off (finding 20).

Weak spots of the tmux-free design, plainly:

- A box started in a plain terminal is mortal and invisible again, and
  nothing tells the user.
- `attach` and panel Enter cannot reach a running agent; whatever they do
  instead must be chosen and documented.
- With tmux's defaults every box tab reads `wormhole`; telling boxes apart
  needs the user's `tmux.conf` or the agents' titles.
- A push link reaches window 0 to 9 at best, and the hook names the
  window's active pane, not the pane that rang.
- Codex boxes under the default `notification_condition` ring only after a
  focus-out. With tmux's default `focus-events off`, a tab you leave never
  gets one; with it on, the tab you are looking at gets one only if the
  phone's terminal reports losing focus, which was not run.
- Copy to the phone needs `set-clipboard on`, which lets every pane, boxes
  included, read the newest paste buffer unless `get-clipboard off` (3.7
  and later) is set too; on 3.5a and 3.6 there is no way to keep one
  without the other.
- A desktop and a phone on one session move each other's tabs; the answer
  is a grouped session per client, which the user must set up, and which
  doubles the hook's runs per bell.
- Moshi's image paste cannot reach a box, whatever wormhole does with tmux.

Not adopted: the owner chose tmux as installed, with nothing to set (ADR
0006), and Codex boxes are told to ring always instead of
`focus-events on`. Kept as the record of what each line would buy. A host
`tmux.conf` that matches this, for the guide (the user's, never written by
wormhole):

```tmux
set -g base-index 1                          # tabs 1 to 9 for Moshi's number row and links
set -s focus-events on                       # Codex boxes ring when you look away
set -s extended-keys on                      # Shift+Enter
set -as terminal-features 'xterm*:extkeys'
set -g set-titles on                         # Moshi labels a card with this
set -g set-titles-string '#S:#I #T'
set -g automatic-rename-format '#{?#{==:#{pane_current_command},wormhole},#{b:pane_current_path},#{pane_current_command}}'
set-hook -g alert-bell 'run-shell -b "~/.local/bin/moshi-push #{q:session_name} #{window_index} #{window_id}"'
# Left at their defaults where boxes run: allow-passthrough off, allow-rename off,
# set-clipboard external. See findings 19 and 20 before changing them.
```

## Sources

- wormhole: this repository, working tree on 1e11443; `git diff`,
  `git show HEAD:crates/wormhole/src/main.rs`.
- tmux: <https://github.com/tmux/tmux> tag `3.5a` (`alerts.c`, `cmd-find.c`,
  `environ.c`, `input.c`, `names.c`, `notify.c`, `options-table.c`,
  `osdep-linux.c`, `server-client.c`, `session.c`, `spawn.c`, `tty.c`,
  `tty-keys.c`, `window.c`), tags `3.6` to `3.8` (`options-table.c`, `tmux.1`),
  master (`CHANGES`, `input.c`, `alerts.c`, `cmd-find.c`, `events.c`); tmux(1)
  3.5a from `tmux-doc-3.5a-r1`: OPTIONS, HOOKS, FORMATS, NAMES AND TITLES,
  `new-session`, `run-shell`.
- Linux: <https://github.com/torvalds/linux> `drivers/tty/tty_jobctrl.c`.
- Moshi: <https://getmoshi.app/docs/debug-multiplexer-chooser>,
  `/docs/notifications`, `/docs/jump-to`, `/docs/hooks`, `/docs/tmux`,
  `/docs/moshi-cli`, `/docs/clipboard`, `/docs/image-paste`,
  `/guides/tmux-or-herdr`, `/guides/herdr`,
  `/articles/moshi-keyboard-guide`;
  <https://itunes.apple.com/lookup?id=6757859949>, the App Store version
  history, <https://play.google.com/store/apps/details?id=app.getmoshi.android>;
  <https://github.com/rjyo/moshi-skill> `skills/moshi-best-practices/SKILL.md`.
- Claude Code: the 2.1.303 binary; <https://code.claude.com/docs/en/terminal-config>,
  `/settings-reference`, `/env-vars`, `/sessions`, `/agent-teams`, `/hooks`,
  `/cli-reference`; <https://github.com/anthropics/claude-code> `CHANGELOG.md`
  (2.1.78, 2.1.236, 2.1.259).
- Codex: <https://github.com/openai/codex> tag `rust-v0.163.0`
  (`codex-rs/terminal-detection/src/lib.rs`,
  `codex-rs/tui/src/notifications/mod.rs`, `notifications/osc9.rs`,
  `codex-rs/tui/src/tui.rs`, `codex-rs/tui/src/terminal_title.rs`,
  `codex-rs/tui/src/chatwidget/status_surfaces.rs`,
  `codex-rs/config/src/types.rs`);
  <https://learn.chatgpt.com/docs/config-file/config-sample>.
