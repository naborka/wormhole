# This box

Alpine 3.22, musl. `/bin/sh` is BusyBox ash: no brace expansion, no `pipefail`. Script needs bash: `#!/bin/bash`.

`PATH` is `~/.local/bin` then image dirs, same in exec, `sh -lc` and hooks. Login shell cannot find tool: tool not installed. Say so.

## Disposable PostgreSQL

Server 17 and pgvector installed, no daemon. One database per task, in scratch, gone before done:

```sh
pg=$(mktemp -d /var/tmp/pg.XXXXXX)
initdb -D "$pg" -A trust -U postgres >/dev/null
pg_ctl -D "$pg" -o "-k $pg -p 54329" -l "$pg/log" start
psql -h 127.0.0.1 -p 54329 -U postgres -c 'CREATE EXTENSION vector'
# DATABASE_URL=postgresql://postgres@127.0.0.1:54329/postgres
pg_ctl -D "$pg" stop && rm -rf "$pg"
```

## pnpm

JavaScript pnpm from apk. Repository pins another version in `packageManager`: first run switches to that version's JavaScript build, kept in home. Never `npx pnpm`: unpinned, it may pick native build, which has no musl binary.

## Playwright

Browsers cannot run here: glibc only. `PW_TEST_CONNECT_WS_ENDPOINT` set: suite runs on that server. Host side: `npx playwright run-server --port 3000`, then `wormhole box --env PW_TEST_CONNECT_WS_ENDPOINT=ws://127.0.0.1:3000/`. Empty: report, never install browsers.

## Fetching raw text

Web fetch tool rejects `text/markdown`. Raw file: `curl -fsSL <url>` or GitHub API.
