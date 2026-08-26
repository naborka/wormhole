# Role UX prior art

Primary-source research into how comparable tools solve the UX problems
wormhole's role + box model runs into. Every claim below is cited to an
official doc site, man page, spec repo, or the tool's own source. Anything
that could not be confirmed from a primary source is marked
**UNVERIFIED**.

Read alongside [CONTEXT.md](../../CONTEXT.md) and
[the roles guide](../src/guide/roles.md).

---

## What the evidence says

1. Nobody sniffs three forms out of one argument without regretting it. Nix
   and Terraform both document the ambiguity in prose and both ship a
   type-forcing prefix (`path:`, `git::`) as the escape hatch.
2. The two tools that avoid ambiguity entirely do it with **arity** (asdf,
   mise: 1 arg = name, 2 args = URL) or **separate flags** (cargo:
   `--git` / `--path` / `--registry`, declared mutually exclusive).
3. The rule everyone who sniffs converges on is the same one wormhole uses
   backwards: the **bare** form is the installed/registry name, and the
   **path** form must be marked (`./`, `../`). Nix: "relative paths must
   start with `.` to avoid ambiguity with registry lookups."
4. Almost nobody echoes which form was taken. Nix does — via its error
   messages, which print the normalized `flake:…` form. asdf and mise
   expose it after the fact through `plugin ls --urls`.
5. Install-before-use is nearly extinct. `gh extension` is the only tool
   surveyed with no direct-run path, and it justifies that with a trust
   argument. uv states the opposite principle outright: "In most cases,
   executing a tool with `uvx` is more appropriate than installing."
6. Only `npx` gates a remote fetch on a human, and it says why:
   typosquatting. It also documents exactly how the gate disappears —
   "When standard input is not a TTY or a CI environment is detected,
   `--yes` is assumed." That is the opposite of wormhole's rule.
7. Content-digest-keyed trust exists and works: direnv's approval token
   *is* `sha256(abs path + contents)`, so a changed file is structurally
   un-approved. mise does this only in `paranoid` mode; by default it keys
   trust to the path and survives edits.
8. Name-keyed trust is the common case (Homebrew, VS Code, mise default)
   and its docs are candid that it covers all *future* content at that
   name. Cargo has no gate at all, by explicit Rust policy.
9. Instance identity is keyed to the tuple that makes resume correct.
   The devcontainer CLI uses exactly `(workspace folder, config file)` —
   the same shape as wormhole's `(workspace, role)`.
10. On vocabulary, Homebrew has the only table that names every layer
    separately — formula, keg, rack, opt prefix, keg-only — and Terraform's
    changelog is the clearest admission that a name collision is worth a
    rename, plus the clearest proof that renaming can land in a second one.

---

## 1. Three-form arguments

The question: tools whose single flag or positional accepts "a name, a
path, or a remote ref" and disambiguates by syntax.

### Four strategies, in order of how much ambiguity they leave

| Strategy | Tools | Ambiguity |
|---|---|---|
| Separate flags | `cargo install` | none |
| Arity | `asdf plugin add`, `mise plugins install` | none |
| Bare = registry, path must be marked | `nix`, Terraform, `go install` | resolved by a documented rule |
| Unconstrained sniffing | `docker build`, `pipx install` | undocumented |

### cargo — separate flags, mutually exclusive, and it refuses the sniffing form

<https://doc.rust-lang.org/cargo/commands/cargo-install.html>

> There are multiple sources from which a crate can be installed. The
> default source location is crates.io but the `--git`, `--path`, and
> `--registry` flags can change this source.

The synopsis is four separate grammars, not one argument:

```
cargo install [options] crate[@version]…
cargo install [options] --path path
cargo install [options] --git url [crate…]
```

They are declared mutually exclusive in
[`src/bin/cargo/commands/install.rs`](https://github.com/rust-lang/cargo/blob/master/src/bin/cargo/commands/install.rs):

```rust
opt("git", "Git URL to install the specified crate from").conflicts_with_all(&["path", "index", "registry"]),
opt("path", "Filesystem path to local crate to install from").conflicts_with_all(&["git", "index", "registry"]),
```

And cargo actively rejects the syntax-sniffing form with an error that
names the right flag:

```
error: invalid package name: `https://github.com/foo/bar`
    Use `cargo install --git https://github.com/foo/bar` if you meant to install from a git repository.
```

Cargo does not echo which source it resolved to.

### Nix — the closest analogue to wormhole, and it documents the exact trap

<https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-flake.html#flake-references>

> Flakes corresponding to a local path can also be referred to by a direct
> path reference, either `/absolute/path/to/the/flake` or
> `./relative/path/to/the/flake`. **Note that the leading `./` is mandatory
> for relative paths. If it is omitted, the path will be interpreted as
> URL-like syntax**, which will cause error messages like this:
>
> ```
> error: cannot find flake 'flake:relative/path/to/the/flake' in the flake registries
> ```

And the rationale, stated outright:

> Note that if you omit `path:`, relative paths must start with `.` **to
> avoid ambiguity with registry lookups** (e.g. `nixpkgs` is a registry
> lookup; `./nixpkgs` is a relative path).

Two things to notice.

**First, the precedence is the reverse of wormhole's.** Nix's default type
is `indirect` — the registry lookup — and the *path* form carries the
burden of marking itself. Wormhole checks transport first, then treats
anything containing `/` as a path, and only a bare word as an installed
name. Nix reaches the same set of outcomes with the opposite default: the
bare word wins, and `./` is what buys you a path.

**Second, Nix echoes the form it took — in the error.** The message prints
`flake:relative/path/to/the/flake`, i.e. the normalized form Nix chose.
That is the single best example in this survey of a tool telling the user
which of three interpretations it picked.

Nix also ships explicit type prefixes (`path:`, `git+file:`, `github:`,
`tarball+https:`) so a wrong sniff can be overridden, and documents when
you need them:

> Note that the search will only include files indexed by git. In
> particular, files which are matched by `.gitignore` or have never been
> `git add`-ed will not be available in the flake. **If this is undesirable,
> specify `path:<directory>` explicitly**

(<https://nix.dev/manual/nix/stable/command-ref/new-cli/nix.html#installables>)

### Terraform — states the rule, and states the design goal

The classic module-sources page
(<https://github.com/hashicorp/terraform/blob/v1.9.8/website/docs/language/modules/sources.mdx>,
rendered at <https://developer.hashicorp.com/terraform/language/modules/sources#local-paths>):

> **A local path must begin with either `./` or `../` to indicate that a
> local path is intended, to distinguish from a module registry address.**

The design goal, from the same page's intro:

> Module source addresses use a *URL-like* syntax, but with extensions to
> support **unambiguous selection of sources** and additional features.

Two more findings worth carrying:

**Absolute paths are not local paths.** A surprising precedence rule that
falls straight out of sniffing:

> Note that Terraform does not consider an *absolute* filesystem path
> (starting with a slash, a drive letter, or similar) to be a local path.
> Instead, Terraform will treat that in a similar way as a remote module
> and copy it into the local module cache.

Wormhole's rule — "anything with no transport and a `/` is a path" — treats
`/abs/path` and `./rel/path` alike, which is the more predictable of the
two. Terraform's split exists because its *sniff* had to also decide
"package or not", which wormhole does not.

**Sniffing can require a network round-trip.** Terraform's Bitbucket
shorthand:

> This shorthand works only for public repositories, **because Terraform
> must access the BitBucket API to learn if the given repository uses Git
> or Mercurial.**

The escape hatches are the `git::`, `hg::`, `s3::`, `gcs::` forced-type
prefixes, and a `?archive=zip` override for when extension-sniffing gets it
wrong:

> **If your URL *doesn't* have one of these extensions but refers to an
> archive anyway, use the `archive` argument to force this interpretation**

HashiCorp's own fetching library says the same thing more bluntly
(<https://github.com/hashicorp/go-getter#forced-protocol>):

> In some cases, the protocol to use is ambiguous depending on the source
> URL. For example, `http://github.com/mitchellh/vagrant.git` could
> reference an HTTP URL or a Git URL. **Forced protocol syntax is used to
> disambiguate this URL.**

> Forced protocols will also override any detectors.

### go install — `@version` splits into two disjoint grammars

<https://go.dev/ref/mod#go-install>

> Since Go 1.16, if the arguments have version suffixes (like `@latest` or
> `@v1.0.0`), `go install` builds packages in module-aware mode, ignoring
> the `go.mod` file in the current directory or any parent directory if
> there is one.

> **To eliminate ambiguity about which module versions are used in the
> build**, if any of the arguments have version suffixes, the arguments
> must satisfy the following constraints:
>
> * Arguments must be package paths or package patterns (with "`...`"
>   wildcards). **They must not be standard packages (like `fmt`),
>   meta-patterns (`std`, `cmd`, `all`, `work`, `tool`), or relative or
>   absolute file paths.**

That third bullet is the decisive move: in `@version` mode, **file paths
are forbidden by grammar**. There is no overlap to disambiguate, so there
is no heuristic. Note the direct parallel — wormhole's remote form is also
`@<pin>`-suffixed and also cannot be a path.

### gh extension install — the local form is the literal string `.`

<https://cli.github.com/manual/gh_extension_install>

> For GitHub repositories, the repository argument can be specified in
> `OWNER/REPO` format or as a full repository URL.

> **For local repositories, often used while developing extensions, use `.`
> as the value of the repository argument.**

In [`pkg/cmd/extension/command.go`](https://github.com/cli/cli/blob/trunk/pkg/cmd/extension/command.go)
the check is an exact string comparison:

```go
if args[0] == "." {
    if pinFlag != "" {
        return fmt.Errorf("local extensions cannot be pinned")
    }
    ...
}
repo, err := ghrepo.FromFullName(args[0])
```

There is no `./foo` and no `/abs/path`. gh sidesteps the ambiguity by
making the local form a single reserved token — the cheapest possible
answer, at the cost of "you can only install the cwd". It echoes the raw
argument back (`✓ Installed extension owner/gh-foo`), not the resolved
form.

Note also `--pin`:

> The `--pin` flag may be used to specify a tag or commit for binary and
> script extensions respectively; the latest version is used otherwise.

Pinning is opt-in and defaults to "latest". Wormhole's pin is mandatory.

### asdf / mise — arity, not syntax

<https://asdf-vm.com/manage/plugins.html>

```
asdf plugin add <name> <git-url>
asdf plugin add <name>
```

One argument means "look this short name up in the plugin registry"; two
means "here is the explicit git URL". No ambiguity is possible. asdf then
recommends the explicit form:

> **Prefer the longer `git-url` method as it is independent of the
> short-name repo.**

Both tools expose the resolution after the fact:

```
$ asdf plugin list --urls
java            https://github.com/halcyon/asdf-java.git
```

```
$ mise plugins ls --urls
1password    https://github.com/mise-plugins/mise-1password-cli.git  HEAD f5d5aab
```

mise adds sniffing on top of the two-positional design and documents it
only in an example comment
(<https://mise.jdx.dev/cli/plugins/install.html>):

```
# install the poetry plugin using the git url only
# (poetry is inferred from the url)
$ mise plugins install https://github.com/mise-plugins/mise-poetry.git
```

mise's tool references, by contrast, use explicit `backend:tool` prefixes
(`npm:prettier`, `aqua:aws/aws-cli`) with the registry short name
documented as a pure alias over the prefixed form — the same shape as
Terraform's `git::` (<https://mise.jdx.dev/registry.html>).

### uv and pipx — the flag exists because the positional is a *command*

`uvx` takes a command name, so the package needs `--from`
(<https://docs.astral.sh/uv/guides/tools/>):

> **`--from from`** — Use the given package to provide the command. By
> default, the package name is assumed to match the command name.

> The `--from` option can also be used to install from alternative sources.
> For example, to pull from git: `uvx --from git+https://github.com/httpie/cli httpie`

`uv tool install` takes a package, so it needs no flag:

> Additionally, package versions can be included without `--from` … And,
> similarly, for package sources: `uv tool install git+https://github.com/httpie/cli`

`pipx run` has the same split — `--spec` and `--path`:

> **`--path`** - Interpret app name as a local path
>
> **`--spec SPEC`** - The package name or specific installation source
> passed to pip.

**uv is the only tool in this survey that asks the user when the argument
is ambiguous.** From
[`crates/uv-requirements/src/sources.rs`](https://github.com/astral-sh/uv/blob/main/crates/uv-requirements/src/sources.rs):

```rust
// If the user provided a `requirements.txt` file without `-r` (as in
// `uv pip install requirements.txt`), prompt them to correct it.
if (name.ends_with(".txt") || name.ends_with(".in")) && Path::new(&name).is_file() {
    let prompt = format!(
        "`{name}` looks like a local requirements file but was passed as a package name. Did you mean `-r {name}`?"
    );
```

pipx's precedence rule appears **only in a source comment**, never in the
docs
([`src/pipx/package_specifier.py`](https://github.com/pypa/pipx/blob/main/src/pipx/package_specifier.py)):

```python
# Match pip's PyPI precedence by checking local paths only for names that PyPI rejects.
```

with a final override — a bare name that is also an existing directory
resolves to the **path**:

```python
if valid_pep508 and valid_local_path:
    # It is a valid local path without "./"
    # Use valid_local_path
    valid_pep508 = None
```

pipx logs the resolved form only at `--verbose`
(`logger.info("cleaned package spec: %s", package_or_url)`).

### docker build — no disambiguation rule documented at all

<https://docs.docker.com/build/concepts/context/>

> ```
> $ docker build [OPTIONS] PATH | URL | -
>                          ^^^^^^^^^^^^^^
> ```
>
> You can pass any of the following inputs as the context for a build:
>
> - The relative or absolute path to a local directory
> - A remote URL of a Git repository, tarball, or plain-text file
> - A plain-text file or tarball piped to the `docker build` command
>   through standard input

The docs enumerate the accepted *types* and describe each. They never say
how the argument is classified. No "must begin with", no scheme table, no
precedence order. This is the least-documented case in the survey.

Docker's *named* contexts flag is the contrast: `--build-context
name=VALUE` uses explicit type prefixes (`docker-image://`,
`oci-layout://`).

### What this implies for wormhole

- Wormhole's three-form `--role` is squarely in the "bare = installed
  name, marked = path" family, with a transport check bolted on front. The
  family exists and is documented by Nix and Terraform. Wormhole is not
  inventing the problem.
- Both members of that family also ship a type-forcing prefix. Wormhole
  has one for remote (`github:`, `file://`) and none for the other two —
  there is no way to say "this is definitely an installed name" or "this
  is definitely a path".
- Only Nix reliably tells the user which branch it took, and it does so in
  the failure path. Wormhole already prints `manifest: role at ./x` /
  `manifest: role <name> (…)` / `manifest: role <name> pinned to <sha>` on
  the success path, which is more than any tool here does.
- The two zero-ambiguity designs (cargo's flags, asdf's arity) both cost a
  word of typing and buy the ability to never be wrong.

---

## 2. Install-before-use vs run-direct

| Tool | Install required? | Direct-run? | Gate before fetching remote code |
|---|---|---|---|
| `nix run` / `nix profile add` | No | Yes | none |
| `npx` / `npm i -g` | No | Yes | **interactive prompt by default** |
| `bunx` | No | Yes | none |
| `uvx` / `uv tool install` | No | Yes | none |
| `go run @v` / `go install @v` | No | Yes | none |
| `gh ext exec` / `gh ext install` | **Yes** | **No** | install *is* the gate |
| `pipx run` / `pipx install` | No | Yes | none |
| `docker run` / `docker pull` | No | Yes | none (`--pull=never` opts out) |

### uv states the principle most clearly, and it argues for run-direct

<https://docs.astral.sh/uv/concepts/tools/> — section "Execution vs
installation":

> **In most cases, executing a tool with `uvx` is more appropriate than
> installing the tool. Installing the tool is useful if you need the tool
> to be available to other programs on your system**, e.g., if some script
> you do not control requires the tool, or if you are in a Docker image and
> want to make the tool available to users.

> Because it is very common to run tools without installing them, a `uvx`
> alias is provided for `uv tool run` — the two commands are exactly
> equivalent.

The lifecycle difference:

> When running a tool with `uvx`, a virtual environment is stored in the uv
> cache directory and is treated as disposable … The environment is only
> cached to reduce the overhead of repeated invocations.

> When installing a tool with `uv tool install`, a virtual environment is
> created in the uv tools directory. The environment will not be removed
> unless the tool is uninstalled.

The reading: **install exists to serve third parties, not the human at the
keyboard.** That is a different justification from wormhole's, which is
about scriptability and about getting the human decision behind you before
launch.

### npx is the only tool that gates on a human, and it says why

<https://docs.npmjs.com/cli/v11/commands/npx>

> **To prevent security and user-experience problems from mistyping package
> names, `npx` prompts before installing anything.** Suppress this prompt
> with the `-y` or `--yes` option.

<https://docs.npmjs.com/cli/v11/commands/npm-exec>

> If any requested packages are not present in the local project
> dependencies, then a prompt is printed, which can be suppressed by
> providing either `--yes` or `--no`. **When standard input is not a TTY or
> a CI environment is detected, `--yes` is assumed.**

That last sentence is the important one, and it is the exact opposite of
wormhole's stated rule ("a question nobody can see is never treated as
answered"). npx resolves the no-TTY case by *assuming yes*; wormhole
resolves it by *refusing*. Both are defensible; they are not the same, and
npx's choice makes its security rationale evaporate in exactly the
environment where a typosquat is least likely to be noticed.

`bunx` is the same shape with no gate at all
(<https://bun.com/docs/cli/bunx>):

> As with `npx`, `bunx` checks for a locally installed package first, then
> falls back to auto-installing it from `npm`.

No prompt, no warning, no rationale.

### gh is the only tool that mandates install, and its reason is trust

<https://cli.github.com/manual/gh_extension>

> **Extensions are not verified, signed, or endorsed by GitHub. When you
> install or upgrade an extension, you are trusting its publisher. It is
> your responsibility to review the source and provenance of any extension
> before use.**

`gh extension exec` is **not** a run-direct mode — that assumption is
wrong. Its short description in
[`pkg/cmd/extension/command.go`](https://github.com/cli/cli/blob/trunk/pkg/cmd/extension/command.go)
is `"Execute an installed extension"`, and the long help says:

> Execute an extension using the short name. … You can use this command
> when the short name conflicts with a core gh command.

It is a name-disambiguation escape hatch for already-installed extensions.
It cannot fetch anything.

### nix run

<https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-run.html>

> `nix run` builds and runs *installable*, which must evaluate to an *app*
> or a regular Nix derivation.

That is the whole Description. **The page does not contain the words
"without installing", "ephemeral", or "ad hoc"** — the no-install property
is implied by the absence of any profile mutation, not stated. The
ephemeral rationale is stated in a tutorial instead
(<https://nix.dev/tutorials/first-steps/ad-hoc-shell-environments.html>):

> In a Nix shell environment, you can immediately use any program packaged
> with Nix, **without installing it permanently**.

Profiles exist for the persistent case
(<https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-profile.html>):

> A Nix profile is a set of packages that can be installed and upgraded
> independently from each other. Nix profiles are versioned, allowing them
> to be rolled back easily.

`nix profile install` is now an alias for `nix profile add`; the
`nix3-profile-install.html` and `nix3-shell.html` manual pages both 404.

### go — both modes carry the same justification sentence

<https://pkg.go.dev/cmd/go>

> If the package argument has a version suffix (like @latest or @v1.0.0),
> "go run" builds the program in module-aware mode, ignoring the go.mod
> file … **This is useful for running programs without affecting the
> dependencies of the main module.**

> If the arguments have version suffixes … "go install" builds packages in
> module-aware mode, ignoring the go.mod file … **This is useful for
> installing executables without affecting the dependencies of the main
> module.**

The rationale is isolation from the local project, not persistence.

### pipx — and the reproducibility hazard of a cached ephemeral run

<https://pipx.pypa.io/stable/>

> **Run the latest version of any app in a temporary environment with
> `pipx run`, without installing it first.**

<https://pipx.pypa.io/latest/explanation/how-pipx-works.html>

> The cache key is a hash of the package name, spec, Python version, and
> pip arguments; **cached environments expire after 14 days**, after which
> the next run rebuilds against the latest release.

That is the sharpest primary-source evidence in this survey that an
ephemeral mode is not reproducible: the same `pipx run foo` silently
changes version after 14 days.

uv has the same hazard, documented:

> `uvx` will use the latest available version of the requested tool _on the
> first invocation_. After that, `uvx` will use the cached version of the
> tool unless a different version is requested, the cache is pruned, or the
> cache is refreshed.

### docker — the only tool with a first-class opt-out of implicit fetching

<https://docs.docker.com/reference/cli/docker/container/run/>

> The `docker run` command runs a command in a new container, **pulling the
> image if needed** and starting the container.

> The default (`missing`) is to only pull the image if it's not present in
> the daemon's image cache. This default allows you to run images that only
> exist locally … and reduces networking.

> The `never` option disables (implicit) pulling images when creating
> containers, and only uses images that are available in the image cache.
> If the specified image is not found, an error is produced, and the
> container is not created. **This option is useful in situations where
> networking is not available, or to prevent images from being pulled
> implicitly when creating containers.**

### What this implies for wormhole

- Wormhole's "a launch never fetches and never asks" is a stronger
  position than any tool here takes. gh is the only other tool that
  mandates install, and it does it for the same reason (install is where
  publisher trust is accepted) but does not also forbid fetching at run
  time — it simply has no run-direct path to forbid it in.
- Wormhole's escape hatch (`--role github:o/r@sha` fetches and asks *when
  a TTY is present*) is npx's design. npx's own docs show where it leads:
  the gate has to decide what to do without a TTY, and npx chose "assume
  yes". Wormhole chose "refuse". The refusal is the safer half of a
  genuine fork in the road, not an obvious default.
- The ephemeral caches in pipx and uv drift. Wormhole's checkouts are
  named by commit and never modified, so it has no equivalent hazard —
  the `@<sha>` requirement is what buys that.

---

## 3. Approval and trust for third-party recipes that execute shell

| Tool | Prompt? | Keyed to | Re-approve when content changes? |
|---|---|---|---|
| **direnv** `allow` | No prompt — hard block + instruction | **SHA-256(abs path + contents)** | **Yes, structurally** |
| **direnv** `deny` | — | SHA-256(abs path) only | No — sticky across edits |
| **mise** (default) | Yes, interactive | path (symlink named by path hash) | **No** |
| **mise** (`paranoid`) | Yes | path + SHA-256(contents) | Yes |
| **VS Code** Workspace Trust | Yes, on folder open | folder path / parent folder path | No |
| **Dev Containers** | Folder trust only | folder path | No |
| **devcontainer Features** | **No** | name + mutable tag (digest possible) | No |
| **Nix flakes** `nixConfig` | Yes, per setting | **(setting name, setting value)** | Only if the value changes |
| **cargo install** | **No, by explicit policy** | nothing | n/a |
| **Homebrew** `brew trust` | No prompt — fails until trusted | **tap name or item name** | **No** |
| **Terraform** | **No** | `h1:` SHA-256 of package contents, auto-recorded | errors, but never asked consent |
| **git hooks** | **No** | nothing — payload never transferred | n/a |

### direnv — the canonical content-digest model

The approval token **is** the digest. From
[`internal/cmd/rc.go`](https://github.com/direnv/direnv/blob/master/internal/cmd/rc.go):

```go
func fileHash(path string) (hash string, err error) {
	if path, err = filepath.Abs(path); err != nil {
		return
	}
	fd, err := os.Open(path)
	...
	hasher := sha256.New()
	_, err = hasher.Write([]byte(path + "\n"))
	...
	if _, err = io.Copy(hasher, fd); err != nil {
		return
	}
	return fmt.Sprintf("%x", hasher.Sum(nil)), nil
}
```

```go
allowPath := filepath.Join(config.AllowDir(), fileHash)
```

and the check is a bare `stat`:

```go
// Allowed checks if the RC file has been granted loading
func (rc *RC) Allowed() AllowStatus {
	...
	// happy path is if this envrc has been explicitly allowed, O(1)ish common case
	_, err = os.Stat(rc.allowPath)
	if err == nil {
		return Allowed
	}
	...
	return NotAllowed
}
```

Edit one byte, get a different digest, address a file that does not exist,
be un-approved. **Nothing has to be invalidated, expired, or garbage
collected** — revocation-on-change falls out of content addressing for
free. The blocked message is a constant in the same file:

```go
const notAllowed = "%s is blocked. Run `direnv allow` to approve its content"
```

The man page (<https://direnv.net/man/direnv.1.html>) gives the rationale:

> This is the security mechanism to avoid loading new files automatically.
> Otherwise any git repo that you pull, or tar archive that you unpack,
> would be able to wipe your hard drive once you `cd` into it.

> `$XDG_DATA_HOME/direnv/allow` — Records which `.envrc` files have been
> `direnv allow`ed.

**A design detail worth stealing: the asymmetry.** *allow* is keyed to
path+content; *deny* is keyed to path only (`pathHash`, no file body). So a
deny survives edits and an allow does not. Both directions fail closed.

direnv's own escape hatch, and its documented danger
(<https://direnv.net/man/direnv.toml.1.html>):

> Specifying whitelist directives marks specific directory hierarchies or
> specific directories as "trusted" … **This feature should be used with
> great care**, as anyone with the ability to write files to that directory
> (including collaborators on VCS repositories) will be able to execute
> arbitrary code on your computer.

> If any of the strings in this list are a prefix of an .envrc file's
> absolute path, that file will be implicitly allowed, **regardless of
> contents or past usage of `direnv allow` or `direnv deny`**.

### mise — correction: NOT content-keyed by default

<https://mise.jdx.dev/cli/trust.html>

> Marks a config file as trusted. This means mise is allowed to parse the
> file when it needs to read config that may execute code or affect the
> environment.

> In normal mode, commands that execute project-defined behavior
> (`mise run`, naked task invocations …, `mise install`, `mise exec`, and
> `mise watch`) **automatically trust their active config**. **Paranoid
> mode requires explicit, content-bound trust for every non-global
> config.**

From
[`src/config/config_file/mod.rs`](https://github.com/jdx/mise/blob/main/src/config/config_file/mod.rs):

```rust
pub(crate) fn trust(path: &Path) -> Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    if !hashed_path.exists() {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink_or_file(path.canonicalize()?.as_path(), &hashed_path)?;
    }
    if Settings::get().paranoid {
        let trust_hash_path = with_appended_extension(&hashed_path, "hash");
        let hash = hash::file_hash_sha256(path, None)?;
        file::write(trust_hash_path, hash)?;
    }
    Ok(())
}
```

`trust_path` hashes the **path**, not the contents. Default mode writes a
symlink named by the path hash and nothing else — **editing the config does
not revoke trust.** Only `paranoid` writes the content-hash sidecar and
compares it. And in the default branch:

```rust
} else if cfg!(test) || ci_info::is_ci() {
    // in tests/CI we trust everything
    return true;
}
```

mise trusts everything in CI.

Trust is also inherited: "All descendant config files are **implicitly
trusted** when the root is trusted"
(<https://mise.jdx.dev/configuration.html>).

### VS Code Workspace Trust — folder-path keyed

<https://code.visualstudio.com/docs/editing/workspaces/workspace-trust>

> The Workspace Trust feature lets you decide whether code in your project
> folder can be executed by VS Code and extensions without your explicit
> approval.

> When you open a new, unfamiliar folder, VS Code opens it in Restricted
> Mode to prevent automatic code execution while you review the contents.

The rationale for tasks is the closest analogue to a shared role:

> VS Code tasks can run scripts and tool binaries. Because task definitions
> are defined in the workspace `.vscode` folder, they are part of the
> committed source code for a repo, and shared to every user of that repo.
> If someone would create a malicious task, it could be unknowingly run by
> anyone who cloned that repository.

Keying, verbatim:

> When you trust a folder, it is added to the **Trusted Folders &
> Workspaces** list

> When you trust a parent folder, all subfolders are trusted, which enables
> you to control Workspace Trust via a repository's location on disk.

And the documented limit:

> Workspace Trust can't prevent a malicious extension from executing code
> and ignoring **Restricted Mode**.

For Dev Containers, the folder trust prompt **is** the whole gate
(<https://code.visualstudio.com/docs/devcontainers/containers>):

> You will be asked to trust the local (or WSL) folder before the window
> reloads.

> you are asked to confirm that cloning a repository means you trust the
> repository. **This is only confirmed once.**

There is no separate prompt before `postCreateCommand`, `onCreateCommand`
or the Dockerfile build, and trust is not re-checked when
`devcontainer.json` changes.

### devcontainer Features — digests exist, but for identity, not trust

<https://containers.dev/implementors/features/>

> The `install.sh` script for each Feature should be executed as `root`
> during a container image build.

> **Note:** The `:latest` version annotation is added implicitly if
> omitted.

The spec defines Feature equality **by digest**:

> **For Features published to an OCI registry**, two Feature are identical
> if their manifest digests are equal, and the options executed against the
> Feature are equal … Identical manifest digests implies that the tgz
> contents of the Feature and its entire `devcontainer-feature.json` are
> identical.

But digests are used for dedup and ordering, not for permission. The
default reference form is a mutable tag, and there is no approval prompt
before `install.sh` runs as root. **UNVERIFIED** that any implementation
prompts.

### Nix flakes — keyed to (setting name, setting value)

<https://nix.dev/manual/nix/stable/command-ref/conf-file.html>

> `accept-flake-config` — Whether to accept Nix configuration settings from
> a flake without prompting. **Default:** `false`

From [`src/libflake/config.cc`](https://github.com/NixOS/nix/blob/master/src/libflake/config.cc):

```cpp
// setting name -> setting value -> allow or ignore.
typedef std::map<std::string, std::map<std::string, bool>> TrustedList;

static std::filesystem::path trustedListPath()
{
    return getDataDir() / "trusted-settings.json";
}
```

The approval is keyed to a `(name, value)` pair, not to the flake, not to a
file hash, not to a path. Changing the *value* re-prompts. A setting
approved for flake A is silently reused for flake B with the same name and
value.

The `trusted-users` warning is the sharpest statement in the Nix manual of
what these gates are protecting:

> Adding a user to `trusted-users` is essentially equivalent to giving that
> user root access to the system.

Nix does have a content digest — `flake.lock`'s `narHash` — but it governs
reproducibility of inputs, not permission to execute. The two are
decoupled.

### cargo — no gate, and Rust says so on purpose

<https://doc.rust-lang.org/cargo/reference/build-scripts.html>

> Placing a file named `build.rs` in the root of a package will cause Cargo
> to compile that script and execute it just before building the package.

Neither the build-scripts page nor the `cargo install` page mentions any
prompt, confirmation, trust check, or sandbox. The Rust security policy
(<https://www.rust-lang.org/policies/security>) makes it official:

> Unless otherwise noted, all components of the Rust toolchain … assume
> that the user's source code and dependencies are fully trusted, reviewed
> and contain no malicious code.

> **We do not consider attacks caused by compiling or analyzing malicious
> projects or dependencies a security vulnerability.**

### Homebrew — name-keyed, and candid that it covers future content

<https://docs.brew.sh/Taps>

> Code in a tap can run with your user's privileges. Read Tap Trust before
> using a non-official tap.

> Tapping a repository does not grant whole-tap trust. Install a fully
> qualified item to trust only that item

<https://docs.brew.sh/Tap-Trust>

> Formulae, casks and external commands are executable package definitions,
> not plain metadata. Homebrew sometimes needs to evaluate Ruby code from a
> tap to resolve dependencies, discover packages or run commands. Trusting
> a tap means accepting that its code may run with your user's privileges
> whenever Homebrew loads it.

> Prefer trusting the specific formula, cask or command you need. **Trust a
> whole tap only when you accept all current and future formulae, casks and
> external commands from that tap.**

> Commands that need to load an untrusted tap or item will fail until the
> relevant trust is granted.

Homebrew fails rather than prompting — the same shape as wormhole's
"refuses and names the `role add` that fixes it".

Trust is invalidated on one specific event
(<https://docs.brew.sh/Homebrew-Security-and-Supply-Chain>):

> When GitHub redirects a tap after its owner or repository is renamed,
> Homebrew follows the verified redirect, retargets the local tap to the
> new canonical remote and **invalidates trust entries for the old tap name
> rather than silently carrying trust across.**

And the most useful sentence in this whole section, for anyone whose design
rests on "show the user the recipe and let them read it":

> **Ruby cannot be safely inspected without executing it.**

Homebrew does content-pin the *downloads*, separately from trusting the
*code*:

> A formula pins each download to an explicit `sha256` checksum that lives
> in the formula file. … Homebrew refuses to install a download whose
> contents do not match.

### Terraform — trust on first use, never consent

<https://developer.hashicorp.com/terraform/language/files/dependency-lock>

> **This checksum verification is intended to represent a _trust on first
> use_ approach.**

> `h1:` a mnemonic for 'hash scheme 1' … Hash scheme 1 is also a SHA256
> hash, but is one computed from the _contents_ of the provider
> distribution package.

> **At present, the dependency lock file tracks only _provider_
> dependencies. Terraform does not remember version selections for remote
> modules.**

That last sentence matters: the thing most analogous to a wormhole role — a
remote module, which can carry arbitrary provisioner shell — is **not
pinned at all** by Terraform's lock file.

### git hooks — trust by omission

<https://git-scm.com/docs/githooks>

> Hooks are programs you can place in a hooks directory to trigger actions
> at certain points in git's execution. **Hooks that don't have the
> executable bit set are ignored.**

<https://git-scm.com/docs/git-init>

> **The sample hooks are all disabled by default. To enable one of the
> sample hooks rename it by removing its `.sample` suffix.**

There is no approval, no hash, no prompt. The payload never arrives: hooks
live in `$GIT_DIR/hooks`, which is populated only from a *local* template
directory, never from the remote.

**Flagged as partly unverified:** the direct sentence "It's important to
note that client-side hooks are **not** copied when you clone a repository"
is from the Pro Git book on git-scm.com
(<https://git-scm.com/book/en/v2/Customizing-Git-Git-Hooks>), which is
official git-scm.com content but not the man pages. The man pages establish
it only structurally.

### Related: two other content-keyed trust precedents

**ssh known_hosts** (<https://man.openbsd.org/ssh_config.5>) is TOFU keyed
to the host key:

> If this flag is set to `ask` (the default), new host keys will be added to
> the user known host files only after the user has confirmed that is what
> they really want to do, **and ssh will refuse to connect to hosts whose
> host key has changed.**

**GitHub Actions** is the closest published guidance to wormhole's
mandatory 40-hex pin
(<https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions>):

> **Pinning an action to a full-length commit SHA is currently the only way
> to use an action as an immutable release.**

> Pinning to a particular SHA helps mitigate the risk of a bad actor adding
> a backdoor to the action's repository, as they would need to generate a
> SHA-1 collision.

> Although pinning to a commit SHA is the most secure option, specifying a
> tag is more convenient and is widely used. … Note that there is risk to
> this approach even if you trust the author, because a tag can be moved or
> deleted if a bad actor gains access to the repository.

### What this implies for wormhole

- Wormhole's approval is keyed to the commit sha
  (`paths::approval_file(data_home, sha)`), which is a content digest of
  the whole tree. It sits in the same family as direnv and is strictly
  stronger than mise's default, VS Code's, and Homebrew's — all of which
  are name- or path-keyed and survive content changes by design.
- Three distinct models are visible and they should not be averaged:
  **content-addressed consent** (direnv, mise-paranoid, wormhole) where
  change-invalidation is structural; **name-scoped consent** (Homebrew,
  VS Code, mise default) which explicitly covers future content;
  **content pinning without consent** (Terraform TOFU, `flake.lock`,
  devcontainer digests) which detects drift but never asks.
- Two quotes worth keeping for the docs: Rust's "we do not consider attacks
  caused by compiling … malicious dependencies a security vulnerability",
  and Homebrew's "Ruby cannot be safely inspected without executing it".
  The second is a direct argument that showing the recipe is not a
  substitute for gating it — wormhole's TOML is more inspectable than Ruby,
  but the `[image] build` lines are not.
- Homebrew's *failing* rather than prompting on untrusted load is the same
  shape as wormhole's refusal-with-a-fix-suggestion, and predates it.

---

## 4. Session and instance identity

| Tool | Identity keyed on | Resume-or-create in one command? |
|---|---|---|
| tmux | session id `$N` + user name | **Yes** — `new-session -A` |
| docker | 64-hex UUID; `--name` optional alias | No — `run` creates, `start` resumes |
| git worktree | path; admin dir named from basename, uniquified | No |
| devcontainer CLI | **`(local_folder, config_file)` labels** | Yes — reuse is automatic |
| toolbx | container name, defaulted from image+release | **Yes** — offers to create |
| distrobox | `--name`, default `my-distrobox` | Yes — `enter` |
| vagrant | per-project `.vagrant/` machine id; `config.vm.define` names | Yes — `vagrant up` |

### tmux — a name, an id, and a documented resume-or-create flag

<https://man.openbsd.org/tmux.1>

> Sessions, window and panes are each numbered with a unique ID; session
> IDs are prefixed with a '$', windows with a '@', and panes with a '%'.
> **These are unique and are unchanged for the life of the session, window
> or pane in the tmux server.**

Note "in the tmux server" — the id counter is a process-global in
[`session.c`](https://github.com/tmux/tmux/blob/master/session.c), so `$0`
is not stable across a server restart. Wormhole's box id, written into the
home directory name, is.

`new-session -A`:

> The `-A` flag makes `new-session` behave like `attach-session` if
> session-name already exists

`attach-session`:

> If run from outside `tmux`, attach to target-session in the current
> terminal. target-session must already exist — to create a new session,
> see the `new-session` command (**with `-A` to create or attach**).

And the resolution order for a target, which is the most permissive in the
survey:

> target-session is tried as, in order:
>
> 1. A session ID prefixed with a `$`.
> 2. An exact name of a session (as listed by the `list-sessions` command).
> 3. The start of a session name, for example '`mysess`' would match a
>    session named '`mysession`'.
> 4. A glob(7) pattern which is matched against the session name.

tmux keeps a stable machine id (`$0`, `$1`) *and* a human name, and lets
you select by either — with prefix and glob matching on the name. The
default name is the decimal session id, retried until unique
(`session.c`); name and id are allocated separately, so a session called
`mysess` still has a `$id`.

**An undocumented asymmetry worth knowing about.** `-t` uses the four-step
resolution above, but `new-session -A -s foo` looks the target up with
`session_find()`, which is an exact `strcmp`
([`cmd-new-session.c`](https://github.com/tmux/tmux/blob/master/cmd-new-session.c)):

```c
if (args_has(args, 'A')) {
        if (sname != NULL)
                as = session_find(sname);
```

So `attach -t foo` matches a session named `foobar` and `new-session -A -s
foo` does not. Two commands, two matching rules, one name. The man page
does not mention it. That is the cost of fuzzy selectors.

Without `-A`, a name collision is a hard error: `cmdq_error(item,
"duplicate session: %s", sname)`.

### docker — id is the identity, name is an alias

<https://docs.docker.com/engine/containers/run/>

> You can identify a container in three ways: UUID long identifier, UUID
> short identifier, [and] Name.

> The UUID identifier is a random ID assigned to the container by the
> daemon.

> **The daemon generates a random string name for containers
> automatically.** You can also define a custom name using the `--name`
> flag. Defining a `name` can be a handy way to add meaning to a container.

The split is hard: `docker run` **always** creates a new container;
`docker container start` — "Start one or more stopped containers" — is the
only way to resume. There is no `run -A`.

### git worktree — identity is the path, the admin name is derived from it

<https://git-scm.com/docs/git-worktree>

> Each linked worktree has a private sub-directory in the repository's
> `$GIT_DIR/worktrees` directory. **The private sub-directory's name is
> usually the base name of the linked worktree's path, possibly appended
> with a number to make it unique.** For example, when
> `$GIT_DIR=/path/main/.git` the command `git worktree add
> /path/other/test-next next` creates the linked worktree in
> `/path/other/test-next` and also creates a `$GIT_DIR/worktrees/test-next`
> directory (or `$GIT_DIR/worktrees/test-next1` if `test-next` is already
> taken).

"Basename of the path, uniquified with a number" is the cheapest scheme
that gives readable names without collisions. Note wormhole already does
the reverse: it names homes `<workspace>-<id>` where the id is opaque.

### devcontainer — identity is `(workspace folder, config file)`, and the spec has a formula for it

**First, an honest scoping note.** The reuse mechanism is *not* in the
normative spec. <https://containers.dev/implementors/spec/> and the spec
repo's `devcontainer-reference.md` say nothing about container reuse or
about the `devcontainer.local_folder` / `devcontainer.config_file` labels.
Reuse is implementation behaviour of the CLI and VS Code.

**But the spec does bless the approach by name**, in
[`docs/specs/devcontainer-id-variable.md`](https://github.com/devcontainers/spec/blob/main/docs/specs/devcontainer-id-variable.md):

> Implementations can choose how to compute this identifier. They must
> ensure that it is **unique among other dev containers on the same Docker
> host** and that it is **stable across rebuilds** of dev containers.

> ### Label-based Implementation
>
> The following assumes that a dev container can be identified among other
> dev containers on the same Docker host by a **set of labels on the
> container**. … **For example, if the dev container is based on a local
> folder, the label could be named `devcontainer.local_folder` and have the
> local folder's path as its value.**

> ### Label-based Computation
>
> - Input the labels as a JSON object with the object's keys being the
>   label names and the object's values being the labels' values.
>   - To ensure implementations get to the same result, the object keys
>     must be sorted and any optional whitespace outside of the keys and
>     values must be removed.
> - Compute a SHA-256 hash from the UTF-8 encoded input string.
> - Use a base-32 encoded representation left-padded with '0' to 52
>   characters as the result.

That is the most transferable primitive in this whole document: **a stable,
unique, alphanumeric instance id derived by hashing a canonicalized set of
identity fields.** Wormhole's twelve-hex box id is assigned rather than
derived, which is a different tradeoff — assigned ids let two boxes share
one `(workspace, role)`, derived ids cannot.

Now the implementation. From
[`src/spec-node/singleContainer.ts`](https://github.com/devcontainers/cli/blob/main/src/spec-node/singleContainer.ts):

```ts
export const hostFolderLabel = 'devcontainer.local_folder';
export const configFileLabel = 'devcontainer.config_file';
```

```ts
export async function findDevContainer(params, labels): Promise<ContainerDetails | undefined> {
	const ids = await listContainers(params, true, labels);
	const details = await inspectContainers(params, ids);
	return details.filter(container => container.State.Status !== 'removing')[0];
}
```

and the labels those are built from, in `src/spec-node/utils.ts`:

```ts
const oldLabels = [`${hostFolderLabel}=${normalizedWorkspaceFolder}`];
const newLabels = [...oldLabels, `${configFileLabel}=${normalizedConfigFile}`];
```

**A dev container is identified for reuse by the normalized workspace
folder path plus the normalized config file path.** That is exactly
wormhole's `(workspace, role)` tuple, arrived at independently. Note the
`oldLabels` / `newLabels` pair: the config-file half was *added later*,
with a live migration path that removes and rebuilds containers still
carrying only the old label. Direct evidence that folder alone stopped
being enough once one folder could have several recipes.

**Keying on a path has a canonicalization tax**, and the CLI paid it in
`utils.ts`:

```ts
// Normalize separators and dot segments, then explicitly lowercase the drive
// letter because devcontainer.local_folder / devcontainer.config_file labels
// should compare case-insensitively on Windows.
```

The reuse policy is exposed as flags
([`devContainersSpecCLI.ts`](https://github.com/devcontainers/cli/blob/main/src/spec-node/devContainersSpecCLI.ts)):

> `--id-label` — **Id label(s) of the format name=value. These will be set
> on the container and used to query for an existing container. If no
> `--id-label` is given, one will be inferred from the `--workspace-folder`
> path.**

> `--remove-existing-container` — Removes the dev container if it already
> exists.

> `--expect-existing-container` — Fail if the container does not exist.

`devcontainer up` is registered as "Create and run dev container" — one
verb for both branches, with explicit flags to force either.

The user-facing statement (<https://code.visualstudio.com/docs/devcontainers/containers>):

> You only have to build a dev container the first time you open it;
> opening the folder after the first successful build will be much quicker.

> From now on, when you open the project folder, VS Code will automatically
> pick up and reuse your dev container configuration.

Reuse is invisible; the escape hatch is an explicit **Dev Containers:
Rebuild Container** command.

### toolbx — resume-or-create, with an offer

From [`doc/toolbox-enter.1.md`](https://github.com/containers/toolbox/blob/main/doc/toolbox-enter.1.md):

> When invoked without any options, `toolbox enter` will try to enter the
> default Toolbx container for the host, or **if there's only one container
> available then it will use it**. On Fedora, the default container is known
> as `fedora-toolbox-N`, where N is the release of the host. **If there
> aren't any containers, `toolbox enter` will offer to create the default
> one for you.**

From [`doc/toolbox-create.1.md`](https://github.com/containers/toolbox/blob/main/doc/toolbox-create.1.md):

> By default, a Toolbx container is named after its corresponding image. If
> the image had a tag, then the tag is included in the name of the
> container, but it's separated by a hyphen, not a colon.

> A different name can be assigned by using the CONTAINER argument.

The "if there's only one, use it" rule is worth noting — it is a
disambiguation-by-uniqueness heuristic wormhole does not have.

Toolbx also runs a **two-level** scheme: a label marks membership in the
tool's universe, the name selects an instance.

> **Toolbx containers can be identified by the
> `com.github.containers.toolbox` label** with various Podman commands
> (like `podman inspect`) or by the presence of the `/run/.toolbxenv` file.

And `enter` is explicitly a start-then-exec:

> **A Toolbx container is an OCI container. Therefore, `toolbox enter` is
> analogous to a `podman start` followed by a `podman exec`.**

### distrobox — name only, and a fixed default

<https://distrobox.it/usage/distrobox-enter/>

> distrobox-enter takes care of entering the container with the name
> specified. Default command executed is your SHELL, but you can specify
> different shells or entire commands to execute.

> `--yes/-y`: non-interactive, auto-create the container if it does not
> exist

The docs leave the default name as an unexpanded shell placeholder; from
[`pkg/config/config.go`](https://github.com/89luca89/distrobox/blob/main/pkg/config/config.go):

```go
"container_name":           "my-distrobox",
```

A fixed constant — **not** derived from image, distro, or cwd. Entering a
stopped container starts it
([`pkg/containermanager/providers/podman.go`](https://github.com/89luca89/distrobox/blob/main/pkg/containermanager/providers/podman.go)):

```go
inspectResult, err := p.InspectContainer(ctx, options.ContainerName)
if err != nil || inspectResult.ContainerStatus != containermanager.RunningStatus {
        ...
        if err := p.startContainer(ctx, options.ContainerName, progress); err != nil {
```

and a missing one is offered
([`internal/cli/enter.go`](https://github.com/89luca89/distrobox/blob/main/internal/cli/enter.go)), with the prompt
`"Create it now, out of image %s?"`.

### vagrant — three layers of identity

Vagrant is the richest case in the survey, and the only one that makes an
instance addressable from outside its own project.

**Layer 1, logical name, scoped to the project**
(<https://developer.hashicorp.com/vagrant/docs/multi-machine>):

> Multiple machines are defined within the same project Vagrantfile using
> the `config.vm.define` method call.

> Commands that only make sense to target a single machine, such as
> `vagrant ssh`, now _require_ the name of the machine to control.

> If Vagrant sees a machine name within forward slashes, it assumes you are
> using a regular expression.

> You can also specify a _primary machine_. The primary machine will be the
> default machine used when a specific machine in a multi-machine
> environment is not specified. … **Only one primary machine may be
> specified.**

**Layer 2, provider resource id, on disk**
(<https://developer.hashicorp.com/vagrant/docs/plugins/providers>):

> In the process of creating and managing a machine, providers generally
> need to store some sort of state somewhere. **Vagrant provides each
> machine with a directory to store this state.** … **This allows the
> provider to track whether the machine is created, running, suspended,
> etc.** … It is important for providers to carefully manage all the
> contents of this directory. **Vagrant core itself does little to clean up
> this directory.**

The layout is not documented; from `lib/vagrant/environment.rb` it is
`.vagrant/machines/<name>/<provider>/id`, so the effective key is
`(project dir, name, provider)`. That is why:

> Vagrant currently restricts you to bringing up one provider per machine.
> … **you cannot back the _same machine_ with both VirtualBox and VMware
> Fusion.**

**Layer 3, a global index UUID**
(<https://developer.hashicorp.com/vagrant/docs/cli/global-status>):

> This command will tell you the state of all active Vagrant environments
> on the system for the currently logged in user.

> **The IDs in the output that look like `a1b2c3` can be used to control
> the Vagrant machine from anywhere on the system.** Any Vagrant command
> that takes a target machine (such as `up`, `halt`, `destroy`) can be used
> with this ID to control it.

The two selectors are documented as doing different jobs
(<https://developer.hashicorp.com/vagrant/docs/cli/up>):

> `name` - Name of machine defined in Vagrantfile. **Using name to specify
> the Vagrant machine to act on must be done from within a Vagrant project
> (directory where the Vagrantfile exists).**
>
> `id` - Machine id found with `vagrant global-status`. **Using id allows
> you to call `vagrant up id` from any directory.**

That is precisely wormhole's split between `wormhole box` (this workspace)
and `wormhole box --id <id>` / the panel (anywhere), stated in HashiCorp's
own words.

And the cautionary half:

> **This command does not actively verify the state of machines, and is
> instead based on a cache. Because of this, it is possible to see stale
> results** (machines say they're running but they're not). For example, if
> you restart your computer, Vagrant would not know. To prune the invalid
> entries, run global status with the `--prune` flag.

> `--prune` - Prunes invalid entries from the list. **This is much more
> time consuming than simply listing the entries.**

A cross-project registry of instance identities goes stale, and reconciling
it is expensive enough to be opt-in. Wormhole avoids that by asking "is
this box free?" with an `flock` — by *taking* the lock rather than reading
a list.

### Five identity keys, and nobody uses only one

| Key | Tools | Property |
|---|---|---|
| User-chosen name | tmux, docker, distrobox | mutable, collides, needs a namespace |
| Name derived from config | toolbx (`fedora-toolbox-41`) | deterministic, no registry, collides across intents |
| Filesystem path | git worktree, vagrant (project layer) | self-describing, breaks on move |
| Labels on the object | dev containers | multi-field, queryable, needs canonicalization |
| Opaque generated id | docker, tmux `$n`, vagrant index UUID | stable, needs a lookup table to be usable |

Every tool that survived pairs a **human selector** with a **machine
identity**: toolbx = label (membership) + name (selector); vagrant = name +
provider id + index UUID; docker = name + 64-hex id; tmux = name + `$id`.

Resume-or-create is the majority design, and it is always a flag or a verb,
never magic: single command always (`vagrant up`, `devcontainer up`,
`toolbox enter`, `distrobox enter`); opt-in flag (`tmux new-session -A`);
deliberately refused (`docker run` vs `docker start`, `git worktree add`).

### What this implies for wormhole

- `(workspace, role)` as the resume key is not idiosyncratic. The
  devcontainer CLI landed on the same tuple, and its `oldLabels` /
  `newLabels` migration shows the config-file half was added because the
  folder alone was insufficient.
- Every tool here pairs a machine identity with a human selector. Wormhole
  has the id (`a3f9c1e40b2d`) and a `NAME` column in `ps`, but no way for a
  user to set the name or select a box by it — `attach` and `--id` take the
  id only. tmux's four-step resolution is the far end of that spectrum, and
  its undocumented `-A` asymmetry is the price of going there.
- Both resume-or-create designs in the survey announce themselves: tmux
  makes it an explicit flag (`-A`), toolbx *asks* before creating. Wormhole
  makes it the default of a bare `wormhole box` and prints `(new)` or
  `(resumed)` after the fact, which is the informative-but-silent middle.
- The devcontainer spec's `base32(SHA-256(sorted-JSON(labels)))` is the one
  reusable primitive here — a derived id from a canonical key set. Wormhole
  assigns ids instead, which is what lets one `(workspace, role)` hold
  several boxes. Different goal, not a worse one.
- Vagrant's `--prune` and its stale-cache warning are the cautionary case
  for any list-based liveness check, and the two tools keying on paths
  (git worktree, devcontainer) both had to add repair or normalization
  machinery afterwards. Wormhole's `flock` has no equivalent staleness.

---

## 5. Naming and vocabulary

### clig.dev — Command Line Interface Guidelines

<https://clig.dev/>

**On multiple forms in one argument** — the guideline is to avoid the
situation:

> **Prefer flags to args.** It's a bit more typing, but it makes it much
> clearer what is going on. It also makes it easier to make changes to how
> you accept input in the future.

**On ambiguity between commands:**

> **Don't have ambiguous or similarly-named commands.** For example, having
> two subcommands called "update" and "upgrade" is quite confusing.

**On subcommands:**

> **Be consistent across subcommands.** Use the same flag names for the
> same things, have similar output formatting, etc.

> **Use consistent names for multiple levels of subcommand.** … Either
> `noun verb` or `verb noun` ordering works, but `noun verb` seems to be
> more common.

**On aliases — directly relevant to installed role names:**

> **Don't allow arbitrary abbreviations of subcommands.** For example, say
> your command has an `install` subcommand. When you added it, you wanted
> to save users some typing, so you allowed them to type any non-ambiguous
> prefix … Now you're stuck: you can't add any more commands beginning with
> `i`, because there are scripts out there that assume `i` means `install`.
>
> **There's nothing wrong with aliases—saving on typing is good—but they
> should be explicit and remain stable.**

> **Don't have a catch-all subcommand.** … This has a serious drawback,
> though: now you can never add a subcommand named `echo`—or _anything at
> all_—without risking breaking existing usages.

**On printing what was resolved:**

> **If you change state, tell the user.** When a command changes the state
> of a system, it's especially valuable to explain what has just happened,
> so the user can model the state of the system in their head.

> **Suggest commands the user should run.** When several commands form a
> workflow, suggesting to the user commands they can run next helps them
> learn.

> **Catch errors and rewrite them for humans.** Example: "Can't write to
> file.txt. You might need to make it writable by running `chmod +w
> file.txt`."

> **Make it easy to see the current state of the system.** If your program
> does a lot of complex state changes and it is not immediately visible in
> the filesystem, make sure you make this easy to view.

**On fetching — the strongest clig.dev quote for wormhole's launch rule:**

> **Actions crossing the boundary of the program's internal world should
> usually be explicit.** This includes things like:
>
> - Reading or writing files that the user didn't explicitly pass as
>   arguments (unless those files are storing internal program state, such
>   as a cache).
> - **Talking to a remote server, e.g. to download a file.**

**On prompts:**

> **Only use prompts or interactive elements if `stdin` is an interactive
> terminal (a TTY).** This is a pretty reliable way to tell whether you're
> piping data into a command or whether it's being run in a script, in
> which case a prompt won't work and you should throw an error telling the
> user what flag to pass.

> **Never _require_ a prompt.** Always provide a way of passing input with
> flags or arguments. If `stdin` is not an interactive terminal, skip
> prompting and just require those flags/args.

> **Confirm before doing anything dangerous.** A common convention is to
> prompt for the user to type `y` or `yes` if running interactively, or
> requiring them to pass `-f` or `--force` otherwise.

Three severity levels are given: **mild** (small local changes like
deleting a file), **moderate** (bigger or remote deletions, requiring
confirmation), **severe** (requiring non-trivial input such as typing the
name of the thing being deleted, or `--confirm="name-of-thing"`).

Note the tension with wormhole: clig.dev says "skip prompting and just
require those flags/args" when stdin is not a TTY. Wormhole refuses
instead, on the grounds that there is no flag that could stand in for
reading a role's recipe. That is a deliberate departure and worth naming as
one.

**On precedence:**

> **Apply configuration parameters in order of precedence.** Here is the
> precedence for config parameters, from highest to lowest: Flags, The
> running shell's environment variables, Project-level configuration (e.g.
> `.env`), User-level configuration, System wide configuration

Wormhole's "an explicit `--role` beats the workspace's own `wormhole.toml`"
is exactly this ordering.

**On standard flag names**, the relevant ones here:

> `-n`, `--dry-run`: Dry run. Do not run the command, but describe the
> changes that would occur if the command were run. For example, `rsync`,
> `git add`.

> `-f`, `--force`: Force. For example, `rm -f` will force the removal of
> files, even if it thinks it does not have permission to do it. **This is
> also useful for commands which are doing something destructive that
> usually require user confirmation, but you want to force it to do that
> destructive action in a script.**

**clig.dev has no `--yes` guideline.** Its documented non-interactive
escapes are `-f`/`--force`, `--confirm="name-of-thing"`, and `--no-input`.
`--yes` is a convention from elsewhere.

**On consistency, and on breaking it deliberately:**

> Where possible, a CLI should follow patterns that already exist. That's
> what makes CLIs intuitive and guessable; that's what makes users
> efficient.
>
> That being said, sometimes consistency conflicts with ease of use. …
> **When following convention would compromise a program's usability, it
> might be time to break with it—but such a decision should be made with
> care.**

> It's ironic that this document implores you to follow existing patterns,
> right alongside advice that contradicts decades of command-line
> tradition. … **The time might come when you, too, have to break the
> rules. Do so with intention and clarity of purpose.**

### Docker — the canonical recipe/instance vocabulary

<https://github.com/docker/docs/blob/main/data/glossary.yaml>

> **image:** An image is a read-only template used to create containers. It
> typically includes a base operating system and application code packaged
> together using a Dockerfile.

> **container:** A container is a runnable instance of an image. You can
> start, stop, move, or delete a container using the Docker CLI or API.

Docker is actually a **four**-level split, and the fourth level is the one
wormhole is arguing about. From
<https://docs.docker.com/get-started/docker-concepts/building-images/build-tag-and-publish-an-image/>:

> `docker run sha256:9924dfd9350407b3df01d1a0e1033b1e543523ce7d5d5e2c83a724480ebe8f00`
>
> That name certainly isn't memorable, which is where tagging becomes
> useful.

> **TAG**: A custom, human-readable identifier that's typically used to
> identify different versions or variants of an image. If no tag is
> specified, `latest` is used by default.

So: **Dockerfile** (source recipe) → **image** (built, immutable,
content-addressed) → **tag** (a mutable human-memorable alias for that
content) → **container** (runnable instance). The alias layer is a separate
named concept, explicitly justified by "that name certainly isn't
memorable".

Wormhole's manifest → image → box maps onto the first, second and fourth.
An installed role name is the third — a human-memorable alias over a
content-addressed thing — and a role from a git repo is a fifth thing again:
a *portable* recipe not tied to any workspace.

Also from the glossary:

> **image:** … Images are versioned using tags and can be pushed to or
> pulled from a container registry like Docker Hub.

and (<https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/>):

> 1. Images are immutable. Once an image is created, it can't be modified.
>    You can only make a new image or add changes on top of it.

### Homebrew — a maintained terminology table that is exactly this split

<https://docs.brew.sh/Formula-Cookbook> ("Homebrew terminology")

| term | description | example |
|---|---|---|
| **formula** | Homebrew package definition that builds from upstream sources | `…/Formula/f/foo.rb` |
| **keg** | installation destination directory of a given **formula** version | `/opt/homebrew/Cellar/foo/0.1` |
| **rack** | directory containing one or more versioned **kegs** | `/opt/homebrew/Cellar/foo` |
| **keg-only** | a **formula** is *keg-only* if it is not symlinked into Homebrew's prefix | the `openjdk` formula |
| **opt prefix** | a symlink to the active version of a **keg** | `/opt/homebrew/opt/foo` |
| **tap** | directory (and usually Git repository) of **formulae**, **casks** and/or **external commands** | `…/Taps/homebrew/homebrew-core` |
| **bottle** | pre-built **keg** poured into a **rack** of the **Cellar** instead of building from upstream sources | `qt--6.5.1.ventura.bottle.tar.gz` |
| **tab** | information about a **keg**, e.g. whether it was poured from a **bottle** or built from source | `…/0.1/INSTALL_RECEIPT.json` |

This is the most complete answer in the survey to "what do you call the
recipe, the installed instance, and the alias". Every layer gets its own
noun:

- recipe → `formula`
- source of recipes → `tap`
- versioned installed instance → `keg`
- all instances of one recipe → `rack`
- **the alias for "the active one"** → `opt prefix`
- **installed but deliberately not aliased into the global namespace** →
  `keg-only`
- provenance record for an instance → `tab`

`keg-only` is worth noting on its own: a documented term for the case where
an instance exists but is *not* given a global name, precisely because
global names can collide.

### Kubernetes — name vs UID, split explicitly

<https://kubernetes.io/docs/concepts/overview/working-with-objects/names/>

> Each object in your cluster has a Name that is unique for that type of
> resource. Every Kubernetes object also has a UID that is unique across
> your whole cluster.

> **Names:** A client-provided string that refers to an object in a
> resource URL

> **UIDs:** A Kubernetes systems-generated string to uniquely identify
> objects. Every object created over the whole lifetime of a Kubernetes
> cluster has a distinct UID. **It is intended to distinguish between
> historical occurrences of similar entities.**

That last sentence is the cleanest statement anywhere of why an opaque id
is needed alongside a human name — which is precisely the job wormhole's
twelve-character box id does. The glossary is explicit that the two differ
in reusability:

> **Name:** … Only one object of a given kind can have a given name at a
> time. However, **if you delete the object, you can make a new object with
> the same name.**

> **UID:** … Every object created over the whole lifetime of a Kubernetes
> cluster has a distinct UID.

And there is a documented hazard of name reuse that maps directly onto
"resume the box for this role":

> In cases when objects represent a physical entity, like a Node
> representing a physical host, when the host is re-created under the same
> name without deleting and re-creating the Node, **Kubernetes treats the
> new host as the old one, which may lead to inconsistencies.**

Kubernetes also documents a naming *constraint* worth copying — names must
be RFC 1123 DNS subdomain names — and Terraform documents another:

> To ensure that workspace names are stored correctly and safely in all
> backends, **the name must be valid to use in a URL path segment without
> escaping.**

Wormhole's role-name rule (`letters, digits, - and _`) is in the same
family and already stated in the error text.

### Dev Containers spec

<https://containers.dev/implementors/spec/>

> A **development container** is a container in which a user can develop an
> application.

> Metadata can be stored in a JSON with Comments file called
> `devcontainer.json` today.

**The spec's own recipe→instances rule, verbatim** — this is the sentence
closest to wormhole's problem:

> **An environment is defined as a logical instance of one or more
> development containers, along with any needed side-car containers. An
> environment is based on one set of metadata that can be managed as a
> single unit. Users can create multiple environments from the same
> configuration metadata for different purposes.**

One set of metadata, many named environments. That is exactly "one role,
many boxes", written down normatively.

The spec keeps three distinct nouns that map onto wormhole's problem:
**dev container** (instance), **Feature**, and **Template**. It draws the
last two apart explicitly:

> Development Container **Features** are self-contained, shareable units of
> installation code and development container configuration.
> (<https://containers.dev/implementors/features/>)

> Development Container **Templates** are source files packaged together
> that encode configuration for a complete development environment. A
> Template can be used in a new or existing project…
> (<https://containers.dev/implementors/templates/>)

A Template is consumed *once*, to scaffold. A Feature is *composed* into an
existing config. Both share the same id rule — "must match the name of the
directory where the `devcontainer-feature.json` resides" — so the naming
mechanics stay uniform while the concepts stay separate.

Notably the spec has **no single word** for "a devcontainer.json that is
not tied to one folder", which is what a wormhole role is. Features and
Templates are the two halves it split that idea into, and a wormhole role
is arguably both at once: it scaffolds like a Template and it carries
install code like a Feature.

### Terraform — the clearest case of a project admitting it named something wrong

The rename is in the changelog, with the reason
(<https://github.com/hashicorp/terraform/blob/v0.10.0/CHANGELOG.md>):

> The `terraform env` family of commands have been renamed to `terraform
> workspace`, **in response to feedback that the previous naming was
> confusing due to collisions with other concepts of the same name.** The
> commands still work the same as they did before, and the `env`
> subcommand is still supported as an alias for backward compatibility.
> The `env` subcommand will be removed altogether in a future release, so
> it's recommended to update any automation or wrapper scripts that use
> these commands.

Note the migration mechanic: keep the old name as an explicit deprecated
alias, announce removal, tell people to update scripts. That is clig.dev's
"aliases … should be explicit and remain stable" applied to a rename.

**And the rename landed in a second collision.**

<https://developer.hashicorp.com/terraform/cli/workspaces>

> Workspaces in the Terraform CLI refer to separate instances of state data
> inside the same Terraform working directory.

> **They are distinctly different from workspaces in HCP Terraform, which
> each have their own Terraform configuration and function as separate
> working directories.**

> Workspaces let you quickly switch between multiple instances of a single
> configuration within its single backend. **They are not designed to solve
> all problems.**

> organizations commonly want to create a strong separation between
> multiple deployments … **CLI workspaces within a working directory use the
> same backend, so they are not a suitable isolation mechanism.**

The blunter version is on the HCP side
(<https://developer.hashicorp.com/terraform/cloud-docs/workspaces>):

> **Both HCP Terraform and Terraform CLI have features called workspaces,
> but they function differently.**

HashiCorp renamed `env` → `workspace` to escape one collision and landed
straight in another, which it has now been papering over with a "these are
different" note in every affected page for five-plus major versions.
**Renaming to escape a collision only works if the new name is not already
claimed elsewhere in your own product surface.**

Worth weighing against wormhole's own use of "workspace" for "the directory
you invoked in" — a word that already means something different in
Terraform CLI, in HCP Terraform, in VS Code, and in Cargo:

> **Workspace** — A workspace is a collection of one or more packages that
> share common dependency resolution (with a shared `Cargo.lock` lock
> file), output directory, and various settings such as profiles.
> (<https://doc.rust-lang.org/cargo/appendix/glossary.html>)

### Terraform modules — four terms, no overloading

<https://developer.hashicorp.com/terraform/language/modules/syntax> (v1.1.x
wording, which is the more precise one):

> To _call_ a module means to include the contents of that module into the
> configuration with specific values for its input variables. …
> A module that includes a `module` block like this is the _calling module_
> of the child module.
>
> **The label immediately after the `module` keyword is a local name, which
> the calling module can use to refer to this instance of the module.**

> `count` - Creates multiple instances of a module from a single `module`
> block.

*module* (the reusable definition) / *module call* (the block that invokes
it) / *local name* (the user-chosen label for **this instance**) / *module
instance* (what `count` multiplies). Four terms, no word doing two jobs.

### Nix — a distinct verb for each transition, and a self-flagged near-collision

<https://nix.dev/manual/nix/stable/glossary>

> **instantiate, instantiation** — Translate a *derivation expression* into
> a *store derivation*.

> **realise, realisation** — Ensure a *store path* is *valid*.

Source text → *instantiate* → recipe-as-store-object → *realise* → output.
Each stage has its own noun and each transition its own verb.

Nix also ships mutual disambiguation notes because two of its own terms are
one morpheme apart:

> **derivation path** — A store path which uniquely identifies a store
> derivation. … **Not to be confused with deriving path.**

> **deriving path** — Deriving paths are a way to refer to store objects
> that might not yet be realised. … **Not to be confused with derivation
> path.**

That is clig.dev's "don't have ambiguous or similarly-named commands"
problem, handled by permanent documentation rather than by renaming.

Nix also separates locking from pinning, which is the vocabulary wormhole
uses for its commit:

> **locking** — … creating a lock file, which maps each mutable evaluation
> input to an immutable reference, so that future evaluations resolve to
> the same immutable versions…

> **pinning** — Like locking, but a pin only locks a single input. … a pin
> serves the bottom-up purpose of fixing an input's reference on demand,
> whereas locking implies a top down approach where all pins are "coerced"
> into a single place.

By that definition wormhole's `@<sha>` is a **pin**, and CONTEXT.md uses
the word correctly.

### Cargo — a project that documents its own overloaded term rather than fixing it

<https://doc.rust-lang.org/cargo/appendix/glossary.html>

> **Target** — **The meaning of the term *target* depends on the context:**
>
> - **Cargo Target** — Cargo packages consist of *targets* which correspond
>   to artifacts that will be produced. …
> - **Target Directory** — Cargo places built artifacts in the *target*
>   directory. …
> - **Target Architecture** — The OS and machine architecture for the built
>   artifacts are typically referred to as a *target*.
> - **Target Triple** — A triple is a specific format for specifying a
>   target architecture. …

> **Crate** — … **Loosely, the term crate may refer to either the source
> code of the target or to the compiled artifact that the target
> produces.** It may also refer to a compressed package fetched from a
> registry.

One word, four meanings, documented rather than resolved. The honest
option, and the one that leaves every reader doing the disambiguation
forever.

### asdf and mise — resolution is exposed as a listing subcommand

Neither echoes resolution at the moment of use; both provide a way to ask
afterwards.

```
$ asdf plugin list --urls
java            https://github.com/halcyon/asdf-java.git
```

```
$ mise plugins ls --urls
1password    https://github.com/mise-plugins/mise-1password-cli.git  HEAD f5d5aab
```

mise documents its registry short names as *aliases*, not as a separate
kind of thing (<https://mise.jdx.dev/registry.html>):

> You can use these shorthands with `mise use`. This allows you to use a
> tool without needing to know the full name. … `mise use aws-cli` instead
> of `mise use aqua:aws/aws-cli`

That framing — the installed name is an alias for the fully-qualified
reference, nothing more — is the simplest available answer to "what is an
installed role, as distinct from the repo it came from".

### Projects admitting their naming was confusing, ranked

1. **Terraform**, changelog v0.10.0 — renamed `terraform env` →
   `terraform workspace` "in response to feedback that the previous naming
   was confusing due to collisions with other concepts of the same name",
   kept the old name as a deprecated alias.
2. **Terraform**, ongoing — "Both HCP Terraform and Terraform CLI have
   features called workspaces, but they function differently", plus a
   standing note in every affected page for five-plus major versions. The
   rename escaped one collision and created another.
3. **Cargo** — "The meaning of the term *target* depends on the context:",
   followed by four definitions; plus "Loosely, the term crate may refer to
   either the source code … or to the compiled artifact".
4. **Nix** — mutual "Not to be confused with" notes between `derivation
   path` and `deriving path`.
5. **Vagrant** — "A common misconception is that a namespace like 'ubuntu'
   represents the official space for Ubuntu boxes. This is untrue."
   (<https://developer.hashicorp.com/vagrant/docs/boxes>)

### What this implies for wormhole

- clig.dev's "Prefer flags to args" and "Don't have ambiguous or
  similarly-named commands" both point away from one argument that means
  three things. It has no guideline endorsing syntax-sniffing.
- clig.dev and wormhole disagree on the no-TTY case. clig.dev: skip the
  prompt, require a flag. wormhole: refuse, because no flag can stand in
  for reading a recipe. Worth stating that departure explicitly rather than
  letting it look like an oversight — clig.dev itself says to break rules
  "with intention and clarity of purpose".
- clig.dev's "Actions crossing the boundary of the program's internal world
  should usually be explicit … Talking to a remote server, e.g. to download
  a file" is a direct endorsement of "a launch never fetches", and equally
  a warning about the TTY exception that does.
- Homebrew's table is the model to compare CONTEXT.md against. It gives
  separate nouns to the recipe (`formula`), the versioned installed
  instance (`keg`), the alias for the active one (`opt prefix`), and the
  deliberately-unaliased case (`keg-only`). Wormhole's "role" currently
  covers the recipe, the installed alias, *and* the checked-out commit.
- The Kubernetes name/UID split and Docker's four-level
  Dockerfile→image→tag→container split are both vocabulary wormhole partly
  mirrors. The gaps: a wormhole box has a UID but no user-settable Name,
  and an installed role name is an alias with no word of its own.
- The devcontainer spec's "Users can create multiple environments from the
  same configuration metadata for different purposes" is the sentence
  wormhole's docs are reaching for when they say a folder holds as many
  boxes as you make.
- Terraform's "workspace" saga is the cautionary tale for reusing a common
  word. Wormhole's "workspace" means the invocation directory; Cargo's,
  Terraform CLI's, HCP Terraform's and VS Code's all mean something else.
  It is the wormhole term with the most existing collisions, and Terraform
  is the proof that renaming out of one collision is not automatically a
  win.

---

## Summary table

| Tool | Install required? | Direct-run? | Trust gate keyed to | Instance identity keyed to |
|---|---|---|---|---|
| **wormhole** (for reference) | Yes for remote (TTY exception) | Yes for path | **commit sha** (content digest) | `(workspace, role)` + 12-hex id |
| `cargo install` | Yes | No | **nothing** (explicit Rust policy) | n/a |
| `uvx` / `uv tool install` | No | Yes | nothing | n/a |
| `pipx run` / `pipx install` | No | Yes | nothing | n/a |
| `npx` | No | Yes | **interactive prompt**, auto-yes on no-TTY/CI | n/a |
| `bunx` | No | Yes | nothing | n/a |
| `gh extension install` | **Yes** | **No** | nothing (warning only); `--pin` optional | n/a |
| `nix run` / `nix profile add` | No | Yes | `(setting name, setting value)` for `nixConfig`; `narHash` for inputs | n/a |
| `go install pkg@v` | No | Yes | nothing | n/a |
| `asdf` / `mise` plugins | Yes | No | mise: **path** (content only in `paranoid`) | n/a |
| `docker build` / `run` | No | Yes | nothing | 64-hex UUID; `--name` alias, unique per daemon |
| Terraform modules | Yes (`init`) | No | **nothing for modules**; TOFU `h1:` digest for providers | n/a |
| Homebrew taps | Yes (`tap` + `trust`) | No | **tap name or item name** | n/a |
| direnv | n/a | n/a | **SHA-256(path + contents)** | n/a |
| VS Code / Dev Containers | n/a | n/a | **folder path** (or parent folder) | **`(local_folder, config_file)` labels** |
| devcontainer Features | n/a | n/a | nothing (digest form exists, unused for trust) | n/a |
| git hooks | n/a | n/a | **nothing** — payload never transferred | n/a |
| tmux | n/a | n/a | n/a | session id `$N` (server lifetime) + name |
| git worktree | n/a | n/a | n/a | path; admin dir from basename |
| toolbx | n/a | n/a | n/a | name derived from `(distro, release)` |
| distrobox | n/a | n/a | n/a | name; default constant `my-distrobox` |
| vagrant | n/a | n/a | n/a | `(project dir, name, provider)` + global index UUID |

---

## Unverified or could not confirm

- `nix3-run` does **not** contain the words "without installing",
  "ephemeral", or "ad hoc". Any claim that the command reference states an
  ephemeral rationale is unverified; the rationale appears only in the
  nix.dev tutorial.
- `nix3-profile-install.html` and `nix3-shell.html` both 404 on the
  official manual. `nix shell` documentation was read from
  `NixOS/nix/src/nix/shell.md`.
- Whether `uv tool install` echoes the resolved source form on success.
- Whether Docker echoes the detected build-context type. Its docs never
  address it.
- Whether any devcontainer implementation prompts before a Feature's
  `install.sh` runs as root. The spec does not require one.
- Whether `distrobox enter` auto-starts a stopped container. The page does
  not say.
- pipx's `--spec` design history and rationale — the `pypa/pipx` README and
  CHANGELOG were unreachable at their raw paths.
- The git "client-side hooks are not copied when you clone" sentence is
  from the Pro Git book on git-scm.com, not from the man pages. The man
  pages establish it only structurally.
- Terraform's `terraform init` module-install output (`Downloading <source>
  for <name>...`) is not documented on the module sources page.
- **"Be chatty"** is not a clig.dev guideline — zero matches in the
  document, and none in the Heroku CLI Style Guide it cites. The nearest
  real guideline is "If you change state, tell the user." Do not attribute
  it to clig.dev.
- **"Show the user what your program did"** is not a clig.dev heading
  either. Same correction.
- clig.dev has **no** `--yes` / `-y` / `--auto-approve` guideline.
- clig.dev's config precedence is **five** tiers, not three.
- The Docker glossary has 17 entries and contains **no `Dockerfile` entry
  and no `tag` entry**. Those were sourced from the Dockerfile reference
  and the build-tag-publish concept page instead.
- The Nix glossary has **no `flake` entry**. Flakes are documented
  elsewhere as an experimental feature.
- The devcontainer **spec** says nothing about container reuse or the
  `local_folder` / `config_file` labels. Reuse is CLI/VS Code
  implementation behaviour; only the id-variable spec blesses the label
  approach, and only as an example.
- What triggers an automatic dev container rebuild is **UNVERIFIED** from
  VS Code docs. The CLI source shows lookup is by label equality only, so
  an edited `devcontainer.json` at the same path still matches and rebuild
  stays user-initiated.
- tmux's default numeric session name is **not documented** in the man
  page; it was read from `session.c`.
- Docker's docs never say in words that the short id is the first 12
  characters — that is inferred from the example values. The name-collision
  behaviour is documented at the API level (`409 conflict`, name pattern),
  not as a CLI error string.
- Vagrant's `.vagrant/machines/<name>/<provider>/id` layout is **not
  documented**; it was read from `lib/vagrant/environment.rb`.
- `toolbox create -c NAME` still works but is absent from the current man
  page — treat it as legacy.
- Homebrew's `brew trust` requirement is recent (Homebrew 6.0.0, June
  2026). Anything written against an earlier Homebrew will describe taps as
  ungated.
