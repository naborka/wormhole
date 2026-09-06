//! Pure decisions behind `wormhole run`: argument parsing, the uid/gid
//! map a box writes, and how a child's wait status becomes our exit code.
//! The syscalls live in the `wormhole` binary crate.

use std::fmt;
use std::net::IpAddr;

/// Command line after the `run` subcommand:
/// `[--grant <path>]... [--dns <address>] [--image <dir>] -- <command> [args...]`.
#[derive(Debug, PartialEq, Eq)]
pub struct RunArgs {
    /// Host paths to bind read-write into the box, as the user typed them.
    pub grants: Vec<String>,
    /// The one resolver the box may use. `None` means no DNS at all, which
    /// is the default: a box with no resolver fails loudly instead of
    /// quietly reaching somewhere. Interim until the broker (Step 8).
    pub dns: Option<IpAddr>,
    /// A built image directory to use as the box's root. Absent means the
    /// interim host `/usr`, which goes away when layers land.
    pub image: Option<String>,
    /// Where `__boxed` writes the host pid of the box's PID 1, so
    /// `wormhole attach` can find its namespaces. Absent writes nothing.
    pub pidfile: Option<String>,
    /// A host CA bundle to mount over the box's own, for networks that
    /// intercept TLS with their own CA. Absent means the box trusts only
    /// what its image ships.
    pub ca: Option<String>,
    /// Files already fetched and proved on the host, as
    /// `(host file, path in the box)`, bound read-only. How a build box
    /// gets the recipe's artifacts without opening a connection itself.
    pub artifacts: Vec<(String, String)>,
    /// The host-side broker's socket. Bound read-only at a fixed box
    /// path, together with wormhole's own binary, so the box can reach the
    /// API without a credential or a route.
    pub broker: Option<String>,
    /// Every host the broker's `CONNECT` leg admits for this box — the
    /// manifest's plus the kept additions. On the struct the banner is
    /// computed from, so a way out can never bypass the line that
    /// promises to name every way out. The child process ignores it;
    /// enforcement is the broker's, host-side.
    pub egress: Vec<String>,
    pub network: Network,
    pub root: RootMode,
    pub command: Vec<String>,
}

/// How the box's root filesystem is made out of its image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootMode {
    /// A writable `cp --reflink` copy of the image, thrown away on exit.
    /// Free where reflink applies and a multi-second physical copy of the
    /// whole image where it does not — which is every ext4 host.
    #[default]
    Copy,
    /// The image itself, bound read-only, with a tmpfs over the few paths
    /// that must be writable. No copy at all, so a box starts in the time
    /// it takes to mount.
    ///
    /// The trade is real and is the point: the box can no longer install a
    /// package at run time. Today it can, and throws the result away when
    /// it exits.
    Readonly,
}

impl fmt::Display for RootMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RootMode::Copy => write!(f, "copy"),
            RootMode::Readonly => write!(f, "readonly"),
        }
    }
}

/// What the box can reach at the network layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// The host's network namespace: every route the host has, the box
    /// has. The opt-in escape hatch (`--network host`), no longer any
    /// default: the broker is how a box reaches out now.
    Host,
    /// A network namespace of the box's own, holding nothing but loopback.
    /// `connect()` to anything off the machine fails because there is no
    /// route, not because something filtered it. The default everywhere —
    /// default deny is the whole design, said once here.
    #[default]
    None,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Host => write!(f, "host"),
            Network::None => write!(f, "none"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    MissingSeparator,
    EmptyCommand,
    UnknownFlag(String),
    GrantWithoutPath,
    DnsWithoutAddress,
    DnsNotAnAddress(String),
    ImageWithoutPath,
    PidfileWithoutPath,
    CaWithoutPath,
    ArtifactWithoutPaths,
    BrokerWithoutPath,
    NetworkWithoutMode,
    NetworkNotAMode(String),
    RootWithoutMode,
    RootNotAMode(String),
    AttachUsage,
    RoleWithoutName,
    IdWithoutValue,
    NewAndId,
    AliasWithoutName,
    UnusableAlias(String),
    EnvWithoutName,
    UnusableEnvName(String),
    BoxUsage,
    PsUsage,
    RemoveUsage,
    ResetUsage,
    RenameUsage,
    GcUsage,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingSeparator => write!(
                f,
                "expected `--` before the command: wormhole run [--grant <path>]... -- <command>"
            ),
            ParseError::EmptyCommand => write!(f, "no command given after `--`"),
            ParseError::UnknownFlag(flag) => write!(f, "unknown flag {flag}"),
            ParseError::GrantWithoutPath => write!(f, "--grant needs a path"),
            ParseError::DnsWithoutAddress => write!(f, "--dns needs an address"),
            ParseError::DnsNotAnAddress(value) => {
                write!(f, "--dns wants an IP address, not {value}")
            }
            ParseError::ImageWithoutPath => write!(f, "--image needs a directory"),
            ParseError::PidfileWithoutPath => write!(f, "--pidfile needs a path"),
            ParseError::CaWithoutPath => write!(f, "--ca needs a path to a CA bundle"),
            ParseError::ArtifactWithoutPaths => write!(
                f,
                "--artifact needs the host file and the path it takes in the box"
            ),
            ParseError::BrokerWithoutPath => write!(f, "--broker needs a socket path"),
            ParseError::NetworkWithoutMode => write!(f, "--network needs host or none"),
            ParseError::RootWithoutMode => write!(f, "--root needs copy or readonly"),
            ParseError::RootNotAMode(value) => {
                write!(f, "--root wants copy or readonly, not {value}")
            }
            ParseError::NetworkNotAMode(value) => {
                write!(f, "--network wants host or none, not {value}")
            }
            ParseError::AttachUsage => {
                write!(
                    f,
                    "usage: wormhole attach <id|name> [-- <command> [args...]]"
                )
            }
            ParseError::RoleWithoutName => write!(f, "--role needs a name"),
            ParseError::AliasWithoutName => write!(f, "--as needs a name"),
            ParseError::EnvWithoutName => write!(f, "--env needs NAME or NAME=VALUE"),
            ParseError::UnusableEnvName(name) => write!(
                f,
                "{name:?} cannot name a variable; letters, digits and _ only, \
                 not starting with a digit"
            ),
            ParseError::UnusableAlias(value) => write!(
                f,
                "{value:?} cannot name a box; names are letters, digits, - and _, \
                 and never twelve hex characters, which is what an id is"
            ),
            ParseError::IdWithoutValue => write!(f, "--id needs a box id or name"),
            ParseError::NewAndId => write!(
                f,
                "--new starts another box and --id names an existing one; pick one"
            ),
            ParseError::BoxUsage => {
                write!(
                    f,
                    "usage: wormhole box [--role <name|dir|ref>] [--new | --id <id|name>] [--as <name>] [-- <command> [args...]]"
                )
            }
            ParseError::PsUsage => write!(f, "usage: wormhole ps [--all]"),
            ParseError::RemoveUsage => write!(f, "usage: wormhole remove <id|name> [<id|name>...]"),
            ParseError::ResetUsage => write!(f, "usage: wormhole reset <id|name>"),
            ParseError::RenameUsage => write!(f, "usage: wormhole rename <id|name> <new name>"),
            ParseError::GcUsage => write!(
                f,
                "usage: wormhole gc [--delete [--unreferenced]] — \
                 `--unreferenced` widens what `--delete` takes and means nothing on its own"
            ),
        }
    }
}

pub fn parse_args(args: &[String]) -> Result<RunArgs, ParseError> {
    let mut parts = args.splitn(2, |a| a == "--");
    let before = parts.next().unwrap_or_default();
    let command = parts.next().ok_or(ParseError::MissingSeparator)?;
    if command.is_empty() {
        return Err(ParseError::EmptyCommand);
    }
    let mut grants = Vec::new();
    let mut dns = None;
    let mut image = None;
    let mut pidfile = None;
    let mut ca = None;
    let mut artifacts = Vec::new();
    let mut broker = None;
    let mut network = Network::default();
    let mut root = RootMode::default();
    let mut flags = before.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--grant" => {
                let path = flags.next().ok_or(ParseError::GrantWithoutPath)?;
                grants.push(path.clone());
            }
            "--dns" => {
                let value = flags.next().ok_or(ParseError::DnsWithoutAddress)?;
                dns = Some(
                    value
                        .parse()
                        .map_err(|_| ParseError::DnsNotAnAddress(value.clone()))?,
                );
            }
            "--image" => {
                let path = flags.next().ok_or(ParseError::ImageWithoutPath)?;
                image = Some(path.clone());
            }
            "--pidfile" => {
                let path = flags.next().ok_or(ParseError::PidfileWithoutPath)?;
                pidfile = Some(path.clone());
            }
            "--ca" => {
                let path = flags.next().ok_or(ParseError::CaWithoutPath)?;
                ca = Some(path.clone());
            }
            "--artifact" => {
                let source = flags.next().ok_or(ParseError::ArtifactWithoutPaths)?;
                let target = flags.next().ok_or(ParseError::ArtifactWithoutPaths)?;
                artifacts.push((source.clone(), target.clone()));
            }
            "--network" => {
                let value = flags.next().ok_or(ParseError::NetworkWithoutMode)?;
                network = match value.as_str() {
                    "host" => Network::Host,
                    "none" => Network::None,
                    other => return Err(ParseError::NetworkNotAMode(other.to_owned())),
                };
            }
            "--root" => {
                let value = flags.next().ok_or(ParseError::RootWithoutMode)?;
                root = match value.as_str() {
                    "copy" => RootMode::Copy,
                    "readonly" => RootMode::Readonly,
                    other => return Err(ParseError::RootNotAMode(other.to_owned())),
                };
            }
            "--broker" => {
                let path = flags.next().ok_or(ParseError::BrokerWithoutPath)?;
                broker = Some(path.clone());
            }
            other => return Err(ParseError::UnknownFlag(other.to_owned())),
        }
    }
    Ok(RunArgs {
        grants,
        dns,
        image,
        pidfile,
        ca,
        artifacts,
        broker,
        // A bare `run` spawns no broker, so there is nothing to admit.
        egress: Vec::new(),
        network,
        root,
        command: command.to_vec(),
    })
}

/// `RunArgs` back as the argv `parse_args` reads — the flags for the
/// re-exec into `__boxed`. The one serializer, beside the one parser, so
/// a new flag is added in exactly one file and the round-trip test
/// catches a missed field.
pub fn to_argv(args: &RunArgs) -> Vec<String> {
    let mut argv = Vec::new();
    for grant in &args.grants {
        argv.push("--grant".to_owned());
        argv.push(grant.clone());
    }
    if let Some(dns) = args.dns {
        argv.push("--dns".to_owned());
        argv.push(dns.to_string());
    }
    if let Some(image) = &args.image {
        argv.push("--image".to_owned());
        argv.push(image.clone());
    }
    if let Some(pidfile) = &args.pidfile {
        argv.push("--pidfile".to_owned());
        argv.push(pidfile.clone());
    }
    if let Some(ca) = &args.ca {
        argv.push("--ca".to_owned());
        argv.push(ca.clone());
    }
    for (source, target) in &args.artifacts {
        argv.push("--artifact".to_owned());
        argv.push(source.clone());
        argv.push(target.clone());
    }
    if let Some(broker) = &args.broker {
        argv.push("--broker".to_owned());
        argv.push(broker.clone());
    }
    argv.push("--root".to_owned());
    argv.push(args.root.to_string());
    argv.push("--network".to_owned());
    argv.push(args.network.to_string());
    argv.push("--".to_owned());
    argv.extend(args.command.iter().cloned());
    argv
}

/// Command line after `box` (and `build`, which shares `--role`):
/// `[--role <name|dir>] [-- <command> [args...]]`.
#[derive(Debug, PartialEq, Eq)]
pub struct BoxArgs {
    /// A role to use instead of the workspace's own manifest — a name in
    /// the config dir, or a directory when it has a path separator.
    pub role: Option<String>,
    /// Start this exact kept box again, by the id `wormhole ps --all`
    /// shows. `None` resumes the workspace's most recently used free box.
    pub id: Option<String>,
    /// Start another box in this workspace rather than resuming one. A
    /// workspace holds as many boxes as you make; each keeps its own home.
    pub new: bool,
    /// What to call this box, so it can be reached by that instead of by
    /// its twelve-character id. Set on the box this start makes or resumes.
    pub alias: Option<String>,
    /// A command that replaces the agent. `None` runs the agent.
    pub command: Option<Vec<String>>,
    /// Variables declared on the command line, on top of the manifest's.
    pub env: Vec<EnvArg>,
}

/// One `--env` flag: a variable named with the host's value (`NAME`) or
/// with a spelled one (`NAME=VALUE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvArg {
    pub name: String,
    pub value: Option<String>,
}

fn parse_env_arg(text: &str) -> Result<EnvArg, ParseError> {
    let (name, value) = match text.split_once('=') {
        Some((name, value)) => (name, Some(value.to_owned())),
        None => (text, None),
    };
    let mut chars = name.chars();
    let named_like_one = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !named_like_one {
        return Err(ParseError::UnusableEnvName(name.to_owned()));
    }
    Ok(EnvArg {
        name: name.to_owned(),
        value,
    })
}

pub fn parse_box_args(args: &[String]) -> Result<BoxArgs, ParseError> {
    let mut parts = args.splitn(2, |a| a == "--");
    let before = parts.next().unwrap_or_default();
    let command = match parts.next() {
        None => None,
        Some([]) => return Err(ParseError::BoxUsage),
        Some(command) => Some(command.to_vec()),
    };
    let mut role = None;
    let mut id = None;
    let mut new = false;
    let mut alias = None;
    let mut env = Vec::new();
    let mut flags = before.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--env" => env.push(parse_env_arg(
                flags.next().ok_or(ParseError::EnvWithoutName)?,
            )?),
            "--role" => role = Some(flags.next().ok_or(ParseError::RoleWithoutName)?.clone()),
            "--as" => {
                let wanted = flags.next().ok_or(ParseError::AliasWithoutName)?;
                if !crate::home::is_usable_alias(wanted) {
                    return Err(ParseError::UnusableAlias(wanted.clone()));
                }
                alias = Some(wanted.clone());
            }
            "--id" => {
                // An id or an alias — which it is comes from the string
                // itself, and only the store can say whether either names
                // a box, so nothing is judged here.
                id = Some(flags.next().ok_or(ParseError::IdWithoutValue)?.clone());
            }
            "--new" => new = true,
            _ => return Err(ParseError::BoxUsage),
        }
    }
    // Naming a box and asking for another one are opposite instructions;
    // guessing which was meant would silently start the wrong box.
    if new && id.is_some() {
        return Err(ParseError::NewAndId);
    }
    Ok(BoxArgs {
        role,
        id,
        new,
        alias,
        command,
        env,
    })
}

/// Command line after the `attach` subcommand:
/// `<id> [--env NAME[=VALUE]]... [-- <command> [args...]]`.
#[derive(Debug, PartialEq, Eq)]
pub struct AttachArgs {
    /// The box to join: the id `wormhole ps` shows. The same id `--id`
    /// takes and `--all` lists — wormhole names a box exactly one way.
    pub id: String,
    /// A command that replaces the default. `None` means the box's own
    /// agent, or a shell when it has none (`manifest::attach_command`).
    pub command: Option<Vec<String>>,
    /// Variables set or refreshed for the box from here on.
    pub env: Vec<EnvArg>,
}

pub fn parse_attach_args(args: &[String]) -> Result<AttachArgs, ParseError> {
    let mut parts = args.splitn(2, |a| a == "--");
    let before = parts.next().unwrap_or_default();
    let command = match parts.next() {
        None => None,
        Some([]) => return Err(ParseError::AttachUsage),
        Some(command) => Some(command.to_vec()),
    };
    let [id, rest @ ..] = before else {
        return Err(ParseError::AttachUsage);
    };
    if id.starts_with('-') {
        return Err(ParseError::AttachUsage);
    }
    let mut env = Vec::new();
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--env" => env.push(parse_env_arg(
                flags.next().ok_or(ParseError::EnvWithoutName)?,
            )?),
            _ => return Err(ParseError::AttachUsage),
        }
    }
    Ok(AttachArgs {
        id: id.clone(),
        command,
        env,
    })
}

/// Command line after the `ps` subcommand: `[--all]`.
#[derive(Debug, PartialEq, Eq)]
pub struct PsArgs {
    /// Every box this host keeps, not only the running ones. An idle box
    /// is the one you resume, so it has to be listable.
    pub all: bool,
}

pub fn parse_ps_args(args: &[String]) -> Result<PsArgs, ParseError> {
    match args {
        [] => Ok(PsArgs { all: false }),
        [flag] if flag == "--all" || flag == "-a" => Ok(PsArgs { all: true }),
        _ => Err(ParseError::PsUsage),
    }
}

/// Command line after `rm`: the boxes to take away.
#[derive(Debug, PartialEq, Eq)]
pub struct RemoveArgs {
    /// Each one an id or the name somebody gave a box — the two forms
    /// every command that takes a box takes.
    pub ids: Vec<String>,
}

/// Whether an argument is a flag rather than the name of something.
///
/// One rule for every command that takes boxes by name, so `remove`,
/// `reset` and `rename` cannot disagree about whether `--foo` is a typo
/// or a box nobody has.
fn is_flag(arg: &str) -> bool {
    arg.starts_with('-')
}

pub fn parse_remove_args(args: &[String]) -> Result<RemoveArgs, ParseError> {
    if args.is_empty() || args.iter().any(|arg| is_flag(arg)) {
        return Err(ParseError::RemoveUsage);
    }
    Ok(RemoveArgs { ids: args.to_vec() })
}

/// Command line after `reset`: the one box to empty.
#[derive(Debug, PartialEq, Eq)]
pub struct ResetArgs {
    pub id: String,
}

pub fn parse_reset_args(args: &[String]) -> Result<ResetArgs, ParseError> {
    match args {
        [id] if !is_flag(id) => Ok(ResetArgs { id: id.clone() }),
        _ => Err(ParseError::ResetUsage),
    }
}

/// Command line after `rename`: the box, then what to call it.
#[derive(Debug, PartialEq, Eq)]
pub struct RenameArgs {
    pub id: String,
    pub alias: String,
}

pub fn parse_rename_args(args: &[String]) -> Result<RenameArgs, ParseError> {
    let [id, alias] = args else {
        return Err(ParseError::RenameUsage);
    };
    if is_flag(id) {
        return Err(ParseError::RenameUsage);
    }
    // The same rule `--as` is held to, asked in the same place: what may
    // name a box is one decision, and two spellings of it would drift.
    if !crate::home::is_usable_alias(alias) {
        return Err(ParseError::UnusableAlias(alias.clone()));
    }
    Ok(RenameArgs {
        id: id.clone(),
        alias: alias.clone(),
    })
}

/// Command line after `gc`: whether to give anything back, and how much.
///
/// Parsed straight into the sweep `gc` acts on, so the flags a person
/// typed and the policy the report and the delete loop both read are one
/// value rather than three that have to agree.
pub fn parse_gc_args(args: &[String]) -> Result<crate::gc::Sweep, ParseError> {
    let mut parsed = crate::gc::Sweep::default();
    for flag in args {
        match flag.as_str() {
            "--delete" => parsed.delete = true,
            "--unreferenced" => parsed.unreferenced = true,
            _ => return Err(ParseError::GcUsage),
        }
    }
    // On its own it widens nothing, so it is a half-typed
    // `--delete --unreferenced` and is answered as one.
    if parsed.unreferenced && !parsed.delete {
        return Err(ParseError::GcUsage);
    }
    Ok(parsed)
}

/// One-line identity map: the same id inside and outside, so a file the
/// box writes lands on the host owned by the invoking user.
pub fn identity_map(id: u32) -> String {
    format!("{id} {id} 1")
}

/// One-line root map, used only while building an image: inside the box
/// we are uid 0 so a package manager can install, outside we are still
/// ourselves, so every file it writes belongs to us.
pub fn root_map(id: u32) -> String {
    format!("0 {id} 1")
}

/// How the boxed child ended, as reported by `wait`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Exited(i32),
    Signaled(i32),
}

/// The exit code `wormhole run` itself reports: the child's own code, or
/// the shell convention 128+signal when the child was killed.
pub fn exit_code(outcome: WaitOutcome) -> i32 {
    match outcome {
        WaitOutcome::Exited(code) => code,
        WaitOutcome::Signaled(signal) => 128 + signal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    /// One shape, two directions, one owner. A field that misses the
    /// serializer would silently drop a box property between the host
    /// process and PID 1 — this test is what makes that impossible.
    #[test]
    fn every_field_survives_the_argv_round_trip() {
        let full = RunArgs {
            grants: strings(&["/home/me/.ssh", "/opt/data"]),
            dns: Some("1.1.1.1".parse().expect("address")),
            image: Some("/images/abc".to_owned()),
            pidfile: Some("/run/box.pid".to_owned()),
            ca: Some("/etc/ca/bundle.pem".to_owned()),
            artifacts: vec![
                (
                    "/data/wormhole/artifacts/aa".to_owned(),
                    "/tmp/one".to_owned(),
                ),
                (
                    "/data/wormhole/artifacts/bb".to_owned(),
                    "/tmp/two".to_owned(),
                ),
            ],
            broker: Some("/data/wormhole/broker.sock".to_owned()),
            egress: Vec::new(),
            network: Network::None,
            root: RootMode::Readonly,
            command: strings(&["sh", "-c", "ls -a"]),
        };
        assert_eq!(parse_args(&to_argv(&full)), Ok(full));

        let minimal = RunArgs {
            grants: Vec::new(),
            dns: None,
            image: None,
            pidfile: None,
            ca: None,
            artifacts: Vec::new(),
            broker: None,
            egress: Vec::new(),
            network: Network::default(),
            root: RootMode::default(),
            command: strings(&["/bin/true"]),
        };
        assert_eq!(parse_args(&to_argv(&minimal)), Ok(minimal));
    }

    #[test]
    fn command_after_separator_is_accepted() {
        let parsed = parse_args(&strings(&["--", "/bin/true"]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: Vec::new(),
                dns: None,
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["/bin/true"])
            })
        );
    }

    #[test]
    fn command_arguments_are_kept_in_order() {
        let parsed = parse_args(&strings(&["--", "ls", "-a", "/"]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: Vec::new(),
                dns: None,
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["ls", "-a", "/"])
            })
        );
    }

    #[test]
    fn a_second_separator_belongs_to_the_command() {
        let parsed = parse_args(&strings(&["--", "sh", "--", "-c"]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: Vec::new(),
                dns: None,
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["sh", "--", "-c"])
            })
        );
    }

    #[test]
    fn missing_separator_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["/bin/true"])),
            Err(ParseError::MissingSeparator)
        );
    }

    #[test]
    fn an_unknown_flag_before_the_separator_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["--verbose", "--", "/bin/true"])),
            Err(ParseError::UnknownFlag("--verbose".to_owned()))
        );
    }

    #[test]
    fn grants_are_collected_in_order() {
        let parsed = parse_args(&strings(&[
            "--grant",
            "/home/me/.claude",
            "--grant",
            "/opt/data",
            "--",
            "/bin/true",
        ]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: strings(&["/home/me/.claude", "/opt/data"]),
                dns: None,
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["/bin/true"]),
            })
        );
    }

    /// `--ca` carries the host bundle `wormhole box` found (or the user
    /// chose) into the mount plan.
    #[test]
    fn a_ca_bundle_is_parsed() {
        let parsed = parse_args(&strings(&[
            "--ca",
            "/etc/ssl/bundle.pem",
            "--",
            "/bin/true",
        ]));
        assert_eq!(
            parsed.map(|args| args.ca),
            Ok(Some("/etc/ssl/bundle.pem".to_owned()))
        );
        assert_eq!(
            parse_args(&strings(&["--ca", "--", "/bin/true"])),
            Err(ParseError::CaWithoutPath)
        );
    }

    #[test]
    fn a_resolver_is_parsed_as_an_address() {
        let parsed = parse_args(&strings(&["--dns", "1.1.1.1", "--", "/bin/true"]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: Vec::new(),
                dns: Some("1.1.1.1".parse().expect("valid address")),
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["/bin/true"]),
            })
        );
    }

    #[test]
    fn a_resolver_that_is_not_an_address_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["--dns", "my-router", "--", "/bin/true"])),
            Err(ParseError::DnsNotAnAddress("my-router".to_owned()))
        );
    }

    #[test]
    fn a_grant_without_a_path_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["--grant", "--", "/bin/true"])),
            Err(ParseError::GrantWithoutPath)
        );
    }

    #[test]
    fn a_grant_after_the_separator_belongs_to_the_command() {
        let parsed = parse_args(&strings(&["--", "sh", "--grant", "/x"]));
        assert_eq!(
            parsed,
            Ok(RunArgs {
                grants: Vec::new(),
                dns: None,
                image: None,
                pidfile: None,
                ca: None,
                artifacts: Vec::new(),
                broker: None,
                egress: Vec::new(),
                network: Network::default(),
                root: RootMode::default(),
                command: strings(&["sh", "--grant", "/x"]),
            })
        );
    }

    #[test]
    fn empty_command_is_rejected() {
        assert_eq!(parse_args(&strings(&["--"])), Err(ParseError::EmptyCommand));
    }

    #[test]
    fn no_args_at_all_is_missing_separator() {
        assert_eq!(parse_args(&[]), Err(ParseError::MissingSeparator));
    }

    #[test]
    fn an_image_directory_is_kept_as_given() {
        let parsed = parse_args(&strings(&[
            "--image",
            "/data/images/abc",
            "--",
            "/bin/true",
        ]));
        assert_eq!(
            parsed.expect("valid").image.as_deref(),
            Some("/data/images/abc")
        );
    }

    #[test]
    fn an_image_without_a_path_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["--image", "--", "/bin/true"])),
            Err(ParseError::ImageWithoutPath)
        );
    }

    #[test]
    fn a_bare_box_runs_the_agent_without_a_role() {
        assert_eq!(
            parse_box_args(&[]),
            Ok(BoxArgs {
                role: None,
                id: None,
                new: false,
                alias: None,
                command: None,
                env: Vec::new()
            })
        );
    }

    #[test]
    fn a_role_and_a_command_can_be_combined() {
        assert_eq!(
            parse_box_args(&strings(&["--role", "architect", "--", "sh"])),
            Ok(BoxArgs {
                role: Some("architect".to_owned()),
                id: None,
                new: false,
                alias: None,
                command: Some(strings(&["sh"])),
                env: Vec::new()
            })
        );
        assert_eq!(
            parse_box_args(&strings(&["--role", "architect"])),
            Ok(BoxArgs {
                role: Some("architect".to_owned()),
                id: None,
                new: false,
                alias: None,
                command: None,
                env: Vec::new()
            })
        );
    }

    #[test]
    fn box_refuses_a_bare_role_flag_an_empty_command_and_stray_words() {
        assert_eq!(
            parse_box_args(&strings(&["--role"])),
            Err(ParseError::RoleWithoutName)
        );
        assert_eq!(parse_box_args(&strings(&["--"])), Err(ParseError::BoxUsage));
        assert_eq!(parse_box_args(&strings(&["sh"])), Err(ParseError::BoxUsage));
    }

    /// A workspace holds as many boxes as you make. `--new` is how you
    /// make another; `--id` is how you go back to one.
    #[test]
    fn a_box_can_be_asked_for_as_another_one_or_named_outright() {
        assert!(parse_box_args(&strings(&["--new"])).expect("valid").new);
        assert_eq!(
            parse_box_args(&strings(&["--id", "0123456789ab"]))
                .expect("valid")
                .id,
            Some("0123456789ab".to_owned())
        );
    }

    /// Opposite instructions. Guessing which was meant would silently
    /// start the wrong box — a whole other history and toolchain.
    #[test]
    fn asking_for_another_box_and_naming_one_together_is_refused() {
        assert_eq!(
            parse_box_args(&strings(&["--new", "--id", "0123456789ab"])),
            Err(ParseError::NewAndId)
        );
    }

    /// `--id` takes an id or the name you gave a box. Which it is comes
    /// from the string, and only the store can say whether either names
    /// anything — so nothing is judged here beyond the flag being given
    /// a value at all.
    #[test]
    fn naming_a_box_takes_an_id_or_an_alias() {
        for wanted in ["0123456789ab", "api"] {
            assert_eq!(
                parse_box_args(&strings(&["--id", wanted])).map(|args| args.id),
                Ok(Some(wanted.to_owned()))
            );
        }
        assert_eq!(
            parse_box_args(&strings(&["--id"])),
            Err(ParseError::IdWithoutValue)
        );
    }

    /// A name a person can type instead of twelve hex characters. Refused
    /// where it is set if it could be read as an id, because then
    /// `attach <that>` would name two things.
    #[test]
    fn a_box_can_be_given_a_name_that_is_not_an_id() {
        assert_eq!(
            parse_box_args(&strings(&["--new", "--as", "api"])).map(|args| args.alias),
            Ok(Some("api".to_owned()))
        );
        assert_eq!(
            parse_box_args(&strings(&["--as", "0123456789ab"])),
            Err(ParseError::UnusableAlias("0123456789ab".to_owned()))
        );
        assert_eq!(
            parse_box_args(&strings(&["--as", "a/b"])),
            Err(ParseError::UnusableAlias("a/b".to_owned()))
        );
        assert_eq!(
            parse_box_args(&strings(&["--as"])),
            Err(ParseError::AliasWithoutName)
        );
    }

    /// `--env NAME` carries the host's value without spelling it, so a
    /// secret stays out of shell history; `--env NAME=VALUE` spells it.
    #[test]
    fn box_and_attach_declare_variables_by_name_or_by_value() {
        assert_eq!(
            parse_box_args(&strings(&[
                "--env",
                "SENTRY_TOKEN",
                "--env",
                "PGDATABASE=geohod"
            ]))
            .map(|args| args.env),
            Ok(vec![
                EnvArg {
                    name: "SENTRY_TOKEN".to_owned(),
                    value: None
                },
                EnvArg {
                    name: "PGDATABASE".to_owned(),
                    value: Some("geohod".to_owned())
                },
            ])
        );
        assert_eq!(
            parse_attach_args(&strings(&["api", "--env", "PGUSER=root"])).map(|args| args.env),
            Ok(vec![EnvArg {
                name: "PGUSER".to_owned(),
                value: Some("root".to_owned())
            }])
        );
    }

    #[test]
    fn a_variable_must_be_named_and_named_like_one() {
        assert_eq!(
            parse_box_args(&strings(&["--env"])),
            Err(ParseError::EnvWithoutName)
        );
        for wrong in ["=x", "1BAD=x", "A-B", ""] {
            assert_eq!(
                parse_box_args(&strings(&["--env", wrong])),
                Err(ParseError::UnusableEnvName(
                    wrong.split('=').next().unwrap_or_default().to_owned()
                )),
                "{wrong}"
            );
        }
    }

    #[test]
    fn ps_lists_the_running_boxes_and_with_all_every_kept_one() {
        assert_eq!(parse_ps_args(&[]), Ok(PsArgs { all: false }));
        assert_eq!(
            parse_ps_args(&strings(&["--all"])),
            Ok(PsArgs { all: true })
        );
        assert_eq!(parse_ps_args(&strings(&["-a"])), Ok(PsArgs { all: true }));
        assert_eq!(
            parse_ps_args(&strings(&["--every"])),
            Err(ParseError::PsUsage)
        );
    }

    #[test]
    fn attach_with_only_an_id_leaves_the_command_to_the_default() {
        assert_eq!(
            parse_attach_args(&strings(&["0123456789ab"])),
            Ok(AttachArgs {
                id: "0123456789ab".to_owned(),
                command: None,
                env: Vec::new()
            })
        );
    }

    #[test]
    fn attach_takes_a_command_after_the_separator() {
        assert_eq!(
            parse_attach_args(&strings(&["0123456789ab", "--", "claude", "-r"])),
            Ok(AttachArgs {
                id: "0123456789ab".to_owned(),
                command: Some(strings(&["claude", "-r"])),
                env: Vec::new()
            })
        );
    }

    /// `attach` takes the same two forms everything else does, so a word
    /// is an alias to be looked up rather than a typo to refuse here.
    #[test]
    fn attach_takes_an_alias_and_refuses_an_empty_command() {
        assert_eq!(
            parse_attach_args(&strings(&["api"])).map(|args| args.id),
            Ok("api".to_owned())
        );
        assert_eq!(
            parse_attach_args(&strings(&["0123456789ab", "--"])),
            Err(ParseError::AttachUsage)
        );
        assert_eq!(parse_attach_args(&[]), Err(ParseError::AttachUsage));
    }

    #[test]
    fn a_pidfile_path_is_kept_as_given() {
        let parsed = parse_args(&strings(&[
            "--pidfile",
            "/data/boxes/1/init.pid",
            "--",
            "sh",
        ]));
        assert_eq!(
            parsed.expect("valid").pidfile.as_deref(),
            Some("/data/boxes/1/init.pid")
        );
    }

    #[test]
    fn a_pidfile_without_a_path_is_rejected() {
        assert_eq!(
            parse_args(&strings(&["--pidfile", "--", "sh"])),
            Err(ParseError::PidfileWithoutPath)
        );
    }

    /// `rm` takes several, because clearing up after a day's work is the
    /// case it exists for and typing the command once per box is not it.
    #[test]
    fn rm_takes_one_box_or_several() {
        assert_eq!(
            parse_remove_args(&strings(&["0123456789ab"])).map(|args| args.ids),
            Ok(vec!["0123456789ab".to_owned()])
        );
        assert_eq!(
            parse_remove_args(&strings(&["api", "web"])).map(|args| args.ids),
            Ok(vec!["api".to_owned(), "web".to_owned()])
        );
        assert_eq!(parse_remove_args(&[]), Err(ParseError::RemoveUsage));
    }

    /// Resetting is per box on purpose: it throws away one agent's whole
    /// history, and a typo that did it to several at once is not a thing
    /// this command should be able to do.
    #[test]
    fn reset_takes_exactly_one_box() {
        assert_eq!(
            parse_reset_args(&strings(&["api"])).map(|args| args.id),
            Ok("api".to_owned())
        );
        assert_eq!(parse_reset_args(&[]), Err(ParseError::ResetUsage));
        assert_eq!(
            parse_reset_args(&strings(&["api", "web"])),
            Err(ParseError::ResetUsage)
        );
    }

    #[test]
    fn rename_takes_the_box_then_its_new_name() {
        let parsed = parse_rename_args(&strings(&["0123456789ab", "api"])).expect("parses");
        assert_eq!(parsed.id, "0123456789ab");
        assert_eq!(parsed.alias, "api");
        assert_eq!(
            parse_rename_args(&strings(&["api"])),
            Err(ParseError::RenameUsage)
        );
    }

    /// The same rule `--as` is held to, in the same words: one place
    /// decides what may name a box, so the two spellings cannot drift.
    #[test]
    fn rename_refuses_a_name_that_could_be_an_id() {
        assert_eq!(
            parse_rename_args(&strings(&["api", "0123456789ab"])),
            Err(ParseError::UnusableAlias("0123456789ab".to_owned()))
        );
    }

    #[test]
    fn gc_looks_by_default_and_deletes_when_told_to() {
        assert_eq!(
            parse_gc_args(&[]),
            Ok(crate::gc::Sweep {
                delete: false,
                unreferenced: false
            })
        );
        assert_eq!(
            parse_gc_args(&strings(&["--delete"])),
            Ok(crate::gc::Sweep {
                delete: true,
                unreferenced: false
            })
        );
        assert_eq!(
            parse_gc_args(&strings(&["--delete", "--unreferenced"])),
            Ok(crate::gc::Sweep {
                delete: true,
                unreferenced: true
            })
        );
    }

    /// `--unreferenced` widens what `--delete` takes. On its own it would
    /// widen nothing, so it is a typo for `--delete --unreferenced` and
    /// is said to be one rather than quietly doing nothing.
    #[test]
    fn gc_refuses_unreferenced_without_delete() {
        assert_eq!(
            parse_gc_args(&strings(&["--unreferenced"])),
            Err(ParseError::GcUsage)
        );
    }

    #[test]
    fn root_map_makes_us_uid_zero_inside_only() {
        assert_eq!(root_map(1000), "0 1000 1");
    }

    #[test]
    fn identity_map_maps_id_to_itself() {
        assert_eq!(identity_map(1000), "1000 1000 1");
    }

    #[test]
    fn child_exit_code_passes_through() {
        assert_eq!(exit_code(WaitOutcome::Exited(0)), 0);
        assert_eq!(exit_code(WaitOutcome::Exited(7)), 7);
    }

    #[test]
    fn signal_death_becomes_128_plus_signal() {
        assert_eq!(exit_code(WaitOutcome::Signaled(9)), 137);
        assert_eq!(exit_code(WaitOutcome::Signaled(15)), 143);
    }
}
