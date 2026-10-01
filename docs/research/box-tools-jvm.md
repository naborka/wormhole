# Tools for the JVM agent box, and whether codex works in a box

Second review in the series begun by [box-tools.md](box-tools.md). Two
questions: what the `alphaca-scentbird` role (Java, Kotlin, Gradle, Maven,
node, Rust, protobuf) should carry for an agent, and whether the `codex`
product in `alphaca` gets what the role thinks it gets.

Everything measured inside a running `alphaca` box: Alpine 3.22.2,
OpenJDK 21.0.11, Gradle 8.14.1, Maven 3.9.9, rtk 0.50.0, codex 0.159.3,
Claude Code 2.1.286. Scentbird's repositories are not mounted here, so the
JVM numbers come from a `gradle init` Java application and a one-test
Maven project: they measure the tools' own behaviour, not Scentbird's
build graph.

## 0. The finding in one paragraph

The JVM side needs little: Gradle and Maven already print plainly into a
pipe, rtk already filters both (`rtk gradlew`, `rtk mvn`), and the GNU
utilities from the first review are the only package the role was missing
for correctness. Two environment lines earn their place by measurement:
`MAVEN_ARGS="-B -ntp"` turns a first `mvn test` from 65 KB of download
progress into 1.2 KB, and `org.gradle.caching=true` in the kept home's
`gradle.properties` makes a clean rebuild hit the cache (4.8 s to 1.6 s,
every compile task `FROM-CACHE`), which is what a box serving many
worktrees does all day. ast-grep parses Java but Alpine's build has no
Kotlin parser. **codex is the real finding:** in an `alphaca` codex box,
rtk does nothing. `rtk hook codex` returns no rewrite for the payload
Codex documents (`rtk hook claude`, `droid` and `cursor` all rewrite the
same payload), and the `@RTK.md` line `rtk init --codex` appends goes
through wormhole's `~/.codex/AGENTS.md` symlink into the canonical
`~/AGENTS.md`, which the next start rewrites, and codex has no `@` import
syntax anyway. Codex itself runs fine on musl, with its own `rg`, no
approvals and no inner sandbox, as the role intends.

## 1. The scentbird box, JVM and node

### What rtk already covers

The PreToolUse hook rewrites the commands an agent writes for this stack:

| Written | Rewritten to |
|---|---|
| `./gradlew test`, `gradle build` | `rtk gradlew …` |
| `mvn -q test`, `./mvnw verify` | `rtk mvn …` |
| `npx tsc --noEmit` | `rtk tsc …` |
| `npx eslint .` | `rtk lint .` |
| `psql -c …` | `rtk psql …` |
| `npm test`, `java -version`, `buf lint`, `cargo nextest run` | unchanged |

`rtk gradlew` falls back to `gradle` when there is no wrapper: in a
directory without `gradlew` it still ran Gradle.

### Output, bytes the agent reads

Gradle, warm daemon, one JUnit test:

| Command | Bytes |
|---|---|
| `./gradlew test --rerun-tasks` | 363 |
| `./gradlew test --rerun-tasks --console=plain` | 313 |
| `./gradlew test --rerun-tasks -q` | 0 |
| `rtk gradlew test --rerun-tasks` | 54 |
| `./gradlew test`, one failing test | 748 |
| `./gradlew test -q --console=plain`, one failing test | 320 |
| `rtk gradlew test`, one failing test | 288 (test name, counts, report path, `rtk recall` id) |

Gradle detects the pipe: no ANSI byte in any run, `--console=plain` buys
50 bytes. The brief's advice to force plain console is unnecessary here.

Maven, one JUnit test:

| Command | Bytes |
|---|---|
| `mvn test`, first run, empty local repository | 65,130 |
| `mvn -B -ntp test`, first run, empty local repository | 1,220 |
| `mvn test`, warm | 1,219 |
| `mvn -B -ntp -q test`, warm | 0 |
| `rtk mvn test`, warm | 226 |
| `rtk mvn test`, first run, empty repository | 227 |
| `MAVEN_ARGS="-B -ntp" mvn test` | 1,219 (the variable is honoured, Maven 3.9.9) |

The 65 KB is transfer progress, one line per artifact. An agent's first
Maven build in a fresh worktree pays it once per dependency set; `-ntp`
removes it and rtk removes it too, so the variable is the belt for a
command that bypasses the hook (`mvn` inside a script, say).

### Time

| Gradle | Seconds |
|---|---|
| first build, daemon start, dependency download | 50.6 |
| up-to-date build, warm daemon (3 runs) | 1.67, 1.24, 1.35 |
| up-to-date build, `--no-daemon` | 8.73 |
| up-to-date build, daemon restarting | 8.29 |
| `clean test --build-cache`, cold cache | 4.76 |
| `clean test --build-cache`, warm cache, 3 tasks `FROM-CACHE` | 1.63 |

The daemon is on by default and it is 7 seconds a command. It is one JVM
at 352 MB resident after a build, in the box's PID namespace, so it dies
with the box. The build cache is off by default; one line in
`~/.gradle/gradle.properties` turns it on for every project the box
serves, and `~/.gradle` is in the kept home.

### ast-grep for Java and Kotlin

| Build | Java | Kotlin |
|---|---|---|
| Alpine `ast-grep` 0.28.1 | matches | `not implemented: this ast-grep was compiled without kotlin tree-sitter parser`, aborts |
| npm `@ast-grep/cli` 0.45.3 | no musl binary published, install fails | — |
| GitHub release `app-x86_64-unknown-linux-gnu.zip` 0.45.3 under `gcompat` | matches | matches; `ldd` reports `__fprintf_chk: symbol not found`, lazily bound, so an error path may still abort |

Java is covered by the package. Kotlin is not, by any build that is safe
to ship; the glibc binary works until it does not.

### Gates and lints

Kotlin and Java formatting and lint (ktlint, detekt, spotless,
google-java-format) are Gradle plugins at the version each project pins,
and a standalone binary on `PATH` would disagree with them, exactly as
`rules/java.md` says of `kotlinc`. Nothing to add. `yq` (11.9 MB) for the
`gitlab/` CI YAML and Kubernetes manifests is the one JVM-adjacent format
tool the stack reads that `jq` does not; optional.

### Verdicts for `alphaca-scentbird`

| Change | Verdict | Rating | Evidence |
|---|---|---|---|
| GNU grep, findutils, sed, coreutils, diffutils, gawk, tree | adopt (done, first review) | 10 | same BusyBox, same rtk `--null`, same breakage |
| `[env.MAVEN_ARGS] fixed = "-B -ntp"` | adopt | 7 | 65 KB to 1.2 KB on a first build; honoured by 3.9.9 |
| `org.gradle.caching=true` in `~/.gradle/gradle.properties`, written by preflight when absent | adopt | 7 | clean rebuild 4.8 s to 1.6 s; many worktrees, one cache |
| mold, cc-mold wrapper, linker env (done, first review) | adopt | 8 | relink 2.3 s to 1.0 s |
| nextest status-level env (done, first review) | adopt | 7 | 59 KB to 510 B |
| ast-grep (done, first review) | adopt for Java, Rust, TypeScript | 6 | Java matches; Kotlin not in Alpine's build |
| hyperfine (done, first review) | adopt | 6 | the benchmark rule has a tool |
| `yq-go` | optional | 3 | CI and k8s YAML; `jq` cannot read it |
| sccache | optional | 4 | deps compile once across worktrees; or point every worktree's `CARGO_TARGET_DIR` at one directory in the kept home, no tool |
| `--console=plain`, `NO_COLOR` | nothing to do | — | Gradle, Maven, rtk all detect the pipe |
| ktlint, detekt, google-java-format binaries | reject | — | the project's plugin at the project's version is the arbiter |
| Kotlin language server | reject (already) | — | glibc only |

## 2. codex in the alphaca box

`alphaca` runs as `claude`, `codex` or `grok`. The first review's defects
were found through Claude; this section asks what codex sees.

### What works

- **The musl package runs.** `codex-package-x86_64-unknown-linux-musl`
  0.159.3 starts, `codex doctor` passes every check but the login it has
  not made, and finds its own `rg` in `codex-path/`. So codex's searches
  never touched BusyBox grep; its Bash commands did.
- **No approvals, no inner sandbox.** The launch flag is
  `--dangerously-bypass-approvals-and-sandbox`
  (`manifest.rs:586`), and the seeded `~/.codex/config.toml` carries
  `approval_policy = "never"`, `sandbox_mode = "danger-full-access"`,
  `web_search = "live"` (`seed.rs:345`), so a `codex resume` typed in an
  attached shell behaves the same.
- **Instructions reach it.** `~/.codex/AGENTS.md` is a symlink to the
  canonical `~/AGENTS.md`; codex reads `AGENTS.md` in `CODEX_HOME` and
  follows the link. 8.3 KB against the 32 KiB `project_doc_max_bytes` cap
  that counts global and repository files together.
- **Hooks exist.** `codex features list` reports `hooks  stable  true`,
  `~/.codex/hooks.json` is read, and a `PreToolUse` hook may return
  `hookSpecificOutput.updatedInput.command`, the same contract Claude Code
  uses.
- **GNU utilities now.** Codex has no shell functions shadowing `grep` and
  `find`, so before the first review every GNU flag it wrote hit BusyBox
  directly. Fixed by the package list.

### C1 — `rtk hook codex` rewrites nothing

The payload Codex documents for `PreToolUse`:

```
{"session_id":"s1","cwd":"/w","hook_event_name":"PreToolUse","model":"…",
 "permission_mode":"…","turn_id":"t","tool_name":"Bash","tool_use_id":"u1",
 "tool_input":{"command":"grep -rn x ."}}
```

| Processor | stdout |
|---|---|
| `rtk hook codex` | empty, exit 0 |
| `rtk hook claude` | `{"hookSpecificOutput":{…"updatedInput":{"command":"rtk grep -rn x ."}}}` |
| `rtk hook droid` | same rewrite |
| `rtk hook cursor` | `{"continue":true,"permission":"ask","updated_input":{"command":"rtk grep -rn x ."}}` |
| `rtk hook check 'grep -rn x .'` | `rtk grep -rn x .` |

Every field shape I could derive from the documentation was tried: with
and without `hook_event_name`, `tool_name` as `Bash` and `shell`, command
as string and as array. rtk 0.50.0's codex processor is silent for all of
them while its engine rewrites the command. Not run end to end inside
codex, which needs a login this box does not have; the hook contract is
the same JSON in both directions, so the processor is where the
difference is.

### C2 — the `@RTK.md` line goes through a symlink and is wiped

`rtk init --codex` appends `@$CODEX_HOME/RTK.md` to `$CODEX_HOME/AGENTS.md`
and is idempotent about it. In the box that path is a symlink
(`manifest.rs:589`, `Pointer::Symlink`), so the write lands in
`~/AGENTS.md`:

```
$ cat ~/AGENTS.md          # after rtk init --codex
CANONICAL

@/…/.codex/RTK.md
```

`seed_instructions` rewrites `~/AGENTS.md` on every start
(`seed.rs:30-34`, `seed_file` writes unconditionally at `seed.rs:71-80`),
and the preflight's stamp keeps `rtk init` from running again. So the line
lives for one session. It would not have mattered: codex's own
documentation names no `@` include, and wormhole chose a symlink for it
for that reason. The same write-through happens for grok
(`CODEX_HOME=~/.grok/rules`), where it is harmless because grok reads
every `*.md` in that directory, `RTK.md` included.

The claude side has the same shape with a softer edge: `~/.claude/CLAUDE.md`
is an import file (`manifest.rs:560`) rewritten by `seed_file` every
start, so the `@RTK.md` line `rtk init --auto-patch` appends there is
also gone by the second session. In this box `CLAUDE.md` was written at
11:06:11, after the start's `AGENTS.md` at 11:05:40 and before the stamp
at 11:06:19: the first start after a hook change, the only one where the
line is present. Claude keeps rtk anyway, because the hook in
`settings.json` survives the merge. Codex keeps nothing.

### C3 — what codex is left with

No rewrite, no `RTK.md`. A codex session in `alphaca` reads every
command's full output. The first review's `git diff` was 324 KB raw and
18 KB through rtk; `cargo test` 44 KB raw and 42 bytes through rtk.

### Verdicts for codex

| Change | Verdict | Rating | Why |
|---|---|---|---|
| In `hooks/preflight.sh`, codex branch: write `~/.codex/hooks.json` pointing at `rtk hook claude` instead of calling `rtk init --codex`, and skip the `AGENTS.md` append | adopt, then verify in a logged-in codex box | 8 | the one processor that answers the documented payload; C1 |
| Report the silent processor to rtk-ai/rtk with the payload above | adopt | 6 | the fix belongs upstream; the hook swap is the interim |
| Stop relying on `rtk init` to put `RTK.md` into any agent's instructions; carry the two paragraphs in `ROLE.md` or a `rules/` file | adopt | 6 | C2: a start wipes it for claude and codex alike; rules survive by construction |
| Make `seed_file` for a `Pointer::Import` preserve lines a tool appended | reject | — | the pointer is one line by design, so a past session's writes cannot drift it; the fix is for the hook not to depend on that file |
| Leave grok's `CODEX_HOME` trick | keep | — | grok reads the directory, so `RTK.md` lands where it is read |

## 3. Applied

On 2026-10-01: `MAVEN_ARGS`, the Gradle cache line in preflight and the
Java rule in `alphaca-scentbird`; the codex `hooks.json` and the
command-output section in both roles' `ROLE.md`; the pointer sentence in
the roles guide. Still to do: run a logged-in codex box and confirm a
`grep` arrives as `rtk grep`.

Issue for rtk-ai/rtk, not filed from here:

> `rtk hook codex` (0.50.0) prints nothing for the PreToolUse payload Codex
> documents (`tool_name: "Bash"`, `tool_input.command` string, with
> `session_id`, `cwd`, `hook_event_name`, `tool_use_id`). `rtk hook claude`,
> `droid` and `cursor` rewrite the same payload, and `rtk hook check` shows
> the rewrite. Expected: `hookSpecificOutput.updatedInput.command`.

## 4. Not measured

- **Scentbird's own builds.** Not mounted. The Gradle and Maven numbers
  are the tools' overheads on a toy; a real multi-project build changes
  the absolute times, not which of these settings matter.
- **codex end to end.** No login here. C1 is a processor test and a
  contract read, not a session transcript.
- **grok.** Not downloaded; the write-through is by the same code path.
- **The Kotlin daemon.** The toy is Java; a Kotlin project adds a second
  resident JVM.
