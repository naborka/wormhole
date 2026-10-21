# From your phone

A box runs in the terminal you start it in. Start it in a tmux window and a
phone running Moshi, or any SSH app, lists it, opens it on the live agent
and leaves it running. wormhole runs no server for it and asks tmux for
nothing: tmux and Moshi work as installed, with nothing to set.

```
phone --(mosh over Tailscale)--> host: tmux session > window > wormhole box > agent
```

## On the host, once

```sh
sudo pacman -S tmux mosh openssh      # or apt, dnf, apk
```

OpenSSH with keys only (`PasswordAuthentication no`), reached over
Tailscale or WireGuard; mosh starts `mosh-server` through it. That is what
any phone app needs to reach a host, box or no box.

## On the phone

1. Install Tailscale and Moshi, and join the tailnet.
2. Add the host: its tailnet address, your user, a key Moshi made. Use mosh.
   Easy Pair (the QR code) only installs that key; it is not moshi-hook.
3. Connect. The session picker lists every tmux session; tap one. The
   picker is Moshi Pro after ten free uses; without it, Skip to the shell
   and run `tmux new -A -s main`.

## Every day

- **A box per tab.** Open a tab (`c` in Moshi's tmux panel, `prefix c` on a
  desktop) and start a box in it: `wormhole box`, or `wormhole` for the
  panel and Enter on a box. The panel runs the box right there and comes
  back when it ends.
- **Find it again.** Switch tabs with Moshi's number row, or `prefix` and
  the number. Every box tab is named `wormhole` until you name it:
  `prefix ,`.
- **Leave it running.** Detach (`prefix d`), close the app or lose the
  network: every box keeps running in its tab. Stop one with
  `wormhole stop` or `d` in the panel.
- **A shell beside the agent.** `wormhole attach api` opens one in the box,
  from any terminal; `wormhole attach api -- <cmd>` runs a command there.
  The agent stays in its tab.

A box started outside any multiplexer lives and dies with its terminal, as
any program does, and no phone can reach it.

## When a box needs you

Claude Code and Codex boxes ring the terminal bell when they need you:
wormhole sets both to, inside the box, unless the box chose otherwise.
tmux marks that tab `!` in its status line and passes the bell to every
terminal attached. A push to a locked phone needs your Moshi token on the
host and something there to send it; wormhole sends none.

## Keep tmux's defaults where boxes run

They are the safe ones:

- `set-clipboard` stays `external`. Moshi's clipboard page asks for `on`,
  which lets any pane, every box included, read what you last copied in
  tmux. tmux 3.7 and later can refuse that with `get-clipboard off`.
- `allow-passthrough` stays `off`. No box needs it, and with it on a box
  can write anything to your phone's terminal.
- `allow-rename` stays `off`, so a box cannot rename your tabs.

## Never

- **moshi-hook.** Its gateway listens on `127.0.0.1:24543` and obeys
  whoever reaches it: it types into panes, reads and writes files and
  launches workspaces. Every box is on the host's network and reaches it.
  Moshi's terminal, session picker and tabs need no moshi-hook; its
  "Hooks: needs attention" is how it should stay.
- **Tailscale SSH.** `tailscale up --ssh` trusts every process on a tailnet
  device, and a box is a process on this one, on its network. With it on
  here, or on any device this one may reach, a box can open an SSH session
  there without a key.
- **Granting `/tmp`, `$XDG_RUNTIME_DIR`, `~/.config/herdr` or a tmux socket
  directory.** Each would hand a box the multiplexer's control socket, and
  with it every pane on the host.

## herdr and zellij

The same holds: a box in a herdr or zellij pane is that pane's program, and
Moshi lists their sessions too. herdr reads each agent from the host,
through the pane's process group, and says whether it is working, blocked
or done. Do not run tmux inside a herdr pane: herdr cannot see an agent
behind it.

Why each of these, with sources: `docs/research/tmux-free.md`,
`docs/research/remote-control.md` and ADR 0006.
