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

They are installed. Prefer them — they are faster, and their output is easier to
read correctly.

| Instead of | Use | Why |
|---|---|---|
| `grep -r` | `rg` | Skips ignored files, much faster |
| `find` | `fd` | Simpler patterns, respects `.gitignore` |
| `ls` | `eza` | Clearer listing; `eza --tree` for trees |
| `du` | `ncdu` | Interactive, finds what is actually large |
| `top` | `btop` | Readable process view |

`rg --files` beats `find . -type f`. `fd -e rs` beats `find . -name '*.rs'`.

Do not write shell aliases for these: a non-interactive shell never reads them,
so call the real names.

## How to work

- Read before you change. Match the style of the code around you.
- Prefer the smallest change that is correct.
- Run the tests. Report failures with their output rather than summarising them.
- When a command fails, quote the shortest line that explains why.
- Say what you did and what you did not do. Do not report a task complete while
  part of it is unfinished.
