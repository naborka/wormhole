//! `wormhole remove`, `reset` and `rename` against a synthetic store, and what
//! `gc` can now prove about an image. No namespaces involved — every one of
//! these only reads and writes the host's own data home.

use std::path::{Path, PathBuf};

use wormhole_core::{home, manifest, paths, registry};

mod common;
use common::{a_record as record, keep_box as keep, said, wormhole};

/// A workspace that exists, because `gc` asks whether a box's tree is
/// still there before it reads the recipe in it.
fn workspace(under: &Path, name: &str) -> PathBuf {
    let dir = under.join(name);
    std::fs::create_dir_all(&dir).expect("workspace");
    dir.canonicalize().expect("canonical workspace")
}

/// The id a workspace's first box gets, so a test names boxes the way
/// wormhole does rather than inventing hex.
fn first_id(workspace: &Path) -> String {
    paths::box_id(workspace, 0)
}

#[test]
fn remove_takes_the_home_and_everything_in_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, None));
    let snapshot = paths::snapshot_dir(data, &paths::box_key(&ws, &id));
    std::fs::create_dir_all(&snapshot).expect("snapshot");

    let output = wormhole(data, &ws, &["remove", &id]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(said(&output).contains(&format!("box {id} removed")));
    assert!(!home.exists(), "the home survived");
    assert!(!snapshot.exists(), "the snapshot survived");
}

/// The name is what a person types, so it is what `remove` has to take.
#[test]
fn remove_takes_the_name_you_gave_a_box() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, Some("api")));

    let output = wormhole(data, &ws, &["remove", "api"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(!home.exists(), "the home survived");
}

#[test]
fn remove_takes_several_boxes_at_once() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let homes: Vec<PathBuf> = (0..2)
        .map(|n| keep(data, &record(&ws, &paths::box_id(&ws, n), None)))
        .collect();
    let ids: Vec<String> = (0..2).map(|n| paths::box_id(&ws, n)).collect();

    let output = wormhole(data, &ws, &["remove", &ids[0], &ids[1]]);
    assert!(output.status.success(), "{}", said(&output));
    for home in &homes {
        assert!(!home.exists(), "{} survived", home.display());
    }
}

/// Half a list removed and the rest refused over a typo at the end is the
/// worst outcome this command can have. Every name is resolved first.
#[test]
fn remove_names_every_box_before_it_removes_any() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, None));

    let output = wormhole(data, &ws, &["remove", &id, "nosuchbox"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("nosuchbox"), "{}", said(&output));
    assert!(home.exists(), "a box went while the command was refusing");
}

/// The claim is the only honest answer to "is this box running", and it
/// is the kernel's, so the test takes a real one.
#[test]
fn remove_refuses_a_running_box_and_says_what_to_do_first() {
    use nix::fcntl::{Flock, FlockArg};

    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, None));

    let lock = paths::lock_file(data, &paths::box_key(&ws, &id));
    std::fs::create_dir_all(lock.parent().expect("locks dir")).expect("locks dir");
    let file = std::fs::File::create(&lock).expect("lock file");
    let _held = Flock::lock(file, FlockArg::LockExclusiveNonblock).expect("the claim");

    let output = wormhole(data, &ws, &["remove", &id]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("is running"), "{}", said(&output));
    assert!(
        said(&output).contains(&format!("wormhole stop {id}")),
        "{}",
        said(&output)
    );
    assert!(home.exists(), "a running box was removed");
}

/// The gap this closes: a home whose record cannot be read is listed as a
/// problem, never as a box — so it could be started by `--id`, which names
/// a box by its directory, and never removed by anything. A box you can
/// see and cannot get rid of is the one state this store must not have.
#[test]
fn remove_takes_a_box_whose_record_cannot_be_read() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = paths::home_dir(data, &paths::box_key(&ws, &id));
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("history.jsonl"), "orphaned\n").expect("history");

    let output = wormhole(data, &ws, &["remove", &id]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(!home.exists(), "the unreadable home survived");
}

/// A name lives in the record, so a box with none has nothing to rename —
/// and the refusal says which command does work on it.
#[test]
fn rename_refuses_a_box_that_has_no_record_and_says_what_does() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    std::fs::create_dir_all(paths::home_dir(data, &paths::box_key(&ws, &id))).expect("home");

    let output = wormhole(data, &ws, &["rename", &id, "api"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(
        said(&output).contains("no readable record"),
        "{}",
        said(&output)
    );
    assert!(
        said(&output).contains("wormhole remove"),
        "{}",
        said(&output)
    );
}

#[test]
fn reset_empties_the_home_and_keeps_the_box() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, Some("api")));

    let output = wormhole(data, &ws, &["reset", "api"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(!home.join("history.jsonl").exists(), "history survived");
    let kept = home::parse(
        &std::fs::read_to_string(home.join(home::RECORD)).expect("the record came back"),
    )
    .expect("valid record");
    assert_eq!(kept.id, id);
    assert_eq!(kept.alias.as_deref(), Some("api"));
    assert_eq!(kept.workspace, ws);
}

#[test]
fn rename_sets_the_name_without_starting_the_box() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let home = keep(data, &record(&ws, &id, None));

    let output = wormhole(data, &ws, &["rename", &id, "api"]);
    assert!(output.status.success(), "{}", said(&output));
    let kept = home::parse(&std::fs::read_to_string(home.join(home::RECORD)).expect("record"))
        .expect("valid record");
    assert_eq!(kept.alias.as_deref(), Some("api"));
    // And the name now names it everywhere else.
    let listed = wormhole(data, &ws, &["ps", "--all"]);
    assert!(said(&listed).contains("api"), "{}", said(&listed));
}

#[test]
fn rename_refuses_a_name_another_box_here_already_answers_to() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    keep(data, &record(&ws, &paths::box_id(&ws, 0), Some("api")));
    let second = paths::box_id(&ws, 1);
    keep(data, &record(&ws, &second, None));

    let output = wormhole(data, &ws, &["rename", &second, "api"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("already names"), "{}", said(&output));
}

/// One recipe, one image directory named by its digest — the shape `gc`
/// has to reason about.
fn recipe(workspace: &Path) -> String {
    let text = format!(
        "version = {}\n[image]\nbase = \"https://x.test/r.tar.gz\"\nbase_sha256 = \"{}\"\n",
        manifest::VERSION,
        "a".repeat(64)
    );
    std::fs::write(workspace.join("wormhole.toml"), &text).expect("manifest");
    manifest::recipe_digest(&manifest::parse(&text).expect("valid"))
}

fn put_image(data_home: &Path, digest: &str) -> PathBuf {
    let dir = paths::image_dir(data_home, digest);
    std::fs::create_dir_all(&dir).expect("image dir");
    std::fs::write(dir.join("marker"), "a gigabyte, in spirit").expect("image file");
    dir
}

/// The gap this closes: every version bump left an image behind and
/// nothing could ever prove it was safe to take.
#[test]
fn gc_proves_an_image_no_box_starts_from_is_unreferenced() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let used = recipe(&ws);
    keep(data, &record(&ws, &first_id(&ws), None));
    let live = put_image(data, &used);
    let stale = put_image(data, &"b".repeat(64));

    let looked = wormhole(data, &ws, &["gc"]);
    assert!(looked.status.success(), "{}", said(&looked));
    assert!(said(&looked).contains("unreferenced"), "{}", said(&looked));
    assert!(
        said(&looked).contains("--unreferenced"),
        "the report never offered the flag:\n{}",
        said(&looked)
    );

    // A bare `--delete` is the smaller, certain claim: it leaves this.
    let deleted = wormhole(data, &ws, &["gc", "--delete"]);
    assert!(deleted.status.success(), "{}", said(&deleted));
    assert!(stale.exists(), "a bare --delete took an unreferenced image");

    let widened = wormhole(data, &ws, &["gc", "--delete", "--unreferenced"]);
    assert!(widened.status.success(), "{}", said(&widened));
    assert!(!stale.exists(), "--unreferenced left the stale image");
    assert!(live.exists(), "the image a kept box starts from was taken");
}

/// One unreadable recipe may be the very one that references an image, so
/// an incomplete answer proves nothing — and says which it is.
#[test]
fn gc_proves_nothing_while_a_box_recipe_cannot_be_read() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    // A workspace that is still there and holds no manifest at all.
    keep(data, &record(&ws, &first_id(&ws), None));
    let image = put_image(data, &"b".repeat(64));

    let output = wormhole(data, &ws, &["gc", "--delete", "--unreferenced"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("unproven"), "{}", said(&output));
    assert!(image.exists(), "an image went on a gap in the evidence");
}

/// A running box is copied from its image, and under `rootfs = "readonly"`
/// it *is* the image. Nothing may take it while the box is in flight.
#[test]
fn gc_keeps_the_image_a_running_box_is_using() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    recipe(&ws);
    let digest = "b".repeat(64);
    let image = put_image(data, &digest);

    let pid = std::process::id();
    let dir = paths::box_dir(data, pid);
    std::fs::create_dir_all(&dir).expect("box dir");
    let entry = registry::Entry {
        pid,
        box_id: first_id(&ws),
        workspace: ws.clone(),
        image: image.display().to_string(),
        agent: Some("claude".to_owned()),
        name: None,
        alias: None,
        started_unix: 1,
    };
    std::fs::write(
        dir.join("box.toml"),
        registry::to_toml(&entry).expect("toml"),
    )
    .expect("entry");

    let output = wormhole(data, &ws, &["gc", "--delete", "--unreferenced"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(image.exists(), "the image a running box uses was taken");
}

/// `rm` leaves the lock file behind on purpose. `gc` is what reclaims it,
/// and only once the box it claims is provably gone.
#[test]
fn gc_reclaims_a_lock_whose_box_is_gone() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let ws = workspace(data, "proj");
    let id = first_id(&ws);
    let orphan = paths::lock_file(data, &paths::box_key(&ws, &id));
    std::fs::create_dir_all(orphan.parent().expect("locks dir")).expect("locks dir");
    std::fs::write(&orphan, "").expect("lock");

    let output = wormhole(data, &ws, &["gc", "--delete"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(!orphan.exists(), "the orphaned lock survived");
}
