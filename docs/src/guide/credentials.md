# Credentials

The box is on the host's network and talks to the model API for itself.
The one question left is what it logs in as, and that is yours to
answer, per manifest or per start:

```toml
[access]
credentials = "none"      # or "copy", "share"
```

```sh
wormhole box --credentials copy     # this start only; beats the manifest
```

| Mode | What the box gets | Who refreshes | The host file |
|---|---|---|---|
| `none` (default) | a clean home; `/login` inside the box | the box, its own copy | never read |
| `copy` | your login files, copied into the box home once | the box, its own copy | never written |
| `share` | your login file, bound read-write at the same place | whoever runs next | written by the box |

## `none`

Nothing of yours is read. The agent's own login flow runs inside the
box — Claude Code prints a URL, you finish it in a browser, paste the
code back — and the result lands in the box home, where it survives
every restart of that box. A second box in the same folder is a second
login, because it is a second home.

This is the default because it is the one mode where a box acts as an
account you chose for it, not as you. A separate account for boxed work
is the honest answer to [what a login really carries](access.md#what-a-credential-really-carries).

## `copy`

On a start that finds the box home without a login, the agent's
credential files are copied in from your home — `~/.claude/.credentials.json`
for `claude`, `~/.codex/auth.json` for `codex`, `~/.grok/auth.json` for
`grok` — and, for `claude`, the account fields beside it in
`.claude.json`, because the token alone is half a login. Once: a box that
already has a login keeps it, so a refresh the box made is never
clobbered by the next start.

From then on the box and the host hold two tokens for one account, and
each refreshes its own. Your host file is never written by a box.

## `share`

Your credential file is bound read-write at the same path in the box
home. One login, and a refresh in the box lands on the host. The banner
counts it as a grant, because it is one: a host path the box can write.

Only for an agent that writes that file in place. A bind is a mount
point, and nothing can rename over a mount point — so an agent that
saves a login by writing a new file and moving it into place gets
`Resource busy` instead, after the whole login. `grok` is one, so a
`share` naming it is refused before the box starts, with `copy` and
`none` named as the two that work.
For `claude` the account fields in `.claude.json` are carried into the
box home the same way `copy` does it; that file stays the box's own.

The refresh token rotates on every use, so the host agent and a box
refreshing at the same moment can invalidate each other — the cost is a
`/login` on whichever side lost. Two boxes sharing one file have the
same race with each other. `copy` avoids it at the price of a second
token.

## What a box cannot do with any of them

Widen what the login can reach: that is the account's, decided on its
provider's side. Reach anything on disk beyond the workspace, its home
and the grants: that is the mount plan's, and it is checked by the box
itself before the agent starts. Regain a capability: none are left to
regain.
