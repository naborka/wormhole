//! `wormhole build` end to end against a `file://` tarball, so it needs no
//! network and no namespaces.

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{build_in, one_image, rootfs_tarball};

/// The one kept home a test's box created.
fn kept_home(data_home: &Path) -> std::path::PathBuf {
    std::fs::read_dir(data_home.join("wormhole/homes"))
        .expect("homes dir")
        .next()
        .expect("one kept home")
        .expect("entry")
        .path()
}

/// `extra` goes above `[image]`: TOML reads a top-level key written below
/// a header as part of that table.
fn write_manifest(dir: &Path, url: &str, sha256: &str, extra: &str) {
    write_recipe(dir, url, sha256, extra, "");
}

/// The same, plus lines after `[image]` — where its own keys and its
/// `[[image.artifact]]` tables have to go.
fn write_recipe(dir: &Path, url: &str, sha256: &str, extra: &str, after: &str) {
    let version = wormhole_core::manifest::VERSION;
    std::fs::write(
        dir.join("wormhole.toml"),
        format!(
            "version = {version}\n{extra}[image]\nbase = \"file://{url}\"\nbase_sha256 = \"{sha256}\"\n{after}"
        ),
    )
    .expect("manifest written");
}

/// Writes a file and returns the path a `file://` URL names it by.
fn a_file_holding(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("artifact written");
    path.display().to_string()
}

#[test]
fn build_fetches_verifies_and_extracts_the_rootfs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");

    let output = build_in(&workspace, &data_home);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fetching file://"),
        "the build must say what it fetches:\n{stdout}"
    );

    let base = data_home.join("wormhole/bases").join(&digest);
    assert_eq!(
        std::fs::read_to_string(base.join("bin/hello")).expect("extracted file"),
        "#!/bin/sh\necho hi\n"
    );
    assert!(!base.join("rootfs.tar").exists(), "tarball was left behind");
    assert!(
        one_image(&data_home).join("bin/hello").exists(),
        "image has no rootfs"
    );
}

/// A box with no image is not a refusal. The image is what a box is made
/// of, and making it is wormhole's work, not a step the user has to
/// remember before every first start.
///
/// The box itself needs namespaces this host may not have, so its exit
/// status is ignored on purpose; the image is on the host either way.
#[test]
fn a_box_with_no_image_builds_one_instead_of_refusing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");

    let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no image for this manifest yet"),
        "{stdout}"
    );
    assert!(
        one_image(&data_home).join("bin/hello").exists(),
        "the box refused instead of building:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Two wormholes wanting one image is ordinary now that a box builds its
/// own. They assemble the same `<digest>.partial` directory, so the second
/// must wait for the first rather than write into its work.
#[test]
fn a_second_build_waits_for_the_one_already_running() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");

    let text = std::fs::read_to_string(workspace.join("wormhole.toml")).expect("manifest");
    let recipe =
        wormhole_core::manifest::recipe_digest(&wormhole_core::manifest::parse(&text).expect("m"));
    let lock = wormhole_core::paths::build_lock(&data_home, &recipe);
    std::fs::create_dir_all(lock.parent().expect("locks dir")).expect("locks dir");
    let held = nix::fcntl::Flock::lock(
        std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock)
            .expect("open the build claim"),
        nix::fcntl::FlockArg::LockExclusiveNonblock,
    )
    .expect("this test holds the build");

    let mut child = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .arg("build")
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("wormhole binary should spawn");

    // Blocks until it says it is waiting, so nothing here depends on how
    // long the second build takes to reach the claim. The read ends when
    // the child exits, so a build that never waits fails rather than hangs.
    let mut reader = std::io::BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut line = String::new();
    let mut waited = false;
    while std::io::BufRead::read_line(&mut reader, &mut line).unwrap_or(0) > 0 {
        if line.contains("waiting") {
            waited = true;
            break;
        }
        line.clear();
    }
    assert!(waited, "the second build did not wait for the first");
    assert!(
        !data_home.join("wormhole/images").join(&recipe).is_dir(),
        "the second build finished while the first held the claim"
    );

    drop(held);
    // Drained, and only then waited for: the pipe stays open to the end,
    // so the freed build is never killed by writing into a closed one.
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut reader, &mut rest).expect("the rest of the build");
    assert!(child.wait().expect("child").success(), "{rest}");
    assert!(one_image(&data_home).join("bin/hello").exists());
}

#[test]
fn a_second_build_reuses_the_image() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");

    assert!(build_in(&workspace, &data_home).status.success());
    let marker = data_home
        .join("wormhole/bases")
        .join(&digest)
        .join("reused-proof");
    std::fs::write(&marker, "kept").expect("marker written");

    assert!(build_in(&workspace, &data_home).status.success());
    assert!(marker.exists(), "the image was rebuilt instead of reused");
}

#[test]
fn a_wrong_digest_refuses_and_leaves_no_image() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, _) = rootfs_tarball(temp.path());
    let wrong = "0".repeat(64);
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &wrong, "");

    let output = build_in(&workspace, &data_home);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("digest does not match"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!data_home.join("wormhole/bases").join(&wrong).is_dir());
}

/// A cached artifact is disk like any other cache — one of this repo's own
/// is 100 MB — so `gc` has to be able to name it. It is reported and never
/// removed, for the same reason a base is: nothing records which recipe
/// still wants it.
#[test]
fn gc_names_a_cached_artifact_it_cannot_prove_unwanted() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data_home = temp.path().join("data");
    let digest = "a".repeat(64);
    let cached = data_home.join("wormhole/artifacts").join(&digest);
    std::fs::create_dir_all(cached.parent().expect("a directory")).expect("cache dir");
    std::fs::write(&cached, "some fetched bytes").expect("cached artifact");

    let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .arg("gc")
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&digest), "{stdout}");
    assert!(cached.is_file(), "gc removed what it cannot prove unwanted");
}

/// An artifact is proved by its digest exactly as the base is, and on the
/// host, before any box starts. Bytes that do not match are not the bytes
/// the recipe named, so there is nothing to build from.
#[test]
fn a_wrong_artifact_digest_refuses_and_leaves_no_image() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let artifact = a_file_holding(temp.path(), "tool", "the real bytes");
    let wrong = "0".repeat(64);
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_recipe(
        &workspace,
        &url,
        &digest,
        "",
        &format!(
            "build = [\"true\"]\n[[image.artifact]]\nurl = \"file://{artifact}\"\nsha256 = \"{wrong}\"\ninto = \"/tmp/tool\"\n"
        ),
    );

    let output = build_in(&workspace, &data_home);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("digest does not match"), "{stderr}");
    assert!(!data_home.join("wormhole/images").is_dir(), "{stderr}");
}

/// `wormhole box` prepares the workspace's kept home — the directory and
/// the agent's instructions file — before it enters any namespace, so
/// this holds even where `unshare` is denied and the box itself cannot
/// start. The box's exit status is ignored on purpose.
#[test]
fn box_seeds_the_kept_home_with_the_instructions() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(
        &workspace,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\ninstructions = \"ROLE.md\"\n",
    );
    std::fs::write(workspace.join("ROLE.md"), "Be the architect.\n").expect("role written");
    assert!(build_in(&workspace, &data_home).status.success());

    let _ = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");

    let home = kept_home(&data_home);
    assert!(
        home.file_name()
            .expect("name")
            .to_string_lossy()
            .starts_with("workspace-"),
        "home named after the workspace: {}",
        home.display()
    );
    let text = std::fs::read_to_string(home.join("AGENTS.md")).expect("instructions seeded");
    let default = text.find("isolated box").expect("built-in instructions");
    let extra = text.find("Be the architect.").expect("extra instructions");
    assert!(default < extra, "extra instructions must come last");
    // The agent's own file is a pointer, so the text exists exactly once.
    let pointer = std::fs::read_to_string(home.join(".claude/CLAUDE.md")).expect("pointer seeded");
    assert_eq!(pointer, "@~/AGENTS.md\n");

    let config = std::fs::read_to_string(home.join(".claude.json")).expect("config seeded");
    for answered in [
        "\"hasCompletedOnboarding\": true",
        "\"bypassPermissionsModeAccepted\": true",
        "\"hasTrustDialogAccepted\": true",
    ] {
        assert!(config.contains(answered), "{answered} missing:\n{config}");
    }
    assert!(
        config.contains(&format!("\"{}\"", workspace.display())),
        "trust must name the workspace:\n{config}"
    );
}

/// A workspace holds as many boxes as you make, each with its own kept
/// home — and a bare `wormhole box` goes back to one instead of piling up
/// a stranger every time.
///
/// The box itself needs namespaces this may not have; the homes are
/// prepared before any of that, so the exit status is ignored on purpose.
#[test]
fn a_workspace_holds_as_many_boxes_as_you_make() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");
    assert!(build_in(&workspace, &data_home).status.success());

    let box_here = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
            .arg("box")
            .args(args)
            .args(["--", "/bin/true"])
            .current_dir(&workspace)
            .env("XDG_DATA_HOME", &data_home)
            .output()
            .expect("wormhole binary should spawn");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    let first = box_here(&[]);
    let second = box_here(&["--new"]);
    assert!(first.contains("(new)"), "{first}");
    assert!(second.contains("(new)"), "{second}");

    let homes: Vec<PathBuf> = std::fs::read_dir(data_home.join("wormhole/homes"))
        .expect("homes dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(homes.len(), 2, "{homes:?}");
    let ids: std::collections::BTreeSet<String> = homes
        .iter()
        .map(|home| {
            let text = std::fs::read_to_string(home.join(".wormhole/box.toml"))
                .unwrap_or_else(|e| panic!("{}: {e}", home.display()));
            wormhole_core::home::parse(&text).expect("record").id
        })
        .collect();
    assert_eq!(ids.len(), 2, "two boxes must not share an id: {ids:?}");

    // A third bare run is not a third box: it goes back to one of these.
    let third = box_here(&[]);
    assert!(third.contains("(resumed)"), "{third}");
    assert_eq!(
        std::fs::read_dir(data_home.join("wormhole/homes"))
            .expect("homes dir")
            .count(),
        2
    );
    assert!(
        ids.iter().any(|id| third.contains(id.as_str())),
        "resumed a box that is not one of {ids:?}: {third}"
    );
}

/// What a start is told lands where it means it: the name after `--as`
/// is the name the box answers to, and where its role comes from is what
/// the next start matches on. Every earlier test wrote records by hand, so
/// a start that filed the two the wrong way round passed all of them.
#[test]
fn a_start_records_its_name_and_its_role_where_each_belongs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let role = temp.path().join("roles/tester");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&role).expect("role dir");
    write_manifest(&role, &url, &digest, "");
    let role = role.canonicalize().expect("real role dir");
    let role_arg = role.display().to_string();

    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
            .args(args)
            .current_dir(&workspace)
            .env("XDG_DATA_HOME", &data_home)
            .output()
            .expect("wormhole binary should spawn");
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr)
    };

    let first = run(&[
        "box",
        "--role",
        &role_arg,
        "--new",
        "--as",
        "api",
        "--",
        "/bin/true",
    ]);
    let text = std::fs::read_to_string(kept_home(&data_home).join(".wormhole/box.toml"))
        .unwrap_or_else(|e| panic!("record written: {e}\n{first}"));
    let record = wormhole_core::home::parse(&text).expect("record");
    assert_eq!(record.alias.as_deref(), Some("api"), "{text}");
    assert_eq!(
        record.source,
        Some(wormhole_core::source::dir_source(&role)),
        "{text}"
    );

    assert!(run(&["ps", "--all"]).contains("api"));
    let again = run(&["box", "--role", &role_arg, "--", "/bin/true"]);
    assert!(
        again.contains(&format!("box: {} (resumed)", record.id)),
        "{again}"
    );
    let named = run(&["box", "--id", "api", "--role", &role_arg, "--", "/bin/true"]);
    assert!(!named.contains("no box api"), "{named}");
}

/// A home kept before boxes had ids must be picked up as its workspace's
/// first box. Orphaning it would throw away the agent's whole history and
/// everything the preflight hook installed, for a change that renames
/// nothing on disk.
#[test]
fn a_home_from_before_box_ids_is_resumed_not_orphaned() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");
    assert!(build_in(&workspace, &data_home).status.success());

    // A home exactly as an older wormhole left it: named by a digest of
    // the workspace, stamped with that path and nothing else.
    let real = workspace.canonicalize().expect("real workspace");
    let id = wormhole_core::paths::box_id(&real, 0);
    let legacy = data_home
        .join("wormhole/homes")
        .join(format!("workspace-{id}"));
    std::fs::create_dir_all(legacy.join(".wormhole")).expect("legacy home");
    std::fs::write(
        legacy.join(".wormhole/workspace"),
        format!("{}\n", real.display()),
    )
    .expect("legacy stamp");
    std::fs::write(legacy.join("history"), "what the agent remembers").expect("kept state");

    let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains(&format!("box: {id} (resumed)")), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(legacy.join("history")).expect("kept state survived"),
        "what the agent remembers"
    );
    // Resumed, so no second home was made beside it.
    assert_eq!(
        std::fs::read_dir(data_home.join("wormhole/homes"))
            .expect("homes dir")
            .count(),
        1
    );
    // And it now carries the full record, so the next start needs no
    // guessing at all.
    let text = std::fs::read_to_string(legacy.join(".wormhole/box.toml")).expect("record written");
    assert_eq!(wormhole_core::home::parse(&text).expect("record").id, id);
}

/// A `wormhole ps --all` that could not see an idle box would make it
/// unreachable however well it was kept.
#[test]
fn an_idle_box_is_listed_so_it_can_be_resumed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(&workspace, &url, &digest, "");
    assert!(build_in(&workspace, &data_home).status.success());
    let _ = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");

    let listed = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
            .arg("ps")
            .args(args)
            .env("XDG_DATA_HOME", &data_home)
            .output()
            .expect("wormhole binary should spawn");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    let id = wormhole_core::paths::box_id(&workspace.canonicalize().expect("real"), 0);
    let all = listed(&["--all"]);
    assert!(all.contains(&id), "{all}");
    assert!(all.contains("idle"), "{all}");
    // Running boxes only, by default: nothing is running here.
    assert!(listed(&[]).contains("no boxes running"), "{}", listed(&[]));
}

/// A role is a manifest plus instructions kept in the user's config; it
/// serves workspaces that carry no `wormhole.toml` of their own.
#[test]
fn a_role_serves_a_workspace_without_a_manifest() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let config_home = temp.path().join("config");
    let role = config_home.join("wormhole/roles/tester");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&role).expect("role dir");
    write_manifest(
        &role,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\ninstructions = \"ROLE.md\"\n",
    );
    std::fs::write(role.join("ROLE.md"), "You test things.\n").expect("role instructions");

    let build = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["build", "--role", "tester"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("wormhole binary should spawn");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let _ = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--role", "tester", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("wormhole binary should spawn");
    let text = std::fs::read_to_string(kept_home(&data_home).join("AGENTS.md"))
        .expect("instructions seeded");
    assert!(text.contains("You test things."), "{text}");
}

/// `--role` is the user speaking; the workspace file is a default. The
/// explicit word wins, or a workspace with its own manifest could never
/// try another role.
#[test]
fn an_explicit_role_beats_the_workspace_manifest() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let config_home = temp.path().join("config");
    let role = config_home.join("wormhole/roles/tester");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&role).expect("role dir");
    write_manifest(
        &workspace,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\ninstructions = \"WS.md\"\n",
    );
    std::fs::write(workspace.join("WS.md"), "From the workspace.\n").expect("ws instructions");
    write_manifest(
        &role,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\ninstructions = \"ROLE.md\"\n",
    );
    std::fs::write(role.join("ROLE.md"), "From the role.\n").expect("role instructions");
    assert!(build_in(&workspace, &data_home).status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--role", "tester", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("wormhole binary should spawn");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("manifest: role tester"),
        "the box must say which manifest it uses:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let text = std::fs::read_to_string(kept_home(&data_home).join("AGENTS.md"))
        .expect("instructions seeded");
    assert!(text.contains("From the role."), "{text}");
    assert!(!text.contains("From the workspace."), "{text}");
}

/// The hook is role data like the instructions: seeded into the kept home
/// where the box can run it, freshly on every start.
#[test]
fn box_seeds_the_preflight_hook_into_the_kept_home() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let config_home = temp.path().join("config");
    let role = config_home.join("wormhole/roles/tester");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(role.join("hooks")).expect("role dir");
    write_manifest(
        &role,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\npreflight = \"hooks/preflight.sh\"\n",
    );
    std::fs::write(role.join("hooks/preflight.sh"), "echo ready\n").expect("hook written");

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wormhole"))
            .args(args)
            .current_dir(&workspace)
            .env("XDG_DATA_HOME", &data_home)
            .env("XDG_CONFIG_HOME", &config_home)
            .output()
            .expect("wormhole binary should spawn")
    };
    assert!(run(&["build", "--role", "tester"]).status.success());
    let _ = run(&["box", "--role", "tester", "--", "/bin/true"]);

    assert_eq!(
        std::fs::read_to_string(kept_home(&data_home).join(".wormhole/preflight"))
            .expect("hook seeded"),
        "echo ready\n"
    );
}

/// A manifest that names a hook it does not carry must stop the launch on
/// the host, not leave the box to die on a file that is not there.
#[test]
fn a_missing_preflight_script_is_refused_before_the_box_starts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_manifest(
        &workspace,
        &url,
        &digest,
        "[agent]\nrun = \"claude\"\npreflight = \"hooks/preflight.sh\"\n",
    );
    assert!(build_in(&workspace, &data_home).status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--", "/bin/true"])
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("wormhole binary should spawn");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("hooks/preflight.sh"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A role argument with a path separator is the role's directory itself —
/// no install into the config dir needed to try one out.
#[test]
fn a_role_can_be_named_by_its_path() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (url, digest) = rootfs_tarball(temp.path());
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let role = temp.path().join("elsewhere/tester");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&role).expect("role dir");
    write_manifest(&role, &url, &digest, "");

    for named_as in [role.display().to_string(), "../elsewhere/tester".to_owned()] {
        let output = Command::new(env!("CARGO_BIN_EXE_wormhole"))
            .args(["build", "--role", &named_as])
            .current_dir(&workspace)
            .env("XDG_DATA_HOME", &data_home)
            .output()
            .expect("wormhole binary should spawn");
        assert!(
            output.status.success(),
            "--role {named_as}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn a_missing_manifest_says_so() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output = build_in(temp.path(), &temp.path().join("data"));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("wormhole.toml"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
