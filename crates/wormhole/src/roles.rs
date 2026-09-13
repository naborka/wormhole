//! Roles: installing one, listing them, and resolving the manifest a
//! start uses — the workspace's own, or a role reached by name, by
//! directory, or by pinned commit.

use std::path::{Path, PathBuf};

use wormhole_core::{manifest, paths, source};

use crate::{MANIFEST, config_home, data_home, fail, image, panel, replace_file, usage};

pub(crate) fn role_cmd(args: &[String]) -> ! {
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
pub(crate) struct Installed {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
    /// Whether it is a pointer at a commit rather than a recipe on this
    /// machine. A bool and not the kind: the scan already refused the
    /// third case, so a row that renders "not a role" cannot happen.
    pub(crate) pinned: bool,
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
pub(crate) fn preview_of(dir: &Path, source_line: &str) -> String {
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
pub(crate) fn preview_screen(dir: &Path, source_line: &str) -> String {
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
        let _claim = crate::claim_build(data_home, &pinned.sha, "this role");
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

/// Every role installed under the config dir, sorted by name.
pub(crate) fn installed_roles() -> Vec<Installed> {
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

/// A resolved recipe: the manifest, where its files are, and what names
/// the role it came from.
pub(crate) struct Resolved {
    pub(crate) manifest: manifest::Manifest,
    /// What the manifest's relative paths (`instructions`, `hooks`)
    /// resolve against.
    pub(crate) dir: PathBuf,
    /// Where this role comes from, which is what decides whether two
    /// starts mean one box. `None` for the workspace's own manifest,
    /// which the workspace path already names.
    pub(crate) source: Option<String>,
    /// The reference as the user wrote it: what the `ROLE` column shows,
    /// what `--role` gets handed back when the panel resumes this box, and
    /// what a home from before identities is matched on.
    ///
    /// Read from here and never from the raw arguments again, so the
    /// string the record keeps and the string `resumable` matched on
    /// cannot be two different things.
    pub(crate) typed: Option<String>,
    /// The line saying which role was picked; `None` for the workspace's own
    /// manifest, the default, which goes unsaid. Returned rather than
    /// printed, so a caller that only wants to read a recipe does not get
    /// a line from inside the resolver.
    pub(crate) label: Option<String>,
}

/// The manifest a box or build uses, and the directory its relative paths
/// (`instructions`, `hooks`) resolve against. An explicit `--role` is the
/// user speaking and wins; the workspace's own `wormhole.toml` is the
/// default for the rest and goes unsaid. Says which role it picked, so a
/// box built from the wrong manifest never has to be diagnosed from its
/// contents. Said here, once, for every command that must have an answer —
/// the panel uses the fallible form and draws on its own screen instead.
pub(crate) fn resolve_manifest(role: Option<&str>) -> Resolved {
    let resolved = try_resolve_manifest(role).unwrap_or_else(|e| fail(&e));
    if let Some(label) = &resolved.label {
        println!("manifest: {label}");
    }
    resolved
}

/// The same answer, for the one caller that must survive not getting it:
/// the panel, which draws on its own screen and has other rows to offer.
/// Every other caller is a command that has nothing else to do.
pub(crate) fn try_resolve_manifest(role: Option<&str>) -> Result<Resolved, String> {
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
pub(crate) enum Fetch {
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
pub(crate) fn try_resolve_manifest_in(
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
        (
            at.dir.join(MANIFEST),
            at.dir,
            Some(at.source),
            Some(at.label),
        )
    } else if workspace_manifest.is_file() {
        (workspace_manifest, workspace.to_path_buf(), None, None)
    } else {
        return Err(format!(
            "no {MANIFEST} in this workspace; write one or pick a role with --role"
        ));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let manifest = manifest::parse(&text).map_err(|e| e.to_string())?;
    // A box made from a workspace's own file runs only in that workspace,
    // and a cloned repository must not decide how far its box reaches.
    if source.is_none() && manifest.agent.resume == manifest::Resume::Anywhere {
        return Err(format!(
            "{} says resume = \"anywhere\", but a box made from a workspace's own \
             {MANIFEST} runs only there; that line belongs in a role",
            path.display()
        ));
    }
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
pub(crate) fn workspace_role(manifest: &Path) -> Result<Option<String>, String> {
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
pub(crate) struct RoleAt {
    pub(crate) dir: PathBuf,
    pub(crate) source: String,
    pub(crate) label: String,
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
pub(crate) fn someone_is_present() -> bool {
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
