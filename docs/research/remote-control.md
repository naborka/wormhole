# Every box from a phone: herdr, Moshi, tmux

Primary-source research into driving every agent in every wormhole box from
a phone running Moshi, and into what such a setup must never include.
Checked 2026-10-16 against herdr 0.9.5 (docs and source, `herdrdev/herdr`),
the Moshi docs at `getmoshi.app/docs`, the Moshi 4.0.2 Android app (run, see
"Run, not read"), the moshi-hook 0.4.30 Linux binary (strings read, its web
client run in a VM), tmux source (`tmux/tmux` master), the Tailscale
knowledge base and the Claude Code docs. "Inference" marks reasoning of my
own rather than a quote.

The question: a phone opens Moshi, connects to the wormhole host, and should
see every running box, open the one it wants on the live agent (the same
process, not a second one), answer it, leave it running, and hear about the
next box that needs it.

---

## What the evidence says

1. **herdr is a terminal multiplexer with agent state.** A background server
   owns real terminals; clients attach and detach. It marks every pane
   holding a known agent `working`, `blocked`, `done` or `idle`, read from
   the bottom of the screen against per-agent detection manifests. Its own
   phone guidance is plain SSH plus `herdr`, and it names Moshi.
2. **herdr finds an agent through host `/proc`, by process group.** It reads
   the pane's foreground process group and walks every member, reading
   `comm` and `cmdline` (`foreground_job`, `src/platform/linux.rs`, capped at
   512 processes). A box's agent is a host process in the same process group
   as the `wormhole box` that launched it: nothing in the launch chain calls
   `setsid` or `setpgid` (`boundary.rs`), and a PID namespace does not change
   what host `/proc` shows. Tested with the herdr 0.9.5 binary: a pane whose
   foreground job was `wormhole`, its child `claude` and that one's child,
   all one process group, was listed as agent `claude`, state `working`.
   Then with real boxes, on a kernel that allows user namespaces (Alpine
   6.12 under qemu): a box started in a herdr pane, its agent PID 1 of its
   own PID namespace, was listed by herdr as agent `claude`, state
   `working`.
3. **herdr restores an agent only from a session id its hooks reported.**
   After a server restart it relaunches `claude --resume <id>` and the like
   from ids its integrations wrote. A box cannot reach herdr's socket, so no
   boxed agent ever reports one, and a restart cannot relaunch a boxed agent
   on the host, outside its box.
4. **Moshi's session picker is one SSH command, no daemon.** At connect it
   runs `sh -lc 'command -v tmux; tmux list-sessions'`, and the same probe
   for `herdr session list --json` and `zellij`. Every session on the host's
   default tmux server is therefore listed and attachable. Moshi's Free vs
   Pro page puts the multiplexer pickers under Pro; the free 4.0.2 app
   counts picker uses down from ten ("10 of 10 free uses left").
5. **Everything agent-aware in Moshi goes through moshi-hook.** Inbox,
   approvals, Chat View, the Jump To tree, swipe detection and the diff
   viewer need the moshi-hook daemon: hooks written into the host agents'
   config files, a unix socket they post to, and a gateway on
   `127.0.0.1:24543` that the app reaches through an SSH port forward.
6. **That gateway trusts whoever can reach it.** Moshi's debugging page
   drives it with bare `curl` and `wscat`. The binary says it itself: "web:
   listening on a non-loopback address; anyone who can reach it controls
   this Moshi daemon". Its handlers include `handleKeys`, `handlePrompt`,
   `handlePaste`, `handleFilesRead`, `handleFilesUpload`, `handleFilesDelete`,
   `handleWorkspacesLaunch`, `handleIntegrationsInstall` and `handleUpdate`.
   moshi-hook's own API reference (`docs/api.md` in its release archive)
   says it plainly: "Gateway HTTP is loopback-only and does not require
   `Authorization`." A box shares the host's network namespace, so every box
   reaches `127.0.0.1:24543`. **On a wormhole host, moshi-hook is a door out
   of every box.**
7. **Tailscale SSH trusts every process on the source node.** It
   authenticates by the source device's tailnet identity, and "any OS user on
   the client machine can connect to SSH servers over Tailscale" (KB 1193).
   A box is on the host's network and so carries the host's identity. With
   Tailscale SSH on any device the policy lets this node reach, this host
   included, a box opens an SSH session there with no key. Moshi's own setup
   guide suggests `tailscale up --ssh`. On a wormhole host, use OpenSSH with
   keys over the tailnet instead; mosh needs OpenSSH anyway.
8. **A push needs no daemon.** `POST https://api.getmoshi.app/api/webhook`
   with the device's token sends one, and `data: {"type": "url", "url":
   "moshi://tmux?session=<name>"}` makes tapping it reopen that session's
   card. The token can only send notifications.
9. **Claude Code rings the bell when it needs you.** With
   `preferredNotifChannel` set to `"terminal_bell"` it rings when it finishes
   or waits on a prompt and "you appear to be away". The bell is a byte on
   the pane. tmux, on the host, sees it and can run a host command through
   its `alert-bell` hook. A push can be sent from the host with the token
   kept there and nothing added to any box.
10. **The multiplexers' control sockets are out of a box's reach.** tmux
    listens under `/tmp/tmux-<uid>` (or `TMUX_TMPDIR`); herdr uses a
    filesystem local socket and opens no TCP port (tokio is built without
    `net`; `interprocess` local sockets); zellij keeps its sockets in the
    runtime directory. A box has its own `/tmp`, `/run` and `$HOME`, so it
    sees none of them unless a grant mounts one. What a box does reach is
    loopback TCP and abstract unix sockets.
11. **tmux arguments are a channel to keep empty.** One client command is
    capped at 16 KB (`MAX_IMSGSIZE`, "command too long", `client.c`). Any
    argument ending in an unescaped `;` ends the command and starts another
    (`cmd-parse.y`). `-e NAME=VALUE` overrides the `TERM` tmux sets for a pane
    (`environ.c`, `spawn.c`). An environment, a path or a typed value must
    therefore never travel as tmux arguments.
12. **A pane id reaches its pane from anywhere.** A bare `%7` target is
    parsed as a pane whatever the command expects (`cmd-find.c`), and
    `attach-session` and `switch-client` both make that pane's window and
    pane current (`cmd-attach-session.c`, `cmd-switch-client.c`).

---

## Run, not read

On a kernel that allows user namespaces (Alpine 6.12 under qemu, no KVM),
with real boxes whose agent is PID 1 of its own PID namespace:

- A box started from a plain terminal ran in a tmux session of its own with
  no setting; text typed from outside reached the agent; `wormhole attach`
  reached the same agent; its bell became a push message with a deep link.
- Moshi's picker probe listed it, locally and over SSH; a client attached
  over SSH saw the agent answer.
- herdr read a box in its pane as `claude`, `working`; zellij ran one in its
  pane; Moshi's probes listed both sessions.
- Moshi's own client, `moshi` from moshi-hook 0.4.30, listed the tmux session
  with agent `claude`, and its `POST /v1/prompt` reached the boxed agents in
  tmux and in herdr: the path its phone app's chat view uses.
- From inside a box, `wget -qO- http://127.0.0.1:24543/v1/muxes` returned the
  daemon's data. Finding 6, shown rather than read.

Then the Moshi app itself: 4.0.2 for Android, the Google Play build (every
APK matched the SHA-256 Play published for it), on an emulated phone,
Android 13 (BlissOS 16 under qemu, no KVM, Moshi's ARM64 code through
Google's `libndk_translation`). The phone reached nothing but that host's
sshd, over plain SSH. Four boxes ran: two started from a plain terminal,
one in a herdr pane, one in a zellij pane.

- The picker listed the plain-terminal boxes as tmux sessions `api` and
  `docs`, the herdr workspace `boxes` and the zellij session `boxes`.
- Each one opened on its boxed agent, PID 1 of its own namespace; a line
  typed on the phone came back from all four as `got: …`. tmux listed
  Moshi's clients as `xterm-256color`; herdr showed its box as `1 working`.
- Skip, then `wormhole attach api` in Moshi's plain shell, reached the same
  agent: the way in that needs no picker.
- `moshi://tmux?session=api`, the link the push recipe sends, reopened that
  session's card.
- No push went through Moshi's service. Its API token shows under Settings,
  Notifications, only once the phone has registered for push, and this
  phone had no route out. The Hooks page showed a `moshi-hook pair --token`
  command even so; whether that token is the push token, neither the app
  nor the docs say.

Only an emulator needs two things: Moshi's terminal (Ghostty) wants OpenGL
ES 3.1, which BlissOS provides with `ANGLE=1`, and React Native's loader
looks for native code under the first ABI only, so Moshi's own ARM64
libraries went where Android extracts native libraries.

## herdr

The README's own line: "the runtime your coding agents live on". One Rust
binary, Apache-2.0, 0.9.5 with protocol 22 and an endpoint generation of 1,
pre-1.0 and moving fast (`distribution/latest.json`).

- **Shape.** Session (one server) holds workspaces, which hold tabs, which
  hold panes. A pane is a real terminal. `ctrl+b q` detaches, `herdr`
  reattaches. Named sessions are separate servers.
- **Agent state.** Bundled manifests for Claude Code, Codex, Grok and about
  twenty more; remote manifest updates are fetched from herdr.dev and applied
  live. `blocked` only when the screen matches a known prompt; otherwise
  `idle`. An agent may also report its own state through the CLI or socket.
- **Automation.** A socket API and a CLI (`herdr pane read`, `send-text`,
  `run`, `agent wait`, `events.subscribe`) let scripts and agents drive
  panes and wait on each other.
- **Remote.** SSH and run `herdr` there; `herdr --remote <host>` draws the
  UI locally over SSH; saved machines put several hosts in one window.
- **Phone.** "Herdr works on your phone without a mobile app or web
  dashboard": SSH in and run `herdr`, the TUI adapts to narrow screens.
- **Sandboxes.** A wrapper that hides the agent can set `HERDR_AGENT=<agent>`
  on its own command to name the manifest to use (`fence`, `nono` are the
  documented examples). A box does not hide its agent (finding 2), so it
  should not need the hint.

## Moshi

A closed-source iOS and Android terminal (Pro subscription for most of the
multiplexer features) built around coding agents.

- **Transports.** SSH, mosh, Eternal Terminal. mosh keeps the session
  through network changes and sleep; Moshi pairs it with a multiplexer for
  scrollback and persistence.
- **Multiplexers.** tmux and herdr in the top tier, zellij below it. tmux is
  the most tested path and the only one with the `moshi DIR` launcher; herdr
  adds agent status to the Jump To rows. Both tiers' extras beyond the picker
  run through moshi-hook (finding 5).
- **moshi-hook.** A Go daemon. `moshi-hook install` writes hook entries into
  `~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.grok/hooks/` and the
  others; agents post events to its unix socket; it keeps a WebSocket to
  Moshi's service for pushes and approvals, and serves the gateway (finding
  6). Its hooks would sit in the host's agent config, not a box's, so they
  would never fire for a boxed agent anyway.
- **Deep links.** `moshi://tmux?session=<name>&window=<n>&pane=<id>` and
  `moshi://herdr?session=<s>&workspace=<id>` resume an active card; they never
  open a new connection. `window` is one digit, 0 to 9, and a pane is
  focused only through moshi-hook (corrected 2026-10-19, see
  [tmux-free.md](tmux-free.md), finding 7).

## Other tools

- **tmux.** The base everything else assumes. Session names are what the
  picker lists and what a deep link names.
- **zellij.** Listed by the picker, approvals through moshi-hook, no deep
  links. Its web client (`zellij web`) listens on loopback TCP behind a login
  token; a box can reach the port.
- **dtach, abduco.** Hold a terminal for reattaching, list nothing, and are
  invisible to the picker.
- **Claude Code Remote Control.** Continues a running Claude Code session
  from the Claude app. One product only, and it relays through Anthropic's
  service; it says nothing about codex or grok boxes.

---

## A wormhole host, component by component

| Component | Reachable from a box | Verdict |
|---|---|---|
| tmux server | no: socket in the host's `/tmp` | use |
| herdr server | no: filesystem socket, no TCP | use |
| mosh-server | its UDP port, but every packet is sealed with a per-session key the box never sees | use |
| OpenSSH with keys only | its port, with nothing to log in with | use; `PasswordAuthentication no` |
| Moshi webhook token | only if put in the box | keep it host-side |
| moshi-hook gateway `127.0.0.1:24543` | **yes**, and it controls the daemon | **do not install** |
| Tailscale SSH | **yes**, with the host's tailnet identity | **do not enable** where boxes run, or on devices this node may reach |
| zellij web | yes, behind a login token | leave off |
| grants of `/tmp`, `$XDG_RUNTIME_DIR`, `~/.config/herdr` | they mount the sockets above | never grant |

---

## What follows for wormhole

What stops a phone today is one structural fact: **a box borrows the
terminal it was started in.** It dies with that terminal, nothing can list
it, and `wormhole attach` cannot reach it, so attach starts a second agent on
the same home, which Claude Code documents as interleaving one transcript.
The panel cannot return while a box runs for the same reason. CONCEPT §9
planned a PTY supervisor with a `vt100` screen model to fix all of it; tmux
is that supervisor, with scrollback, size policy and every phone client
already speaking it.

- **herdr:** nothing to build. Start boxes in herdr panes; herdr reads each
  agent's state from the host side, and Moshi lists the herdr session.
- **tmux:** four small pieces, recorded in the first version of ADR 0006
  (built, never released, and replaced on 2026-10-19 by
  [a box keeps its terminal](../adr/0006-a-box-keeps-its-terminal.md), after
  [tmux-free.md](tmux-free.md)):
  1. a box started inside tmux records its pane (`TMUX`, `TMUX_PANE`) in its
     registry entry;
  2. `wormhole attach <box>` with no command makes that pane current: the
     same agent, not a second one;
  3. the panel, run inside tmux, starts every session in a tmux session of
     its own, so it never blocks and every box is its own row in the
     picker;
  4. a box the panel started names that session after itself, so the
     picker and a deep link say `api`, not `3`;
  5. every agent started from a plain terminal gets the same, whenever the
     host has tmux: a tmux session of its own, this terminal attached, the
     box's exit status handed back. `terminal = "here"` in the host's
     `config.toml` keeps boxes in their terminals.
- **Pushes:** host-side. The agent rings the bell, a tmux `alert-bell` hook
  posts to Moshi's webhook with a deep link to the session. The token stays
  in a host file.

Considered and rejected:

- **wormhole's own PTY supervisor** (CONCEPT §9). A second tmux, invisible
  to every phone client and to Moshi's picker.
- **Hosting only when the config asks.** Leaves every box out of reach
  until someone finds the setting. The default hosts whenever tmux is
  there and says so when it is not; `here` opts out. Never inside herdr (it
  cannot see an agent behind a nested tmux) and never for `box -- cmd`,
  whose output would vanish.
- **Passing the panel's environment as `tmux -e`.** The 16 KB cap and the
  `;` rule (finding 11) make it fail on large environments and misparse
  values. A private handoff file carries the environment, the directory and
  the command; tmux carries only that file's path.
- **Hooks in the box posting to Moshi.** Works, but puts the token where a
  hostile agent reads it and can flood the phone with it.
- **moshi-hook, Tailscale SSH.** Findings 6 and 7. What would make
  moshi-hook safe is not in it but in the box: a network namespace whose
  route out does not reach the host's loopback (pasta, say), so every
  loopback service, moshi-hook's gateway included, stops being a door out
  of every box. That reverses the reach ADR 0002's retirement settled on,
  and is a decision of its own.

## Sources

- herdr: <https://github.com/herdrdev/herdr>, docs under
  `docs/versions/0.9.5/website/src/content/docs/` (`concepts`, `agents`,
  `how-to-work`, `persistence-remote`, `session-state`, `cli-reference`,
  `socket-api`), source `src/platform/linux.rs`, `src/ipc.rs`, `Cargo.toml`.
- Moshi: <https://getmoshi.app/docs/multiplexer>, `/docs/tmux`,
  `/docs/herdr`, `/docs/hooks`, `/docs/install-moshi-hook`,
  `/docs/debug-gateway`, `/docs/debug-multiplexer-chooser`,
  `/docs/notifications`, `/docs/chat-view`, `/docs/subscription`,
  `/docs/first-session`,
  <https://getmoshi.app/articles/best-setup-coding-agent-phone>; the app,
  `app.getmoshi.android` 4.0.2 (version code 280) from Google Play.
- moshi-hook: `https://cdn.getmoshi.app/hook/v0.4.30/moshi-hook_Linux_x86_64.tar.gz`,
  read with `strings`.
- tmux: <https://github.com/tmux/tmux> `client.c`, `environ.c`, `spawn.c`,
  `cmd-parse.y`, `cmd-find.c`, `cmd-attach-session.c`, `cmd-switch-client.c`.
- Tailscale SSH: <https://tailscale.com/kb/1193/tailscale-ssh>.
- Claude Code: <https://code.claude.com/docs/en/terminal-config>.
