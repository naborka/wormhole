# One role may run several products; each product is its own box

A role is a persona and a toolchain, not a vendor CLI. Cloning `alphaca` into `alphaca-codex` and `alphaca-grok` stuffed the product into role identity, so one persona became three directories and three copies of ROLE.md. A role may now name every product it offers (`run = ["claude", "codex", "grok"]`); `--run` picks one; box identity is workspace + role source + product, so each product keeps its own home, login and history.

Launch facts (argv, instruction pointer, credential write, first-run seed) stay in the `KnownAgent` table. Skills, plugins and MCP stay opaque bytes in the role's preflight, which branches on `WORMHOLE_RUN`. Claude plugins have no grok or Codex analogue and are skipped there. One home for two products is refused: a hostile grok session would see a shared `~/.codex/auth.json`. N sessions in one box (CONCEPT.md §4 / §7) still waits on sibling namespaces.

Considered and rejected: putting MCP/skills/plugins in wormhole (a lagging copy of CLIs it does not control); `[agents.claude]` tables that bake one image and two credential files into one home; `--agent` that skips the role hook and invents a provisioner.
