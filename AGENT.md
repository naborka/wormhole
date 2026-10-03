# Default box instructions

These apply to every agent in every wormhole box, whatever the role. A role's
own instructions are added on top and win where they disagree.

## Where you are

You are inside an isolated box. The workspace is mounted read-write at the same
path it has on the host, and it is the only place your edits persist. Your home
is kept for this box between starts, so history, settings and logins survive.
A box can serve other workspaces too; what they left in the home is not this
workspace's. Everything else — the root filesystem, `/tmp` — is a throwaway
copy that disappears when the box exits.

Nothing of the host is visible except the paths the box was explicitly granted.
If something you expect is missing, it was not granted; say so rather than
working around it.

Permission prompts are off because the box, not the prompt, is what holds the
line. Act without asking for permission to read, write or run things inside the
box. This is not licence to be careless with the workspace: it is the one place
your mistakes survive.

## Use the modern tools

They are installed. Prefer them — they skip what you do not want to read, and
they print less.

| Instead of | Use | Why |
|---|---|---|
| `grep -r` | `rg` | Skips `.gitignore`d files, `.git` and binaries; `rg -c` to count, `rg --heading` to read |
| `find` | `fd` | Same skipping, simpler patterns; `fd -u` to include ignored files |
| `ls -la` | `eza -l --no-permissions --no-user --no-time` | A third of the bytes |
| `du` | `du -sh <dir>/*` | One line per entry; `ncdu` is for a person at a terminal |
| regex edits to code | `ast-grep run -p '<pattern>' -r '<rewrite>' -l <lang>` | Matches syntax, not text |
| `time` | `hyperfine` | Warmup, repeats, a table |

`rg --files` beats `find . -type f`. `fd -e rs` beats `find . -name '*.rs'`.
`grep`, `find`, `sed`, `diff`, `tar` and `tree` are the GNU ones here, not BusyBox,
so their usual flags work.

Do not write shell aliases for these: a non-interactive shell never reads them,
so call the real names.

## How to work

- Read before you change. Match the style of the code around you.
- Prefer the smallest change that is correct.
- Run the tests. Report failures with their output rather than summarising them.
- When a command fails, quote the shortest line that explains why.
- Say what you did and what you did not do. Do not report a task complete while
  part of it is unfinished.
