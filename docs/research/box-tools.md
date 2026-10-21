# Tools for the agent box: what helps an agent, measured in alphaca

A review of which command-line tools belong in a role's image when the user
of the box is an AI agent, not a person. Every number here was measured
inside a running `alphaca` box (Alpine 3.22.2, rustc 1.97.1, Claude Code
2.1.286, rtk 0.50.0) against this repository: 100 tracked files, 20k lines
of Rust, a 2.1 GB `target/`. Published benchmark figures were not used.

The brief this answers rated tools for a person at a terminal. Those
ratings do not carry over: an agent never sees colour, never scrolls a
pager, never presses a key in a TUI, and pays for every byte a command
prints. What it needs is commands that do not break, finish in bounded
time, and print little.

## 0. The finding in one paragraph

The biggest problem in the box today is not a missing modern tool, it is
the old ones: `grep`, `find`, `tree`, `diff` and `sed` are BusyBox applets,
and BusyBox refuses the GNU flags every agent and every tool wrapper
assumes. In the stock image a plain `grep -rn pattern .` from Claude is
rewritten by the rtk hook into `rtk grep`, rtk passes `--null`, BusyBox
grep rejects it, and the command fails every time (D1). Run directly,
BusyBox grep took 267 seconds over this tree where GNU grep took 9 and
ripgrep 0.4 (D2). `rtk tree` prints garbage because `/usr/bin/tree` is a
BusyBox applet that ignores its options (D3). Installing the six GNU
packages costs 2.6 MB and removes the whole class. After that, three
additions earn their place by measurement: **mold** halves every
incremental relink (2.3 s to 1.0 s), **cargo-nextest** runs the suite 25%
faster and, with two environment variables, reports 595 tests in 510 bytes,
and **ast-grep** answers structural questions exactly where ripgrep returns
lines to read (0 matches versus 25 for "unwrap outside tests"). The rest of
the brief's list is either already handled by rtk, or is for people.

## 1. Where the agent's commands actually go

Three layers sit between what Claude writes in a Bash call and what runs,
and the interaction between them is where the defects live.

1. **Claude Code's shell snapshot** (`~/.claude/shell-snapshots/*.sh`)
   defines functions `grep`, `find` and, when none is on `PATH`, `rg`.
   They exec the `claude` binary itself under the names `ugrep` 7.8.4,
   `bfs` 4.1.1 and ripgrep 14.1.1. So in a Claude box, `grep -rn` is
   already a fast GNU-compatible grep and `find` is already bfs, with no
   package installed.
2. **The rtk PreToolUse hook** (`~/.claude/settings.json`) rewrites the
   command before the shell sees it: `grep …` becomes `rtk grep …`,
   `find …` becomes `rtk find …`, `cat f` becomes `rtk read f`, `tree`,
   `ls`, `du`, `wc`, `diff`, `git`, `cargo`, `curl` likewise. `fd`, `eza`,
   `sed` and `cargo nextest` pass through unchanged.
3. **rtk execs the real binary from `PATH`.** It is a separate process,
   so the shell functions from layer 1 never apply to it. Whatever `grep`
   is in the image is what runs, and in `alphaca` that is BusyBox.

The consequence: layer 1 is bypassed by layer 2 for exactly the commands
layer 1 was meant to fix. codex and grok have no layer 1 at all. Either
way the image's own `grep` and `find` must be GNU, or the agent's commands
break.

## 2. Defects in the box as shipped

### D1 — every plain `grep` from Claude fails

Simulated by putting BusyBox applets first on `PATH`, which is the stock
image's state (`roles/alphaca/wormhole.toml` installs no `grep` package,
and nothing in its list depends on one):

```
$ rtk grep -rn "fn main" crates
grep: unrecognized option: null
BusyBox v1.37.0 (2025-08-05 16:40:33 UTC) multi-call binary.
```

rtk adds `--null` to the grep it execs. BusyBox grep has no `--null`, no
`--exclude-dir`, no `--include`. The agent sees a usage message, retries
with another spelling, and eventually reaches for `rg`. With GNU grep on
`PATH` the same command returns the match.

### D2 — BusyBox grep over the tree takes four and a half minutes

Single runs, whole workspace including the 2.1 GB `target/`, page cache warm:

| Command | Time |
|---|---|
| `busybox grep -rn "fn main" .` | 267.33 s |
| `/bin/grep -rn "fn main" .` (GNU) | 9.08 s |
| `/bin/grep -rnI "fn main" .` (GNU, skip binaries) | 0.11 s |
| `rg -n -uuu "fn main" .` (no ignore rules, binaries included) | 0.39 s |
| `rg -n "fn main"` | 0.02 s |

An agent's Bash call times out at 120 s. A grep that takes 267 s is a
hang from the agent's point of view, and it will be read as a broken box.

### D3 — `rtk tree` prints garbage

`/usr/bin/tree` is the BusyBox applet (`apk info -W /usr/bin/tree`:
`busybox-1.37.0-r19`). It takes no options, so the `-I <ignores> -L 2`
that rtk prepends comes back as `-I [error opening dir]`, one line per
argument, before the real listing. The `tree` package is 0.1 MB.

### D4 — AGENT.md tells the agent to use two TUIs

`AGENT.md` maps `du` to `ncdu` and `top` to `btop`. Both are full-screen
curses programs; neither prints anything useful into a pipe. An agent that
follows the table gets a hung command. `btop` and `ncdu` are fine for a
person in `wormhole attach`; they do not belong in the agent's table.

### D5 — `cargo nextest` without flags prints more than `cargo test`

If nextest is added without configuration, a green run of this suite
prints 59,764 bytes (one `PASS` line per test) against 44,541 for
`cargo test`, and rtk does not rewrite `cargo nextest` at all. See §4 for
the two variables that bring it to 510 bytes.

### D6 — the role's own hook has a shellcheck finding

```
roles/alphaca/hooks/preflight.sh:259:28: warning: This loop will only ever
run once. Bad quoting or missing glob/expansion? [SC2043]
```

The loop is `for marketplace in anthropics/claude-plugins-official`, a
one-item list written as a loop. Not a bug today, but the tool that would
have said so at review time is not in the box that develops this
repository. `typos` likewise finds `boxs` in `crates/wormhole/tests/kernel.rs:294`
and `tests/ps.rs:176` (and false positives on `ratatui`, which a
`_typos.toml` would silence).

## 3. Measurements

### Search, source tree only, hot cache, 15 runs

| Command | Mean |
|---|---|
| `busybox grep -rn "fn main" crates docs` | 69.3 ms |
| GNU `grep -rn --exclude-dir=target "fn main" .` | 13.5 ms |
| ugrep via `claude` (what Bash `grep` is in a Claude box) | 15.2 ms |
| `rg -n "fn main"` | 10.4 ms |
| `busybox find . -path ./target -prune -o -name "*.rs" -print` | 4.2 ms |
| GNU `find`, same | 4.5 ms |
| bfs via `claude` (what Bash `find` is in a Claude box) | 10.7 ms |
| `fd -e rs` | 11.2 ms |
| `busybox ls -R crates docs` | 2.6 ms |
| GNU `ls -R crates docs` | 3.3 ms |
| `eza -R crates docs` | 5.5 ms |

On a tree this size every tool is under 20 ms once `target/` is out of
the way. Speed is not the argument for `rg` and `fd` here; what is the
argument is that they skip `target/` and binaries by default, which is the
difference between 0.02 s and 267 s when the agent forgets to.

### Bytes the agent has to read

| Command | Bytes | ~Tokens |
|---|---|---|
| `busybox ls -la crates/wormhole-core/src` | 1458 | 364 |
| GNU `ls -la …` | 1018 | 254 |
| `eza -la …` | 809 | 202 |
| `eza -l --no-permissions --no-user --no-time …` | 289 | 72 |
| `rtk ls …` | 318 | 79 |
| `find … -name "*.rs"` (GNU) | 1584 | 396 |
| `fd -e rs` | 1494 | 373 |
| `rtk find . -name "*.rs"` | 626 | 156 |
| `grep -rn "\.unwrap()" crates` (GNU) | 2080 | 520 |
| `rg -n "\.unwrap\(\)" crates` | 2080 | 520 |
| `rg -n --heading "\.unwrap\(\)" crates` | 1145 | 286 |
| `rtk grep` / `rtk rg`, same pattern | 2080 | 520 |
| `rg -c "\.unwrap\(\)" crates` | 41 | 10 |
| `ast-grep run -p '$X.unwrap()' -l rust crates` | 4440 | 1110 |
| `du -sh target/*` (GNU) | 55 | 13 |
| `dust -d1 -b target` | 142 | 35 |
| `tree -L 2 crates` (GNU) | 227 | 57 |
| `eza --tree -L 2 crates` | 197 | 49 |
| `git --no-pager diff` (current working tree) | 323,678 | 80,919 |
| `rtk git diff` | 17,642 | 4410 |
| `cargo test --workspace`, 595 pass | 44,541 | 11,135 |
| `rtk cargo test --workspace` | 42 | 10 |
| `cargo nextest run --workspace` | 59,764 | 14,941 |
| `cargo nextest run` with `NEXTEST_STATUS_LEVEL=fail NEXTEST_FINAL_STATUS_LEVEL=fail` | 510 | 128 |

Tokens are bytes divided by four. Two things stand out. rtk's filters do
the heavy lifting on `git diff` and `cargo test`; they did nothing on a
25-line grep result, where `rg --heading` or `rg -c` is what saves tokens.
And `ast-grep` prints more than `rg` for the same pattern because it
prints the whole matched expression; its value is precision (§4), not
brevity.

None of the tools above emitted an ANSI escape when piped: `rg`, `fd`,
`eza`, `ast-grep`, `cargo`, `nextest`, `typos`, `hyperfine`, `dust`,
`tokei`, `jq`, `shellcheck` all count zero `ESC` bytes. `NO_COLOR` and
`--color=never` in the box are belt and braces, not a fix for anything
observed.

### Tests

| Command | Mean of 3 |
|---|---|
| `cargo test --workspace` | 2.418 s ± 0.104 s |
| `cargo nextest run --workspace` | 1.808 s ± 0.024 s |

595 tests, 14 binaries, all passing in the box.

### Linking (debug profile, separate target directories, gcc 14.2.0)

| Build | Default linker | mold 2.39.1 |
|---|---|---|
| cold `cargo build --workspace` | 25.0 s | 23.6 s |
| touch `crates/wormhole/src/main.rs`, rebuild (3 runs) | 2.315 s ± 0.029 s | 0.961 s ± 0.052 s |
| touch `crates/wormhole-core/src/lib.rs`, rebuild (3 runs) | 3.118 s ± 0.041 s | 1.740 s ± 0.048 s |

`readelf -p .comment` on the result confirms `mold 2.39.1`. The agent's
loop is edit, build, test, many times a task; the relink is the part that
repeats, and mold takes 1.3 to 1.4 s off each one.

The linker is part of cargo's fingerprint: switching it rebuilt most of
the tree (15.7 s, not 1 s). A role must set it once and never toggle it.
`target/` is already shared with host builds that use a different
toolchain, so a one-time rebuild on adoption is the whole cost.

### sccache 0.10.0

| Build | Time |
|---|---|
| cold, `CARGO_INCREMENTAL=0`, no sccache | 24.6 s |
| cold, sccache, empty cache | 29.1 s |
| `target/` deleted, sccache warm (86 hits, 86 misses) | 12.0 s |

It pays only when `target/` is gone, costs 4.5 s when it is not, requires
incremental compilation off (which slows the edit loop it is meant to
help), and runs a server process. In `alphaca` the workspace, and so
`target/`, lives on host disk and survives the box. Reject there.

### Structural search

The Rust rule says no `.unwrap()` outside tests. `rg` finds 25 lines in
`crates/`, all inside `#[cfg(test)] mod tests`, and the agent reads all 25
to learn that. One ast-grep rule (pattern `$X.unwrap()`, `not inside` a
`mod_item` that `follows` an `attribute_item` matching `cfg\(test\)`, nor a
`function_item` that follows `#[test]`) returns 0 matches, and 25 again
with the exclusion removed. rtk has an `ast-grep` filter, so the hook
compacts its output.

### Gates run on this repository

| Tool | Result |
|---|---|
| `typos --format brief .` | 2 real (`boxs`), the rest `ratatui` false positives |
| `shellcheck` on the four role scripts | SC2043 in `preflight.sh:259`, SC2086 at `:171`, SC1007 in `refresh-digests.sh:10` |
| `shfmt -d -i 4 -ci` on two scripts | 111 lines of style diff, nothing semantic |
| `taplo fmt --check` on six manifests | two "not properly formatted", style only |

## 4. Verdicts

Rating is 0 to 10 for an agent box: breakage removed, time saved, bytes
saved, with image cost and isolation weighed against it. Sizes are the
installed size from Alpine 3.22's package index. "net" means the tool
makes no network connection and keeps no state unless noted.

### Adopt

| Tool | Package | Installed | Why | Rating |
|---|---|---|---|---|
| GNU grep, findutils, sed, coreutils, diffutils, gawk | `grep findutils sed coreutils diffutils gawk` | 2.6 MB | Removes D1, D2 and the whole class: every GNU flag an agent or a wrapper emits works. codex and grok have no other grep. | **10** |
| tree | `tree` | 0.1 MB | Removes D3. | **9** |
| jq | `jq` | 0.3 MB | Absent from alphaca; `cargo metadata`, `gh api`, every JSON an agent meets. `jaq` is not packaged for Alpine 3.22 and jq 1.8 is fast enough at these sizes. | **8** |
| mold | `mold` | 20.7 MB (+ libtbb, mimalloc) | Relink 2.3 s to 1.0 s, every edit. No network, no state. | **8** |
| cargo-nextest | `cargo-nextest` | 5.5 MB | Suite 25% faster, per-test processes, 510-byte report with the two variables below. Already pinned as an artifact in `alphaca-scentbird`. | **7** |
| ast-grep | `ast-grep` | 20.6 MB | Exact structural answers; rtk filter exists; `sg` alias is not shipped, call it `ast-grep`. Rewrites by pattern are safer than `sed` on code. | **7** |
| hyperfine | `hyperfine` | 0.9 MB | `rules/rust.md` demands benchmarks; `hyperfine --export-markdown` is how every table in this document was made. | **7** |
| shellcheck | `shellcheck` | 17.3 MB | This repository ships four shell scripts that run as root in a build or in every box start; D6 shows it finds things. For a role that develops wormhole, not for every role. | **6** |
| typos | `typos` | 9.2 MB | Found two real typos; needs a `_typos.toml` in the repository for `ratatui`. Cheap gate. | **5** |

### Optional

| Tool | Installed | Note | Rating |
|---|---|---|---|
| tokei | 2.3 MB | Repository shape in 20 lines; `scc` (6.8 MB) does the same with complexity counts. | 4 |
| dust | 1.9 MB | `du -sh target/*` already answers the disk rule in 55 bytes; `dust -b` only wins on deep trees. `dua` and `duf` are not packaged. | 3 |
| just | 2.6 MB | Named, reviewed commands for the agent are a good idea; this repository has no justfile, so nothing to run yet. | 4 |
| sd | 1.8 MB | Fewer escaping mistakes than `sed -i`, but ast-grep covers code and GNU sed covers the rest. | 3 |
| shfmt, taplo | 3.1 MB, 5.2 MB | Style gates only; taplo links OpenSSL and can fetch schemas, not verified here. | 3 |
| sccache | 9.6 MB | Reject for alphaca (§3). Optional for a role that builds many worktrees of one project from scratch; runs a server and must set `CARGO_INCREMENTAL=0`. | 3 |
| cargo-audit, cargo-deny | 7.8 MB, 9.5 MB | Real gates, but both fetch an advisory database into `~/.cargo` over the network at every run. Add when a role wants that traffic. | 4 |

### Reject for the agent

| Tool | Why |
|---|---|
| difftastic | 81.8 MB installed for a diff format that is not a patch and that rtk cannot filter. `rtk git diff` took this tree's diff from 324 KB to 18 KB. |
| bat | `cat` is rewritten to `rtk read`; `bat -pp` is `cat`. |
| delta | A pager. No TTY, no use. |
| eza, btop, ncdu | Keep for the person in `wormhole attach`; strike `btop` and `ncdu` from the agent's table (D4). `eza` stays in the table: `eza -l --no-permissions --no-user --no-time` is 72 tokens where `ls -la` is 254. |
| xh, procs, tealdeer, zoxide, atuin, fzf, yazi, zellij | Interactive, history-learning, or a second spelling of something the agent already knows. `tealdeer` is also not packaged for Alpine 3.22, and context7 answers the same question. `atuin` syncs by default. |
| nushell, fish | Not POSIX; every generated script breaks. |
| uutils coreutils | GNU coreutils is 1.0 MB and is what every script assumes. |

### Prior conclusions, checked

- **Never alias `grep` to `rg`.** Confirmed, and sharpened: Claude Code
  already shadows `grep` and `find` with compatible implementations, and
  rtk bypasses that shadow. The fix is a GNU grep in the image, not an alias.
- **Strongest candidates for agents are ripgrep, fd, jaq, hyperfine, just.**
  ripgrep and fd yes, for what they skip rather than for speed on this
  tree. jaq is not packaged; jq is. hyperfine yes. just has nothing to run.
  The list missed the three with measured gains: GNU utilities, mold,
  nextest.
- **Plain output needs `NO_COLOR`, `PAGER=cat`.** Not observed to be
  needed: every tool tested detects the pipe. Harmless to set.

## 5. What to change

Applied on 2026-10-01 to `roles/alphaca`, `templates/wormhole.toml`, the
workspace's own `wormhole.toml`, `AGENT.md`, and the `alphaca-scentbird`
role beside this repository. Two tests keep it: one for the GNU packages in
every shipped Alpine recipe, one for the tools table naming no program that
needs a person. The snippets below are what was proposed.

### `roles/alphaca/wormhole.toml`

```toml
packages = [
  "bash",
  "nodejs",
  "npm",
  "git",
  "curl",
  "github-cli",
  "openssh-client",
  "ca-certificates",
  "build-base",
  # GNU, not BusyBox: rtk and every agent emit GNU flags.
  "grep",
  "findutils",
  "sed",
  "coreutils",
  "diffutils",
  "gawk",
  "tree",
  "jq",
  "ripgrep",
  "fd",
  "eza",
  "ast-grep",
  "hyperfine",
  "mold",
  "cargo-nextest",
  "shellcheck",
  "typos",
  "btop",
  "ncdu",
  "zstd",
]

build = [
  # ... existing rustup lines ...
  # cc that links with mold; cargo is pointed at it below.
  "printf '#!/bin/sh\\nexec cc -fuse-ld=mold \"$@\"\\n' > /usr/local/bin/cc-mold && chmod 0755 /usr/local/bin/cc-mold",
]

# The linker is in cargo's fingerprint: set once, never toggled.
[env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER]
fixed = "/usr/local/bin/cc-mold"

# nextest prints one line per passing test otherwise (59 KB for this suite).
[env.NEXTEST_STATUS_LEVEL]
fixed = "fail"

[env.NEXTEST_FINAL_STATUS_LEVEL]
fixed = "fail"
```

`cargo-nextest` from apk is 0.9.94 and depends on the apk `cargo`, which
would pull Alpine's rustc 1.87 beside the rustup toolchain. Take it as an
`[[image.artifact]]` instead, exactly as `alphaca-scentbird` does
(`cargo-nextest-0.9.143-x86_64-unknown-linux-musl.tar.gz`). The same
applies to any `cargo-*` apk package.

Image cost of the adopt list: about 77 MB installed, of which mold and
ast-grep are 41 MB. The brief's reading that these tools "barely change
CPU or RAM" holds; they are processes that live for milliseconds.

### `AGENT.md`, the tools table

Replace the `du` and `top` rows, and say what the agent should do with
output:

| Instead of | Use | Why |
|---|---|---|
| `grep -r` | `rg` | Skips `target/`, `.git` and binaries; `rg -c` for counts, `--heading` when reading |
| `find` | `fd` | Same skipping; `fd -e rs`, `fd -u` to include ignored |
| `ls -la` | `eza -l --no-permissions --no-user --no-time` | A third of the tokens |
| `du` | `du -sh <dir>/*` | Plain and short; `ncdu` is for a person |
| regex edits to code | `ast-grep run -p '<pattern>' -r '<rewrite>' -l rust` | Edits syntax, not text |
| `time` | `hyperfine` | Warmup, repeats, a table |

Drop the `top` row. Add: "`tree`, `grep`, `find`, `sed`, `diff` are GNU
in this image; BusyBox is not on `PATH` for them."

### `ROLE.md` or `rules/rust.md`

- Tests: `cargo nextest run --workspace`; `cargo test --doc` for doctests,
  which nextest does not run.
- Benchmarks: `hyperfine -N -w 3 -r 10 --export-markdown <file> '<cmd>'`.
  Still alone, still no `RUSTFLAGS`.
- Gates before done, in addition to fmt, clippy and tests: `typos`, and
  `shellcheck` on every `.sh` touched.
- Structural search before a claim about the codebase: `ast-grep run -p`
  with `--json=compact` when the result is to be counted, not read.

### `roles/alphaca/hooks/preflight.sh`

Nothing tool-related. The SC2043 loop at line 259 should either become a
plain call or gain a second marketplace; shellcheck will say so on every
run until it does.

## 6. Not measured, and why

- **Real agent traffic.** The box home holds three sessions, two of them
  this review. Host-side transcripts are not mounted. The command counts
  that would say how often an agent reaches for `grep` versus `rg` live
  there. rtk's own `discover` over the same three sessions already lists
  `eza` and `fd` as unhandled and `cat /etc/os-release` as the top missed
  rewrite.
- **codex and grok.** Not started here. The reasoning in §1 applies more
  strongly to them, since they have no shadow functions, but it was not run.
- **A large repository.** This tree is small; the 267 s figure comes from
  `target/`, not from source. On a monorepo the hot-cache gaps between
  tools would widen in ripgrep's favour; they would not reverse.
- **Claude Code's own Grep and Glob tools.** They do not go through Bash
  or rtk and were not timed.
