mod boundary;
mod image;
mod lock;
mod panel;
mod probes;
mod roles;
mod seed;
mod terminfo;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nix::fcntl::Flock;

use wormhole_core::{
    boxenv, doctor, gc, help, home, launch, limits_cgroup, manifest, paths, registry, run, tui,
};

const MANIFEST: &str = "wormhole.toml";

/// The instructions every box hands its agent, whatever the role. Baked
/// into the binary so every workspace gets them without carrying a copy.
const DEFAULT_INSTRUCTIONS: &str = include_str!("../../../AGENT.md");

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
        Some("role") => roles::role_cmd(&args[1..]),
        Some("init") => init_cmd(&args[1..]),
        Some("stop") => stop_cmd(&args[1..]),
        Some("remove") => remove_cmd(&args[1..]),
        Some("reset") => reset_cmd(&args[1..]),
        Some("rename") => rename_cmd(&args[1..]),
        Some("gc") => gc_cmd(&args[1..]),
        Some("ps") => ps(&args[1..]),
        Some("attach") => attach(&args[1..]),
        Some("secret") => secret_cmd(&args[1..]),
        None => tui(),
        // The boundary on its own, with no manifest: what the kernel tests
        // drive. Its flags are the `__boxed` round trip's, not anybody's.
        Some("__run") => match run::parse_args(&args[1..]) {
            Ok(run_args) => std::process::exit(boundary::run(
                &run_args,
                None,
                None,
                &limits_cgroup::Limits::default(),
            )),
            Err(e) => usage(&e.to_string()),
        },
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
    let resolved = roles::resolve_manifest(parsed.role.as_deref());
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
    print!("{}", roles::preview_of(Path::new("."), WORKSPACE_LABEL));
    println!("wrote {MANIFEST}; `wormhole box` starts a box from it");
    std::process::exit(0);
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
        let ca = seed::host_ca(manifest).inspect(|bundle| {
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
pub(crate) fn claim_build(data_home: &Path, digest: &str, what: &str) -> Flock<std::fs::File> {
    let file = paths::build_lock(data_home, digest);
    lock::wait_for_lock(&file, &format!("another wormhole is building {what}"))
        .unwrap_or_else(|e| fail(&e))
}

/// The product a multi-run start did not name. On a terminal, the same
/// list `n` in the panel already shows; off one, `--run` is the answer.
fn pick_product(names: &[String]) -> String {
    if !roles::someone_is_present() {
        fail(&manifest::need_run(names));
    }
    match panel::choose_from(tui::RUN_HEADING, names).unwrap_or_else(|e| fail(&e)) {
        Some(index) => names[index].clone(),
        None => fail(&manifest::need_run(names)),
    }
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
        .map(|id| lock::claim_named(&data_home, &workspace, id));

    // Which role this is has to be settled before the search for a box to
    // resume: `--role alphaca` and `--role ./roles/alphaca` name one role,
    // and only resolving them says so.
    let resolved = roles::resolve_manifest(parsed.role.as_deref());

    // `--id` already names the box, so its recorded product is the one
    // this start runs unless `--run` says otherwise — and a disagreement
    // is a refusal, not a silent switch of the home's agent.
    let recorded = named.as_ref().and_then(|(id, _)| {
        let home = paths::home_dir(&data_home, &paths::box_key(&workspace, id));
        read_record(&home).ok().and_then(|record| record.agent)
    });
    let mut requested = parsed.run.clone().or(recorded.clone());
    let roles::Resolved {
        manifest,
        dir: manifest_dir,
        source: role_source,
        typed: role_typed,
        ..
    } = resolved;
    // A new box with several products and no `--run` has nothing to
    // resume, so the product is asked before the box exists. `--id` and
    // `--run` already named one; a single product is not a choice.
    if requested.is_none()
        && parsed.new
        && let Some(names) = tui::pick_run(&manifest)
    {
        requested = Some(pick_product(names));
    }
    let unbound = requested.is_none() && tui::pick_run(&manifest).is_some();
    let mut manifest = if unbound {
        manifest
    } else {
        manifest::bound(manifest, requested.as_deref()).unwrap_or_else(|e| fail(&e.to_string()))
    };
    if !unbound
        && let Some(refusal) =
            manifest::product_mismatch(recorded.as_deref(), manifest.agent.product())
    {
        fail(&refusal);
    }

    // Which box this is. A workspace holds as many as you make: this
    // resumes the most recently used one that is free, or starts another.
    // The root is thrown away, the home is kept — per box, so each keeps
    // its own history, settings, logins and installed toolchain.
    //
    // Held until this process exits — `exit` runs no destructor, so the
    // kernel is what ends it.
    let (box_id, _claim) = named.unwrap_or_else(|| {
        lock::claim_free(
            &data_home,
            &workspace,
            parsed.new,
            home::Wanted {
                source: role_source.as_deref(),
                typed: role_typed.as_deref(),
                run: if unbound {
                    None
                } else {
                    manifest.agent.product()
                },
            },
        )
    });
    if unbound {
        let home = paths::home_dir(&data_home, &paths::box_key(&workspace, &box_id));
        requested = read_record(&home)
            .ok()
            .and_then(|record| record.agent)
            .or_else(|| tui::pick_run(&manifest).map(pick_product));
        manifest = manifest::bound(manifest, requested.as_deref())
            .unwrap_or_else(|e| fail(&e.to_string()));
    }
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

    // Before the home and the image: a login this start cannot deliver is
    // not something to find out after a build.
    let credentials = parsed.credentials.unwrap_or(manifest.access.credentials);
    if let Some(refusal) = manifest::credentials_refusal(&manifest, credentials) {
        fail(&refusal);
    }

    let home = paths::home_dir(&data_home, &box_key);
    std::fs::create_dir_all(&home)
        .unwrap_or_else(|e| fail(&format!("cannot create box home {}: {e}", home.display())));
    // The environment is settled — and refused — before anything is built
    // or claimed: a start missing a value it was asked for costs nothing
    // but the line that says so. The box is told which terminal it is in
    // only once it can look that terminal up, hence `carried` after the
    // home exists.
    let host = terminfo::carried(host_env(), &home);
    let declared = manifest::declarations(&manifest);
    let store = read_secrets();
    let mut baked = boxenv::resolve(&declared, &parsed.env, &host, &store);
    // An answer lands in the store, and the store is re-resolved — the
    // resolution rule has one body, and this is not a second one.
    if let Some(store) = ask_secrets(&declared, &baked, store) {
        baked = boxenv::resolve(&declared, &parsed.env, &host, &store);
    }
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
    let now = now_unix();
    write_record(
        &home,
        home::Record {
            id: box_id.clone(),
            workspace: workspace.clone(),
            role: role_typed,
            source: role_source,
            alias: box_alias.clone(),
            name: manifest.name.clone(),
            agent: manifest.agent.product().map(str::to_owned),
            created_unix: now,
            started_unix: now,
        },
    );
    seed::seed_instructions(&manifest, &manifest_dir, &home);
    seed::seed_preflight(&manifest, &manifest_dir, &home);
    seed::seed_agent_config(&manifest, &workspace, &home, credentials);
    let shared_credentials = seed::seed_credentials(credentials, &manifest, &home);
    if manifest.access.dns.is_none() && !Path::new("/etc/resolv.conf").exists() {
        fail("the host has no /etc/resolv.conf; set [access] dns");
    }

    // A read-only root needs no copy at all — that is the whole of what it
    // buys. The image is shared by every box using it, and read-only is
    // what makes sharing safe rather than merely fast.
    let root = match manifest.runtime.rootfs {
        run::RootMode::Readonly => image.clone(),
        run::RootMode::Copy => {
            let copy = box_dir.join("root");
            image::copy(&image, &copy).unwrap_or_else(|e| fail(&e));
            copy
        }
    };

    let run_args = run::RunArgs {
        grants: manifest
            .access
            .grants
            .iter()
            .map(|g| seed::expand_home(g))
            .collect(),
        shared_credentials,
        dns: manifest.access.dns,
        image: Some(root.display().to_string()),
        pidfile: Some(box_dir.join("init.pid").display().to_string()),
        ca: seed::host_ca(&manifest).inspect(|bundle| println!("trusting host CA bundle {bundle}")),
        artifacts: Vec::new(),
        root: manifest.runtime.rootfs,
        command,
    };
    // What the box was handed beyond the baseline, as the last line before
    // the agent takes the terminal.
    if let Some(banner) = launch::banner(&run_args) {
        println!("{banner}");
    }
    write_baked_env(&box_dir, &baked);
    let code = boundary::run(
        &run_args,
        Some(&boxenv::to_env(&baked)),
        Some(&home),
        &manifest.limits,
    );
    image::discard(&box_dir);
    std::process::exit(code);
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
/// and `--id` can bring it back. The first start's time is kept.
///
/// Takes the record whole: three `Option<&str>` in a row once let a start
/// file its name as its role's identity, and named fields cannot be passed
/// in the wrong order.
fn write_record(home: &Path, record: home::Record) {
    let created_unix = read_record(home)
        .ok()
        .map(|kept| kept.created_unix)
        .filter(|created| *created != 0)
        .unwrap_or(record.created_unix);
    write_box_record(
        home,
        &home::Record {
            created_unix,
            ..record
        },
    )
    .unwrap_or_else(|e| fail(&e));
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
        agent: manifest.agent.product().map(str::to_owned),
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
/// — a registry entry, a compiled terminal description.
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
        match roles::try_resolve_manifest_in(
            &record.workspace,
            record.role.as_deref(),
            roles::Fetch::Never,
        ) {
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
/// unreachable in practice however well it was kept. An id or name shows
/// that one box and, while it runs, the environment it runs under.
fn ps(args: &[String]) -> ! {
    let parsed = run::parse_ps_args(args).unwrap_or_else(|e| usage(&e.to_string()));
    let data_home = data_home();
    let scan = box_listings(&data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&scan.problems);
    let now = now_unix();
    match parsed {
        run::PsArgs::All => print_boxes(&scan.boxes, now, home::NO_BOXES_YET),
        run::PsArgs::Running => {
            let running: Vec<home::Listing> = scan
                .boxes
                .into_iter()
                .filter(|listing| listing.running.is_some())
                .collect();
            print_boxes(&running, now, home::NO_BOXES_RUNNING);
        }
        run::PsArgs::One(wanted) => {
            let listing = home::listed(&scan.boxes, &wanted)
                .unwrap_or_else(|| fail(&home::no_box_found(&wanted)));
            print!("{}", home::list(std::slice::from_ref(listing), now));
            if let Some(pid) = listing.running
                && let Some(baked) = read_baked_env(&paths::box_dir(&data_home, pid))
            {
                print!("\n{}", boxenv::table(&baked));
            }
        }
    }
    std::process::exit(0);
}

/// The one table, or the caller's own words for an empty one.
fn print_boxes(boxes: &[home::Listing], now_unix: u64, empty: &str) {
    if boxes.is_empty() {
        println!("{empty}");
    } else {
        print!("{}", home::list(boxes, now_unix));
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
    let (kept, unreadable) = kept_boxes(data_home)?;
    problems.extend(unreadable);
    Ok(home::Scan {
        boxes: home::scan(kept, &live),
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
    let roles = roles::installed_roles();
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
        let resolved = match roles::try_resolve_manifest(role) {
            Ok(resolved) => resolved,
            Err(why) => {
                let _ = panel::confirm(&format!("{}\n\n{why}\n\nq back\n", labels[index]))
                    .unwrap_or_else(|e| fail(&e));
                continue;
            }
        };
        // Several products: pick one. One product: nothing to choose.
        // Backing out returns to the role list, not the box list.
        let run = if let Some(names) = wormhole_core::tui::pick_run(&resolved.manifest) {
            let names = names.to_vec();
            match panel::choose_from(tui::RUN_HEADING, &names).unwrap_or_else(|e| fail(&e)) {
                Some(i) => Some(names[i].clone()),
                None => continue,
            }
        } else {
            None
        };
        let shown = manifest::bound(resolved.manifest, run.as_deref())
            .unwrap_or_else(|e| fail(&e.to_string()));
        let digest = manifest::recipe_digest(&shown);
        let image = paths::image_dir(data_home, &digest);
        let text = wormhole_core::tui::preview(&shown, &labels[index], image.is_dir())
            + wormhole_core::tui::PREVIEW_HINTS;
        if panel::confirm(&text).unwrap_or_else(|e| fail(&e)) {
            // `--new`, because the key is `n` for new: an idle box is
            // resumed with Enter on its own row, where you can see which
            // one you are getting.
            let mut args = vec!["--new".to_owned()];
            if let Some(role) = role {
                args.push("--role".to_owned());
                args.push(role.to_owned());
            }
            if let Some(run) = run {
                args.push("--run".to_owned());
                args.push(run);
            }
            run_box(&args);
        }
    }
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
        let claim =
            lock::try_lock(&paths::lock_file(data_home, &target.key))?.ok_or_else(|| {
                format!(
                    "box {id} is running{held}; `wormhole stop {id}` first",
                    id = target.id,
                    held = lock::holder(data_home, &target.key)
                )
            })?;
        Ok(Claimed {
            target,
            _claim: claim,
        })
    }

    /// Takes the box away: the home it kept.
    ///
    /// The lock file stays. It is the claim being held right now, and
    /// unlinking it would let another start take a second, different lock
    /// on the same box while this one is still deleting. It costs nothing,
    /// it is reused when the ordinal is, and `gc` reclaims the rest.
    fn remove(&self, data_home: &Path) -> Result<(), String> {
        image::remove(&paths::home_dir(data_home, &self.target.key))
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

/// `wormhole secret list|set|remove`: the host-side store of values a
/// manifest's `ask` fills once. Names print; values never do. `set` reads
/// the value masked from the terminal, or from a piped stdin — never from
/// argv, where it would land in shell history.
fn secret_cmd(args: &[String]) -> ! {
    let strings: Vec<&str> = args.iter().map(String::as_str).collect();
    match strings.as_slice() {
        ["list"] => {
            let store = read_secrets();
            if store.is_empty() {
                println!("no secrets kept");
            }
            for name in store.keys() {
                println!("{name}");
            }
        }
        ["set", name] => {
            use std::io::IsTerminal;
            let value = if std::io::stdin().is_terminal() {
                prompt_secret(name)
                    .unwrap_or_else(|| fail("no terminal to ask on; pipe the value in instead"))
            } else {
                let mut piped = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut piped)
                    .unwrap_or_else(|e| fail(&format!("cannot read the value: {e}")));
                piped.trim_end_matches(['\r', '\n']).to_owned()
            };
            if value.is_empty() {
                fail("an empty value would keep nothing; give one");
            }
            let mut store = read_secrets();
            store.insert((*name).to_owned(), value);
            write_secrets(&store);
            println!("{name} kept for every box");
        }
        ["remove", name] => {
            let mut store = read_secrets();
            if store.remove(*name).is_none() {
                fail(&format!("no secret {name} is kept"));
            }
            write_secrets(&store);
            println!("{name} removed; the next box that asks for it asks you");
        }
        _ => usage("usage: wormhole secret list | set NAME | remove NAME"),
    }
    std::process::exit(0)
}

/// The host-side secret store, absent file meaning empty store.
fn read_secrets() -> wormhole_core::secrets::Store {
    let file = paths::secrets_file(&config_home());
    match std::fs::read_to_string(&file) {
        Ok(text) => wormhole_core::secrets::parse(&text).unwrap_or_else(|e| fail(&e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => fail(&format!("cannot read {}: {e}", file.display())),
    }
}

fn write_secrets(store: &wormhole_core::secrets::Store) {
    let text = wormhole_core::secrets::to_toml(store).unwrap_or_else(|e| fail(&e));
    replace_file_private(&paths::secrets_file(&config_home()), text).unwrap_or_else(|e| fail(&e));
}

/// Asks for every `ask` variable still without a value, once ever: the
/// answer goes to the host-side store, so every later box — any role,
/// any workspace — already has it. Off a terminal nothing can ask, so
/// the start says what would fill the gap and moves on.
fn ask_secrets(
    declared: &BTreeMap<String, manifest::EnvVar>,
    baked: &boxenv::BakedEnv,
    mut store: wormhole_core::secrets::Store,
) -> Option<wormhole_core::secrets::Store> {
    let mut kept = false;
    for name in boxenv::to_ask(declared, baked) {
        let Some(value) = prompt_secret(name) else {
            eprintln!(
                "wormhole: {name} has no value and no terminal to ask on; \
                 run `wormhole secret set {name}`"
            );
            continue;
        };
        if value.is_empty() {
            continue;
        }
        store.insert(name.to_owned(), value);
        kept = true;
    }
    if !kept {
        return None;
    }
    write_secrets(&store);
    println!(
        "kept in {} for every box",
        paths::secrets_file(&config_home()).display()
    );
    Some(store)
}

/// One masked line read from the terminal itself, not stdin: `/dev/tty`
/// is what makes this work under a pipe, and its absence is what makes
/// "no terminal" true. `None` means nobody can answer; an empty answer
/// means "not now" and is the caller's to interpret.
fn prompt_secret(name: &str) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let _ = write!(
        tty,
        "{name} (asked once, kept for every box; empty skips): "
    );
    let _ = tty.flush();
    let quiet = nix::sys::termios::tcgetattr(&tty).ok().inspect(|original| {
        let mut masked = original.clone();
        masked.local_flags &= !nix::sys::termios::LocalFlags::ECHO;
        let _ = nix::sys::termios::tcsetattr(&tty, nix::sys::termios::SetArg::TCSANOW, &masked);
    });
    let mut value = String::new();
    let read = BufReader::new(&tty).read_line(&mut value);
    if let Some(original) = quiet {
        let _ = nix::sys::termios::tcsetattr(&tty, nix::sys::termios::SetArg::TCSANOW, &original);
    }
    let _ = writeln!(tty);
    read.ok()?;
    Some(value.trim_end_matches(['\r', '\n']).to_owned())
}

/// A start or attach that would quietly run without something asked for
/// refuses instead, naming the fix.
fn refuse_unfilled(cli: &[run::EnvArg], env: &boxenv::BakedEnv) {
    if let Some(first) = boxenv::unfilled_cli(cli, env).first() {
        fail(&format!(
            "--env {first}: {first} has no value anywhere; export it or spell --env {first}=VALUE"
        ));
    }
}

fn host_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
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
