//! The box recipe, as written in `wormhole.toml`: which base to start
//! from, what to install into it, which agent runs, and what it may see.
//! Parsing, validation and the decisions that follow from it — fetching,
//! installing and launching live in the `wormhole` binary.
//!
//! Only `version` and `name` sit at the top level; everything else is in
//! a named table. TOML reads a top-level key written below a header as
//! part of that header's table, so no two tables share a key spelling —
//! a misplaced line lands on a field that does not exist and is refused
//! by name, rather than quietly meaning something else.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// The only manifest version this build understands. A manifest names it
/// so a newer one is refused clearly instead of half-read.
pub const VERSION: u32 = 1;

/// A SHA-256 written out: 64 lowercase hex characters. The base and every
/// artifact are held to it.
const SHA256_HEX: usize = 64;

/// A box recipe.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    /// What the box calls itself in `wormhole ps` and the panel. Nothing
    /// functional hangs off it.
    #[serde(default)]
    pub name: Option<String>,
    pub image: Image,
    #[serde(default)]
    pub agent: Agent,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub runtime: Runtime,
    /// What the box may use of the machine.
    #[serde(default)]
    pub limits: crate::limits_cgroup::Limits,
    /// The box's whole environment, declared. Nothing else reaches it.
    #[serde(default)]
    pub env: BTreeMap<String, EnvVar>,
}

/// What gets built, and nothing else. The digest of this table alone names
/// the image, so changing anything here rebuilds and changing anything
/// outside it never does.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    /// Where the base rootfs tarball comes from.
    pub base: String,
    /// The tarball's SHA-256, lowercase hex. The build refuses anything else.
    pub base_sha256: String,
    /// Replaces `/etc/apk/repositories` before anything is installed. A
    /// minirootfs ships none that is usable, so packages need this.
    #[serde(default)]
    pub package_sources: Vec<String>,
    /// Packages for `apk add`, installed once when the image is built.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Shell lines run inside the image after the packages, in order.
    #[serde(default)]
    pub build: Vec<String>,
    /// Files fetched on the host, proved by digest, and placed in the
    /// build box read-only. Written `[[image.artifact]]`, one table per
    /// file; the field is plural because a list of them is what it is.
    #[serde(default, rename = "artifact")]
    pub artifacts: Vec<Artifact>,
}

/// One file a build needs, named by URL and proved by its SHA-256.
///
/// The rule the base rootfs already lives by, applied to everything else a
/// recipe installs. Wormhole fetches it on the host, where the machine's
/// own resolver, proxy and trust store already work, checks the digest,
/// and binds it into the build box read-only. The box opens no connection
/// for it, so nothing about the transport has to be trusted and a network
/// that intercepts TLS cannot break the build.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Where the bytes come from. Any scheme `curl` speaks.
    pub url: String,
    /// Their SHA-256, lowercase hex. A mismatch stops the build.
    pub sha256: String,
    /// Where the file appears in the build box. Under
    /// [`crate::mount_plan::BUILD_SCRATCH`], which is a tmpfs, so the
    /// finished image carries what the artifact did and not the artifact.
    pub into: String,
}

/// Who runs in the box and what it reads before it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// Products this role may run. A string or a list of known names.
    /// Empty means a box with no agent. A list is a choice at start:
    /// `--run` names one, a terminal picks, a kept box keeps its own.
    /// Not `name`: the box has one of those, and two keys spelled alike in
    /// adjacent tables is how a misplaced line becomes a valid one.
    #[serde(default, deserialize_with = "deserialize_run")]
    pub run: Vec<String>,
    /// Passed to the agent as its model, when it takes one.
    #[serde(default)]
    pub model: Option<String>,
    /// A file beside the manifest with extra instructions, appended after
    /// the built-in ones so it wins where they disagree.
    #[serde(default)]
    pub instructions: Option<String>,
    /// A script beside the manifest, run in the box before the agent
    /// starts. Where skills or extra tooling get set up. Seeded into the
    /// box home and run from there, because the manifest's directory — a
    /// role's especially — is not in the box.
    #[serde(default)]
    pub preflight: Option<String>,
}

impl Agent {
    /// The product this start runs: the only name after [`bound`], or the
    /// default (first) name before it. `None` when the role runs none.
    #[must_use]
    pub fn product(&self) -> Option<&str> {
        self.run.first().map(String::as_str)
    }
}

/// A string is one product; a list is the products this role may run.
fn deserialize_run<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(name) => Ok(vec![name]),
        OneOrMany::Many(names) if names.is_empty() => Err(D::Error::custom(
            "run must name at least one agent; omit the key for none",
        )),
        OneOrMany::Many(names) => Ok(names),
    }
}

/// What the box can reach. Every field defaults to the answer that keeps
/// an agent working; each is one line away from the tighter one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    /// Host paths the box may see, as written — `~` allowed, expanded by
    /// the caller because a home directory is not a pure decision.
    #[serde(default)]
    pub grants: Vec<String>,
    /// The one resolver the box uses, build and run alike. Absent means
    /// the host's own `/etc/resolv.conf`, bound read-only — the box is on
    /// the host's network, so the host's resolver is the natural one.
    #[serde(default)]
    pub dns: Option<IpAddr>,
    /// How the agent gets a login. `"none"` (the default) starts with a
    /// clean box home and the agent's own `/login` does the rest;
    /// `"copy"` seeds the host's credential into the box home once, and
    /// the box refreshes its own copy from then on; `"share"` binds the
    /// host's credential file read-write, so a refresh in the box lands on
    /// the host too. A start typed with `--credentials` beats this.
    #[serde(default)]
    pub credentials: Credentials,
    /// Adds the host's CA bundle to the box's own trust, for networks that
    /// intercept TLS with their own CA — Cloudflare WARP, a corporate
    /// proxy. Adds, not replaces: OpenSSL reads `SSL_CERT_DIR` as well as
    /// the bundle and trusts the union, so the image's own roots keep
    /// working. The host path is found at start, not written here, so a
    /// role stays host-independent.
    ///
    /// It reaches the build box too. A recipe cannot pin what `npm` or
    /// `apk` resolve for themselves, so a build terminates some TLS of its
    /// own; everything a recipe *can* pin belongs in
    /// `[[image.artifact]]`, which needs no trust at all.
    #[serde(default)]
    pub host_ca: bool,
}

/// Where a box's agent login comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Credentials {
    /// Nothing seeded; the agent's own `/login` in the box fills the home.
    #[default]
    None,
    /// The host's credential files copied into the box home, once: a box
    /// that already has a login keeps it. The host copy is never written.
    Copy,
    /// The host's credential files bound read-write in the box home. One
    /// login; a host agent and a box refreshing at once can cost a `/login`.
    Share,
}

impl fmt::Display for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credentials::None => write!(f, "none"),
            Credentials::Copy => write!(f, "copy"),
            Credentials::Share => write!(f, "share"),
        }
    }
}

/// How the box is made from its image and what it leaves behind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// `"copy"` (the default) or `"readonly"`. `"readonly"` binds the
    /// cached image itself instead of copying it, with a tmpfs seeded from
    /// the image over `/etc` and `/var`. A box then starts in the time it
    /// takes to mount rather than the time it takes to copy a gigabyte —
    /// and cannot install a package at run time, which is the trade.
    #[serde(default)]
    pub rootfs: crate::run::RootMode,
}

/// Where the preflight hook is seeded, relative to the box home. The
/// launcher writes it there; `launch_command` runs it from there.
pub const PREFLIGHT_SEED: &str = ".wormhole/preflight";

/// The product this start bound, so a preflight hook can branch without
/// wormhole parsing it. Injected as a fixed env variable.
pub const RUN_ENV: &str = "WORMHOLE_RUN";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    /// Used when the host has no value for it. An empty default means
    /// "declared, may be empty" — the host can fill it in.
    #[serde(default)]
    pub default: String,
    /// The host's value is ignored. For anything that describes the box
    /// rather than you — `SHELL` is a path inside the box, and your
    /// `/usr/bin/zsh` does not exist in here.
    #[serde(default)]
    pub fixed: Option<String>,
    /// With no value anywhere, a start on a terminal asks for one, once,
    /// and keeps the answer host-side for every box. A credential by
    /// nature, so it is masked wherever wormhole prints it.
    #[serde(default)]
    pub ask: bool,
}

/// Where every agent's instructions live: one canonical file at the box
/// home root. Each agent reads its own path, so the seeding writes this
/// file once and a per-agent pointer at the path the agent actually reads
/// — the text exists exactly once, whoever runs.
pub const INSTRUCTIONS_SEED: &str = "AGENTS.md";

/// How an agent's own instructions path delivers the canonical file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// A file whose whole content is an import line the agent follows.
    Import(&'static str),
    /// A symlink to the canonical file, for an agent with no import syntax.
    Symlink,
}

/// Whether what a start writes replaces what the file already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The box settles it, so every start writes it over whatever a past
    /// session left there.
    Settled,
    /// A starting point, written only where the file has no value: a
    /// choice the agent or its user makes later survives.
    Initial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Toml,
}

/// A table on the way to a key a start writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Name(&'static str),
    /// The box's workspace path, which agents keep per-directory answers
    /// under.
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Literal {
    Bool(bool),
    Text(&'static str),
}

/// One key a start writes, as the registry spells it.
struct Answer {
    table: &'static [Key],
    key: &'static str,
    value: Literal,
    rule: Rule,
}

const fn settled(table: &'static [Key], key: &'static str, value: Literal) -> Answer {
    Answer {
        table,
        key,
        value,
        rule: Rule::Settled,
    }
}

const fn initial(table: &'static [Key], key: &'static str, value: Literal) -> Answer {
    Answer {
        table,
        key,
        value,
        rule: Rule::Initial,
    }
}

impl Answer {
    fn resolve(&self, workspace: &str) -> Entry {
        Entry {
            table: self
                .table
                .iter()
                .map(|key| match key {
                    Key::Name(name) => (*name).to_owned(),
                    Key::Workspace => workspace.to_owned(),
                })
                .collect(),
            key: self.key.to_owned(),
            value: match self.value {
                Literal::Bool(value) => serde_json::Value::Bool(value),
                Literal::Text(value) => serde_json::Value::String(value.to_owned()),
            },
            rule: self.rule,
        }
    }
}

/// One of an agent's own config files, and what a start writes into it.
struct ConfigFile {
    /// Relative to the box home.
    file: &'static str,
    format: Format,
    answers: &'static [Answer],
    /// Where the manifest's `model` goes, for an agent that reads no env
    /// var for one.
    model_key: Option<&'static str>,
    /// Top-level fields of the host's own copy that are the other half of
    /// a login handed over; the credential file is the first.
    login_fields: &'static [&'static str],
}

/// A config file as one start writes it, every value resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWrite {
    /// Relative to the box home, and to the host's home for `login_fields`.
    pub file: &'static str,
    pub format: Format,
    pub entries: Vec<Entry>,
    pub login_fields: &'static [&'static str],
}

/// One value, written at `key` inside the nested `table`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub table: Vec<String>,
    pub key: String,
    pub value: serde_json::Value,
    pub rule: Rule,
}

/// How an agent saves a login, which decides whether its credential file
/// can be a shared one: `credentials = "share"` binds that file, and a
/// bind mount is a mount point, which `rename` cannot replace. An agent
/// that writes a new file and renames it over the old therefore fails
/// with `EBUSY` — inside the agent, after a whole login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialWrite {
    /// Writes the file it was given. Verified for codex by saving a
    /// login through a bind mount; assumed for claude, whose roles ship
    /// `credentials = "none"`.
    InPlace,
    /// Replaces the file. Verified for grok, which fails a shared login
    /// with `Failed to save credentials: Resource busy (os error 16)`.
    Replace,
}

/// One agent wormhole knows how to launch. Everything agent-specific
/// lives here; a call site that compares `agent.run` to a string instead
/// of asking this table is the bug this table exists to prevent.
struct KnownAgent {
    name: &'static str,
    /// Permissions are bypassed because the box, not the prompt, is what
    /// holds the line.
    command: &'static [&'static str],
    /// Where the agent reads its own instructions, relative to the box home.
    instructions: &'static str,
    /// How that path delivers the canonical `AGENTS.md`.
    pointer: Pointer,
    /// The env var this agent reads its model from; `None` means the
    /// model is seeded into the agent's config file instead. Every agent
    /// does one or the other, or a manifest's `model` would go nowhere.
    model_env: Option<&'static str>,
    /// The files, relative to a home, that hold this agent's login: what
    /// `credentials = "copy"` copies and `"share"` binds.
    credential_files: &'static [&'static str],
    /// How the agent saves one, which decides whether it can share one.
    credential_write: CredentialWrite,
    /// The agent's own config files and what a start writes into each:
    /// every question the box existing already answers.
    config: &'static [ConfigFile],
    /// Variables the box sets for this agent, for what it settles that no
    /// config file holds. A manifest that declares one means it and wins.
    env: &'static [(&'static str, &'static str)],
}

const TRUE: Literal = Literal::Bool(true);
const PROJECT: &[Key] = &[Key::Name("projects"), Key::Workspace];
const EFFORT_NUDGE: &[Key] = &[Key::Name("hasSeenEffortMediumNudgeByModel")];

/// Claude Code's state file: questions it asks once. Key names read off
/// Claude Code 2.1.270, and present in 2.1.246, the oldest a role pins.
const CLAUDE_STATE: ConfigFile = ConfigFile {
    file: ".claude.json",
    format: Format::Json,
    answers: &[
        settled(&[], "hasCompletedOnboarding", TRUE),
        // Newer builds read `skipDangerousModePermissionPrompt` instead and
        // move this one there themselves; an older pinned build may read
        // only this.
        settled(&[], "bypassPermissionsModeAccepted", TRUE),
        settled(&[], "hasAcknowledgedCostThreshold", TRUE),
        // An offer to make auto mode the default, brought on by the
        // `defaultMode` the settings carry. Accepted, the box is gated by a
        // classifier.
        settled(&[], "hasSeenAutoDefaultNudge", TRUE),
        // An offer to drop effort to medium. Builds before 2.1.270 read the
        // flat key; later ones one key per model they offer it for.
        settled(&[], "hasSeenEffortMediumNudge", TRUE),
        settled(EFFORT_NUDGE, "claude-opus-5", TRUE),
        settled(EFFORT_NUDGE, "claude-fable-5-1", TRUE),
        initial(&[], "theme", Literal::Text("dark")),
        settled(PROJECT, "hasTrustDialogAccepted", TRUE),
        settled(PROJECT, "hasCompletedProjectOnboarding", TRUE),
        // A CLAUDE.md importing from outside the workspace asks first, and
        // an unanswered ask loads nothing. The box can import only what it
        // can see.
        settled(PROJECT, "hasClaudeMdExternalIncludesApproved", TRUE),
        settled(PROJECT, "hasClaudeMdExternalIncludesWarningShown", TRUE),
    ],
    model_key: None,
    login_fields: &["oauthAccount"],
};

/// Claude Code's user settings. The flag reaches only the process wormhole
/// starts; a resumed session, a background worker and a `claude` typed in
/// an attached shell start from these.
const CLAUDE_SETTINGS: ConfigFile = ConfigFile {
    file: ".claude/settings.json",
    format: Format::Json,
    answers: &[
        settled(&[], "skipDangerousModePermissionPrompt", TRUE),
        settled(
            &[Key::Name("permissions")],
            "defaultMode",
            Literal::Text("bypassPermissions"),
        ),
        settled(&[], "enableAllProjectMcpServers", TRUE),
        // After a safeguard flag, pause rather than switch to an older
        // model. CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK goes further but is
        // undocumented; this documented key is what holds if it goes.
        initial(&[], "switchModelsOnFlag", Literal::Bool(false)),
    ],
    model_key: None,
    login_fields: &[],
};

/// Codex's config. The flag's two answers are here as well, for the paths
/// it does not reach: a `codex resume` typed in an attached shell, an
/// app-server thread.
const CODEX_CONFIG: ConfigFile = ConfigFile {
    file: ".codex/config.toml",
    format: Format::Toml,
    answers: &[
        // Asked per directory, even under the bypass flag.
        settled(PROJECT, "trust_level", Literal::Text("trusted")),
        settled(&[], "approval_policy", Literal::Text("never")),
        settled(&[], "sandbox_mode", Literal::Text("danger-full-access")),
        // Otherwise `cached`: an index of the web, not the web.
        settled(&[], "web_search", Literal::Text("live")),
        // A popup offering a cheaper model near the rate limit.
        settled(&[Key::Name("notice")], "hide_rate_limit_model_nudge", TRUE),
    ],
    model_key: Some("model"),
    login_fields: &[],
};

/// Grok's config. `grok agent`, a session its leader spawns and any start
/// without the flag read the mode here, and the TUI saves a mode toggled
/// by hand to the same key.
const GROK_CONFIG: ConfigFile = ConfigFile {
    file: ".grok/config.toml",
    format: Format::Toml,
    answers: &[
        settled(
            &[Key::Name("ui")],
            "permission_mode",
            Literal::Text("always-approve"),
        ),
        // Off by default, which leaves search and no fetch.
        settled(&[Key::Name("features")], "web_fetch", TRUE),
    ],
    model_key: None,
    login_fields: &[],
};

const KNOWN_AGENTS: [KnownAgent; 3] = [
    KnownAgent {
        name: "claude",
        command: &["claude", "--dangerously-skip-permissions"],
        instructions: ".claude/CLAUDE.md",
        // `@AGENTS.md` alone would resolve beside CLAUDE.md, inside
        // `.claude/`; the canonical file is at the home root.
        pointer: Pointer::Import("@~/AGENTS.md\n"),
        model_env: Some("ANTHROPIC_MODEL"),
        // The account fields in `.claude.json` travel with it; see
        // `CLAUDE_STATE`.
        credential_files: &[".claude/.credentials.json"],
        credential_write: CredentialWrite::InPlace,
        config: &[CLAUDE_STATE, CLAUDE_SETTINGS],
        env: &[
            // A safeguard flag ends the turn instead of moving the session,
            // or a subagent, to an older model.
            ("CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK", "1"),
            // Capacity errors and usage limits are waited out rather than
            // ending the run.
            ("CLAUDE_CODE_RETRY_WATCHDOG", "1"),
            // The box is the sandbox: no server-forced Bash sandbox inside
            // it, and no trust dialog for a nested repository the seeded
            // trust does not name.
            ("IS_SANDBOX", "1"),
            ("CLAUDE_CODE_SANDBOXED", "1"),
        ],
    },
    KnownAgent {
        name: "codex",
        // The analogue of claude's flag; codex's own docs reserve it for
        // an isolated runner, which the box is. Also necessary: codex's
        // Landlock/bwrap sandbox cannot be assumed to nest in here.
        command: &["codex", "--dangerously-bypass-approvals-and-sandbox"],
        instructions: ".codex/AGENTS.md",
        // Codex has no import syntax, so the pointer is a symlink.
        pointer: Pointer::Symlink,
        model_env: None,
        credential_files: &[".codex/auth.json"],
        credential_write: CredentialWrite::InPlace,
        config: &[CODEX_CONFIG],
        env: &[],
    },
    KnownAgent {
        name: "grok",
        // Two gates the box already answers: approvals, and the folder
        // trust that gates whether a headless start reads the workspace's
        // instructions at all. Grok's own sandbox is off by default, so
        // there is nothing there to turn off.
        command: &["grok", "--always-approve", "--trust"],
        // Grok reads no file of its own at the home root; what it always
        // reads, whatever directory it starts in, is `$GROK_HOME/rules/`.
        instructions: ".grok/rules/AGENTS.md",
        pointer: Pointer::Symlink,
        model_env: Some("GROK_DEFAULT_MODEL"),
        credential_files: &[".grok/auth.json"],
        credential_write: CredentialWrite::Replace,
        config: &[GROK_CONFIG],
        // `--trust` grants the launch directory alone; a nested repository
        // or a moved workspace would ask again.
        env: &[("GROK_FOLDER_TRUST", "0")],
    },
];

fn known(name: &str) -> Option<&'static KnownAgent> {
    KNOWN_AGENTS.iter().find(|agent| agent.name == name)
}

impl KnownAgent {
    fn command(&self) -> Vec<String> {
        self.command.iter().map(|part| (*part).to_owned()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    Syntax(String),
    Version(String),
    Sha256NotHex(String),
    /// An artifact destination that is not an absolute path inside the box.
    ArtifactPath(String),
    /// An artifact in a recipe with no build line at all.
    ArtifactWithNoBuild(String),
    UnknownAgent(String),
    /// `run` names the same product twice.
    DuplicateAgent(String),
    /// `model` with more than one `run` name: no one product to give it to.
    ModelOnManyRuns,
    /// `--run` named a known product this role does not offer.
    ProductNotInRole {
        wanted: String,
        offered: Vec<String>,
    },
    /// A fixed variable that also carries `default`.
    EnvFixedConflict(String),
    /// `ask` beside a value that is never missing.
    EnvAskConflict(String),
    NoAgent,
    /// An `[access]` key the broker took with it.
    Removed(String),
    /// A manifest that both names a role and carries a recipe.
    RoleAndImage,
    /// A recipe was wanted and this manifest hands its box to a role.
    HandsToARole,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Syntax(detail) => write!(f, "wormhole.toml is not valid: {detail}"),
            ManifestError::HandsToARole => write!(
                f,
                "this wormhole.toml hands its box to a role rather than describing one, \
                 and a role cannot hand over again — give it an `[image]` of its own"
            ),
            ManifestError::RoleAndImage => write!(
                f,
                "a manifest that names a `role` carries no recipe of its own; \
                 remove `[image]`, or remove `role` and keep the recipe"
            ),
            ManifestError::Version(found) => {
                write!(
                    f,
                    "this wormhole reads manifest version {VERSION}, not {found}"
                )
            }
            ManifestError::Sha256NotHex(value) => write!(
                f,
                "a sha256 must be 64 lowercase hex characters, not {value}"
            ),
            ManifestError::ArtifactPath(into) => write!(
                f,
                "[[image.artifact]] into must name a file under {scratch}, \
                 the build box's throwaway directory — not {into}",
                scratch = crate::mount_plan::BUILD_SCRATCH
            ),
            ManifestError::ArtifactWithNoBuild(url) => write!(
                f,
                "[[image.artifact]] {url} is fetched for a recipe with no `build` line \
                 at all, so nothing could read it and the image would be the same without it"
            ),
            ManifestError::UnknownAgent(name) => write!(
                f,
                "unknown agent {name}; wormhole knows {}",
                KNOWN_AGENTS
                    .iter()
                    .map(|agent| agent.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ManifestError::DuplicateAgent(name) => {
                write!(f, "run names {name} twice")
            }
            ManifestError::ModelOnManyRuns => write!(
                f,
                "[agent] model needs a single run; a list of products has no one place to put it"
            ),
            ManifestError::ProductNotInRole { wanted, offered } if offered.is_empty() => {
                write!(
                    f,
                    "this role runs no agent, so --run {wanted} has nothing to pick"
                )
            }
            ManifestError::ProductNotInRole { wanted, offered } => write!(
                f,
                "this role does not run {wanted}; it runs {}",
                offered.join(", ")
            ),
            ManifestError::EnvAskConflict(name) => write!(
                f,
                "[env.{name}] has a value already, so `ask` could never \
                 act; drop it or drop `fixed`/`default`"
            ),
            ManifestError::EnvFixedConflict(name) => write!(
                f,
                "[env.{name}] is fixed, so `default` could never act; \
                 drop one of them"
            ),
            ManifestError::NoAgent => write!(f, "no agent named, so there is nothing to run"),
            ManifestError::Removed(key) => write!(
                f,
                "[access] {key} is gone with the broker: the box is on the host's \
                 network; `credentials` says which login it gets"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// The file as TOML, before anything is asked of it. Parsed once; the
/// version, the role and the removed keys are read off this table and
/// the manifest is built from it.
fn table(text: &str) -> Result<toml::Table, ManifestError> {
    toml::from_str(text).map_err(|e| ManifestError::Syntax(e.message().to_owned()))
}

/// What a wormhole file claims as its version, written exactly as it wrote
/// it — `1`, `"v1alpha1"`, or `None` for a file that names none.
///
/// Read before the file itself, so one from another version is named as
/// such instead of surfacing as a type error on one field or a heap of
/// unknown keys. That is the whole reason the key exists.
fn claimed_version(table: &toml::Table) -> Option<String> {
    table.get("version").map(|found| found.to_string())
}

/// The role a workspace's manifest hands its box over to, if it does.
///
/// Read before the manifest is parsed, because a manifest that names a
/// role carries no recipe of its own — `[image]` is required and there
/// would be nothing to put in it. A project says which role it uses in one
/// line, everyone who clones it types `wormhole box` and nothing else, and
/// the pin moves in a pull request rather than in somebody's shell history.
///
/// Not inheritance. `role` names the recipe *instead of* carrying one; a
/// manifest holding both is refused rather than merged, because merging is
/// where a key like this turns into an override system nobody can predict.
pub fn names_a_role(text: &str) -> Result<Option<String>, ManifestError> {
    role_in(&table(text)?)
}

fn role_in(table: &toml::Table) -> Result<Option<String>, ManifestError> {
    let Some(role) = table.get("role") else {
        return Ok(None);
    };
    let role = role
        .as_str()
        .ok_or_else(|| ManifestError::Syntax("`role` must be a string".to_owned()))?;
    if table.contains_key("image") {
        return Err(ManifestError::RoleAndImage);
    }
    if role.trim().is_empty() {
        return Err(ManifestError::Syntax(
            "`role` names no role; give it a directory, a name, or a pinned ref".to_owned(),
        ));
    }
    Ok(Some(role.to_owned()))
}

/// The `[access]` keys the broker took with it, so an old manifest is
/// told what replaced them rather than "unknown field".
fn removed_key(table: &toml::Table) -> Option<&'static str> {
    let access = table.get("access")?.as_table()?;
    ["network", "broker", "egress"]
        .into_iter()
        .find(|key| access.contains_key(*key))
}

pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let table = table(text)?;
    match claimed_version(&table) {
        Some(found) if found == VERSION.to_string() => {}
        Some(found) => return Err(ManifestError::Version(found)),
        None => return Err(ManifestError::Version("nothing".to_owned())),
    }
    // Said in this project's words rather than serde's `unknown field`.
    if role_in(&table)?.is_some() {
        return Err(ManifestError::HandsToARole);
    }
    if let Some(key) = removed_key(&table) {
        return Err(ManifestError::Removed(key.to_owned()));
    }
    let manifest: Manifest = table
        .try_into()
        .map_err(|e: toml::de::Error| ManifestError::Syntax(e.message().to_owned()))?;
    if !crate::is_lowercase_hex(&manifest.image.base_sha256, SHA256_HEX) {
        return Err(ManifestError::Sha256NotHex(manifest.image.base_sha256));
    }
    let mut destinations = std::collections::BTreeSet::new();
    for artifact in &manifest.image.artifacts {
        if !crate::is_lowercase_hex(&artifact.sha256, SHA256_HEX) {
            return Err(ManifestError::Sha256NotHex(artifact.sha256.clone()));
        }
        // Two artifacts at one destination is one bind over another: the
        // second wins and the first is fetched and never read.
        if !crate::mount_plan::is_in_build_scratch(&artifact.into)
            || !destinations.insert(&artifact.into)
        {
            return Err(ManifestError::ArtifactPath(artifact.into.clone()));
        }
    }
    if manifest.image.build.is_empty()
        && let Some(first) = manifest.image.artifacts.first()
    {
        return Err(ManifestError::ArtifactWithNoBuild(first.url.clone()));
    }
    let mut seen = std::collections::BTreeSet::new();
    for name in &manifest.agent.run {
        if known(name).is_none() {
            return Err(ManifestError::UnknownAgent(name.clone()));
        }
        if !seen.insert(name) {
            return Err(ManifestError::DuplicateAgent(name.clone()));
        }
    }
    if manifest.agent.run.len() > 1 && manifest.agent.model.is_some() {
        return Err(ManifestError::ModelOnManyRuns);
    }
    for (name, var) in &manifest.env {
        if var.fixed.is_some() && !var.default.is_empty() {
            return Err(ManifestError::EnvFixedConflict(name.clone()));
        }
        if var.ask && (var.fixed.is_some() || !var.default.is_empty()) {
            return Err(ManifestError::EnvAskConflict(name.clone()));
        }
    }
    Ok(manifest)
}

/// The command that starts this box's agent. Permissions are bypassed
/// because the box, not the agent, is what holds the line.
pub fn agent_command(manifest: &Manifest) -> Result<Vec<String>, ManifestError> {
    let name = manifest.agent.product().ok_or(ManifestError::NoAgent)?;
    let agent = known(name).ok_or_else(|| ManifestError::UnknownAgent(name.to_owned()))?;
    Ok(agent.command())
}

/// What `attach` runs when the user names no command: the box's agent
/// when it has one, a shell otherwise.
pub fn attach_command(agent: Option<&str>) -> Vec<String> {
    match agent.and_then(known) {
        Some(agent) => agent.command(),
        None => vec!["/bin/sh".to_owned()],
    }
}

/// Where the agent reads its own instructions, relative to the box home,
/// and how that path delivers the canonical `AGENTS.md`. One lookup, so
/// the path and the pointer planted at it cannot disagree. `None` when
/// the box has no agent to instruct.
pub fn instructions_pointer(manifest: &Manifest) -> Option<(&'static str, Pointer)> {
    known(manifest.agent.product()?).map(|agent| (agent.instructions, agent.pointer))
}

/// What a `Pointer::Symlink` at `target` holds. Relative, so a kept home
/// survives being moved: one `..` per directory between the agent's own
/// path and the home root the canonical file sits at.
#[must_use]
pub fn pointer_link(target: &str) -> String {
    let mut link = "../".repeat(target.matches('/').count());
    link.push_str(INSTRUCTIONS_SEED);
    link
}

/// Every config file a start writes for this agent, every value resolved:
/// the registry's answers at this workspace, and the manifest's model
/// where the agent reads it from a file. Empty with no agent.
#[must_use]
pub fn config_writes(manifest: &Manifest, workspace: &str) -> Vec<ConfigWrite> {
    let Some(agent) = manifest.agent.product().and_then(known) else {
        return Vec::new();
    };
    agent
        .config
        .iter()
        .map(|config| {
            let mut entries: Vec<Entry> = config
                .answers
                .iter()
                .map(|answer| answer.resolve(workspace))
                .collect();
            if let (Some(key), Some(model)) = (config.model_key, manifest.agent.model.as_deref()) {
                entries.push(Entry {
                    table: Vec::new(),
                    key: key.to_owned(),
                    value: serde_json::Value::String(model.to_owned()),
                    rule: Rule::Settled,
                });
            }
            ConfigWrite {
                file: config.file,
                format: config.format,
                entries,
                login_fields: config.login_fields,
            }
        })
        .collect()
}

/// Why a start that did not name a product cannot go on.
///
/// Several products are several boxes. The first name in the list is not
/// an answer: that would silently open a claude home when the last session
/// was grok. `--run` is how a script names one; a terminal is shown the
/// same list the panel's `n` already uses.
#[must_use]
pub fn need_run(offered: &[String]) -> String {
    format!(
        "this role runs {}; pass --run {}",
        offered.join(", "),
        offered.join("|"),
    )
}

/// Which product this start runs. `requested` is `--run`; omitted, the
/// first name in `[agent] run`. `None` when the role runs no agent.
///
/// # Errors
///
/// Unknown names, and a known name this role does not offer.
pub fn bind<'a>(
    manifest: &'a Manifest,
    requested: Option<&str>,
) -> Result<Option<&'a str>, ManifestError> {
    let Some(want) = requested else {
        return Ok(manifest.agent.product());
    };
    if known(want).is_none() {
        return Err(ManifestError::UnknownAgent(want.to_owned()));
    }
    manifest
        .agent
        .run
        .iter()
        .map(String::as_str)
        .find(|&name| name == want)
        .map(Some)
        .ok_or_else(|| ManifestError::ProductNotInRole {
            wanted: want.to_owned(),
            offered: manifest.agent.run.clone(),
        })
}

/// Why `--id` cannot take `--run` for a different product, when it
/// cannot. The recorded product is the home's; switching it would put
/// the new CLI in the old login's directory.
#[must_use]
pub fn product_mismatch(recorded: Option<&str>, bound: Option<&str>) -> Option<String> {
    match (recorded, bound) {
        (Some(have), Some(want)) if have != want => Some(format!(
            "that box runs {have}, not {want}; omit --run to resume it"
        )),
        _ => None,
    }
}

/// The recipe with `[agent] run` narrowed to the product this start
/// actually launches, so every later lookup reads one name.
///
/// # Errors
///
/// The same as [`bind`].
pub fn bound(mut manifest: Manifest, requested: Option<&str>) -> Result<Manifest, ManifestError> {
    let chosen = bind(&manifest, requested)?.map(str::to_owned);
    manifest.agent.run = match chosen {
        Some(name) => vec![name],
        None => Vec::new(),
    };
    Ok(manifest)
}

/// Why this start cannot hand the box the login it asked for, when it
/// cannot. A launch assertion, not a warning: the alternative is the
/// agent failing on its own save, after a whole login, with an `EBUSY`
/// nothing in the box explains.
#[must_use]
pub fn credentials_refusal(manifest: &Manifest, mode: Credentials) -> Option<String> {
    if mode != Credentials::Share {
        return None;
    }
    let name = manifest.agent.product()?;
    let agent = known(name)?;
    if agent.credential_write == CredentialWrite::InPlace {
        return None;
    }
    let files = agent.credential_files.join(", ");
    Some(format!(
        "credentials = \"share\" cannot work for {name}: it saves a login by replacing \
         {files}, and a shared file is a mount point, which nothing can replace. \
         Use --credentials copy for your login in the box, or none to log in there"
    ))
}

/// The files, relative to a home, that hold this agent's login — what a
/// `copy` seeds and a `share` binds. Empty with no agent.
#[must_use]
pub fn credential_files(manifest: &Manifest) -> &'static [&'static str] {
    manifest
        .agent
        .product()
        .and_then(known)
        .map_or(&[], |agent| agent.credential_files)
}

/// The instructions file a box's agent gets: the built-in text, then the
/// manifest's extra ones after a blank line, so they win where they
/// disagree.
pub fn compose_instructions(default: &str, extra: Option<&str>) -> String {
    let mut text = default.trim_end().to_owned();
    text.push('\n');
    if let Some(extra) = extra {
        text.push('\n');
        text.push_str(extra.trim_end());
        text.push('\n');
    }
    text
}

/// What the box actually runs. With no preflight hook that is the agent
/// itself, as PID 1. With one, a shell runs the hook first and then
/// *replaces itself* with the agent, so the agent is still PID 1 and still
/// owns the terminal. A failing hook stops the box instead of starting an
/// agent that is set up wrong.
/// Everything that has to happen inside the box before the agent does,
/// as shell lines — empty when nothing does.
///
/// Each line guards itself rather than leaning on `set -e`: a caller that
/// cannot run under it must still be able to read this list.
pub fn pre_agent_script(manifest: &Manifest) -> String {
    let mut script = String::new();
    if manifest.agent.preflight.is_some() {
        script.push_str(&format!(
            "/bin/sh \"$HOME/{PREFLIGHT_SEED}\" || {{\n\
             \x20 echo \"wormhole: the role's preflight hook failed; nothing starts\" >&2\n\
             \x20 exit 1\n\
             }}\n"
        ));
    }
    script
}

pub fn launch_command(manifest: &Manifest) -> Result<Vec<String>, ManifestError> {
    let agent = agent_command(manifest)?;
    let before = pre_agent_script(manifest);
    if before.is_empty() {
        return Ok(agent);
    }
    Ok(wrapped(&before, &agent))
}

fn wrapped(before: &str, command: &[String]) -> Vec<String> {
    let mut script = String::from("set -eu\n");
    script.push_str(before);
    script.push_str(&format!(
        "exec {}",
        command
            .iter()
            .map(|part| shell_quote(part))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script]
}

/// One line from the build to whoever started it, in wormhole's voice.
///
/// The build box has no other narrator: a shell under `set -eu` dies
/// silently, and an exit code alone was what made a broken fetch take a
/// day to find. Every step is said before it runs, so the last line
/// printed is the step that failed.
fn step(line: &str) -> String {
    format!("{}{line}\n", say(line))
}

fn say(message: &str) -> String {
    format!(
        "echo {} >&2\n",
        shell_quote(&format!("wormhole: {message}"))
    )
}

/// Refuses an image whose trust store `apk` left unusable.
///
/// Alpine's `ca-certificates` trigger is `update-ca-certificates` with
/// its output discarded and `exit 0` after it, so a bundle it failed to
/// write is still reported as a successful install. Every https fetch in
/// the build then fails with a verify error that names no cause. The
/// store the packages left behind is the build's own business, so the
/// build is where it is checked.
///
/// A bundle that is absent is not judged: nothing claimed there would be
/// one. Only one that exists and holds no certificate is a failure.
fn assert_trust_store() -> String {
    let bundle = crate::ca::CA_BUNDLE_IN_BOX;
    let refusal = say("the packages left an empty CA store; \
         nothing in this image can verify a certificate");
    format!(
        "if [ -e {bundle} ] && ! grep -q 'BEGIN CERTIFICATE' {bundle}; then\n\
         \x20 {refusal}\
         \x20 exit 1\n\
         fi\n"
    )
}

/// Single quotes, with embedded quotes closed and reopened. A path or an
/// argument from the manifest must never become shell syntax.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The box's environment: every variable the manifest declares, taking the
/// host's value when it has one and the default when it does not. Anything
/// the manifest does not name never reaches the box.
///
/// Everything the manifest declares plus what the recipe implies — `[env]`
/// as written, with the model and the CA pointers folded in as fixed ones
/// and the terminal as defaults. The single input every env screen and
/// every resolution starts from.
#[must_use]
pub fn declarations(manifest: &Manifest) -> BTreeMap<String, EnvVar> {
    let fixed = |value: String| EnvVar {
        fixed: Some(value),
        ..EnvVar::default()
    };
    let mut declared = manifest.env.clone();
    for (name, default) in crate::terminfo::ENV_DEFAULTS {
        declared.entry(name.to_owned()).or_insert_with(|| EnvVar {
            default: default.to_owned(),
            ..EnvVar::default()
        });
    }
    // Only when this agent reads a model variable at all: exporting
    // ANTHROPIC_MODEL to codex would be a line nobody reads, and worse, a
    // lie about how the model was actually passed.
    if let Some(model) = manifest.agent.model.as_deref()
        && let Some(var) = manifest
            .agent
            .product()
            .and_then(known)
            .and_then(|agent| agent.model_env)
    {
        declared.insert(var.to_owned(), fixed(model.to_owned()));
    }
    if let Some(name) = manifest.agent.product() {
        declared.insert(RUN_ENV.to_owned(), fixed(name.to_owned()));
    }
    let agent_env = manifest
        .agent
        .product()
        .and_then(known)
        .map_or(&[][..], |agent| agent.env);
    for (name, value) in agent_env {
        declared
            .entry((*name).to_owned())
            .or_insert_with(|| fixed((*value).to_owned()));
    }
    // Mounting the host's bundle is not enough on its own: the clients in
    // the box mostly do not read that path unless they are told to, and
    // the agent's own runtime never reads the filesystem at all. Named as
    // defaults, so a manifest that declares one of these means it and wins.
    if manifest.access.host_ca {
        for (name, bundle) in crate::ca::readers_pointing_at(crate::ca::CA_BUNDLE_IN_BOX) {
            declared
                .entry(name.to_owned())
                .or_insert_with(|| fixed(bundle));
        }
    }
    declared
}

/// Everything that changes what a built image contains, and nothing that
/// does not — which is exactly `[image]`. The caller hashes this to name
/// the image, so every other table is absent on purpose: changing where
/// the box looks for your keys must not force a reinstall.
///
/// The labels are this function's own and deliberately do not track the
/// manifest's key names. The digest answers "what is in this image", and
/// renaming a key changes nothing about that — spelling the labels after
/// the keys would throw away every cached image on a rename that installs
/// exactly the same bytes.
pub fn recipe_text(manifest: &Manifest) -> String {
    let image = &manifest.image;
    let mut text = format!("rootfs {}\nsha256 {}\n", image.base, image.base_sha256);
    for source in &image.package_sources {
        text.push_str(&format!("repository {source}\n"));
    }
    for package in &image.packages {
        text.push_str(&format!("package {package}\n"));
    }
    for artifact in &image.artifacts {
        text.push_str(&format!(
            "artifact {} {} {}\n",
            artifact.url, artifact.sha256, artifact.into
        ));
    }
    for line in &image.build {
        text.push_str(&format!("setup {line}\n"));
    }
    text
}

/// The recipe digest is the image's whole identity: same digest, same
/// image, reused; changed digest, new build. One function owns the
/// recipe-to-key step so no caller can compose it differently.
pub fn recipe_digest(manifest: &Manifest) -> String {
    crate::sha256_hex(recipe_text(manifest).as_bytes())
}

/// Every digest the store keeps on this recipe's behalf: the image it
/// builds to, the base rootfs it starts from, and every artifact it pins.
///
/// This is what makes a built thing provably referenced. The store names
/// all three by digest — `images/<recipe>`, `bases/<sha256>`,
/// `artifacts/<sha256>` — so one list of digests decides which of them a
/// recipe is still keeping alive, without anything having to record a
/// back-pointer it would then have to maintain.
pub fn referenced_digests(manifest: &Manifest) -> Vec<String> {
    let mut digests = vec![recipe_digest(manifest), manifest.image.base_sha256.clone()];
    digests.extend(
        manifest
            .image
            .artifacts
            .iter()
            .map(|artifact| artifact.sha256.clone()),
    );
    digests
}

/// The shell script that turns a base into the image: package sources
/// first, then packages, then the manifest's own build lines, stopping at
/// the first failure. Nothing when there is nothing to install.
pub fn build_script(manifest: &Manifest) -> Option<String> {
    let image = &manifest.image;
    if image.packages.is_empty() && image.build.is_empty() {
        return None;
    }
    let mut script = String::from("set -eu\n");
    if !image.package_sources.is_empty() {
        script.push_str("mkdir -p /etc/apk\n: > /etc/apk/repositories\n");
        for source in &image.package_sources {
            script.push_str(&format!("echo {source} >> /etc/apk/repositories\n"));
        }
    }
    if !image.packages.is_empty() {
        script.push_str(&step(&format!(
            "apk add --no-cache {}",
            image.packages.join(" ")
        )));
        script.push_str(&assert_trust_store());
    }
    for line in &image.build {
        script.push_str(&step(line));
    }
    Some(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// The smallest manifest that parses. `extra` goes above `[image]`,
    /// where TOML requires a top-level key to be.
    fn text(extra: &str) -> String {
        format!(
            "version = {VERSION}\n{extra}[image]\nbase = \"https://example.test/rootfs.tar.gz\"\nbase_sha256 = \"{DIGEST}\"\n"
        )
    }

    fn minimal() -> String {
        text("")
    }

    /// What `wormhole gc` needs to prove a built thing is still wanted:
    /// one recipe names the image it builds to, the base it starts from,
    /// and every artifact it pins — the three things the store keeps.
    #[test]
    fn a_recipe_names_every_digest_the_store_keeps_for_it() {
        const PINNED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
        let manifest = parse(&format!(
            "{}build = [\"true\"]\n[[image.artifact]]\nurl = \"https://example.test/tool.tgz\"\nsha256 = \"{PINNED}\"\ninto = \"/tmp/tool.tgz\"\n",
            minimal()
        ))
        .expect("valid");
        let digests = referenced_digests(&manifest);
        assert_eq!(digests[0], recipe_digest(&manifest));
        assert!(digests.contains(&DIGEST.to_owned()), "{digests:?}");
        assert!(digests.contains(&PINNED.to_owned()), "{digests:?}");
    }

    fn full(extra: &str) -> Manifest {
        parse(&text(extra)).expect("valid")
    }

    /// A manifest whose `[image]` carries `extra`. `build` lines and
    /// `[[image.artifact]]` tables belong to that table, so they go after
    /// its header, not above it where `text` puts things.
    fn image(extra: &str) -> String {
        format!("{}{extra}", text(""))
    }

    /// `declarations` resolved as a bare start would: no `--env` flags.
    /// What these tests pin is the fold above, not the resolver, which
    /// `boxenv` proves for itself.
    fn box_env(manifest: &Manifest, host: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        crate::boxenv::to_env(&crate::boxenv::resolve(
            &declarations(manifest),
            &[],
            host,
            &BTreeMap::new(),
        ))
    }

    fn host(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_version_and_a_base_are_enough() {
        let manifest = parse(&minimal()).expect("valid");
        assert_eq!(manifest.image.base, "https://example.test/rootfs.tar.gz");
        assert!(manifest.image.packages.is_empty());
        assert!(manifest.agent.run.is_empty());
        assert_eq!(manifest.access.dns, None);
        assert_eq!(manifest.agent.preflight, None);
        assert!(manifest.env.is_empty());
    }

    #[test]
    fn a_manifest_from_another_version_is_refused_not_guessed_at() {
        let text = minimal().replace(
            &format!("version = {VERSION}"),
            &format!("version = {}", VERSION + 1),
        );
        assert_eq!(
            parse(&text),
            Err(ManifestError::Version((VERSION + 1).to_string()))
        );
    }

    /// The version key earns its place on exactly this case: a file whose
    /// keys have been renamed since. It must say which version it is, not
    /// fail on the first key that moved.
    #[test]
    fn a_manifest_from_before_the_keys_were_renamed_is_named_as_old() {
        let text = "version = \"v1alpha1\"\nrootfs = \"https://x.test/r.tar.gz\"\n";
        let err = parse(text).unwrap_err();
        assert_eq!(err, ManifestError::Version("\"v1alpha1\"".to_owned()));
        assert!(err.to_string().contains("version 1"), "{err}");
    }

    #[test]
    fn a_manifest_that_names_no_version_is_refused() {
        let text = minimal().replace(&format!("version = {VERSION}\n"), "");
        assert_eq!(
            parse(&text),
            Err(ManifestError::Version("nothing".to_owned()))
        );
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let err = parse(&format!("{}packagez = [\"git\"]\n", minimal())).unwrap_err();
        assert!(
            matches!(err, ManifestError::Syntax(ref d) if d.contains("packagez")),
            "{err}"
        );
    }

    #[test]
    fn a_short_digest_is_refused() {
        let text = minimal().replace(DIGEST, "abc123");
        assert_eq!(
            parse(&text),
            Err(ManifestError::Sha256NotHex("abc123".to_owned()))
        );
    }

    #[test]
    fn an_uppercase_digest_is_refused_so_comparisons_stay_simple() {
        let text = minimal().replace(DIGEST, &DIGEST.to_uppercase());
        assert!(matches!(parse(&text), Err(ManifestError::Sha256NotHex(_))));
    }

    #[test]
    fn an_agent_wormhole_cannot_launch_is_refused_at_parse_time() {
        let text = format!("{}[agent]\nrun = \"telepathy\"\n", minimal());
        assert_eq!(
            parse(&text),
            Err(ManifestError::UnknownAgent("telepathy".to_owned()))
        );
    }

    #[test]
    fn the_agent_command_bypasses_permissions_because_the_box_holds_the_line() {
        let manifest = full("[agent]\nrun = \"claude\"\n");
        assert_eq!(
            agent_command(&manifest),
            Ok(vec![
                "claude".to_owned(),
                "--dangerously-skip-permissions".to_owned()
            ])
        );
    }

    #[test]
    fn without_a_hook_the_agent_is_the_command_itself() {
        let manifest = full("[agent]\nrun = \"claude\"\n");
        assert_eq!(launch_command(&manifest), agent_command(&manifest));
    }

    /// The default hands the box no login of yours: a manifest that says
    /// nothing about credentials starts clean, and `/login` inside the
    /// box is the whole story. The two ways to hand one in are named.
    #[test]
    fn a_manifest_starts_with_no_login_unless_it_asks_for_one() {
        assert_eq!(
            full("[agent]\nrun = \"claude\"\n").access.credentials,
            Credentials::None
        );
        for (spelled, wanted) in [
            ("none", Credentials::None),
            ("copy", Credentials::Copy),
            ("share", Credentials::Share),
        ] {
            let manifest = full(&format!("[access]\ncredentials = \"{spelled}\"\n"));
            assert_eq!(manifest.access.credentials, wanted);
            assert_eq!(wanted.to_string(), spelled);
        }
        assert!(parse(&text("[access]\ncredentials = \"borrow\"\n")).is_err());
    }

    /// Each agent names the files its login lives in, relative to a home;
    /// a copy or a share moves exactly those and nothing else. No agent,
    /// nothing to move.
    #[test]
    fn each_agent_names_its_login_files() {
        assert_eq!(
            credential_files(&full("[agent]\nrun = \"claude\"\n")),
            &[".claude/.credentials.json"]
        );
        assert_eq!(
            credential_files(&full("[agent]\nrun = \"codex\"\n")),
            &[".codex/auth.json"]
        );
        assert_eq!(
            credential_files(&full("[agent]\nrun = \"grok\"\n")),
            &[".grok/auth.json"]
        );
        assert!(credential_files(&full("")).is_empty());
    }

    /// `share` binds the credential file itself, and a rename onto a
    /// mount point is `EBUSY` — so an agent that saves its login by
    /// replacing the file cannot be handed a shared one. Refused before
    /// the box starts, because the failure otherwise lands inside the
    /// agent as `Resource busy (os error 16)` after a whole login.
    #[test]
    fn an_agent_that_replaces_its_login_file_is_refused_a_shared_one() {
        let refusal = credentials_refusal(&full("[agent]\nrun = \"grok\"\n"), Credentials::Share)
            .expect("refused");
        assert!(refusal.contains(".grok/auth.json"), "{refusal}");
        assert!(refusal.contains("copy"), "{refusal}");
    }

    /// Codex writes its `auth.json` in place — verified by writing one
    /// through a bind mount — so sharing does what it says.
    #[test]
    fn an_agent_that_writes_its_login_file_in_place_may_share_it() {
        for agent in ["claude", "codex"] {
            let manifest = full(&format!("[agent]\nrun = \"{agent}\"\n"));
            assert_eq!(credentials_refusal(&manifest, Credentials::Share), None);
        }
        assert_eq!(credentials_refusal(&full(""), Credentials::Share), None);
    }

    /// Only a share binds anything. A copy is a plain file in the box
    /// home, which any agent may replace.
    #[test]
    fn a_copy_is_never_refused_because_nothing_is_bound() {
        for mode in [Credentials::None, Credentials::Copy] {
            assert_eq!(
                credentials_refusal(&full("[agent]\nrun = \"grok\"\n"), mode),
                None
            );
        }
    }

    /// The keys the broker took with it are refused by name, with where
    /// to look now, instead of serde's "unknown field".
    #[test]
    fn a_manifest_still_naming_the_broker_is_told_what_replaced_it() {
        for key in [
            "network = \"host\"",
            "broker = false",
            "egress = [\"crates.io\"]",
        ] {
            let name = key.split(' ').next().expect("a key");
            assert_eq!(
                parse(&text(&format!("[access]\n{key}\n"))),
                Err(ManifestError::Removed(name.to_owned())),
                "{key}"
            );
        }
    }

    /// The terminal is wormhole's business: it carries the description in,
    /// so it names the fallback too. A manifest that says otherwise wins.
    #[test]
    fn every_box_is_told_a_terminal_without_the_manifest_saying_so() {
        let bare = full("");
        assert_eq!(
            box_env(&bare, &host(&[])).get("TERM").map(String::as_str),
            Some("xterm-256color")
        );
        let told = box_env(
            &bare,
            &host(&[("TERM", "xterm-ghostty"), ("COLORTERM", "truecolor")]),
        );
        assert_eq!(told.get("TERM").map(String::as_str), Some("xterm-ghostty"));
        assert_eq!(told.get("COLORTERM").map(String::as_str), Some("truecolor"));
        assert_eq!(told.get("TERM_PROGRAM").map(String::as_str), Some(""));
        let own = full("[env.TERM]\nfixed = \"vt100\"\n");
        assert_eq!(
            box_env(&own, &host(&[("TERM", "xterm-ghostty")]))
                .get("TERM")
                .map(String::as_str),
            Some("vt100")
        );
    }

    /// The box speaks to the API for itself, so nothing about a proxy or
    /// a stand-in key ever reaches its environment.
    #[test]
    fn a_box_is_told_nothing_about_a_proxy_or_a_stand_in_key() {
        for agent in KNOWN_AGENTS.iter().map(|agent| agent.name) {
            let env = box_env(&full(&format!("[agent]\nrun = \"{agent}\"\n")), &host(&[]));
            for name in [
                "HTTPS_PROXY",
                "https_proxy",
                "NO_PROXY",
                "NODE_USE_ENV_PROXY",
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_API_KEY",
            ] {
                assert!(!env.contains_key(name), "{agent}: {env:?}");
            }
        }
    }

    /// The hook runs from its seeded copy in the box home, so a role's
    /// hook works even though the role directory is not in the box.
    #[test]
    fn a_hook_runs_first_from_its_seed_and_then_the_agent_replaces_the_shell() {
        let manifest = full("[agent]\nrun = \"claude\"\npreflight = \"hooks/pre.sh\"\n");
        let command = launch_command(&manifest).expect("command");
        assert_eq!(command[0], "/bin/sh");
        assert_eq!(command[1], "-c");
        let script = &command[2];
        let hook = script
            .find(&format!("/bin/sh \"$HOME/{PREFLIGHT_SEED}\""))
            .expect("seeded hook");
        let agent = script.find("exec 'claude'").expect("exec");
        assert!(script.starts_with("set -eu\n"), "{script}");
        assert!(hook < agent, "{script}");
    }

    /// The manifest's own hook path is a host-side name; inside the box
    /// only the seed exists, so the path must never reach the script.
    #[test]
    fn the_manifests_hook_path_never_reaches_the_launch_script() {
        let manifest = full("[agent]\nrun = \"claude\"\npreflight = \"a'; rm -rf /; '\"\n");
        let command = launch_command(&manifest).expect("command");
        assert!(!command[2].contains("rm -rf"), "{}", command[2]);
    }

    #[test]
    fn attach_without_a_command_runs_the_agent_or_a_shell() {
        assert_eq!(
            attach_command(Some("claude")),
            vec!["claude", "--dangerously-skip-permissions"]
        );
        assert_eq!(attach_command(None), vec!["/bin/sh"]);
        assert_eq!(attach_command(Some("telepathy")), vec!["/bin/sh"]);
    }

    #[test]
    fn no_agent_means_nothing_to_run() {
        assert_eq!(agent_command(&full("")), Err(ManifestError::NoAgent));
    }

    #[test]
    fn a_declared_variable_takes_the_hosts_value_when_it_has_one() {
        let manifest = full("[env.CONTEXT7_API_KEY]\ndefault = \"\"\n");
        let env = box_env(&manifest, &host(&[("CONTEXT7_API_KEY", "secret")]));
        assert_eq!(
            env.get("CONTEXT7_API_KEY").map(String::as_str),
            Some("secret")
        );
    }

    #[test]
    fn a_declared_variable_falls_back_to_its_default() {
        let manifest = full("[env.CAVEMAN_DEFAULT_MODE]\ndefault = \"ultra\"\n");
        let env = box_env(&manifest, &host(&[]));
        assert_eq!(
            env.get("CAVEMAN_DEFAULT_MODE").map(String::as_str),
            Some("ultra")
        );
    }

    /// `SHELL` names a path inside the box. The host's `/usr/bin/zsh` is
    /// not in there, and letting it win breaks every tool that reads it.
    #[test]
    fn a_fixed_value_ignores_the_host() {
        let manifest = full("[env.SHELL]\nfixed = \"/bin/bash\"\n");
        let env = box_env(&manifest, &host(&[("SHELL", "/usr/bin/zsh")]));
        assert_eq!(env.get("SHELL").map(String::as_str), Some("/bin/bash"));
    }

    /// A fixed value already answers the question `default` asks, so a
    /// manifest combining them is confused, not flexible.
    #[test]
    fn a_fixed_variable_takes_no_default() {
        let extra = "[env.SHELL]\nfixed = \"/bin/bash\"\ndefault = \"/bin/sh\"\n";
        assert!(
            matches!(parse(&text(extra)), Err(ManifestError::EnvFixedConflict(_))),
            "{extra}"
        );
    }

    /// The keys the broker era added and nothing used. Refused by name
    /// rather than read and ignored.
    #[test]
    fn required_and_secret_are_no_longer_keys() {
        for extra in [
            "[env.SENTRY_TOKEN]\nrequired = true\n",
            "[env.SENTRY_TOKEN]\nsecret = true\n",
        ] {
            assert!(
                matches!(parse(&text(extra)), Err(ManifestError::Syntax(_))),
                "{extra}"
            );
        }
    }

    #[test]
    fn a_variable_can_be_asked_for() {
        let manifest = full("[env.CONTEXT7_API_KEY]\nask = true\n");
        assert!(manifest.env["CONTEXT7_API_KEY"].ask);
    }

    /// A value that is fixed or defaulted is never missing, so asking for
    /// it could never happen; a manifest combining them is confused.
    #[test]
    fn an_asked_variable_takes_no_fixed_and_no_default() {
        for extra in [
            "[env.KEY]\nask = true\nfixed = \"x\"\n",
            "[env.KEY]\nask = true\ndefault = \"x\"\n",
        ] {
            assert!(
                matches!(parse(&text(extra)), Err(ManifestError::EnvAskConflict(_))),
                "{extra}"
            );
        }
    }

    #[test]
    fn an_undeclared_host_variable_never_reaches_the_box() {
        let env = box_env(&full(""), &host(&[("AWS_SECRET_ACCESS_KEY", "leak")]));
        assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"), "{env:?}");
    }

    #[test]
    fn a_model_becomes_the_variable_claude_reads() {
        let manifest = full("[agent]\nrun = \"claude\"\nmodel = \"claude-sonnet-4-6\"\n");
        let env = box_env(&manifest, &host(&[]));
        assert_eq!(
            env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("claude-sonnet-4-6")
        );
    }

    /// The hook runs before the agent, and the agent still replaces the
    /// shell, so it stays PID 1 and owns the terminal.
    #[test]
    fn the_hook_runs_first_and_the_agent_still_replaces_the_shell() {
        let manifest = full("[agent]\nrun = \"claude\"\npreflight = \"p.sh\"\n");
        let command = launch_command(&manifest).expect("command");
        let script = command.last().expect("script");
        let hook = script.find(PREFLIGHT_SEED).expect("hook");
        let agent = script.find("exec ").expect("exec");
        assert!(hook < agent, "{script}");
    }

    #[test]
    fn nothing_to_install_means_no_script() {
        assert_eq!(build_script(&full("")), None);
    }

    #[test]
    fn the_script_writes_package_sources_then_installs_then_builds() {
        let manifest = parse(&format!(
            "version = {VERSION}\n[image]\nbase = \"https://x.test/r.tar.gz\"\nbase_sha256 = \"{DIGEST}\"\npackage_sources = [\"http://cdn.test/main\"]\npackages = [\"nodejs\", \"npm\"]\nbuild = [\"npm i -g claude\"]\n"
        ))
        .expect("valid");
        let script = build_script(&manifest).expect("script");
        let sources = script.find("/etc/apk/repositories").expect("sources");
        let install = script.find("apk add").expect("install");
        let build = script.find("npm i -g claude").expect("build");
        assert!(script.starts_with("set -eu\n"), "{script}");
        assert!(sources < install && install < build, "{script}");
        assert!(script.contains("apk add --no-cache nodejs npm"), "{script}");
    }

    /// The bug this pins: a build died on `exit 1` and named no command,
    /// because nothing said which line was running. Diagnosing it meant
    /// rebuilding the box by hand. Every step announces itself first, so
    /// the last thing printed is the thing that failed.
    #[test]
    fn every_step_says_itself_before_it_runs() {
        let manifest = parse(&image(
            "packages = [\"git\"]\nbuild = [\"npm i -g claude\"]\n",
        ))
        .expect("valid");
        let script = build_script(&manifest).expect("script");
        let said = script.find("wormhole: npm i -g claude").expect("announced");
        let ran = script.find("\nnpm i -g claude\n").expect("ran");
        assert!(said < ran, "{script}");
        assert!(script.contains("wormhole: apk add"), "{script}");
    }

    /// The bug this pins: Alpine's `ca-certificates` trigger runs
    /// `update-ca-certificates > /dev/null 2>&1` and then `exit 0`, so a
    /// half-written bundle is reported as a successful install and every
    /// later https fetch fails with a verify error naming no cause.
    #[test]
    fn an_empty_trust_store_stops_the_build_where_apk_would_not() {
        let manifest = parse(&image(
            "packages = [\"ca-certificates\"]\nbuild = [\"wget https://x.test/f\"]\n",
        ))
        .expect("valid");
        let script = build_script(&manifest).expect("script");
        let checked = script.find(crate::ca::CA_BUNDLE_IN_BOX).expect("checked");
        let fetched = script.find("wget https://x.test/f\n").expect("fetch");
        assert!(checked < fetched, "{script}");
        assert!(script.contains("BEGIN CERTIFICATE"), "{script}");
    }

    /// A recipe that installs nothing has no store to judge, and must not
    /// grow a check that can only fail.
    #[test]
    fn a_recipe_that_installs_no_package_checks_no_store() {
        let manifest = parse(&image("build = [\"true\"]\n")).expect("valid");
        let script = build_script(&manifest).expect("script");
        assert!(!script.contains("BEGIN CERTIFICATE"), "{script}");
    }

    /// An artifact is the base rule applied to every other file a build
    /// needs: named by URL, proved by digest. The box never opens the
    /// connection, so no trust store of its own can be wrong about it.
    #[test]
    fn a_recipe_names_a_file_by_url_and_digest() {
        let manifest = parse(&image(&format!(
            "build = [\"true\"]\n[[image.artifact]]\nurl = \"https://x.test/f\"\n\
             sha256 = \"{DIGEST}\"\ninto = \"/tmp/f\"\n"
        )))
        .expect("valid");
        assert_eq!(manifest.image.artifacts.len(), 1);
        let artifact = &manifest.image.artifacts[0];
        assert_eq!(artifact.url, "https://x.test/f");
        assert_eq!(artifact.sha256, DIGEST);
        assert_eq!(artifact.into, "/tmp/f");
    }

    #[test]
    fn an_artifact_digest_is_held_to_the_rule_the_base_is() {
        let text = image(
            "build = [\"true\"]\n[[image.artifact]]\nurl = \"https://x.test/f\"\n\
             sha256 = \"nothex\"\ninto = \"/tmp/f\"\n",
        );
        assert!(matches!(parse(&text), Err(ManifestError::Sha256NotHex(_))));
    }

    /// An artifact is bound over the build box's root, which is the image
    /// being built. Anywhere but the scratch tmpfs would leave an empty
    /// file behind in the finished image once the mount goes — so the
    /// scratch is the only place one may land, and `..` is refused before
    /// the prefix is believed.
    #[test]
    fn an_artifact_lands_in_the_build_scratch_or_nowhere() {
        for bad in ["tmp/f", "/tmp/../etc/passwd", "/", "/opt/f", "/tmp"] {
            let text = image(&format!(
                "build = [\"true\"]\n[[image.artifact]]\nurl = \"https://x.test/f\"\n\
                 sha256 = \"{DIGEST}\"\ninto = \"{bad}\"\n"
            ));
            assert!(
                matches!(parse(&text), Err(ManifestError::ArtifactPath(_))),
                "{bad} was accepted"
            );
        }
    }

    /// An artifact is installed bytes, so it belongs to the image's
    /// identity exactly as a package does. Its digest is in the recipe,
    /// not just its URL: two files at one address are two images.
    #[test]
    fn the_recipe_carries_every_artifact_and_its_digest() {
        let other = DIGEST.replace('0', "f");
        let recipe = |sha| {
            parse(&image(&format!(
                "build = [\"true\"]\n[[image.artifact]]\nurl = \"u\"\n\
                 sha256 = \"{sha}\"\ninto = \"/tmp/f\"\n"
            )))
            .expect("valid")
        };
        let one = recipe(DIGEST);
        let two = recipe(&other);
        assert_eq!(
            recipe_text(&one),
            format!(
                "rootfs https://example.test/rootfs.tar.gz\nsha256 {DIGEST}\n\
                 artifact u {DIGEST} /tmp/f\nsetup true\n"
            )
        );
        assert_ne!(recipe_digest(&one), recipe_digest(&two));
    }

    /// The digest answers "what is in this image". An artifact no build
    /// line can reach changes nothing about that, so two byte-identical
    /// images would sit under two digests — the digest would be saying
    /// something untrue. Refused rather than fetched and ignored.
    #[test]
    fn an_artifact_in_a_recipe_that_runs_nothing_is_refused() {
        let artifact =
            format!("[[image.artifact]]\nurl = \"u\"\nsha256 = \"{DIGEST}\"\ninto = \"/tmp/f\"\n");
        assert!(matches!(
            parse(&image(&artifact)),
            Err(ManifestError::ArtifactWithNoBuild(_))
        ));
        assert!(parse(&image(&format!("build = [\"true\"]\n{artifact}"))).is_ok());
    }

    /// Two artifacts at one destination is one bind over another: the
    /// second wins, the first is fetched and never read, and the recipe
    /// says something that is not true of the image it builds.
    #[test]
    fn two_artifacts_may_not_land_on_one_path() {
        let both = format!(
            "build = [\"true\"]\n\
             [[image.artifact]]\nurl = \"one\"\nsha256 = \"{DIGEST}\"\ninto = \"/tmp/f\"\n\
             [[image.artifact]]\nurl = \"two\"\nsha256 = \"{DIGEST}\"\ninto = \"/tmp/f\"\n"
        );
        assert!(matches!(
            parse(&image(&both)),
            Err(ManifestError::ArtifactPath(_))
        ));
    }

    /// The recipe is `[image]` and nothing else, which is what makes the
    /// rebuild rule readable off the file's shape.
    #[test]
    fn the_recipe_ignores_every_table_but_image() {
        let bare = full("");
        let decorated = full(concat!(
            "name = \"decorated\"\n",
            "[agent]\nrun = \"claude\"\n",
            "[access]\ngrants = [\"~/.ssh\"]\ndns = \"9.9.9.9\"\nhost_ca = true\n",
            "[runtime]\nrootfs = \"readonly\"\n",
            "[limits]\ncpu = \"1\"\n",
            "[env.X]\ndefault = \"1\"\n",
        ));
        assert_eq!(recipe_text(&bare), recipe_text(&decorated));
    }

    /// The digest names what is in the image, so a manifest that installs
    /// the same bytes must keep the same digest however its keys are
    /// spelled. This pins the wire format against exactly that: a rename
    /// that moves it would silently orphan every image on every host.
    #[test]
    fn the_recipe_text_does_not_move_when_a_key_is_renamed() {
        let manifest = parse(&format!(
            "version = {VERSION}\n[image]\nbase = \"u\"\nbase_sha256 = \"{DIGEST}\"\npackage_sources = [\"s\"]\npackages = [\"p\"]\nbuild = [\"b\"]\n"
        ))
        .expect("valid");
        assert_eq!(
            recipe_text(&manifest),
            format!("rootfs u\nsha256 {DIGEST}\nrepository s\npackage p\nsetup b\n")
        );
    }

    #[test]
    fn the_recipe_changes_when_an_installed_thing_changes() {
        let one = parse(&minimal().replace("\n[image]", "\n[image]\npackages = [\"git\"]"));
        let two =
            parse(&minimal().replace("\n[image]", "\n[image]\npackages = [\"git\", \"curl\"]"));
        assert_ne!(
            recipe_text(&one.expect("valid")),
            recipe_text(&two.expect("valid"))
        );
    }

    /// The union is the point, and it is deliberate: `SSL_CERT_DIR`
    /// defaults to the image's own `/etc/ssl/certs`, whose hash links
    /// verify on their own, and naming it here would replace the image's
    /// roots rather than add the host's to them. Measured: with
    /// `SSL_CERT_DIR` left alone, a bundle holding one unrelated root
    /// still verifies everything the image's roots cover.
    #[test]
    fn trusting_the_host_ca_adds_to_the_images_roots_rather_than_replacing_them() {
        assert!(
            !crate::ca::READERS.contains(&"SSL_CERT_DIR"),
            "naming SSL_CERT_DIR would drop the image's own roots"
        );
    }

    /// The bug this pins: `host_ca = true` mounted the host's bundle and
    /// stopped. The agent is Node, Node carries its own roots and never
    /// reads a bundle off the filesystem, so on a network that intercepts
    /// TLS the one program wormhole exists to run still refused to connect.
    #[test]
    fn trusting_the_host_ca_points_every_client_at_the_bundle() {
        let env = box_env(&full("[access]\nhost_ca = true\n"), &BTreeMap::new());
        for name in crate::ca::READERS {
            assert_eq!(
                env.get(name).map(String::as_str),
                Some(crate::ca::CA_BUNDLE_IN_BOX),
                "{name}"
            );
        }
    }

    /// Off unless asked for. A box that was never told to trust the host
    /// must not be pointed at a bundle it does not have.
    #[test]
    fn no_host_ca_names_no_bundle() {
        let env = box_env(&full(""), &BTreeMap::new());
        for name in crate::ca::READERS {
            assert!(!env.contains_key(name), "{name}");
        }
    }

    /// A manifest that declares one of these names itself means it.
    #[test]
    fn a_declared_bundle_beats_the_one_host_ca_implies() {
        let manifest = full(
            "[access]\nhost_ca = true\n\
             [env.NODE_EXTRA_CA_CERTS]\nfixed = \"/etc/mine.pem\"\n",
        );
        let env = box_env(&manifest, &BTreeMap::new());
        assert_eq!(
            env.get("NODE_EXTRA_CA_CERTS").map(String::as_str),
            Some("/etc/mine.pem")
        );
        // The rest still get the mounted bundle.
        assert_eq!(
            env.get("GIT_SSL_CAINFO").map(String::as_str),
            Some(crate::ca::CA_BUNDLE_IN_BOX)
        );
    }

    /// A project either wrote out a whole recipe or everyone working on it
    /// typed `--role X` by hand, every time, correctly, forever. There was
    /// no third option; this is it.
    #[test]
    fn a_workspace_can_hand_its_box_to_a_role() {
        let sha = "1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e";
        let text = format!("version = 1\nrole = \"github:you/java@{sha}\"\n");
        assert_eq!(
            names_a_role(&text),
            Ok(Some(format!("github:you/java@{sha}")))
        );
    }

    /// A role's own manifest naming a role would chain, and roles do not.
    /// The rule used to be enforced only by accident — `Manifest` has no
    /// `role` field, so serde said `unknown field` and named the key
    /// rather than the rule.
    #[test]
    fn a_manifest_that_hands_over_is_refused_where_a_recipe_was_wanted() {
        let refused = parse("version = 1\nrole = \"alphaca\"\n").expect_err("no recipe");
        assert_eq!(refused, ManifestError::HandsToARole);
        let said = refused.to_string();
        assert!(said.contains("role"), "{said}");
        assert!(!said.contains("unknown field"), "{said}");
    }

    /// A manifest that carries its own recipe hands over to nothing.
    #[test]
    fn a_manifest_with_a_recipe_names_no_role() {
        assert_eq!(names_a_role(&minimal()), Ok(None));
    }

    /// Not inheritance. Merging a role's recipe with a local one is where
    /// a key like this turns into an override system nobody can predict,
    /// so both together is refused rather than resolved.
    #[test]
    fn naming_a_role_and_carrying_a_recipe_is_refused() {
        let text = text("role = \"alphaca\"\n");
        assert_eq!(names_a_role(&text), Err(ManifestError::RoleAndImage));
    }

    /// An empty `role` is a line somebody meant to fill in. Said here,
    /// rather than resolved as a role with no name.
    #[test]
    fn a_role_that_names_nothing_is_refused() {
        for empty in ["", "   "] {
            let text = format!("version = 1\nrole = \"{empty}\"\n");
            assert!(names_a_role(&text).is_err(), "{empty:?}");
        }
    }

    /// Trusting the host's CA store is a per-box decision, off unless the
    /// manifest says so — trust must be named, never assumed.
    #[test]
    fn trusting_the_host_ca_is_declared_and_off_by_default() {
        assert!(full("[access]\nhost_ca = true\n").access.host_ca);
        assert!(!full("").access.host_ca);
    }

    #[test]
    fn a_resolver_that_is_not_an_address_is_refused() {
        let text = format!("{}[access]\ndns = \"my-router\"\n", minimal());
        assert!(matches!(parse(&text), Err(ManifestError::Syntax(_))));
    }

    #[test]
    fn extra_instructions_are_kept_as_written() {
        let manifest = full("[agent]\ninstructions = \"ROLE.md\"\n");
        assert_eq!(manifest.agent.instructions.as_deref(), Some("ROLE.md"));
        assert_eq!(full("").agent.instructions, None);
    }

    #[test]
    fn no_agent_means_no_instructions_file() {
        assert_eq!(instructions_pointer(&full("")), None);
    }

    /// The canonical text lives once, at the home root; claude's own file
    /// is an import line pointing there. `@AGENTS.md` alone would resolve
    /// inside `.claude/`, which is why the pointer spells the home out.
    #[test]
    fn claudes_instructions_file_is_an_import_of_the_canonical_one() {
        let manifest = full("[agent]\nrun = \"claude\"\n");
        assert_eq!(
            instructions_pointer(&manifest),
            Some((".claude/CLAUDE.md", Pointer::Import("@~/AGENTS.md\n")))
        );
        assert_eq!(INSTRUCTIONS_SEED, "AGENTS.md");
    }

    /// Codex reads `$CODEX_HOME/AGENTS.md` and has no import syntax, so
    /// its pointer is a symlink to the same canonical file.
    #[test]
    fn codexs_instructions_are_a_symlink_into_its_config_dir() {
        let manifest = full("[agent]\nrun = \"codex\"\n");
        assert_eq!(
            instructions_pointer(&manifest),
            Some((".codex/AGENTS.md", Pointer::Symlink))
        );
    }

    #[test]
    fn codex_bypasses_its_own_sandbox_because_the_box_holds_the_line() {
        let manifest = full("[agent]\nrun = \"codex\"\n");
        assert_eq!(
            agent_command(&manifest),
            Ok(vec![
                "codex".to_owned(),
                "--dangerously-bypass-approvals-and-sandbox".to_owned()
            ])
        );
    }

    #[test]
    fn a_pointer_symlink_climbs_back_to_the_canonical_file() {
        assert_eq!(pointer_link(".codex/AGENTS.md"), "../AGENTS.md");
        assert_eq!(pointer_link(".grok/rules/AGENTS.md"), "../../AGENTS.md");
    }

    #[test]
    fn groks_instructions_are_a_symlink_into_the_rules_it_always_reads() {
        let manifest = full("[agent]\nrun = \"grok\"\n");
        assert_eq!(
            instructions_pointer(&manifest),
            Some((".grok/rules/AGENTS.md", Pointer::Symlink))
        );
    }

    #[test]
    fn a_grok_model_reaches_it_as_the_variable_grok_reads() {
        let manifest = full("[agent]\nrun = \"grok\"\nmodel = \"grok-code-fast-1\"\n");
        let env = box_env(&manifest, &host(&[]));
        assert_eq!(
            env.get("GROK_DEFAULT_MODEL").map(String::as_str),
            Some("grok-code-fast-1")
        );
        assert!(!env.contains_key("ANTHROPIC_MODEL"), "{env:?}");
    }

    #[test]
    fn grok_starts_approved_and_trusting_because_the_box_holds_the_line() {
        let manifest = full("[agent]\nrun = \"grok\"\n");
        assert_eq!(
            agent_command(&manifest),
            Ok(vec![
                "grok".to_owned(),
                "--always-approve".to_owned(),
                "--trust".to_owned()
            ])
        );
    }

    /// The bug this pins: the model was exported as ANTHROPIC_MODEL for
    /// *any* agent — a line codex never reads, saying the model was passed
    /// when it was not. The model reaches codex through its config file.
    #[test]
    fn a_codex_model_is_not_exported_as_an_anthropic_variable() {
        let manifest = full("[agent]\nrun = \"codex\"\nmodel = \"gpt-5-codex\"\n");
        let env = box_env(&manifest, &host(&[]));
        assert!(!env.contains_key("ANTHROPIC_MODEL"), "{env:?}");
    }

    #[test]
    fn each_agent_says_which_files_a_start_writes() {
        let files = |extra: &str| -> Vec<&str> {
            config_writes(&full(extra), "/w")
                .iter()
                .map(|write| write.file)
                .collect()
        };
        assert_eq!(
            files("[agent]\nrun = \"claude\"\n"),
            [".claude.json", ".claude/settings.json"]
        );
        assert_eq!(files("[agent]\nrun = \"codex\"\n"), [".codex/config.toml"]);
        assert_eq!(files("[agent]\nrun = \"grok\"\n"), [".grok/config.toml"]);
        assert!(files("").is_empty());
    }

    /// A manifest's `model` must have somewhere to go: an env var the
    /// agent reads, or a key in a config file a start writes. An agent
    /// with neither would take a `model` and silently drop it.
    #[test]
    fn every_agent_has_somewhere_to_put_a_model() {
        for agent in &KNOWN_AGENTS {
            assert!(
                agent.model_env.is_some() || agent.config.iter().any(|f| f.model_key.is_some()),
                "{} takes a model nowhere",
                agent.name
            );
        }
    }

    /// The box is the sandbox, so claude is told it is in one, and a
    /// safeguard flag ends a turn rather than moving the session to an
    /// older model. The host cannot undo any of it.
    #[test]
    fn a_claude_box_tells_claude_what_the_box_settles() {
        let env = box_env(
            &full("[agent]\nrun = \"claude\"\n"),
            &host(&[("IS_SANDBOX", "0")]),
        );
        for (name, value) in [
            ("CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK", "1"),
            ("CLAUDE_CODE_RETRY_WATCHDOG", "1"),
            ("CLAUDE_CODE_SANDBOXED", "1"),
            ("IS_SANDBOX", "1"),
        ] {
            assert_eq!(env.get(name).map(String::as_str), Some(value), "{name}");
        }
    }

    #[test]
    fn each_agent_is_told_only_its_own_settings() {
        let grok = box_env(&full("[agent]\nrun = \"grok\"\n"), &host(&[]));
        assert_eq!(grok.get("GROK_FOLDER_TRUST").map(String::as_str), Some("0"));
        assert!(!grok.contains_key("IS_SANDBOX"), "{grok:?}");
        let codex = box_env(&full("[agent]\nrun = \"codex\"\n"), &host(&[]));
        assert!(!codex.contains_key("IS_SANDBOX"), "{codex:?}");
        assert!(!codex.contains_key("GROK_FOLDER_TRUST"), "{codex:?}");
    }

    /// A role that wants a flagged message to switch models after all
    /// says so in its own `[env]`.
    #[test]
    fn a_manifest_takes_back_a_setting_by_declaring_it() {
        let env = box_env(
            &full(
                "[agent]\nrun = \"claude\"\n[env.CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK]\nfixed = \"\"\n",
            ),
            &host(&[]),
        );
        assert_eq!(
            env.get("CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK")
                .map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn instructions_are_the_default_alone_when_no_extra_exist() {
        assert_eq!(
            compose_instructions("be sharp\n", None),
            "be sharp\n".to_owned()
        );
    }

    /// The extra instructions come after the default so they win where
    /// they disagree, and a blank line keeps the two texts apart.
    #[test]
    fn extra_instructions_follow_the_default_after_a_blank_line() {
        let text = compose_instructions("be sharp", Some("be architect"));
        assert_eq!(text, "be sharp\n\nbe architect\n");
    }

    #[test]
    fn a_box_name_is_kept_and_defaults_to_none() {
        assert_eq!(
            full("name = \"architect\"\n").name.as_deref(),
            Some("architect")
        );
        assert_eq!(full("").name, None);
    }

    #[test]
    fn a_preflight_hook_is_kept_as_written() {
        let manifest = full("[agent]\npreflight = \"hooks/preflight.sh\"\n");
        assert_eq!(
            manifest.agent.preflight.as_deref(),
            Some("hooks/preflight.sh")
        );
    }

    #[test]
    fn a_run_list_names_every_product_the_role_may_launch() {
        let manifest = full("[agent]\nrun = [\"claude\", \"codex\", \"grok\"]\n");
        assert_eq!(
            manifest.agent.run,
            vec!["claude".to_owned(), "codex".to_owned(), "grok".to_owned()]
        );
        assert_eq!(manifest.agent.product(), Some("claude"));
    }

    #[test]
    fn an_empty_run_list_is_refused_rather_than_read_as_no_agent() {
        let err = parse(&text("[agent]\nrun = []\n")).unwrap_err();
        assert!(
            matches!(err, ManifestError::Syntax(ref d) if d.contains("at least one")),
            "{err}"
        );
    }

    #[test]
    fn a_run_list_with_a_duplicate_is_refused() {
        assert_eq!(
            parse(&text("[agent]\nrun = [\"claude\", \"claude\"]\n")),
            Err(ManifestError::DuplicateAgent("claude".to_owned()))
        );
    }

    #[test]
    fn a_run_list_cannot_carry_a_model() {
        assert_eq!(
            parse(&text(
                "[agent]\nrun = [\"claude\", \"grok\"]\nmodel = \"claude-fable-5\"\n"
            )),
            Err(ManifestError::ModelOnManyRuns)
        );
    }

    #[test]
    fn bind_picks_the_first_name_when_none_is_asked_for() {
        let manifest = full("[agent]\nrun = [\"claude\", \"grok\"]\n");
        assert_eq!(bind(&manifest, None), Ok(Some("claude")));
        assert_eq!(bind(&manifest, Some("grok")), Ok(Some("grok")));
    }

    /// The words a start says when it cannot guess which box. Pinned here
    /// so the CLI, the picker backing out, and the docs cannot drift.
    #[test]
    fn a_role_with_several_products_names_every_one_and_how_to_pick() {
        assert_eq!(
            need_run(&["claude".to_owned(), "codex".to_owned(), "grok".to_owned()]),
            "this role runs claude, codex, grok; pass --run claude|codex|grok"
        );
    }

    #[test]
    fn bind_refuses_a_product_the_role_does_not_offer() {
        let manifest = full("[agent]\nrun = [\"claude\"]\n");
        assert_eq!(
            bind(&manifest, Some("grok")),
            Err(ManifestError::ProductNotInRole {
                wanted: "grok".to_owned(),
                offered: vec!["claude".to_owned()],
            })
        );
        assert_eq!(
            bind(&full(""), Some("claude")),
            Err(ManifestError::ProductNotInRole {
                wanted: "claude".to_owned(),
                offered: Vec::new(),
            })
        );
    }

    #[test]
    fn bind_refuses_an_unknown_product_before_the_role_list() {
        let manifest = full("[agent]\nrun = [\"claude\", \"grok\"]\n");
        assert_eq!(
            bind(&manifest, Some("telepathy")),
            Err(ManifestError::UnknownAgent("telepathy".to_owned()))
        );
    }

    #[test]
    fn bound_narrows_the_recipe_to_the_product_this_start_runs() {
        let manifest = bound(
            full("[agent]\nrun = [\"claude\", \"grok\"]\n"),
            Some("grok"),
        )
        .expect("bound");
        assert_eq!(manifest.agent.run, vec!["grok".to_owned()]);
        assert_eq!(
            agent_command(&manifest),
            Ok(vec![
                "grok".to_owned(),
                "--always-approve".to_owned(),
                "--trust".to_owned()
            ])
        );
    }

    #[test]
    fn an_id_cannot_switch_the_product_a_home_already_runs() {
        assert_eq!(
            product_mismatch(Some("claude"), Some("grok")).as_deref(),
            Some("that box runs claude, not grok; omit --run to resume it")
        );
        assert_eq!(product_mismatch(Some("grok"), Some("grok")), None);
        assert_eq!(product_mismatch(None, Some("grok")), None);
    }

    #[test]
    fn the_bound_product_is_a_fixed_variable_the_preflight_can_read() {
        let manifest = bound(
            full("[agent]\nrun = [\"claude\", \"grok\"]\n"),
            Some("grok"),
        )
        .expect("bound");
        let env = box_env(&manifest, &host(&[]));
        assert_eq!(env.get(RUN_ENV).map(String::as_str), Some("grok"));
    }
}
