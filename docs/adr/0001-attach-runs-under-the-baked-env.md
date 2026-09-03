# Attach runs under the baked env, and its `--env` updates persist

An attach used to carry the attacher's whole shell environment into the box, so
variables a start declared were silently missing from every later session. Now a
start writes the environment it computed (`boxes/<pid>/env.toml`) and every attach
session runs under exactly that, refreshed: a value the attaching shell exports
wins, the baked one survives, `fixed` never moves. `--env` on attach writes the
change back, so later attaches inherit it — a box has one environment, not one
per session.

Considered and rejected: recomputing from the manifest at attach (loses values
the host no longer exports, and the manifest may have drifted from the running
box); session-only `--env` overrides (an update that silently vanishes on the
next attach surprises more than one that sticks; `wormhole env` makes the stuck
state visible). PID 1's own tree keeps its start values regardless — Linux
writes no other process's environment — so a refreshed value reaches the agent
only when a session starts it anew.
