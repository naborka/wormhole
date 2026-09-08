# Egress changes live, because policy never lived in the box

**Superseded on 2026-09-08.** The broker was removed; a box is on the host's network and `[access] credentials` says what it logs in as. Kept as the record of what was built and why.

A blocked host used to mean editing the manifest and restarting the
box — an unusable loop for any browsing-shaped work. The fix is
structural, not a feature: the allowlist is enforced by the host-side
broker, so nothing about the box (namespaces, mounts, processes) is
involved in changing it. The broker now reads the box's live allowlist
file on every tunnel; `wormhole allow <box> <host>` appends to it and
is in force on the agent's next retry. `deny` removes; open tunnels
live until they close, named plainly. Additions are kept in the box's
home so the same box remembers them across restarts; the manifest is
never edited on the user's behalf — it stays the role's word.

Considered and rejected: signaling the broker to reload (a per-tunnel
file read is cheap and cannot miss); a control socket (more surface for
the same effect); auto-appending to the manifest (a tool must not write
policy into a file the person owns); letting the agent request hosts
from inside the box (a hostile agent widening its own allowlist is the
exact hole the list exists to close — only a person on the host types
`allow`).
