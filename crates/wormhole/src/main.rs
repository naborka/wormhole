mod boundary;
mod broker;
mod image;
mod panel;
mod probes;
mod terminfo;
mod usage;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nix::fcntl::{Flock, FlockArg};

use wormhole_core::{
    boxenv, doctor, gc, help, home, launch, limits_cgroup, manifest, paths, receipt, registry, run,
    seed, source, tui,
};

const MANIFEST: &str = "wormhole.toml";

/// The instructions every box hands its agent, whatever the role. Baked
/// into the binary so every workspace gets them without carrying a copy.
const DEFAULT_INSTRUCTIONS: &str = include_str!("../../../AGENT.md");

const STATUSLINE_SEED: &str = ".claude/wormhole-statusline.sh";

/// The status line the agent draws during conversation. It only reads the
/// limits file wormhole keeps fresh from the host — no credential and no
/// network inside the box. Seeded fresh on every start, like the preflight
/// hook, so a wormhole upgrade updates it.
fn statusline_script() -> String {
    format!(
        "#!/bin/sh\n\
         # Seeded by wormhole on every box start. Claude Code draws this line\n\
         # while you talk to the agent; wormhole refreshes the file from the\n\
         # host about once a minute.\n\
         cat \"$HOME/{limits}\" 2>/dev/null || printf 'LIMITS: unknown'\n",
        limits = usage::LIMITS_FILE,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // Asked for, so it goes to stdout and exits 0 — a page you have to
        // redirect stderr to read is a page nobody pipes anywhere.
        Some("help" | "--help" | "-h") => {
            print!("{}", help::page());
            std::process::exit(0);
        }
        Some("doctor") => {
            let verdict = doctor::evaluate(probes::run_all());
            print!("{verdict}");
            std::process::exit(verdict.status.exit_code());
        }
        Some("build") => build(&args[1..]),
        Some("box") => run_box(&args[1..]),
        Some("role") => role_cmd(&args[1..]),
        Some("init") => init_cmd(&args[1..]),
        Some("stop") => stop_cmd(&args[1..]),
        Some("remove") => remove_cmd(&args[1..]),
        Some("reset") => reset_cmd(&args[1..]),
        Some("rename") => rename_cmd(&args[1..]),
        Some("gc") => gc_cmd(&args[1..]),
        Some("ps") => ps(&args[1..]),
        Some("usage") => account_usage(),
        Some("attach") => attach(&args[1..]),
        Some("env") => env_cmd(&args[1..]),
        Some("tui") | None => tui(),
        // The banner is not `box`'s: §1 says every launch says which stage
        // its boundary is in, and a bare `run` is a launch.
        Some("run") => match run::parse_args(&args[1..]) {
            Ok(run_args) => {
                println!("{}", launch::Banner::for_run(&run_args));
                std::process::exit(boundary::run(
                    &run_args,
                    None,
                    None,
                    &limits_cgroup::Limits::default(),
                ))
            }
            Err(e) => usage(&e.to_string()),
        },
        // The host half: holds the credential, reaches the API, and is the
        // only thing that does either.
        Some("broker") => broker::serve(
            &paths::broker_socket(&data_home()),
            &host_home().join(".claude/.credentials.json"),
        ),
        Some("__boxed") => boundary::boxed_child(&args[1..]),
        Some("__build") => boundary::boxed_build(&args[1..]),
        Some("__probe") => match args.get(1) {
            Some(name) => probes::child_probe(name),
            None => usage("__probe needs a name"),
        },
        _ => usage("unknown command"),
    }
}

/// Builds the image this workspace's manifest describes, or reports that
/// it is already built. Two caches: the fetched rootfs under its tarball
/// digest, the installed image under its recipe digest.
fn build(args: &[String]) -> ! {
    let parsed = run::parse_box_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    if parsed.command.is_some() || !parsed.env.is_empty() {
        usage("usage: wormhole build [--role <name|dir|ref>]");
    }
    let resolved = resolve_manifest(parsed.role.as_deref());
    let image = ensure_image(&resolved.manifest, &data_home());
    println!("image ready: {}", image.display());
    std::process::exit(0);
}

/// What the workspace's own manifest is called on any screen that says
/// which recipe it picked. One text, so `init`, the panel and a launch
/// cannot drift by a word — the tests assert on it.
pub const WORKSPACE_LABEL: &str = "wormhole.toml (this workspace)";

/// The starter recipe, kept as one file so the binary that writes it and
/// the handbook that prints it can never disagree about what a new
/// workspace gets.
const STARTER: &str = include_str!("../../../templates/wormhole.toml");

/// `wormhole init`: put a working manifest in this workspace.
///
/// The wall used to be here rather than anywhere interesting — a person
/// had to source a rootfs URL and its sixty-four hex characters before
/// anything at all would run.
fn init_cmd(args: &[String]) -> ! {
    if !args.is_empty() {
        usage("usage: wormhole init");
    }
    let path = PathBuf::from(MANIFEST);
    // Never over an existing one. It is somebody's recipe and there is no
    // undo; wormhole writes into the workspace exactly once, here.
    if path.exists() {
        fail(&format!(
            "this workspace already has a {MANIFEST}; \
             `wormhole box` starts a box from it"
        ));
    }
    std::fs::write(&path, STARTER)
        .unwrap_or_else(|e| fail(&format!("cannot write {MANIFEST}: {e}")));
    // The same screen every install shows. What `init` writes grants a
    // credential and runs shell on this machine when the image is built,
    // so it is read out rather than left for somebody to find.
    print!("{}", preview_of(Path::new("."), WORKSPACE_LABEL));
    println!("wrote {MANIFEST}; `wormhole box` starts a box from it");
    std::process::exit(0);
}

fn role_cmd(args: &[String]) -> ! {
    match args {
        [verb, reference] if verb == "add" => role_add(reference, None),
        [verb, reference, flag, name] if verb == "add" && flag == "--as" => {
            role_add(reference, Some(name.clone()))
        }
        [verb] if verb == "list" => role_list(),
        [verb, name] if verb == "show" => role_show(name),
        [verb, name] if verb == "remove" => role_remove(name),
        _ => usage(ROLE_USAGE),
    }
}

const ROLE_USAGE: &str = "usage: wormhole role add <dir|<url|github:owner/repo>@<sha>> [--as <name>] \
     | wormhole role list | wormhole role show <name|dir> | wormhole role remove <name>";

/// The name a role is installed under: what was asked for, or what its
/// source implies. One rule about what may name a role, applied once,
/// whichever kind of source it came from.
fn role_name(given: Option<String>, implied: impl FnOnce() -> String) -> String {
    let name = given.unwrap_or_else(implied);
    if !source::is_usable_name(&name) {
        fail(&source::SourceError::UnusableName(name).to_string());
    }
    name
}

/// Which of the two kinds of source this is.
///
/// Only two, unlike `--role`: an installed name is what `add` produces,
/// never what it takes, so a bare word here is a directory rather than a
/// third form to be ambiguous with.
fn role_add(reference: &str, name: Option<String>) -> ! {
    match source::names(reference) {
        source::Names::Repo(_) => add_pinned(reference, name),
        source::Names::Dir(_) | source::Names::Installed(_) => add_local(reference, name),
    }
}

/// One role under the config home, as the scan of that directory found
/// it. The kind is carried rather than dropped: `role list` needs it, and
/// deciding it twice cost two more stats per role.
struct Installed {
    name: String,
    dir: PathBuf,
    /// Whether it is a pointer at a commit rather than a recipe on this
    /// machine. A bool and not the kind: the scan already refused the
    /// third case, so a row that renders "not a role" cannot happen.
    pinned: bool,
}

/// `wormhole role add <dir> [--as <name>]`: give a role you wrote a name.
///
/// A symlink, not a copy: the directory is the role, and a copy would
/// leave the name quietly running whatever the recipe said on the day it
/// was installed. Nothing is fetched and nothing is approved — there is
/// no commit to approve, and a directory can change a second after any
/// approval, so a gate here would be a promise wormhole cannot keep.
fn add_local(path: &str, name: Option<String>) -> ! {
    let dir =
        std::fs::canonicalize(path).unwrap_or_else(|e| fail(&format!("cannot read {path}: {e}")));
    if !dir.join(MANIFEST).is_file() {
        fail(&format!(
            "{path} has no {MANIFEST} in it, so it is not a role"
        ));
    }
    let name = role_name(name, || {
        dir.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    });

    let slot = paths::role_dir(&config_home(), &name);
    // Asked of the link itself, never through it: a name already taken is
    // a name already taken, whatever kind of role is behind it.
    if std::fs::symlink_metadata(&slot).is_ok() {
        fail(&format!(
            "role {name} is already installed at {}; \
             `wormhole role remove {name}` frees the name",
            slot.display()
        ));
    }
    let roles = paths::roles_dir(&config_home());
    std::fs::create_dir_all(&roles)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", roles.display())));
    std::os::unix::fs::symlink(&dir, &slot)
        .unwrap_or_else(|e| fail(&format!("cannot install role {name}: {e}")));

    print!(
        "{}",
        preview_of(&dir, &format!("role {name} at {}", dir.display()))
    );
    println!("role {name} installed; start it with `wormhole box --role {name}`");
    std::process::exit(0);
}

/// What every installed role is, where it points, and whether it could
/// start right now. The panel shows this too, but only to somebody
/// sitting at it.
fn role_list() -> ! {
    let roles = installed_roles();
    if roles.is_empty() {
        println!(
            "no roles installed in {}",
            paths::roles_dir(&config_home()).display()
        );
        std::process::exit(0);
    }
    let data_home = data_home();
    let mut rows = vec![["NAME", "FROM", "STATE"].map(str::to_owned).to_vec()];
    for role in roles {
        let (from, state) = if role.pinned {
            pinned_row(&role.dir.join(source::POINTER), &data_home)
        } else {
            // `read_link` and not `canonicalize`: the row says where this
            // name points, which is what `role remove` would unlink.
            (
                std::fs::read_link(&role.dir)
                    .unwrap_or(role.dir)
                    .display()
                    .to_string(),
                source::Missing::Nothing.state().to_owned(),
            )
        };
        rows.push(vec![role.name, from, state]);
    }
    print!("{}", wormhole_core::table::render(&rows));
    std::process::exit(0);
}

/// Where a pinned role points and whether this host holds it. A pin that
/// was never fetched is listed rather than hidden — it is installed, and
/// the row is where you find out one command is missing.
fn pinned_row(pointer: &Path, data_home: &Path) -> (String, String) {
    let unreadable = || ("?".to_owned(), "unreadable pointer".to_owned());
    let Ok(text) = std::fs::read_to_string(pointer) else {
        return unreadable();
    };
    let Ok(pinned) = source::parse(&text) else {
        return unreadable();
    };
    let state = source::missing(
        paths::checkout_dir(data_home, &pinned.sha).is_dir(),
        paths::approval_file(data_home, &pinned.sha).is_file(),
    );
    (source::describe(&pinned), state.state().to_owned())
}

/// The screen `role add` asks with, for a role that is already here.
///
/// A dry run for `--role`, and the only way to read an approval screen
/// again once it has been answered.
fn role_show(name: &str) -> ! {
    if let source::Names::Repo(_) = source::names(name) {
        fail("a repository is shown by `wormhole role add`, which fetches it first");
    }
    let at = role_source_dir(name, Fetch::IfAsked).unwrap_or_else(|e| fail(&e));
    // The resolver's own line is the preview's header, so `show` prints
    // one `manifest:` line rather than two.
    print!("{}", preview_of(&at.dir, &at.label));
    std::process::exit(0);
}

/// The whole recipe a manifest would run, printed to a stdout with nobody
/// at it. One body, so what `add` shows, what `show` shows and what the
/// approval screen shows can never drift apart.
fn preview_of(dir: &Path, source_line: &str) -> String {
    let path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", path.display())));
    let manifest = manifest::parse(&text).unwrap_or_else(|e| fail(&e.to_string()));
    let image = paths::image_dir(&data_home(), &manifest::recipe_digest(&manifest));
    wormhole_core::tui::preview(&manifest, source_line, image.is_dir())
}

/// The same recipe drawn for somebody sitting at a terminal, with the keys
/// it offers. Only `panel::confirm` gets this one — a key offered to a
/// stdout nobody is watching is a lie.
fn preview_screen(dir: &Path, source_line: &str) -> String {
    preview_of(dir, source_line) + wormhole_core::tui::PREVIEW_HINTS
}

/// Takes a name back. Replaces the `rm -r` the handbook used to give.
///
/// The link is removed, never followed: a recursive delete through an
/// installed local role would take the directory the user works in with
/// it.
fn role_remove(name: &str) -> ! {
    let config = config_home();
    let slot = paths::role_dir(&config, name);
    let found = std::fs::symlink_metadata(&slot).unwrap_or_else(|_| {
        fail(&format!(
            "no role {name} in {}",
            paths::roles_dir(&config).display()
        ))
    });
    let removed = if found.file_type().is_symlink() {
        std::fs::remove_file(&slot)
    } else {
        std::fs::remove_dir_all(&slot)
    };
    removed.unwrap_or_else(|e| fail(&format!("cannot remove role {name}: {e}")));
    println!("role {name} removed");
    std::process::exit(0);
}

/// `wormhole role add <ref>@<sha> [--as <name>]`: fetch a role out of a
/// git repository, show what it would run, ask, and write the pointer that
/// makes `--role <name>` reach it.
///
/// Everything expensive and everything human happens here, on purpose. A
/// launch afterwards touches no network and asks no question, so a role
/// from a repository is as usable from a script as one on this machine.
fn add_pinned(reference: &str, name: Option<String>) -> ! {
    let pinned = source::parse_ref(reference).unwrap_or_else(|e| fail(&e.to_string()));
    let name = role_name(name, || {
        source::default_name(&pinned.url).unwrap_or_else(|e| fail(&e.to_string()))
    });

    let dir = paths::role_dir(&config_home(), &name);
    let pointer = dir.join(source::POINTER);
    // Re-pinning is where a role's grants and build shell can change under
    // a name that is already trusted, so it is the one thing that must
    // never happen quietly.
    let replacing = match source::role_kind(dir.join(MANIFEST).is_file(), pointer.is_file()) {
        source::RoleKind::Pinned => {
            let text = std::fs::read_to_string(&pointer)
                .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", pointer.display())));
            Some(source::parse(&text).unwrap_or_else(|e| fail(&e.to_string())))
        }
        source::RoleKind::Written => fail(&format!(
            "role {name} is a directory you wrote, at {}; \
             move it aside or pass --as <another name>",
            dir.display()
        )),
        source::RoleKind::Absent => None,
    };
    if let Some(old) = &replacing {
        if old == &pinned {
            println!(
                "role {name} is already pinned to {}",
                source::short(&pinned.sha)
            );
            std::process::exit(0);
        }
        println!(
            "role {name} moves from {} to {}",
            source::short(&old.sha),
            source::short(&pinned.sha)
        );
    }

    fetch_and_approve(&data_home(), &pinned);

    let text = source::to_toml(&pinned).unwrap_or_else(|e| fail(&e));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", dir.display())));
    replace_file(&dir.join(source::POINTER), &text)
        .unwrap_or_else(|e| fail(&format!("cannot write the pointer for role {name}: {e}")));
    println!("role {name} installed; start it with `wormhole box --role {name}`");
    std::process::exit(0);
}

/// Fetches a pin if this host does not hold it, then shows what it would
/// run and asks. Returns only once the commit is both fetched and
/// approved; anything else exits.
fn fetch_and_approve(data_home: &Path, pinned: &source::Source) {
    let checkout = paths::checkout_dir(data_home, &pinned.sha);
    if !checkout.is_dir() {
        let _claim = claim_build(data_home, &pinned.sha, "this role");
        if !checkout.is_dir() {
            fetch_pin(data_home, pinned).unwrap_or_else(|e| fail(&e));
        }
    }

    let approval = paths::approval_file(data_home, &pinned.sha);
    if approval.is_file() {
        return;
    }
    let preview = preview_screen(&checkout, &source::describe(pinned));
    // Somebody else wrote this manifest. The screen is the whole of what
    // stands between its `[image] build` lines and this machine, so it is
    // shown before the pointer is written and before anything is built.
    if !panel::confirm(&preview).unwrap_or_else(|e| fail(&e)) {
        fail("not approved; nothing was installed");
    }
    replace_file(&approval, pinned.url.as_bytes())
        .unwrap_or_else(|e| fail(&format!("cannot record the approval: {e}")));
}

/// One commit of one repository, on disk.
///
/// `git` is what verifies it. Every object it receives is checked against
/// its own hash, so the commit id *is* the proof — no digest of ours to
/// keep, and nothing about the transport to trust. Assembled under
/// `.partial` and renamed, so a fetch that dies half-way leaves nothing a
/// later launch could mistake for a finished checkout.
fn fetch_pin(data_home: &Path, pinned: &source::Source) -> Result<(), String> {
    let checkout = paths::checkout_dir(data_home, &pinned.sha);
    let partial = paths::partial(&checkout);
    image::discard(&partial);
    image::create(&partial)?;

    let at = source::describe(pinned);
    println!("fetching {at}");
    let git = |args: &[&str]| {
        image::run(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&partial),
            &format!("fetch {at}"),
        )
    };
    let fetched = git(&["init", "--quiet"])
        .and_then(|()| git(&["fetch", "--quiet", "--depth", "1", &pinned.url, &pinned.sha]))
        .and_then(|()| git(&["checkout", "--quiet", "FETCH_HEAD"]))
        // The repository is what was wanted, not what was found in it. A
        // role is its manifest, and a checkout without one would otherwise
        // be diagnosed later, as a missing file with no hint of why.
        .and_then(|()| {
            if partial.join(MANIFEST).is_file() {
                Ok(())
            } else {
                Err(format!(
                    "{at} has no {MANIFEST} at its root, so it is not a role"
                ))
            }
        });
    if let Err(e) = fetched {
        image::discard(&partial);
        return Err(e);
    }
    // `.git` is the fetch's own bookkeeping, not the role. Removed so the
    // checkout is the role's files and nothing else.
    image::discard(&partial.join(".git"));
    image::finish(&partial, &checkout).map(|_| ())
}

/// The image this manifest describes, built here if nothing has built it
/// yet. Every launch goes through this: `wormhole build`, a box, a group's
/// members. An image is what a box is made of, so producing one is
/// wormhole's work — a launch that stopped to tell the user to run another
/// command was making them the build system.
///
/// Building is claimed by the digest of what is being built, and waited
/// for rather than refused. Two starts wanting one image is ordinary now
/// that a box builds its own, and both assemble the same `.partial`
/// directory: without the claim the loser would write into the winner's
/// work, and the box that came second would start from an image half made
/// of somebody else's.
fn ensure_image(manifest: &manifest::Manifest, data_home: &Path) -> PathBuf {
    let recipe = manifest::recipe_digest(manifest);
    let image = paths::image_dir(data_home, &recipe);
    if image.is_dir() {
        return image;
    }
    let _claim = claim_build(data_home, &recipe, "this image");
    // Asked again, because waiting for the claim may have been waiting for
    // exactly this image to be built.
    if image.is_dir() {
        return image;
    }

    println!("no image for this manifest yet; building it");
    // The base is shared by every manifest naming the same tarball, so it
    // is claimed by its own digest rather than by this recipe's.
    let base = {
        let _claim = claim_build(data_home, &manifest.image.base_sha256, "this rootfs");
        image::ensure_base(manifest, data_home).unwrap_or_else(|e| fail(&e))
    };
    // Before the copy, so a recipe naming bytes that do not exist — or
    // that arrive wrong — costs nothing and leaves nothing behind.
    let artifacts = fetch_artifacts(manifest, data_home);
    let partial = paths::partial(&image);
    image::copy(&base, &partial).unwrap_or_else(|e| fail(&e));

    if let Some(script) = manifest::build_script(manifest) {
        println!("installing into {}", partial.display());
        let ca = host_ca(manifest).inspect(|bundle| {
            println!("the build trusts host CA bundle {bundle}");
        });
        if let Err(e) = boundary::build_in(&partial, &script, manifest.access.dns, &artifacts, ca) {
            image::discard(&partial);
            fail(&e);
        }
    }
    image::finish(&partial, &image).unwrap_or_else(|e| fail(&e))
}

/// Every artifact the recipe names, fetched on the host and proved, as
/// `(host file, path in the box)`.
///
/// The fetch is the host's because the host is where a resolver, a proxy
/// and a trust store already work. What reaches the build box is bytes
/// whose digest the recipe fixed, so the box needs no route to them and
/// nothing about the transport has to be trusted.
fn fetch_artifacts(manifest: &manifest::Manifest, data_home: &Path) -> Vec<(String, String)> {
    manifest
        .image
        .artifacts
        .iter()
        .map(|artifact| {
            // Claimed by the artifact's own digest, like the base is by
            // its tarball's: two builds wanting one file fetch it once,
            // and the second reads what the first proved. Without it both
            // write one `.partial` and neither digest matches.
            let file = {
                let _claim = claim_build(data_home, &artifact.sha256, "this artifact");
                image::ensure_artifact(&artifact.url, &artifact.sha256, data_home)
                    .unwrap_or_else(|e| fail(&e))
            };
            (file.display().to_string(), artifact.into.clone())
        })
        .collect()
}

/// Claims the right to build what `digest` names, waiting for whoever is
/// building it now. Held until the returned lock is dropped.
fn claim_build(data_home: &Path, digest: &str, what: &str) -> Flock<std::fs::File> {
    let file = paths::build_lock(data_home, digest);
    wait_for_lock(&file, &format!("another wormhole is building {what}"))
        .unwrap_or_else(|e| fail(&e))
}

/// Runs this workspace's agent in a fresh copy of its image. The copy is
/// thrown away afterwards, so nothing the agent does can reach the image
/// the next box starts from. Any command after `--` replaces the agent.
fn run_box(args: &[String]) -> ! {
    let parsed = run::parse_box_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();

    let workspace = here();

    // A box named outright is claimed before anything else, so a refusal
    // costs nothing: which box it is was already said, and resolving a
    // role first could fetch over the network and stop to ask a human on
    // the way to `box X is already running`.
    let named = parsed
        .id
        .as_deref()
        .map(|id| claim_named(&data_home, &workspace, id));

    // Which role this is has to be settled before the search for a box to
    // resume: `--role alphaca` and `--role ./roles/alphaca` name one role,
    // and only resolving them says so.
    let resolved = resolve_manifest(parsed.role.as_deref());

    // Which box this is. A workspace holds as many as you make: this
    // resumes the most recently used one that is free, or starts another.
    // The root is thrown away, the home is kept — per box, so each keeps
    // its own history, settings, logins and installed toolchain.
    //
    // Held until this process exits — `exit` runs no destructor, so the
    // kernel is what ends it.
    let (box_id, _claim) =
        named.unwrap_or_else(|| claim_free(&data_home, &workspace, parsed.new, resolved.wanted()));
    let box_key = paths::box_key(&workspace, &box_id);

    // What this box answers to besides its id. `--as` renames it; without
    // one the box keeps the name it already had. Settled before anything
    // is built, because one name on two boxes would make `attach` guess.
    let box_home = paths::home_dir(&data_home, &box_key);
    let box_alias = box_alias(
        &data_home,
        &box_home,
        &workspace,
        &box_id,
        parsed.alias.as_deref(),
    );

    let Resolved {
        manifest,
        dir: manifest_dir,
        source: role_source,
        typed: role_typed,
        ..
    } = resolved;

    let home = paths::home_dir(&data_home, &box_key);
    std::fs::create_dir_all(&home)
        .unwrap_or_else(|e| fail(&format!("cannot create box home {}: {e}", home.display())));
    // The environment is settled — and refused — before anything is built
    // or claimed: a start missing a value it was asked for costs nothing
    // but the line that says so. The box is told which terminal it is in
    // only once it can look that terminal up, hence `carried` after the
    // home exists.
    let host = terminfo::carried(host_env(), &home);
    let baked = boxenv::resolve(&manifest::declarations(&manifest), &parsed.env, &host);
    refuse_unfilled(&parsed.env, &baked);
    if let Some(line) = boxenv::start_line(&baked) {
        println!("{line}");
    }

    let image = ensure_image(&manifest, &data_home);

    let command = match parsed.command {
        None => manifest::launch_command(&manifest).unwrap_or_else(|e| fail(&e.to_string())),
        Some(command) => command,
    };

    // One directory per box, named by the process that owns it, so two
    // boxes in the same workspace never write to each other's root. It
    // holds the throwaway root copy and the registry entry `wormhole ps`
    // reads, and it goes away with the box. Claimed here, before the
    // multi-second root copy: the entry is what makes the check above true
    // for the box starting next.
    let box_dir = paths::box_dir(&data_home, std::process::id());
    std::fs::create_dir_all(&box_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", box_dir.display())));
    register_box(
        &box_dir,
        &box_id,
        &manifest,
        &workspace,
        &image,
        box_alias.as_deref(),
    );

    // The box's own record, in the home that outlives every process that
    // ran it. A home is named by a digest, and a digest cannot be
    // inverted: without this nothing could say which workspace a home
    // belongs to, list it for resuming, or tell whether it is still wanted.
    write_record(
        &home,
        &box_id,
        &manifest,
        &workspace,
        role_typed.as_deref(),
        role_source.as_deref(),
        box_alias.as_deref(),
    );
    seed_instructions(&manifest, &manifest_dir, &home);
    seed_preflight(&manifest, &manifest_dir, &home);
    seed_claude_config(&manifest, &workspace, &home);
    seed_statusline(&manifest, &home);

    // Keeps the limits file in the box home fresh for the status line.
    // The thread dies with this process, which is the box's lifetime.
    // `home` here is the box home; the host's own home — where the
    // credential lives — is the function, shadowed by that binding.
    {
        let (data_home, host_home, box_home) =
            (data_home.clone(), crate::host_home(), home.clone());
        std::thread::spawn(move || usage::feed_box(&data_home, &host_home, &box_home));
    }

    // The undo point, and what the receipt is measured against. Taken
    // before the box exists, so nothing in it can reach the snapshot, and
    // kept outside the box directory, which is thrown away on exit.
    let before = manifest.runtime.snapshot.then(|| {
        let snapshot = paths::snapshot_dir(&data_home, &box_key);
        image::reflink(&workspace, &snapshot).unwrap_or_else(|e| fail(&e));
        println!("snapshot: {}", snapshot.display());
        walk_workspace(&workspace)
    });

    // A read-only root needs no copy at all — that is the whole of what it
    // buys. The image is shared by every box using it, and read-only is
    // what makes sharing safe rather than merely fast.
    let root = match manifest.runtime.rootfs {
        run::RootMode::Readonly => {
            println!("root: {} (read-only, shared)", image.display());
            image.clone()
        }
        run::RootMode::Copy => {
            let copy = box_dir.join("root");
            println!("root: fresh copy of {}", image.display());
            image::copy(&image, &copy).unwrap_or_else(|e| fail(&e));
            copy
        }
    };

    // Brokering needs exactly two things in the box: the socket to speak
    // to, and the binary that speaks to it. Both read-only, neither of
    // them a credential — that stays on this side.
    let broker_socket = manifest.access.broker.then(|| {
        let socket = paths::broker_socket(&data_home);
        if !socket.exists() {
            fail(&format!(
                "this manifest brokers, but nothing is listening on {}; start one with `wormhole broker`",
                socket.display()
            ));
        }
        socket.display().to_string()
    });

    let run_args = run::RunArgs {
        grants: manifest
            .access
            .grants
            .iter()
            .map(|g| expand_home(g))
            .collect(),
        broker: broker_socket,
        dns: manifest.access.dns,
        image: Some(root.display().to_string()),
        pidfile: Some(box_dir.join("init.pid").display().to_string()),
        ca: host_ca(&manifest).inspect(|bundle| println!("trusting host CA bundle {bundle}")),
        artifacts: Vec::new(),
        network: manifest.access.network,
        root: manifest.runtime.rootfs,
        command,
    };
    // CONCEPT.md §1: every launch says which boundary stage it is in, and
    // what the box can reach. A staged boundary that does not say which
    // stage it is in is a lie, so this is the last line before the agent
    // takes the terminal.
    println!("{}", launch::Banner::for_run(&run_args));
    write_baked_env(&box_dir, &baked);
    let code = boundary::run(
        &run_args,
        Some(&boxenv::to_env(&baked)),
        Some(&home),
        &manifest.limits,
    );
    // The receipt, before the box directory that holds the snapshot goes.
    // Proof of what happened, which is the part trust alone never gives.
    if let Some(before) = before {
        print!(
            "{}",
            receipt::render(&receipt::compare(&before, &walk_workspace(&workspace)), 20)
        );
    }
    image::discard(&box_dir);
    std::process::exit(code);
}

/// Every file in the workspace, by path, size and modification time —
/// what a receipt is computed from. Symlinks are recorded, never followed:
/// a link out of the workspace is a change to the link, not to whatever it
/// points at, and following one would walk the host.
fn walk_workspace(root: &Path) -> Vec<receipt::Entry> {
    fn walk(root: &Path, dir: &Path, into: &mut Vec<receipt::Entry>) {
        for entry in read_dir(dir) {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                walk(root, &path, into);
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            into.push(receipt::Entry {
                path: relative.to_owned(),
                bytes: meta.len(),
                modified_unix: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs()),
            });
        }
    }
    let mut entries = Vec::new();
    walk(root, root, &mut entries);
    entries
}

/// Which box this run is, claimed for as long as this process lives.
///
/// `--id` names one outright. `--new` starts another. A bare `wormhole
/// box` resumes this workspace's most recently used box for this role and
/// falls through to a new one when every existing box is busy — so a
/// folder holds as many boxes as you make, and typing the same command
/// twice gets you back the same box rather than a stranger.
///
/// Whether a box is free is asked by taking its lock and never by reading
/// a list, so the answer cannot go stale between the reading and the
/// start. The kernel owns the claim: an `flock` ends when the process
/// holding it ends, however it ends — a box killed at any point leaves
/// nothing stale to reap.
///
/// The lock's file holds the pid of whoever took it, so a refusal can name
/// them. That text is a courtesy; the lock is the claim.
fn claim_named(data_home: &Path, workspace: &Path, wanted: &str) -> (String, Flock<std::fs::File>) {
    // An id names a box by its directory, and nothing else has to be
    // readable for that: a home whose record is corrupt is still a box
    // this can start. An alias lives *in* the record, so it can only be
    // looked up among the records that parse.
    let id = if paths::is_box_id(wanted) {
        let home = paths::home_dir(data_home, &paths::box_key(workspace, wanted));
        if !home.is_dir() {
            fail(&home::no_box(wanted));
        }
        if let Ok(record) = read_record(&home)
            && record.workspace != workspace
        {
            fail(&format!(
                "box {wanted} belongs to {}, not to this workspace",
                record.workspace.display()
            ));
        }
        wanted.to_owned()
    } else {
        let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
        report_problems(&problems);
        home::by_name(&kept, workspace, wanted)
            .unwrap_or_else(|| fail(&home::no_box(wanted)))
            .id
            .clone()
    };
    match claim_id(data_home, workspace, &id) {
        Some(lock) => (id, lock),
        None => fail(&format!(
            "box {id} is already running{}",
            holder(data_home, &paths::box_key(workspace, &id))
        )),
    }
}

/// The box a bare `wormhole box` should be: this workspace's most recently
/// used free one for this role, or another when every one is busy.
fn claim_free(
    data_home: &Path,
    workspace: &Path,
    new: bool,
    wanted: home::Wanted<'_>,
) -> (String, Flock<std::fs::File>) {
    let claim = |id: &str| claim_id(data_home, workspace, id);
    // A home that cannot be read is a box that cannot be resumed, and the
    // silent answer to that is a *new* box. Say so before starting one.
    let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&problems);
    if !new {
        for record in home::resumable(&kept, workspace, wanted) {
            if let Some(lock) = claim(&record.id) {
                println!("box: {} (resumed)", record.id);
                return (record.id.clone(), lock);
            }
        }
    }

    // Every box is busy, or another was asked for. Ordinals are dense and
    // the loser of a race simply takes the next one, so two `--new` at the
    // same instant get two boxes rather than one refusal.
    let mut taken: Vec<String> = kept.iter().map(|record| record.id.clone()).collect();
    loop {
        let id = paths::box_id(workspace, home::free_ordinal(&taken, workspace));
        if let Some(lock) = claim(&id) {
            println!("box: {id} (new)");
            return (id, lock);
        }
        taken.push(id);
    }
}

/// Takes one box's claim, or `None` when another process holds it.
fn claim_id(data_home: &Path, workspace: &Path, id: &str) -> Option<Flock<std::fs::File>> {
    let file = paths::lock_file(data_home, &paths::box_key(workspace, id));
    try_lock(&file).unwrap_or_else(|e| fail(&e))
}

/// Who holds a box's claim, for a refusal that names them. Empty when the
/// file says nothing — the lock is the claim, this is only the courtesy.
///
/// Takes the key rather than the workspace and the id, because a box
/// whose record cannot be read has a key and nothing else.
fn holder(data_home: &Path, key: &str) -> String {
    let pid = std::fs::read_to_string(paths::lock_file(data_home, key)).unwrap_or_default();
    match pid.trim() {
        "" => String::new(),
        pid => format!(" (pid {pid})"),
    }
}

/// Takes an exclusive lock on `file` without waiting, stamping it with our
/// pid so a refusal can name who holds it. `Ok(None)` means someone else
/// has it.
///
/// One body behind every claim wormhole makes on shared host state: a
/// workspace, and the one poll that may ask for the account's usage
/// windows. The kernel ends both when their holder ends, which is the
/// whole reason neither needs reaping.
pub(crate) fn try_lock(file: &Path) -> Result<Option<Flock<std::fs::File>>, String> {
    match Flock::lock(open_lock(file)?, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => Ok(Some(stamp(lock, file))),
        Err((_, nix::errno::Errno::EWOULDBLOCK)) => Ok(None),
        Err((_, e)) => Err(format!("cannot claim {}: {e}", file.display())),
    }
}

/// The same claim, waited for instead of refused — and saying so, because
/// a wait nobody explains looks like a hang.
///
/// The two answers to "somebody else holds this" are not one answer: a box
/// already running cannot be joined, so its claim is a refusal, while a
/// build already running produces exactly what this process is waiting
/// for, so its claim is a queue.
fn wait_for_lock(file: &Path, waiting_for: &str) -> Result<Flock<std::fs::File>, String> {
    let held = match Flock::lock(open_lock(file)?, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => return Ok(stamp(lock, file)),
        Err((handle, nix::errno::Errno::EWOULDBLOCK)) => handle,
        Err((_, e)) => return Err(format!("cannot claim {}: {e}", file.display())),
    };
    println!("{waiting_for}; waiting for it to finish");
    match Flock::lock(held, FlockArg::LockExclusive) {
        Ok(lock) => Ok(stamp(lock, file)),
        Err((_, e)) => Err(format!("cannot claim {}: {e}", file.display())),
    }
}

/// The file behind a claim, created if it is not there. Never truncated on
/// open: its content belongs to whoever holds the lock, and opening is not
/// holding.
fn open_lock(file: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(file)
        .map_err(|e| format!("cannot open {}: {e}", file.display()))
}

/// Writes our pid into a claim we now hold, so a refusal elsewhere can name
/// us. A courtesy, not the claim: the lock is that.
fn stamp(mut lock: Flock<std::fs::File>, file: &Path) -> Flock<std::fs::File> {
    let pid = std::process::id().to_string();
    let wrote = lock
        .set_len(0)
        .and_then(|()| std::io::Write::write_all(&mut *lock, pid.as_bytes()));
    if let Err(e) = wrote {
        eprintln!(
            "wormhole: cannot record the lock holder in {}: {e}",
            file.display()
        );
    }
    lock
}

/// Every box this host keeps, read from the homes that hold them, and
/// what could not be read. A home that cannot be read is set aside rather
/// than taking the whole listing down: one bad directory must not hide the
/// rest. The homes directory itself is the exception, for the same reason
/// the registry is: nothing kept there could be listed, and "no boxes"
/// would be a lie.
fn kept_boxes(data_home: &Path) -> Result<(Vec<home::Record>, Vec<String>), String> {
    let homes = paths::homes_dir(data_home);
    let mut found = Vec::new();
    let mut problems = Vec::new();
    let dir = match std::fs::read_dir(&homes) {
        Ok(dir) => dir,
        // Nothing has been kept yet, which is not a problem with anything.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((found, problems)),
        Err(e) => return Err(format!("cannot read {}: {e}", homes.display())),
    };
    for entry in dir.flatten() {
        match read_record(&entry.path()) {
            Ok(record) => found.push(record),
            Err(problem) => problems.push(problem),
        }
    }
    Ok((found, problems))
}

/// One box's record, or why it could not be read. Homes written before
/// boxes had ids stamped only the workspace; those are read as that
/// workspace's first box, so an upgrade keeps every agent's history and
/// installed toolchain instead of starting everyone over. The next start
/// writes the full record.
///
/// The reason is returned, never printed: the panel calls this every
/// second while it owns the terminal, and a reader that writes there
/// draws over the screen.
fn read_record(home: &Path) -> Result<home::Record, String> {
    if let Ok(text) = std::fs::read_to_string(home.join(home::RECORD)) {
        return home::parse(&text).map_err(|e| format!("{}: {e}", home.display()));
    }
    std::fs::read_to_string(home.join(home::LEGACY_STAMP))
        .ok()
        .and_then(|stamped| home::from_legacy(home, &stamped))
        .ok_or_else(|| format!("{} holds no box record", home.display()))
}

/// Says once, on stderr, what a scan could not read. Every surface but
/// the panel reports this way; the panel draws them on its own screen.
fn report_problems(problems: &[String]) {
    for problem in problems {
        eprintln!("wormhole: {problem}");
    }
}

/// Writes the box's own record into its home, fresh on every start: what
/// it is, where it works and when it last ran, so `ps --all` can list it
/// and `--id` can bring it back.
fn write_record(
    home: &Path,
    id: &str,
    manifest: &manifest::Manifest,
    workspace: &Path,
    role: Option<&str>,
    alias: Option<&str>,
    source: Option<&str>,
) {
    let now = now_unix();
    let created = read_record(home).map_or(now, |record| match record.created_unix {
        0 => now,
        created => created,
    });
    let record = home::Record {
        id: id.to_owned(),
        workspace: workspace.to_owned(),
        role: role.map(str::to_owned),
        source: source.map(str::to_owned),
        alias: alias.map(str::to_owned),
        name: manifest.name.clone(),
        agent: manifest.agent.run.clone(),
        created_unix: created,
        started_unix: now,
    };
    write_box_record(home, &record).unwrap_or_else(|e| fail(&e));
}

/// Writes this box's registry entry beside its root copy. A failure here
/// is loud: a box that cannot be listed would undermine everything N7+
/// builds on it.
///
/// In one step, because every other wormhole on the host scans this file
/// while we write it: a reader that caught a half-written entry used to
/// take `ps`, the panel and every starting box down with it.
fn register_box(
    box_dir: &Path,
    box_id: &str,
    manifest: &manifest::Manifest,
    workspace: &Path,
    image: &Path,
    alias: Option<&str>,
) {
    let entry = registry::Entry {
        pid: std::process::id(),
        box_id: box_id.to_owned(),
        workspace: workspace.to_owned(),
        image: image.display().to_string(),
        agent: manifest.agent.run.clone(),
        name: manifest.name.clone(),
        alias: alias.map(str::to_owned),
        started_unix: now_unix(),
    };
    let text = registry::to_toml(&entry).unwrap_or_else(|e| fail(&e));
    replace_file(&box_dir.join("box.toml"), &text)
        .unwrap_or_else(|e| fail(&format!("cannot write the box registry entry: {e}")));
}

/// Replaces a file in one step: write beside it, then `rename(2)` over it.
/// The one body behind every file another process reads while we write it
/// — a registry entry, a usage reading, a compiled terminal description.
/// The partial is named by the writing process, so two writers never share
/// one.
pub(crate) fn replace_file(file: &Path, content: impl AsRef<[u8]>) -> Result<(), String> {
    replace(file, content.as_ref(), None)
}

/// The same, for a file only its owner may read. A credential must never
/// sit world-readable, not even for the instant between writing it and
/// fixing its mode — so the mode is set on the partial, before the rename
/// that publishes it.
pub(crate) fn replace_file_private(file: &Path, content: impl AsRef<[u8]>) -> Result<(), String> {
    replace(file, content.as_ref(), Some(0o600))
}

fn replace(file: &Path, content: &[u8], mode: Option<u32>) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let partial = file.with_extension(format!("{}.partial", std::process::id()));
    std::fs::write(&partial, content)
        .map_err(|e| format!("cannot write {}: {e}", partial.display()))?;
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("cannot restrict {}: {e}", partial.display()))?;
    }
    std::fs::rename(&partial, file).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        format!("cannot move into {}: {e}", file.display())
    })
}
/// can be given back. Lists by default — a kept home holds an agent's whole
/// history and an image costs a rebuild, so removing either is something a
/// person asks for rather than something a listing does.
fn gc_cmd(args: &[String]) -> ! {
    let sweep = run::parse_gc_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    let mut items = Vec::new();
    // What the store is keeping something alive for, gathered as the walks
    // go: the digests running boxes are using, and the recipes kept ones
    // start from.
    let mut referenced = BTreeSet::new();

    // A dead box's directory holds a whole root copy. Walked once and
    // never reaped here: `ps` and the panel reap, and a reap in the middle
    // of a scan renames the very tree this loop has just measured.
    for entry in read_dir(&paths::boxes_dir(&data_home)) {
        let path = entry.path();
        let verdict = match box_dir_pid(&path) {
            Some(pid) if alive(pid) => {
                // A running box was copied from its image, and under
                // `rootfs = "readonly"` it *is* the image. Its entry says
                // which, so nothing can delete a box's root in flight.
                referenced.extend(running_image(&path));
                gc::Verdict::Live(format!("pid {pid} is running"))
            }
            Some(pid) => gc::Verdict::Dead(format!("pid {pid} is gone")),
            None => gc::Verdict::Dead("a reap that never finished".to_owned()),
        };
        items.push(gc::Item {
            bytes: tree_bytes(&path),
            path,
            verdict,
        });
    }

    // A kept home outlives the workspace it was kept for. It can be
    // reclaimed because a box records which workspace it belongs to — and
    // the same read gives the recipes below, so the homes are walked once.
    let mut records = Vec::new();
    let mut keys = BTreeSet::new();
    let mut unreadable = 0usize;
    for entry in read_dir(&paths::homes_dir(&data_home)) {
        let path = entry.path();
        keys.extend(
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
        );
        let record = read_record(&path).ok();
        let workspace = record
            .as_ref()
            .map(|record| record.workspace.display().to_string());
        items.push(gc::Item {
            bytes: tree_bytes(&path),
            verdict: gc::home_verdict(
                workspace.as_deref(),
                workspace.as_deref().is_some_and(|w| Path::new(w).is_dir()),
            ),
            path,
        });
        match record {
            Some(record) => records.push(record),
            None => unreadable += 1,
        }
    }

    // A lock file is an empty claim token beside a home; the file's stem
    // is exactly the key that home is named by, so the keys just gathered
    // answer this without a stat each.
    for entry in read_dir(&paths::locks_dir(&data_home)) {
        let path = entry.path();
        let Some(key) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        items.push(gc::Item {
            bytes: tree_bytes(&path),
            verdict: gc::lock_verdict(keys.contains(key)),
            path,
        });
    }

    // Images, bases and artifacts are each named by a digest, and a recipe
    // names every digest the store keeps on its behalf — so reading the
    // recipe of every kept box is what turns "nothing records this" into a
    // proof. A home that could not be read is a recipe that could not be
    // asked, so it counts against completeness like an unreadable one.
    let complete = extend_referenced(&records, &mut referenced) && unreadable == 0;
    for (dir, what) in [
        (paths::images_dir(&data_home), "image"),
        (paths::bases_dir(&data_home), "base"),
        (paths::artifacts_dir(&data_home), "artifact"),
    ] {
        for entry in read_dir(&dir) {
            let path = entry.path();
            let digest = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            items.push(gc::Item {
                bytes: tree_bytes(&path),
                verdict: gc::built_verdict(&digest, &referenced, complete, what),
                path,
            });
        }
    }

    // Biggest first: what is worth deciding about is what is worth seeing.
    items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    // Partitioned once, so what the report describes and what the loop
    // removes are the same set rather than two answers that have to agree.
    let plan = gc::plan(&items, sweep);
    print!("{}", gc::report(&items, sweep, &plan));
    let mut refused = 0usize;
    if sweep.delete {
        for item in &plan.take {
            // The loud form: a person asked for this, so space that did
            // not come back must never be reported as though it had.
            match image::remove(&item.path) {
                Ok(()) => println!("removed {}", item.path.display()),
                Err(why) => {
                    eprintln!("wormhole: {why}");
                    refused += 1;
                }
            }
        }
    }
    std::process::exit(i32::from(refused > 0));
}

/// The image a running box is using, from the entry it wrote at start.
/// `None` when the entry cannot be read — a box mid-start has not written
/// one yet, and a scan must not decide anything from that.
fn running_image(box_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(box_dir.join("box.toml")).ok()?;
    let entry = registry::parse(&text).ok()?;
    Path::new(&entry.image)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

/// Adds every digest these boxes' recipes reference, and says whether all
/// of them could be read.
///
/// A box's recipe is its workspace's own manifest, or the role it was
/// started from. Reading one can fail — a role removed, a manifest since
/// made invalid — and the one that failed may be the very one that
/// references an image, so the answer says so rather than quietly
/// proving something on a gap in the evidence.
///
/// Boxes commonly share a workspace and a role, so each distinct recipe is
/// read once: `paths::box_id` derives an id from a workspace, which is to
/// say a folder full of boxes is a folder with one recipe.
///
/// A box whose workspace is gone is skipped: it is already dead, and
/// counting its recipe would keep a gigabyte alive for nobody.
fn extend_referenced(records: &[home::Record], into: &mut BTreeSet<String>) -> bool {
    let mut complete = true;
    let mut seen = BTreeSet::new();
    for record in records {
        if !record.workspace.is_dir() {
            continue;
        }
        if !seen.insert((record.workspace.clone(), record.role.clone())) {
            continue;
        }
        // Never the launch resolver: that one may fetch a pinned commit
        // and stop for a confirm, and a listing that reaches the network
        // or seizes the terminal is not a listing. Reading is all this
        // needs, so reading is all it is allowed.
        match try_resolve_manifest_in(&record.workspace, record.role.as_deref(), Fetch::Never) {
            Ok(resolved) => into.extend(manifest::referenced_digests(&resolved.manifest)),
            Err(_) => complete = false,
        }
    }
    complete
}

/// A directory's entries, or none when it is not there yet. Every gc scan
/// wants the same "absent is empty" reading.
fn read_dir(dir: &Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(dir).map_or_else(|_| Vec::new(), |read| read.flatten().collect())
}

/// How much disk a tree holds. Apparent size, not blocks: a reflinked copy
/// shares its blocks with the image, so counting blocks would report a box
/// as free when deleting it really does give the space back once the image
/// goes too.
fn tree_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    read_dir(path)
        .iter()
        .map(|entry| tree_bytes(&entry.path()))
        .sum()
}

/// Lists running boxes and reaps entries whose process is gone — a box
/// that died without cleanup must not haunt the listing forever.
///
/// `--all` lists every box this host keeps, running or idle. An idle box
/// is exactly the thing you resume, so leaving it invisible would make it
/// unreachable in practice however well it was kept.
fn ps(args: &[String]) -> ! {
    let parsed = run::parse_ps_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    if parsed.all {
        let scan = box_listings(&data_home).unwrap_or_else(|e| fail(&e));
        print!("{}", home::list(&scan.boxes, now_unix()));
        report_problems(&scan.problems);
    } else {
        let (live, problems) = live_boxes(&data_home).unwrap_or_else(|e| fail(&e));
        print!("{}", registry::ps_table(&live, now_unix()));
        report_problems(&problems);
    }
    std::process::exit(0);
}

/// What is left of the account's usage windows, fetched now. The windows
/// belong to the subscription, not to a box, so this asks the host's own
/// credential and says so plainly.
fn account_usage() -> ! {
    match usage::refresh(&data_home(), &host_home()) {
        Ok(limits) => {
            println!(
                "{}",
                wormhole_core::limits::render(Some(&limits), now_unix())
            );
            std::process::exit(0);
        }
        // A failed fetch still has something true to say if a reading is
        // cached: its numbers, with their age.
        Err(e) => {
            eprintln!("wormhole: {e}");
            println!(
                "{}",
                wormhole_core::limits::render(usage::cached(&data_home()).as_ref(), now_unix())
            );
            std::process::exit(1);
        }
    }
}

/// Now as unix seconds; a clock before 1970 reads as zero rather than a
/// panic.
pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Every box whose process still runs, oldest first, reaping the entries
/// of those that died without cleanup along the way.
///
/// A box directory is named by its owning pid, so liveness is decided from
/// the name and the entry is read only for a box that is still there. That
/// order is what lets a dead box be reaped whatever its entry says — even
/// unreadable, even absent because it died mid-start.
///
/// One entry it cannot read is that box's problem and is returned beside
/// the others; a registry it cannot read at all is nobody's box and is an
/// error, because "no boxes running" would then be a lie.
fn live_boxes(data_home: &Path) -> Result<(Vec<registry::Entry>, Vec<String>), String> {
    let boxes = paths::boxes_dir(data_home);
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let dir = match std::fs::read_dir(&boxes) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((entries, problems)),
        Err(e) => return Err(format!("cannot read {}: {e}", boxes.display())),
    };
    for found in dir.flatten() {
        let path = found.path();
        // Only a directory named by a pid is an entry. `reap` renames a
        // dead box's to `<pid>.dead`, which this skips while it is being
        // deleted — otherwise every scan would reap it again.
        let Some(pid) = box_dir_pid(&path) else {
            // Unless the deletion never finished: the process that started
            // it may have exited first. Any later scan picks it back up,
            // so a whole root copy cannot be orphaned on the disk.
            if path.extension().is_some_and(|ext| ext == DEAD) {
                std::thread::spawn(move || image::discard(&path));
            }
            continue;
        };
        if !alive(pid) {
            reap(&path);
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path.join("box.toml")) else {
            continue; // a box that has not written its entry yet
        };
        // The entry belongs to another process: one file we cannot read
        // must not take down a listing, a panel tick or a starting box.
        match registry::parse(&text) {
            Ok(entry) => entries.push(entry),
            Err(e) => problems.push(format!("box {pid} has an unreadable entry: {e}")),
        }
    }
    entries.sort_by_key(|entry| entry.started_unix);
    Ok((entries, problems))
}

/// What `reap` renames a dead box's directory to, so no scan mistakes it
/// for an entry.
const DEAD: &str = "dead";

/// The pid a box directory is named by — the box's identity, the same one
/// `attach` and `stop` are given. `paths::box_dir` writes these names; this
/// is the only place that reads one back.
fn box_dir_pid(box_dir: &Path) -> Option<u32> {
    box_dir.file_name()?.to_str()?.parse().ok()
}

fn alive(pid: u32) -> bool {
    PathBuf::from("/proc").join(pid.to_string()).is_dir()
}

/// Removes a dead box's directory without making the caller wait: the
/// dir holds a whole root copy, and `ps`, `box` and the panel all scan
/// here. The O(1) rename makes it invisible at once — the new name is no
/// longer a pid, which is what `live_boxes` scans for — and the real
/// deletion runs detached.
fn reap(box_dir: &Path) {
    let trash = box_dir.with_extension(DEAD);
    if std::fs::rename(box_dir, &trash).is_err() {
        // Another scan got there first, or it is already gone.
        return;
    }
    std::thread::spawn(move || image::discard(&trash));
}

/// The control surface: bare `wormhole`. Every box this host keeps, not
/// only the running ones. Enter joins a running box's agent or starts an
/// idle one again, `n` starts another box here, `d` stops, `q` quits.
fn tui() -> ! {
    let data_home = data_home();
    loop {
        // A store the panel cannot read is a line on its own screen, not a
        // refusal: there is no way out of here that would not leave the
        // terminal in raw mode, and an empty list that says why is honest.
        let scan = || {
            box_listings(&data_home).unwrap_or_else(|e| home::Scan {
                boxes: Vec::new(),
                problems: vec![e],
            })
        };
        let picked =
            panel::run(scan, |what| panel_act(&data_home, what)).unwrap_or_else(|e| fail(&e));
        match picked {
            panel::Pick::Quit => std::process::exit(0),
            panel::Pick::Attach(id) => attach_box(&id, None, &[]),
            panel::Pick::Resume(id) => resume_from_panel(&data_home, &id),
            panel::Pick::New => new_box_from_panel(&data_home),
        }
    }
}

/// Every kept box with the pid running it, most recently used first —
/// which is resume order, so the list reads top-down as the boxes a bare
/// `wormhole box` would pick — and everything the scan could not read.
fn box_listings(data_home: &Path) -> Result<home::Scan, String> {
    let (live, mut problems) = live_boxes(data_home)?;
    let (mut kept, unreadable) = kept_boxes(data_home)?;
    problems.extend(unreadable);
    kept.sort_by(|a, b| {
        b.started_unix
            .cmp(&a.started_unix)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(home::Scan {
        boxes: kept
            .into_iter()
            .map(|record| home::Listing {
                running: live
                    .iter()
                    .find(|entry| entry.box_id == record.id)
                    .map(|entry| entry.pid),
                record,
            })
            .collect(),
        problems,
    })
}

/// What the panel does to the box its cursor is on, without leaving its
/// own screen.
///
/// Fallible where the commands' lookup exits: the panel owns the terminal
/// and has a line of its own to say this on, and the problems a scan
/// reports are already drawn there rather than printed over it. The same
/// bodies the commands use, so `x` in here and `wormhole remove` out
/// there can never mean two different things.
fn panel_act(data_home: &Path, what: &tui::Act) -> Result<(), String> {
    let id = match what {
        tui::Act::Stop(pid) => return stop_box(data_home, *pid),
        tui::Act::Remove(id) | tui::Act::Reset(id) => id,
    };
    // The panel always hands an id, and an id names its box from
    // anywhere, so the workspace decides nothing here.
    let (found, _, _) = find_targets(data_home, Path::new(""), std::slice::from_ref(id))?;
    let target = found.first().ok_or_else(|| home::no_box_found(id))?;
    let claimed = Claimed::take(data_home, target)?;
    match what {
        tui::Act::Remove(_) => claimed.remove(data_home),
        tui::Act::Reset(_) => claimed.reset(data_home),
        tui::Act::Stop(_) => unreachable!("returned above"),
    }
}

/// Enter on an idle box: start it again with the home, history and
/// toolchain it kept. In *its* workspace, not wherever the panel was
/// opened — a box belongs to one tree, and starting it anywhere else
/// would point its agent at a stranger's code.
fn resume_from_panel(data_home: &Path, id: &str) -> ! {
    let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&problems);
    let record = kept
        .into_iter()
        .find(|record| record.id == id)
        .unwrap_or_else(|| fail(&format!("no box {id} any more")));
    std::env::set_current_dir(&record.workspace)
        .unwrap_or_else(|e| fail(&format!("cannot enter {}: {e}", record.workspace.display())));
    let mut args = vec!["--id".to_owned(), id.to_owned()];
    if let Some(role) = record.role {
        args.push("--role".to_owned());
        args.push(role);
    }
    run_box(&args)
}

/// `n` in the panel: pick what the new box starts from — the workspace's
/// own manifest or an installed role — see everything it will share, then
/// start it. Backing out at any screen returns to the panel.
fn new_box_from_panel(data_home: &Path) {
    let workspace_manifest = PathBuf::from(MANIFEST).is_file();
    let roles = installed_roles();
    if !workspace_manifest && roles.is_empty() {
        fail(&format!(
            "no {MANIFEST} in this workspace and no roles in {}",
            paths::roles_dir(&config_home()).display()
        ));
    }
    let mut sources: Vec<Option<String>> = Vec::new();
    if workspace_manifest {
        sources.push(None);
    }
    sources.extend(roles.into_iter().map(|role| Some(role.name)));
    let label = |source: &Option<String>| match source {
        None => WORKSPACE_LABEL.to_owned(),
        Some(role) => format!("role {role}"),
    };
    let labels: Vec<String> = sources.iter().map(label).collect();
    loop {
        let Some(index) = panel::choose(&labels).unwrap_or_else(|e| fail(&e)) else {
            return;
        };
        let role = sources[index].as_deref();
        // A role the panel can list is not a role it can always start —
        // one pinned to a commit this host has not fetched is listed,
        // startable by name, and needs `wormhole role add` first. Shown on
        // a screen and returned from, because taking the whole panel down
        // over one unready row would lose every other one with it.
        let text = match try_resolve_manifest(role) {
            Ok(resolved) => {
                let digest = manifest::recipe_digest(&resolved.manifest);
                let image = paths::image_dir(data_home, &digest);
                wormhole_core::tui::preview(&resolved.manifest, &labels[index], image.is_dir())
                    + wormhole_core::tui::PREVIEW_HINTS
            }
            Err(why) => {
                let _ = panel::confirm(&format!("{}\n\n{why}\n\nq back\n", labels[index]))
                    .unwrap_or_else(|e| fail(&e));
                continue;
            }
        };
        if panel::confirm(&text).unwrap_or_else(|e| fail(&e)) {
            // `--new`, because the key is `n` for new: an idle box is
            // resumed with Enter on its own row, where you can see which
            // one you are getting.
            let mut args = vec!["--new".to_owned()];
            if let Some(role) = role {
                args.push("--role".to_owned());
                args.push(role.to_owned());
            }
            run_box(&args);
        }
    }
}

/// Every role installed under the config dir, sorted by name.
fn installed_roles() -> Vec<Installed> {
    let dir = paths::roles_dir(&config_home());
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut roles: Vec<Installed> = read
        .filter_map(|entry| entry.ok())
        // Named before it is stat'd: an entry whose name cannot be typed
        // is not a role whatever it holds, and asking that first spares
        // two `join`s and two `stat`s per entry.
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        // One rule about what may name a role, checked by the reader as
        // well as by the writer. A directory put here by hand under a
        // name `role add` would have refused is not offered as a role.
        .filter(|(name, _)| source::is_usable_name(name))
        // A pointer counts, or a role fetched from a repository would be
        // installed, startable by name, and invisible to the one screen
        // that shows what is installed.
        //
        // The kind is kept rather than dropped: `role list` needs it, and
        // deciding it twice meant two more stats per role and an
        // unreachable "not a role" row.
        .filter_map(|(name, dir)| {
            let kind = source::role_kind(
                dir.join(MANIFEST).is_file(),
                dir.join(source::POINTER).is_file(),
            );
            (kind != source::RoleKind::Absent).then_some(Installed {
                name,
                dir,
                pinned: kind == source::RoleKind::Pinned,
            })
        })
        .collect();
    roles.sort_by(|a, b| a.name.cmp(&b.name));
    roles
}

/// Kills a box's PID 1. Its own `wormhole box` sees the exit and cleans
/// up; a box already going away is not an error.
///
/// Reports rather than swallows. Both callers have somewhere to put a
/// failure — the command on stderr, the panel on its own screen — and a
/// stop that could not happen must never look like one that did.
fn stop_box(data_home: &Path, id: u32) -> Result<(), String> {
    let init = init_pid(data_home, id)?;
    match nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(init as i32),
        nix::sys::signal::Signal::SIGKILL,
    ) {
        // Gone before the signal landed is the outcome that was wanted.
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(e) => Err(format!("cannot stop the box's init {init}: {e}")),
    }
}

/// `wormhole stop <id>`: end a running box from anywhere.
///
/// A box outlives the terminal that started it, so ending one cannot
/// require sitting in the panel — which is what `d` alone meant. Takes
/// the same id `ps` prints and `attach` accepts.
fn stop_cmd(args: &[String]) -> ! {
    let [id] = args else {
        usage("usage: wormhole stop <id>");
    };
    let data_home = data_home();
    let entry = running_box(&data_home, id);
    stop_box(&data_home, entry.pid).unwrap_or_else(|e| fail(&e));
    println!("stopping box {id}");
    std::process::exit(0);
}

/// The workspace a command was typed in. A box belongs to one tree, and
/// this is what says which tree the person asking is standing in.
fn here() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|e| fail(&format!("cannot read the working directory: {e}")))
}

/// Every home directory the store holds, by the key each is named by.
///
/// `home::target` needs these to name a box whose record cannot be read,
/// and the walk is done once for a whole command line rather than once per
/// name that missed.
fn home_keys(data_home: &Path) -> Vec<String> {
    read_dir(&paths::homes_dir(data_home))
        .into_iter()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// The boxes named on a command line, and what the scan could not read.
///
/// Every name becomes a box before any of them is touched: half a list
/// removed and the rest refused over a typo at the end is the worst
/// outcome a removal can have, and this is what makes it impossible.
///
/// Fallible, because the panel owns its terminal and has a line of its own
/// to say this on. `targets` is the same answer for a command, which has
/// nothing else to do with a refusal.
/// What one lookup found: the boxes asked for, every record it read on the
/// way — the commands that follow need them — and what it could not read.
type Found = (Vec<home::Target>, Vec<home::Record>, Vec<String>);

fn find_targets(data_home: &Path, workspace: &Path, wanted: &[String]) -> Result<Found, String> {
    let (kept, problems) = kept_boxes(data_home)?;
    let keys = home_keys(data_home);
    let found = wanted
        .iter()
        .map(|name| {
            home::target(&kept, &keys, workspace, name).ok_or_else(|| home::no_box_found(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((found, kept, problems))
}

/// The same, for a command: what the scan could not read is said on
/// stderr, and a name nothing answers to ends the process.
///
/// The kept records come back with the targets because the commands that
/// follow need them — renaming asks whether a name is free — and reading
/// every home twice for one command is reading it once too often.
fn targets(data_home: &Path, wanted: &[String]) -> (Vec<home::Target>, Vec<home::Record>) {
    let (found, kept, problems) =
        find_targets(data_home, &here(), wanted).unwrap_or_else(|e| fail(&e));
    // A home we could not read may be the very box being asked for, so
    // the reason is said before the refusal rather than swallowed by it.
    report_problems(&problems);
    (found, kept)
}

/// The one box a command names. [`targets`] for the commands that take
/// several, this for the ones that take exactly one.
fn one_target(data_home: &Path, wanted: &str) -> (home::Target, Vec<home::Record>) {
    let (mut found, kept) = targets(data_home, &[wanted.to_owned()]);
    (found.remove(0), kept)
}

/// A box whose claim this process holds, and the only thing the lifecycle
/// verbs will act on.
///
/// The claim is the only honest answer to "is this box running": reading a
/// list can go stale between the reading and the removal, and taking the
/// lock cannot. Making it the *type* rather than a line at the top of
/// three functions is what stops the fourth verb from forgetting it — a
/// verb that never sees a `Claimed` cannot touch a home an agent is
/// writing to.
struct Claimed<'a> {
    target: &'a home::Target,
    _claim: Flock<std::fs::File>,
}

impl<'a> Claimed<'a> {
    fn take(data_home: &Path, target: &'a home::Target) -> Result<Self, String> {
        let claim = try_lock(&paths::lock_file(data_home, &target.key))?.ok_or_else(|| {
            format!(
                "box {id} is running{held}; `wormhole stop {id}` first",
                id = target.id,
                held = holder(data_home, &target.key)
            )
        })?;
        Ok(Claimed {
            target,
            _claim: claim,
        })
    }

    /// Takes the box away: the home it kept and the snapshot beside it.
    ///
    /// The lock file stays. It is the claim being held right now, and
    /// unlinking it would let another start take a second, different lock
    /// on the same box while this one is still deleting. It costs nothing,
    /// it is reused when the ordinal is, and `gc` reclaims the rest.
    fn remove(&self, data_home: &Path) -> Result<(), String> {
        image::remove(&paths::home_dir(data_home, &self.target.key))?;
        image::remove(&paths::snapshot_dir(data_home, &self.target.key))
    }

    /// Empties the home and gives the box back its own record.
    ///
    /// The box survives: same id, same name, same workspace, same role.
    /// Only what the agent put there is gone — which is the difference
    /// between starting over and starting somewhere else, and the reason
    /// this is not `remove` followed by `box --new`.
    fn reset(&self, data_home: &Path) -> Result<(), String> {
        let home = paths::home_dir(data_home, &self.target.key);
        self.remove(data_home)?;
        std::fs::create_dir_all(&home)
            .map_err(|e| format!("cannot create box home {}: {e}", home.display()))?;
        // A box with no record to put back is left empty, which is what it
        // already was: the next start writes one.
        self.target
            .record
            .as_ref()
            .map_or(Ok(()), |record| write_box_record(&home, record))
    }

    /// Sets what the box answers to besides its id.
    ///
    /// Refused while it runs, which the claim has already proved: a
    /// running box's registry entry carries the name it started under,
    /// nothing rewrites that entry in flight, and a rename `attach` could
    /// not follow would be a rename in name only.
    fn rename(&self, data_home: &Path, kept: &[home::Record], alias: &str) -> Result<(), String> {
        // A name lives in the record, so a box with none has nothing to
        // rename — and nothing that says which workspace the name would
        // belong to. Starting it once writes the record this needs.
        let record = self.target.record.as_ref().ok_or_else(|| {
            format!(
                "box {id} has no readable record, so it has nothing to name; \
                 start it once, or `wormhole remove {id}`",
                id = self.target.id
            )
        })?;
        if let Some(taken) = home::alias_conflict(kept, &record.workspace, alias, &record.id) {
            return Err(home::alias_taken(alias, &taken.id));
        }
        let renamed = home::Record {
            alias: Some(alias.to_owned()),
            ..record.clone()
        };
        write_box_record(&paths::home_dir(data_home, &self.target.key), &renamed)
    }
}

/// Writes a box's record into its home. The one writer, so a start, a
/// reset and a rename cannot disagree about what a record on disk is.
///
/// Atomically, because the panel reads this file once a second while it
/// draws: a reader must never see half of one.
fn write_box_record(home: &Path, record: &home::Record) -> Result<(), String> {
    replace_file(&home.join(home::RECORD), home::to_toml(record)?)
}

/// `wormhole remove <id|name>...`: take boxes away.
///
/// `gc` reclaims what it can *prove* is dead. This removes a box that is
/// merely finished with, which nothing else could ever prove.
fn remove_cmd(args: &[String]) -> ! {
    let parsed = run::parse_remove_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    let (found, _) = targets(&data_home, &parsed.ids);
    for target in &found {
        let claimed = Claimed::take(&data_home, target).unwrap_or_else(|e| fail(&e));
        claimed.remove(&data_home).unwrap_or_else(|e| fail(&e));
        // The panel's sentence, said here too: one act, one thing it is
        // reported as, whichever surface asked for it.
        println!("{}", tui::Act::Remove(target.id.clone()).done());
    }
    std::process::exit(0);
}

/// `wormhole reset <id|name>`: keep the box, throw away what is in it.
fn reset_cmd(args: &[String]) -> ! {
    let parsed = run::parse_reset_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    let (target, _) = one_target(&data_home, &parsed.id);
    let claimed = Claimed::take(&data_home, &target).unwrap_or_else(|e| fail(&e));
    claimed.reset(&data_home).unwrap_or_else(|e| fail(&e));
    println!("{}", tui::Act::Reset(target.id).done());
    std::process::exit(0);
}

/// `wormhole rename <id|name> <new name>`: the name you type instead of
/// twelve hex characters, settable without starting the box.
fn rename_cmd(args: &[String]) -> ! {
    let parsed = run::parse_rename_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    let (target, kept) = one_target(&data_home, &parsed.id);
    let claimed = Claimed::take(&data_home, &target).unwrap_or_else(|e| fail(&e));
    claimed
        .rename(&data_home, &kept, &parsed.alias)
        .unwrap_or_else(|e| fail(&e));
    println!("box {} is now {}", target.id, parsed.alias);
    std::process::exit(0);
}

fn box_alias(
    data_home: &Path,
    box_home: &Path,
    workspace: &Path,
    box_id: &str,
    wanted: Option<&str>,
) -> Option<String> {
    let Some(wanted) = wanted else {
        // A home is keyed by workspace and id, so this file is already
        // this box in this workspace — one read rather than a scan of
        // every home on the host to look at one field of one of them.
        return read_record(box_home).ok().and_then(|record| record.alias);
    };
    let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&problems);
    // The rule and the sentence both live beside `is_usable_alias`, so a
    // start, a rename and the panel give one answer to one question.
    if let Some(taken) = home::alias_conflict(&kept, workspace, wanted, box_id) {
        fail(&home::alias_taken(wanted, &taken.id));
    }
    Some(wanted.to_owned())
}

/// The running box an id names, or the refusal that says what lists them.
///
/// One reader, so "no box by that id" cannot fork across the commands that
/// ask it — which it already had, with `attach` and `stop` giving two
/// answers to one question.
fn running_box(data_home: &Path, wanted: &str) -> registry::Entry {
    let (live, problems) = live_boxes(data_home).unwrap_or_else(|e| fail(&e));
    // An entry we could not read may be the very box being asked for, so
    // the reason is said before the refusal rather than swallowed by it.
    report_problems(&problems);
    // The same rule a kept box is found by, so `stop api` and
    // `box --id api` can never mean two different boxes.
    live.into_iter()
        .find(|entry| home::answers_to(&entry.box_id, entry.alias.as_deref(), wanted))
        .unwrap_or_else(|| {
            fail(&format!(
                "no box {wanted} is running; `wormhole ps` lists the running ones \
                 and `--all` every kept one"
            ))
        })
}

/// The host pid of a box's PID 1, from the pidfile beside its root copy.
/// One reader, so "what init.pid is" cannot fork; callers pick loud or
/// quiet on the Result.
fn init_pid(data_home: &Path, id: u32) -> Result<u32, String> {
    let file = paths::box_dir(data_home, id).join("init.pid");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read the box's init pid: {e}"))?;
    text.trim()
        .parse()
        .map_err(|_| format!("corrupt init.pid: {text:?}"))
}

/// Joins a running box by its `wormhole ps` id.
fn attach(args: &[String]) -> ! {
    let parsed = run::parse_attach_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    attach_box(&parsed.id, parsed.command, &parsed.env)
}

/// Joins the box: its agent by default — attaching means getting back to
/// the agent, not a shell beside it — or the given command instead.
///
/// Takes the same id everything else does. A box is named one way in this
/// CLI, so what `ps` prints is what `attach` and `--id` accept.
fn attach_box(id: &str, command: Option<Vec<String>>, env_args: &[run::EnvArg]) -> ! {
    let data_home = data_home();
    let entry = running_box(&data_home, id);
    let command = command.unwrap_or_else(|| manifest::attach_command(entry.agent.as_deref()));
    // The baked env, refreshed: the attacher's host wins where it is set,
    // the baked value survives where it is not, fixed never moves. A box
    // started by an older wormhole left none; its attach keeps the old
    // behavior, the attacher's own environment.
    let host = host_env();
    let box_dir = paths::box_dir(&data_home, entry.pid);
    let env = read_baked_env(&box_dir).map(|baked| {
        let (refreshed, changes) = boxenv::refresh(&baked, env_args, &host);
        refuse_unfilled(env_args, &refreshed);
        if let Some(line) = boxenv::diff_line(&changes) {
            println!("{line}");
        }
        if !changes.is_empty() {
            write_baked_env(&box_dir, &refreshed);
        }
        boxenv::to_env(&refreshed)
    });
    if env.is_none() && !env_args.is_empty() {
        fail("this box predates env tracking; stop it and start it again to use --env");
    }
    // Attaching brings a second terminal, and it is rarely the one the box
    // was started in. Its description is carried in before the join, or
    // this terminal would be named to the box without being resolvable in
    // it. A terminal the host itself cannot describe is left as it is:
    // there is nothing truer to say about it from here.
    let home = paths::home_dir(&data_home, &paths::box_key(&entry.workspace, &entry.box_id));
    terminfo::carry(&host, &home);
    let init = init_pid(&data_home, entry.pid).unwrap_or_else(|e| fail(&e));
    boundary::attach(init, &entry.workspace, &command, env.as_ref())
}

/// `wormhole env <id>`: what the box runs under, secrets masked.
fn env_cmd(args: &[String]) -> ! {
    let [id] = args else {
        usage("usage: wormhole env <id|name>");
    };
    let data_home = data_home();
    let entry = running_box(&data_home, id);
    match read_baked_env(&paths::box_dir(&data_home, entry.pid)) {
        Some(baked) => {
            print!("{}", boxenv::table(&baked));
            std::process::exit(0);
        }
        None => fail("this box predates env tracking; stop it and start it again"),
    }
}

/// Secrets live in this file, so it is born owner-only.
fn write_baked_env(box_dir: &Path, baked: &boxenv::BakedEnv) {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let text = boxenv::to_toml(baked).unwrap_or_else(|e| fail(&e));
    let file = paths::baked_env(box_dir);
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&file)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", file.display())));
}

/// `None` when the box never wrote one — started by an older wormhole.
fn read_baked_env(box_dir: &Path) -> Option<boxenv::BakedEnv> {
    let text = std::fs::read_to_string(paths::baked_env(box_dir)).ok()?;
    Some(boxenv::parse(&text).unwrap_or_else(|e| fail(&e)))
}

/// A start or attach that would quietly run without something asked for
/// refuses instead, naming the fix.
fn refuse_unfilled(cli: &[run::EnvArg], env: &boxenv::BakedEnv) {
    if let Some(first) = boxenv::unfilled_cli(cli, env).first() {
        fail(&format!(
            "--env {first}: {first} has no value anywhere; export it or spell --env {first}=VALUE"
        ));
    }
    let missing = boxenv::missing_required(env);
    if let Some(first) = missing.first() {
        fail(&format!(
            "missing required {}; pass --env {first}=... or export {first}",
            missing.join(", ")
        ));
    }
}

/// Writes the agent's instructions file into the kept home, freshly on
/// every start: the manifest's own directory (workspace or role) and the
/// binary are its source of truth, so nothing a past box wrote there can
/// drift away from them.
fn seed_instructions(manifest: &manifest::Manifest, manifest_dir: &Path, home: &Path) {
    let Some(target) = manifest::instructions_target(manifest) else {
        return;
    };
    let extra = manifest.agent.instructions.as_ref().map(|path| {
        std::fs::read_to_string(manifest_dir.join(path))
            .unwrap_or_else(|e| fail(&format!("cannot read instructions {path}: {e}")))
    });
    // The role's own file last, so it still wins where it disagrees with
    // the built-in instructions.
    seed_file(
        home,
        target,
        &manifest::compose_instructions(DEFAULT_INSTRUCTIONS, extra.as_deref()),
    );
}

/// Writes one seeded file into the kept home, creating its directory —
/// the single body behind every artifact a box start plants there.
fn seed_file(home: &Path, target: &str, content: &str) {
    let file = home.join(target);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", parent.display())));
    }
    std::fs::write(&file, content)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", file.display())));
}

/// Copies the preflight hook into the kept home, where the launch script
/// runs it — the manifest's directory (a role's especially) is not in the
/// box. Fresh on every start, and gone when the manifest names none, so
/// nothing stale survives a manifest change. A named hook that does not
/// exist stops the launch here, on the host, with the path in the error —
/// not in the box with a bare "not found".
fn seed_preflight(manifest: &manifest::Manifest, manifest_dir: &Path, home: &Path) {
    let seed = home.join(manifest::PREFLIGHT_SEED);
    let Some(path) = manifest.agent.preflight.as_ref() else {
        if let Err(e) = std::fs::remove_file(&seed)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            fail(&format!("cannot remove stale {}: {e}", seed.display()));
        }
        return;
    };
    let script = std::fs::read_to_string(manifest_dir.join(path))
        .unwrap_or_else(|e| fail(&format!("cannot read preflight hook {path}: {e}")));
    seed_file(home, manifest::PREFLIGHT_SEED, &script);
}

/// Answers Claude Code's first-run questions in the home's `.claude.json`
/// — merged, never overwritten, so logins and the agent's own choices in
/// the kept home survive.
fn seed_claude_config(manifest: &manifest::Manifest, workspace: &Path, home: &Path) {
    if manifest.agent.run.as_deref() != Some("claude") {
        return;
    }
    let file = home.join(".claude.json");
    let existing = match std::fs::read_to_string(&file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => fail(&format!("cannot read {}: {e}", file.display())),
    };
    let config = seed::claude_config(existing.as_deref(), &workspace.display().to_string())
        .unwrap_or_else(|e| fail(&e));
    std::fs::write(&file, config)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", file.display())));
}

/// Plants the status-line script and points Claude Code's settings at it,
/// so the account's usage windows show during conversation. The script is
/// re-seeded every start; the settings entry is merged, and a status line
/// the user configured themselves wins.
fn seed_statusline(manifest: &manifest::Manifest, home: &Path) {
    if manifest.agent.run.as_deref() != Some("claude") {
        return;
    }
    seed_file(home, STATUSLINE_SEED, &statusline_script());
    let file = home.join(".claude/settings.json");
    let existing = match std::fs::read_to_string(&file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => fail(&format!("cannot read {}: {e}", file.display())),
    };
    let command = format!("sh \"$HOME/{STATUSLINE_SEED}\"");
    let settings =
        seed::claude_settings(existing.as_deref(), &command).unwrap_or_else(|e| fail(&e));
    std::fs::write(&file, settings)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", file.display())));
}

/// The host's CA bundle, when the manifest asks to trust it: the first of
/// the paths distros keep it at, followed through symlinks to the real
/// file so the mount cannot dangle. A host without any is an error the
/// user must see — they asked for a trust the box cannot get.
fn host_ca(manifest: &manifest::Manifest) -> Option<String> {
    if !manifest.access.host_ca {
        return None;
    }
    let candidates = [
        "/etc/ssl/certs/ca-certificates.crt", // Debian, Arch, Alpine
        "/etc/pki/tls/certs/ca-bundle.crt",   // Fedora, RHEL
        "/etc/ssl/ca-bundle.pem",             // openSUSE
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem", // CentOS
        "/etc/ssl/cert.pem",                  // FreeBSD, macOS
    ];
    let found = candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .unwrap_or_else(|| {
            fail("[access] host_ca is set, but no CA bundle exists at any known host path")
        });
    let real = std::fs::canonicalize(found)
        .unwrap_or_else(|e| fail(&format!("cannot resolve the host CA bundle {found}: {e}")));
    Some(real.display().to_string())
}

/// `~` in a manifest grant means the host's home, expanded here because a
/// home directory is not something the pure core can know.
fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => host_home().join(rest).display().to_string(),
        None => path.to_owned(),
    }
}

fn host_home() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => fail("HOME is not set, so `~` in a grant cannot be expanded"),
    }
}

fn host_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

/// A resolved recipe: the manifest, where its files are, and what names
/// the role it came from.
struct Resolved {
    manifest: manifest::Manifest,
    /// What the manifest's relative paths (`instructions`, `hooks`)
    /// resolve against.
    dir: PathBuf,
    /// Where this role comes from, which is what decides whether two
    /// starts mean one box. `None` for the workspace's own manifest,
    /// which the workspace path already names.
    source: Option<String>,
    /// The reference as the user wrote it: what the `ROLE` column shows,
    /// what `--role` gets handed back when the panel resumes this box, and
    /// what a home from before identities is matched on.
    ///
    /// Read from here and never from the raw arguments again, so the
    /// string the record keeps and the string `resumable` matched on
    /// cannot be two different things.
    typed: Option<String>,
    /// The one line saying which recipe was picked. Returned rather than
    /// printed, so a caller that only wants to read a recipe does not get
    /// a line from inside the resolver.
    label: String,
}

impl Resolved {
    fn wanted(&self) -> home::Wanted<'_> {
        home::Wanted {
            source: self.source.as_deref(),
            typed: self.typed.as_deref(),
        }
    }
}

/// The manifest a box or build uses, and the directory its relative paths
/// (`instructions`, `hooks`) resolve against. An explicit `--role` is the
/// user speaking and wins; the workspace's own `wormhole.toml` is the
/// default for the rest. Says which one it picked, so a box built from
/// the wrong manifest never has to be diagnosed from its contents.
/// Says which one it picked, so a box built from the wrong manifest never
/// has to be diagnosed from its contents. Said here, once, for every
/// command that must have an answer — the panel uses the fallible form and
/// draws on its own screen instead.
fn resolve_manifest(role: Option<&str>) -> Resolved {
    let resolved = try_resolve_manifest(role).unwrap_or_else(|e| fail(&e));
    println!("manifest: {}", resolved.label);
    resolved
}

/// The same answer, for the one caller that must survive not getting it:
/// the panel, which draws on its own screen and has other rows to offer.
/// Every other caller is a command that has nothing else to do.
fn try_resolve_manifest(role: Option<&str>) -> Result<Resolved, String> {
    try_resolve_manifest_in(Path::new("."), role, Fetch::IfAsked)
}

/// Whether resolving a role may reach the network and ask a question.
///
/// A launch may: typing a pinned ref is a person asking for that commit
/// right now. A scan may not — `wormhole gc` reports what is already on
/// this machine, and a report that fetched a repository, took the whole
/// terminal for a confirm, and exited before printing a line would not be
/// a report. Asked here, at the edge, rather than inferred further down
/// from whether anyone happens to be watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fetch {
    /// Fetch a pin this host does not have, when there is somebody to
    /// approve it.
    IfAsked,
    /// Read what is here and nothing else.
    Never,
}

/// The same answer for a workspace that is not the one we are standing
/// in. `gc` needs it: proving an image unreferenced means reading the
/// recipe of every box on this host, and those boxes belong to other
/// trees.
fn try_resolve_manifest_in(
    workspace: &Path,
    role: Option<&str>,
    fetch: Fetch,
) -> Result<Resolved, String> {
    let workspace_manifest = workspace.join(MANIFEST);
    // The flag is the user speaking; the file is a default. So a `--role`
    // wins outright, and only without one is the workspace's manifest
    // asked whether it hands its box to a role of its own.
    let named = match role {
        Some(role) => Some(role.to_owned()),
        None => workspace_role(&workspace_manifest)?,
    };
    let (path, dir, source, label) = if let Some(role) = &named {
        let at = role_source_dir(role, fetch)?;
        (at.dir.join(MANIFEST), at.dir, Some(at.source), at.label)
    } else if workspace_manifest.is_file() {
        (
            workspace_manifest,
            workspace.to_path_buf(),
            None,
            WORKSPACE_LABEL.to_owned(),
        )
    } else {
        return Err(format!(
            "no {MANIFEST} in this workspace; write one or pick a role with --role"
        ));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let manifest = manifest::parse(&text).map_err(|e| e.to_string())?;
    Ok(Resolved {
        manifest,
        dir,
        source,
        typed: named,
        label,
    })
}

/// The role this workspace's own manifest hands its box to, if it does.
///
/// Read before the manifest is parsed, because a manifest that names a
/// role carries no `[image]` and so is not a manifest this build could
/// parse at all. A workspace with no manifest names no role.
fn workspace_role(manifest: &Path) -> Result<Option<String>, String> {
    if !manifest.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;
    manifest::names_a_role(&text).map_err(|e| e.to_string())
}

/// The directory `--role` names, whichever of the three ways it was
/// written. Asked in this order because the tests overlap: every remote
/// form carries a `/`, so the path rule would swallow all of them.
///
/// Resolution is total — a caller only ever receives a directory holding a
/// real `wormhole.toml`, never a pointer to one. Everything downstream
/// (the recipe digest, the preview, the seeds) can therefore treat a role
/// as a directory, exactly as it did before roles could live anywhere but
/// this machine.
fn role_source_dir(role: &str, fetch: Fetch) -> Result<RoleAt, String> {
    match source::names(role) {
        source::Names::Repo(_) => {
            let pinned = source::parse_ref(role).map_err(|e| e.to_string())?;
            let name = source::default_name(&pinned.url).map_err(|e| e.to_string())?;
            // Typing a ref is a person asking for it right now, so a
            // launch may fetch and ask — when it was asked to, and when
            // there is somebody there to ask. `fetch_and_approve` is
            // idempotent, so an already-ready pin passes straight through.
            if fetch == Fetch::IfAsked && someone_is_present() {
                fetch_and_approve(&data_home(), &pinned);
            }
            Ok(pinned_at(&name, &pinned, checkout_for(&name, &pinned)?))
        }
        // A separator means the role's directory itself, wherever it is —
        // no install into the config dir needed to try one out.
        source::Names::Dir(_) => {
            let dir = PathBuf::from(role);
            Ok(RoleAt {
                source: source::dir_source(&canonical(&dir)),
                label: format!("role at {role}"),
                dir,
            })
        }
        source::Names::Installed(_) => {
            let dir = paths::role_dir(&config_home(), role);
            let pointer = dir.join(source::POINTER);
            match source::role_kind(dir.join(MANIFEST).is_file(), pointer.is_file()) {
                source::RoleKind::Pinned => {
                    let text = std::fs::read_to_string(&pointer)
                        .map_err(|e| format!("cannot read {}: {e}", pointer.display()))?;
                    let pinned = source::parse(&text).map_err(|e| e.to_string())?;
                    Ok(pinned_at(role, &pinned, checkout_for(role, &pinned)?))
                }
                source::RoleKind::Written => Ok(RoleAt {
                    source: source::dir_source(&canonical(&dir)),
                    label: format!("role {role} ({})", dir.display()),
                    dir,
                }),
                // Said here, where the name is still known. Left to the
                // manifest read below, a typo surfaced as a missing
                // `wormhole.toml` under a path the user never typed.
                source::RoleKind::Absent => Err(source::no_such_role(
                    role,
                    &paths::roles_dir(&config_home()).display().to_string(),
                    Path::new(role).join(MANIFEST).is_file(),
                )),
            }
        }
    }
}

/// A role reference, resolved: where its files are, what names it for as
/// long as it is the same role, and the one line that says which role was
/// picked.
///
/// Returned rather than announced, so a caller that only wants to read a
/// recipe — `wormhole role show` — does not get a line it never asked for
/// printed from inside the resolver.
struct RoleAt {
    dir: PathBuf,
    source: String,
    label: String,
}

fn pinned_at(name: &str, pinned: &source::Source, dir: PathBuf) -> RoleAt {
    RoleAt {
        dir,
        source: source::repo_source(&pinned.url),
        label: format!("role {name} pinned to {}", source::describe(pinned)),
    }
}

/// A role's directory, resolved through symlinks and relative parts, so
/// every spelling of one directory is one role and one box.
///
/// A path that cannot be resolved is left as it was written. The manifest
/// read that follows fails on it and names it, which is a better message
/// than anything invented here.
fn canonical(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_owned())
}

/// Whether anything wormhole asks will reach a human. Asked once, at the
/// edge, so that fetching and prompting are decided where the answer is
/// actually known rather than inferred deeper down.
///
/// A question nobody can see must never be treated as answered: without a
/// terminal, everything that would have asked refuses instead.
fn someone_is_present() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

/// The checkout a pin names, ready to start from.
///
/// It only ever reports; it never fetches and never asks. Both need a
/// person, and the person is at `wormhole role add` — a launch that
/// stopped to prompt would hang a scripted start on a question nobody
/// sees.
fn checkout_for(name: &str, pinned: &source::Source) -> Result<PathBuf, String> {
    let data_home = data_home();
    let checkout = paths::checkout_dir(&data_home, &pinned.sha);
    let approval = paths::approval_file(&data_home, &pinned.sha);
    if let Some(refusal) =
        source::missing(checkout.is_dir(), approval.is_file()).refusal(name, pinned)
    {
        return Err(refusal);
    }
    Ok(checkout)
}

fn config_home() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

fn data_home() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

fn xdg_dir(var: &str, home_suffix: &str) -> PathBuf {
    let wanted = std::env::var_os(var).map(PathBuf::from);
    match std::env::var_os("HOME") {
        Some(home) => paths::xdg_dir(wanted.as_deref(), Path::new(&home), home_suffix),
        // Nothing to fall back to, so the variable must carry the whole
        // answer itself.
        None => wanted
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| fail(&format!("neither {var} nor HOME names an absolute path"))),
    }
}

fn fail(error: &str) -> ! {
    eprintln!("wormhole: {error}");
    std::process::exit(1);
}

fn usage(error: &str) -> ! {
    // The parsers already answer with a usage line for the exact command,
    // which beats a general one. Anything else is a sentence, and gets the
    // tool's name in front of it and the general shape under it.
    if error.starts_with("usage:") {
        eprintln!("{error}");
    } else {
        eprintln!("wormhole: {error}");
        eprintln!("{}", help::USAGE);
    }
    eprintln!("{}", help::MORE);
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    /// The one thing a help page must never do: leave a command out.
    ///
    /// Read off the dispatch itself rather than a second list somebody has
    /// to remember to update — that second list is exactly what drifts.
    /// Internal commands (`__boxed` and friends) are not for anybody to
    /// type, so they are not on the page and are skipped here.
    #[test]
    fn every_command_the_dispatch_answers_is_on_the_help_page() {
        let source = include_str!("main.rs");
        let page = wormhole_core::help::page();
        let mut found = 0;
        for arm in source.split("Some(\"").skip(1) {
            let names = arm.split('"').next().expect("a quoted name");
            for name in names.split("\" | \"") {
                if name.starts_with("__") || name.starts_with('-') {
                    continue;
                }
                found += 1;
                assert!(
                    page.contains(name),
                    "`wormhole {name}` is dispatched and is not on the help page"
                );
            }
        }
        // A parser that matched nothing would pass every assertion above.
        assert!(found > 10, "only found {found} commands to check");
    }

    /// And the other way: a page that lists a command the tool does not
    /// answer to sends the reader somewhere that does not exist.
    #[test]
    fn every_command_the_help_page_lists_is_one_the_dispatch_answers() {
        let source = include_str!("main.rs");
        for command in wormhole_core::help::commands() {
            // `role add` and friends are dispatched by their first word,
            // then by their verb inside `role_cmd`.
            let word = command.split_whitespace().next().expect("a word");
            assert!(
                source.contains(&format!("Some(\"{word}\"")),
                "the help page lists `wormhole {command}`, which nothing dispatches"
            );
        }
    }
}
