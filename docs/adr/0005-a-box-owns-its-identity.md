# A box owns its identity, and a role box may serve many workspaces

Supersedes the identity tuple in [0004](0004-one-role-three-homes.md): a box is no longer *workspace + role source + product*.

A box is made from one recipe (a workspace's own manifest, or a role by its source) and one product, fixed for its life. The workspace is a fact about a run, kept as the list of workspaces the box ran in. A bare start resumes a box that ran here; a role that says `[agent] resume = "anywhere"` lets it resume one from any workspace, so one home keeps its login and everything the preflight installed for every project. `--id` starts a role box from any workspace, as the role it records; naming it is the consent. A box made from a workspace's own manifest runs only there, and such a manifest cannot say `anywhere`: a cloned repository must not decide how far its own box reaches.

The record moved out of the home, into `records/<key>.toml` beside the lock. The home is the agent's: a record there let a session rewrite its workspace, which the panel then mounted, or its source, so another role's start resumed its home. The key is found by id and never rebuilt from the caller's directory. Its label stays the basename of the workspace a box was made in, not the role's name: two roles racing for the same free ordinal must meet on one lock file, or both claim one id.

One process per box stays. Two runs of one home at once is what the vendors increasingly support, but wormhole's own start-time seed races a live writer and Codex's `auth.json` write is unlocked; a busy shared box makes a start take another box instead.

`gc` calls a box dead when nothing can start it: its workspaces are gone and its recipe resumes only where it ran, or its role's directory is gone. A box of an `anywhere` role outlives its projects.

Considered and rejected: sharing pieces of the home (`~/.cargo`, the login) between per-workspace boxes, a second policy language that shares the same reach anyway; automatic sharing for every role, which would let an untrusted clone started with a role join a home holding every other project's history; the record in the home with a stand-in directory for older binaries; a claim per (box, workspace) in this change. Research: `docs/research/shared-box.md`, `shared-box-prior-art.md`, `shared-box-concurrency.md`.
