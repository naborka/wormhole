# Box tooling audit, 2026-10-02: what one day in an `alphaca` codex box broke

An audit of one working day in an `alphaca` box running codex, over a
Node and Rust repository. Ten findings, each traced to a cause; what was
done about each is here, so the next audit starts from this one.

| Finding | Cause | Done |
|---|---|---|
| `rtk: not found`, `codex: not found` from every hook and rewritten command | `sh -lc` reads `/etc/profile`, whose first line resets `PATH` to the image's directories; `~/.local/bin` is gone | The box writes its `PATH` into `/etc/profile.d/00-wormhole.sh` at every start. |
| `pnpm: not found`; `npx pnpm` then failed on `@pnpm/exe` for `linux-x64-musl` | No pnpm in the image; unpinned npx picked a build with no musl binary | `pnpm` from apk, the JavaScript build. A `packageManager` pin switches to that version's JavaScript build. Corepack is not an option: Alpine builds node `--without-corepack` |
| `python3: not found` | Not in the image; the skills the preflight installs ship python scripts | `python3` in the image |
| `psql: not found`; a database was assembled from downloaded apks | No server in the image | `postgresql17` and `postgresql-pgvector`, binaries linked onto `PATH`; `rules/box.md` gives the disposable-database recipe |
| `time zone "Europe/Belgrade" not recognized` | A minirootfs has no zoneinfo | `tzdata` in every Alpine recipe, kept by a test |
| Playwright: `Executable doesn't exist` | Playwright ships glibc browsers; Alpine is unsupported, and no env var points it at a system browser | `PW_TEST_CONNECT_WS_ENDPOINT` crosses from the host, so a host `npx playwright run-server` runs the suite |
| `rm -f style commands are not permitted` | Codex's own heuristic; `approval_policy = never` turns its prompt into a refusal, the bypass flag does not reach it | Every codex start writes `~/.codex/rules/wormhole.rules` allowing `rm` |
| Expired token at session start | A stale login in the kept home, refreshed by the next `/login` | Nothing: not a box fault |
| rtk prints emoji | `emoji = true` is rtk's default | The preflight sets `emoji = false` |
| Web fetch refuses `text/markdown` | The fetch tool's own limit | `rules/box.md` says to `curl` raw files |

Two more, outside the fault list: BusyBox `tar` refused a GNU flag, so
GNU `tar` joins the GNU set every Alpine recipe installs; and `/bin/sh`
is BusyBox ash, which `rules/box.md` now says.
