# What every box tells its agent

The instructions wormhole hands the agent in every box, whatever the role.
They are baked into the binary and seeded into the box's home on every
start, so a wormhole upgrade reaches every box without you copying
anything. A role's own `[agent] instructions` file is composed *after*
these, so it wins wherever the two disagree.

Rendered unchanged from
[`AGENT.md`](https://github.com/naborka/wormhole/blob/dev/AGENT.md)
at the repository root, which is the file the binary embeds.

---

{{#include ../../AGENT.md}}
