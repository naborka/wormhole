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

use serde::Deserialize;

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
    /// Which agent wormhole launches. Absent means a box with no agent.
    /// Not `name`: the box has one of those, and two keys spelled alike in
    /// adjacent tables is how a misplaced line becomes a valid one.
    #[serde(default)]
    pub run: Option<String>,
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

/// What the box can reach. Every field defaults to the answer that keeps
/// an agent working; each is one line away from the tighter one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    /// Host paths the box may see, as written — `~` allowed, expanded by
    /// the caller because a home directory is not a pure decision.
    #[serde(default)]
    pub grants: Vec<String>,
    /// The one resolver the build box uses — a build fetches what its
    /// recipe cannot pin — and the running box only under
    /// `network = "host"`; a routeless box could not reach one, and the
    /// broker's `CONNECT` leg resolves names host-side instead. Absent
    /// means no DNS anywhere.
    #[serde(default)]
    pub dns: Option<IpAddr>,
    /// `"none"` (the default) or `"host"`. With `"none"` the box gets a
    /// network namespace of its own holding nothing but loopback, so
    /// there is no route off the machine to take; what the box needs
    /// beyond that goes through the broker. `"host"` hands it every
    /// route the host has — the escape hatch, no longer the default.
    #[serde(default)]
    pub network: crate::run::Network,
    /// Reach everything through the host-side broker: the model API with
    /// no credential in the box, and the `egress` hosts by `CONNECT`.
    /// Unset it defaults to "this box runs an agent" — the pairing with
    /// `network = "none"` that makes the thesis true is what a bare
    /// manifest gets. `broker = false` says this box talks to nothing.
    #[serde(default)]
    pub broker: Option<bool>,
    /// Hosts the box may reach over HTTPS through the broker's `CONNECT`
    /// leg, exact names or one-level wildcards (`*.crates.io`). Empty is
    /// the baseline: nothing is reachable that is not named here.
    #[serde(default)]
    pub egress: Vec<String>,
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

/// The resolver a *running* box gets. The build box always takes the
/// manifest's word — a build fetches what its recipe cannot pin — but a
/// routeless run box could not reach a resolver, and the broker resolves
/// names host-side; a resolv.conf there would be decoration. Decided
/// here, once, so no caller building run arguments can restate it wrong.
#[must_use]
pub fn runtime_dns(manifest: &Manifest) -> Option<std::net::IpAddr> {
    manifest
        .access
        .dns
        .filter(|_| manifest.access.network == crate::run::Network::Host)
}

/// Whether this box speaks through the broker. Explicit wins; unset means
/// "when there is an agent to speak for" — a `box -- sh` manifest with no
/// agent spawns no broker it would never use.
#[must_use]
pub fn brokers(manifest: &Manifest) -> bool {
    manifest.access.broker.unwrap_or(manifest.agent.run.is_some())
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
    /// Takes a reflink copy of the workspace before the box starts, and
    /// prints what changed when it exits. The workspace is the box's whole
    /// remaining local blast radius, so this is the undo point and the
    /// proof — but only on a filesystem that can reflink. Off by default:
    /// on one that cannot, an honest snapshot is a full physical copy of
    /// your tree on every box start, which is a cost nobody should pay
    /// without asking for it.
    #[serde(default)]
    pub snapshot: bool,
}

/// Where the preflight hook is seeded, relative to the box home. The
/// launcher writes it there; `launch_command` runs it from there.
pub const PREFLIGHT_SEED: &str = ".wormhole/preflight";

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
    /// The box refuses to start until the variable has a value.
    #[serde(default)]
    pub required: bool,
    /// The value is masked wherever wormhole prints it.
    #[serde(default)]
    pub secret: bool,
    /// With no value anywhere, a start on a terminal asks for one, once,
    /// and keeps the answer host-side for every box. Implies `secret`.
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

/// Which config file a box start seeds first-run answers into. The
/// formats differ structurally — JSON merged one way, TOML another — so
/// the writer dispatches on this once, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSeed {
    /// Claude Code's `.claude.json`: onboarding, bypass warning, trust.
    ClaudeJson,
    /// Codex's `.codex/config.toml`: workspace trust, and the model —
    /// codex reads no env var for one.
    CodexToml,
}

/// The env pair a brokered box gets so its agent talks to the forwarder:
/// the base-URL variable, and a key variable holding a dummy the broker
/// strips host-side. Absent for an agent whose API leg goes through the
/// `CONNECT` tunnel with its own credential instead.
struct BrokeredApi {
    base_url: &'static str,
    key: &'static str,
    dummy: &'static str,
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
    /// model is seeded into the agent's config file instead.
    model_env: Option<&'static str>,
    /// How the broker redirects this agent's API leg, when it does.
    brokered_api: Option<BrokeredApi>,
    /// Whether the usage feed and its status line apply — they read an
    /// Anthropic endpoint, so they are claude's and nobody else's.
    usage: bool,
    config: ConfigSeed,
}

const KNOWN_AGENTS: [KnownAgent; 2] = [
    KnownAgent {
        name: "claude",
        command: &["claude", "--dangerously-skip-permissions"],
        instructions: ".claude/CLAUDE.md",
        // `@AGENTS.md` alone would resolve beside CLAUDE.md, inside
        // `.claude/`; the canonical file is at the home root.
        pointer: Pointer::Import("@~/AGENTS.md\n"),
        model_env: Some("ANTHROPIC_MODEL"),
        brokered_api: Some(BrokeredApi {
            base_url: "ANTHROPIC_BASE_URL",
            key: "ANTHROPIC_API_KEY",
            dummy: "sk-wormhole-dummy",
        }),
        usage: true,
        config: ConfigSeed::ClaudeJson,
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
        // Codex reaches its API through the CONNECT leg with its own
        // credential; the broker's injection leg speaks only Anthropic.
        brokered_api: None,
        usage: false,
        config: ConfigSeed::CodexToml,
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
    /// A fixed variable that also carries `default` or `required`.
    EnvFixedConflict(String),
    /// `ask` beside a value that is never missing.
    EnvAskConflict(String),
    /// An egress entry the allowlist rules refuse, with the rule it broke.
    Egress(String, String),
    /// An egress list on a box whose network makes it decoration.
    EgressUnenforced,
    NoAgent,
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
            ManifestError::Egress(entry, rule) => {
                write!(f, "[access] egress entry {entry:?}: {rule}")
            }
            ManifestError::EgressUnenforced => write!(
                f,
                "[access] egress is enforced by the broker on a routeless \
                 box; with network = \"host\" or broker = false it would be \
                 decoration, and a rule that only decorates is refused"
            ),
            ManifestError::EnvAskConflict(name) => write!(
                f,
                "[env.{name}] has a value already, so `ask` could never \
                 act; drop it or drop `fixed`/`default`"
            ),
            ManifestError::EnvFixedConflict(name) => write!(
                f,
                "[env.{name}] is fixed, so `default` and `required` \
                 could never act; drop them or drop `fixed`"
            ),
            ManifestError::NoAgent => write!(f, "no agent named, so there is nothing to run"),
        }
    }
}

impl std::error::Error for ManifestError {}

#[derive(Deserialize)]
struct VersionClaim {
    version: Option<toml::Value>,
}

/// What a wormhole file claims as its version, written exactly as it wrote
/// it — `1`, `"v1alpha1"`, or `None` for a file that names none. `Err` is a
/// TOML syntax failure.
///
/// Read before the file itself, so one from another version is named as
/// such instead of surfacing as a type error on one field or a heap of
/// unknown keys. That is the whole reason the key exists.
pub fn claimed_version(text: &str) -> Result<Option<String>, String> {
    let claim: VersionClaim = toml::from_str(text).map_err(|e| e.message().to_owned())?;
    Ok(claim.version.map(|found| found.to_string()))
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
    let claim: RoleClaim =
        toml::from_str(text).map_err(|e| ManifestError::Syntax(e.message().to_owned()))?;
    let Some(role) = claim.role else {
        return Ok(None);
    };
    if claim.image.is_some() {
        return Err(ManifestError::RoleAndImage);
    }
    if role.trim().is_empty() {
        return Err(ManifestError::Syntax(
            "`role` names no role; give it a directory, a name, or a pinned ref".to_owned(),
        ));
    }
    Ok(Some(role))
}

/// Just enough of a manifest to answer "does this hand over to a role".
/// Deliberately not `deny_unknown_fields`: every other key is somebody
/// else's business at this point.
#[derive(Deserialize)]
struct RoleClaim {
    role: Option<String>,
    image: Option<serde::de::IgnoredAny>,
}

pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    match claimed_version(text).map_err(ManifestError::Syntax)? {
        Some(found) if found == VERSION.to_string() => {}
        Some(found) => return Err(ManifestError::Version(found)),
        None => return Err(ManifestError::Version("nothing".to_owned())),
    }

    // Said in this project's words rather than serde's. Without it a
    // manifest that hands over to a role — a role's own, say, which is
    // where "a role may not name a role" is decided — surfaces as
    // `unknown field 'role'`, which names the key and not the rule.
    if names_a_role(text)?.is_some() {
        return Err(ManifestError::HandsToARole);
    }
    let manifest: Manifest =
        toml::from_str(text).map_err(|e| ManifestError::Syntax(e.message().to_owned()))?;
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
    if let Some(name) = &manifest.agent.run
        && known(name).is_none()
    {
        return Err(ManifestError::UnknownAgent(name.clone()));
    }
    for (name, var) in &manifest.env {
        if var.fixed.is_some() && (var.required || !var.default.is_empty()) {
            return Err(ManifestError::EnvFixedConflict(name.clone()));
        }
        if var.ask && (var.fixed.is_some() || !var.default.is_empty()) {
            return Err(ManifestError::EnvAskConflict(name.clone()));
        }
    }
    if !manifest.access.egress.is_empty()
        && (manifest.access.network == crate::run::Network::Host || !brokers(&manifest))
    {
        return Err(ManifestError::EgressUnenforced);
    }
    for entry in &manifest.access.egress {
        if let Some(rule) = crate::broker::egress_entry_error(entry, &manifest.access.egress) {
            return Err(ManifestError::Egress(entry.clone(), rule));
        }
    }
    Ok(manifest)
}

/// The command that starts this box's agent. Permissions are bypassed
/// because the box, not the agent, is what holds the line.
pub fn agent_command(manifest: &Manifest) -> Result<Vec<String>, ManifestError> {
    let name = manifest.agent.run.as_ref().ok_or(ManifestError::NoAgent)?;
    let agent = known(name).ok_or_else(|| ManifestError::UnknownAgent(name.clone()))?;
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

/// Where the agent reads its own instructions, relative to the box home.
/// `None` when the box has no agent to instruct.
pub fn instructions_target(manifest: &Manifest) -> Option<&'static str> {
    known(manifest.agent.run.as_deref()?).map(|agent| agent.instructions)
}

/// How the agent's own instructions path delivers the canonical
/// `AGENTS.md`. `None` when the box has no agent.
pub fn instructions_pointer(manifest: &Manifest) -> Option<Pointer> {
    known(manifest.agent.run.as_deref()?).map(|agent| agent.pointer)
}

/// Which config file a start seeds for this agent. `None` with no agent.
pub fn config_seed(manifest: &Manifest) -> Option<ConfigSeed> {
    known(manifest.agent.run.as_deref()?).map(|agent| agent.config)
}

/// Whether the usage feed and its status line apply to this box's agent.
/// They read an Anthropic endpoint with the host's claude credential, so
/// any other agent — or no agent — means no.
#[must_use]
pub fn usage_feed(manifest: &Manifest) -> bool {
    manifest
        .agent
        .run
        .as_deref()
        .and_then(known)
        .is_some_and(|agent| agent.usage)
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
    // Before the hook: a preflight that reaches the API needs the route
    // the forwarder is, and with `network = "none"` there is no other.
    if brokers(manifest) {
        script.push_str(&crate::broker::forwarder_line());
        script.push('\n');
    }
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

/// A custom command (`box -- sh`) in a brokered box still gets the
/// forwarder — the socket is mounted and `HTTPS_PROXY` points at the
/// forwarder's port either way, and a shell whose proxy answers nothing
/// would blame the network. The preflight hook stays out: a command you
/// typed is not the agent's setup path.
#[must_use]
pub fn brokered_command(manifest: &Manifest, command: Vec<String>) -> Vec<String> {
    if !brokers(manifest) {
        return command;
    }
    let mut before = crate::broker::forwarder_line();
    before.push('\n');
    wrapped(&before, &command)
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
/// Everything the manifest declares plus what the recipe implies —
/// `[env]` as written, with the model, the CA pointers and the broker's
/// two variables folded in as fixed ones. The single input every env
/// screen and every resolution starts from.
#[must_use]
pub fn declarations(manifest: &Manifest) -> BTreeMap<String, EnvVar> {
    let fixed = |value: String| EnvVar {
        fixed: Some(value),
        ..EnvVar::default()
    };
    let mut declared = manifest.env.clone();
    // Only when this agent reads a model variable at all: exporting
    // ANTHROPIC_MODEL to codex would be a line nobody reads, and worse, a
    // lie about how the model was actually passed.
    if let Some(model) = manifest.agent.model.as_deref()
        && let Some(var) = manifest
            .agent
            .run
            .as_deref()
            .and_then(known)
            .and_then(|agent| agent.model_env)
    {
        declared.insert(var.to_owned(), fixed(model.to_owned()));
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
    if brokers(manifest) {
        // The agent talks to the forwarder and nothing else. The dummy key
        // exists only so its client starts at all; the broker strips it and
        // puts the real credential on, host-side. Only for an agent whose
        // API leg the broker actually redirects — one that carries its own
        // credential goes through the CONNECT leg and must not be pointed
        // at an injection leg that speaks a different provider.
        if let Some(api) = manifest
            .agent
            .run
            .as_deref()
            .and_then(known)
            .and_then(|agent| agent.brokered_api.as_ref())
        {
            declared.insert(
                api.base_url.to_owned(),
                fixed(crate::broker::base_url_in_box()),
            );
            declared.insert(api.key.to_owned(), fixed(api.dummy.to_owned()));
        }
        // The CONNECT leg, spelled the way the tools in the box read it:
        // cargo and friends take the uppercase pair, curl the lowercase.
        // Loopback excluded, or the agent's own API leg would try to
        // proxy itself through the proxy it already is.
        for name in ["HTTPS_PROXY", "https_proxy"] {
            declared.insert(name.to_owned(), fixed(crate::broker::base_url_in_box()));
        }
        for name in ["NO_PROXY", "no_proxy"] {
            declared.insert(name.to_owned(), fixed("127.0.0.1,localhost".to_owned()));
        }
        // Node's own fetch ignores the proxy variables unless told; the
        // agent is Node, and its WebFetch would otherwise dial a route
        // the box does not have. A Node too old for the switch ignores
        // it, which costs nothing.
        declared.insert("NODE_USE_ENV_PROXY".to_owned(), fixed("1".to_owned()));
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
    use std::path::PathBuf;

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
        assert_eq!(manifest.agent.run, None);
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
        let manifest = full("[agent]\nrun = \"claude\"\n[access]\nbroker = false\n");
        assert_eq!(launch_command(&manifest), agent_command(&manifest));
    }

    /// The default carries the thesis: an agent manifest that says
    /// nothing about access is routeless and brokered; saying
    /// `broker = false` — or having no agent to speak for — turns it off.
    #[test]
    fn an_agent_manifest_brokers_by_default_and_a_bare_one_does_not() {
        let agent = full("[agent]\nrun = \"claude\"\n");
        assert!(brokers(&agent));
        assert_eq!(agent.access.network, crate::run::Network::None);
        assert!(!brokers(&full("")), "no agent, nothing to broker for");
        assert!(!brokers(&full(
            "[agent]\nrun = \"claude\"\n[access]\nbroker = false\n"
        )));
    }

    /// An allowlist nothing would enforce is decoration, and decoration
    /// is refused: egress needs the broker on a routeless box.
    #[test]
    fn an_unenforced_egress_list_is_refused() {
        for extra in [
            "[access]\negress = [\"crates.io\"]\nnetwork = \"host\"\n[agent]\nrun = \"claude\"\n",
            "[access]\negress = [\"crates.io\"]\nbroker = false\n[agent]\nrun = \"claude\"\n",
            "[access]\negress = [\"crates.io\"]\n", // no agent, so no broker
        ] {
            assert!(
                matches!(parse(&text(extra)), Err(ManifestError::EgressUnenforced)),
                "{extra}"
            );
        }
        let held = "[agent]\nrun = \"claude\"\n[access]\negress = [\"crates.io\"]\n";
        assert!(parse(&text(held)).is_ok());
    }

    /// The egress rules themselves are the broker's (`egress_allows`);
    /// what the manifest holds is that a bad entry is refused by name.
    #[test]
    fn a_bad_egress_entry_is_refused_with_its_rule() {
        let extra = "[agent]\nrun = \"claude\"\n[access]\negress = [\"*.github.io\"]\n";
        assert!(
            matches!(parse(&text(extra)), Err(ManifestError::Egress(entry, _)) if entry == "*.github.io")
        );
    }

    /// The broker hands the box its proxy view of the world: base URL and
    /// dummy key for the agent, the CONNECT proxy for everything else,
    /// loopback excluded so the API leg does not proxy itself.
    #[test]
    fn a_brokered_box_is_pointed_at_the_forwarder_for_everything() {
        let env = box_env(&full("[agent]\nrun = \"claude\"\n"), &host(&[]));
        assert_eq!(
            env.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:8787")
        );
        assert_eq!(env.get("https_proxy"), env.get("HTTPS_PROXY"));
        assert_eq!(
            env.get("NO_PROXY").map(String::as_str),
            Some("127.0.0.1,localhost")
        );
        assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("1"));
    }

    /// A typed command in a brokered box gets the forwarder and not the
    /// preflight hook: the proxy must answer, the setup path is the
    /// agent's own. Without the broker the command runs bare.
    #[test]
    fn a_custom_command_gets_the_forwarder_exactly_when_the_box_brokers() {
        let brokered = full("[agent]\nrun = \"claude\"\n");
        let command = brokered_command(&brokered, vec!["sh".to_owned()]);
        assert_eq!(command[0], "/bin/sh");
        let script = command.last().expect("script");
        assert!(script.contains("/run/wormhole/forward"), "{script}");
        assert!(script.contains("exec 'sh'"), "{script}");
        assert!(!script.contains("preflight"), "{script}");
        let bare = full("[agent]\nrun = \"claude\"\n[access]\nbroker = false\n");
        assert_eq!(
            brokered_command(&bare, vec!["sh".to_owned()]),
            vec!["sh".to_owned()]
        );
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

    #[test]
    fn a_variable_can_be_required_and_secret() {
        let manifest = full("[env.SENTRY_TOKEN]\nrequired = true\nsecret = true\n");
        let var = &manifest.env["SENTRY_TOKEN"];
        assert!(var.required);
        assert!(var.secret);
    }

    /// A fixed value already answers every question the other fields ask,
    /// so a manifest combining them is confused, not flexible.
    #[test]
    fn a_fixed_variable_takes_no_default_and_no_required() {
        for extra in [
            "[env.SHELL]\nfixed = \"/bin/bash\"\nrequired = true\n",
            "[env.SHELL]\nfixed = \"/bin/bash\"\ndefault = \"/bin/sh\"\n",
        ] {
            assert!(
                matches!(parse(&text(extra)), Err(ManifestError::EnvFixedConflict(_))),
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
        assert!(env.is_empty(), "{env:?}");
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

    /// A brokered box talks to the forwarder and holds nothing worth
    /// taking: the dummy key exists only so the agent's client starts, and
    /// the broker strips it host-side before anything goes upstream.
    #[test]
    fn a_brokered_box_is_pointed_at_the_forwarder_and_given_a_dummy_key() {
        let manifest = full("[agent]\nrun = \"claude\"\n[access]\nbroker = true\n");
        let env = box_env(&manifest, &host(&[]));
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some(crate::broker::base_url_in_box().as_str())
        );
        assert!(
            env.get("ANTHROPIC_API_KEY")
                .is_some_and(|k| !k.contains("sk-ant")),
            "{env:?}"
        );
    }

    /// A box that does not broker reaches the API itself and must not be
    /// handed a base URL pointing at a forwarder that is not running.
    #[test]
    fn a_box_that_does_not_broker_is_told_nothing_about_one() {
        let env = box_env(
            &full("[agent]\nrun = \"claude\"\n[access]\nbroker = false\n"),
            &host(&[]),
        );
        assert!(!env.contains_key("ANTHROPIC_BASE_URL"), "{env:?}");
        assert!(!env.contains_key("ANTHROPIC_API_KEY"), "{env:?}");
    }

    /// The forwarder starts before the hook, because a preflight that
    /// reaches the API needs the route the forwarder *is* — and with
    /// `network = "none"` there is no other one.
    #[test]
    fn the_forwarder_starts_before_the_hook_and_the_agent_still_replaces_the_shell() {
        let manifest =
            full("[agent]\nrun = \"claude\"\npreflight = \"p.sh\"\n[access]\nbroker = true\n");
        let command = launch_command(&manifest).expect("command");
        let script = command.last().expect("script");
        let forwarder = script
            .find(crate::broker::FORWARD_IN_BOX)
            .expect("forwarder");
        let hook = script.find(PREFLIGHT_SEED).expect("hook");
        let agent = script.find("exec ").expect("exec");
        assert!(forwarder < hook, "{script}");
        assert!(hook < agent, "{script}");
    }

    /// Brokering alone is enough to need a shell wrapper, hook or no hook.
    #[test]
    fn brokering_without_a_hook_still_starts_the_forwarder() {
        let manifest = full("[agent]\nrun = \"claude\"\n[access]\nbroker = true\n");
        let script = launch_command(&manifest)
            .expect("command")
            .pop()
            .expect("script");
        assert!(script.contains(crate::broker::FORWARD_IN_BOX), "{script}");
        assert!(script.contains("exec "), "{script}");
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
            "[runtime]\nsnapshot = true\n",
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
    fn claudes_instructions_live_in_its_own_config_dir() {
        let manifest = full("[agent]\nrun = \"claude\"\n");
        assert_eq!(
            instructions_target(&manifest).map(PathBuf::from),
            Some(PathBuf::from(".claude/CLAUDE.md"))
        );
    }

    #[test]
    fn no_agent_means_no_instructions_file() {
        assert_eq!(instructions_target(&full("")), None);
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
            Some(Pointer::Import("@~/AGENTS.md\n"))
        );
        assert_eq!(INSTRUCTIONS_SEED, "AGENTS.md");
    }

    /// Codex reads `$CODEX_HOME/AGENTS.md` and has no import syntax, so
    /// its pointer is a symlink to the same canonical file.
    #[test]
    fn codexs_instructions_are_a_symlink_into_its_config_dir() {
        let manifest = full("[agent]\nrun = \"codex\"\n");
        assert_eq!(
            instructions_target(&manifest).map(PathBuf::from),
            Some(PathBuf::from(".codex/AGENTS.md"))
        );
        assert_eq!(instructions_pointer(&manifest), Some(Pointer::Symlink));
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

    /// The bug this pins: the model was exported as ANTHROPIC_MODEL for
    /// *any* agent — a line codex never reads, saying the model was passed
    /// when it was not. The model reaches codex through its config file.
    #[test]
    fn a_codex_model_is_not_exported_as_an_anthropic_variable() {
        let manifest = full("[agent]\nrun = \"codex\"\nmodel = \"gpt-5-codex\"\n");
        let env = box_env(&manifest, &host(&[]));
        assert!(!env.contains_key("ANTHROPIC_MODEL"), "{env:?}");
    }

    /// A brokered codex box still gets the CONNECT proxy — that leg is
    /// provider-neutral — but no Anthropic base URL and no dummy key: its
    /// API leg tunnels with its own credential, and pointing it at the
    /// injection leg would hand its requests to the wrong provider.
    #[test]
    fn a_brokered_codex_box_gets_the_tunnel_and_no_anthropic_redirect() {
        let env = box_env(&full("[agent]\nrun = \"codex\"\n"), &host(&[]));
        assert_eq!(
            env.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:8787")
        );
        assert!(!env.contains_key("ANTHROPIC_BASE_URL"), "{env:?}");
        assert!(!env.contains_key("ANTHROPIC_API_KEY"), "{env:?}");
    }

    /// The usage feed reads an Anthropic endpoint with the host's claude
    /// credential; only a claude box may spawn it.
    #[test]
    fn the_usage_feed_applies_to_claude_and_nobody_else() {
        assert!(usage_feed(&full("[agent]\nrun = \"claude\"\n")));
        assert!(!usage_feed(&full("[agent]\nrun = \"codex\"\n")));
        assert!(!usage_feed(&full("")));
    }

    #[test]
    fn each_agent_names_the_config_file_a_start_seeds() {
        assert_eq!(
            config_seed(&full("[agent]\nrun = \"claude\"\n")),
            Some(ConfigSeed::ClaudeJson)
        );
        assert_eq!(
            config_seed(&full("[agent]\nrun = \"codex\"\n")),
            Some(ConfigSeed::CodexToml)
        );
        assert_eq!(config_seed(&full("")), None);
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
}
