//! `wormhole role add` end to end, against a git repository on this disk.
//!
//! The remote is a real repository built here, so these tests need `git`
//! and no network at all — and they exercise the same code path a
//! `https://` URL would, because `git fetch` does not care which it is
//! handed.

use std::path::PathBuf;
use std::process::{Command, Output};

use wormhole_core::{paths, source};

mod common;
use common::on_a_terminal;

/// One test's whole world: its own data home, config home and workspace,
/// and the temporary directory that owns all three. Held so the directory
/// outlives the test — dropping it deletes everything under it.
struct Sandbox {
    temp: tempfile::TempDir,
    data: PathBuf,
    config: PathBuf,
    workspace: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        Sandbox {
            data: temp.path().join("data"),
            config: temp.path().join("config"),
            workspace,
            temp,
        }
    }

    fn wormhole(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("wormhole should spawn")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
        command
            .args(args)
            .current_dir(&self.workspace)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CONFIG_HOME", &self.config);
        command
    }

    /// A git repository holding the given files, and the commit that is
    /// its pin.
    fn repo(&self, files: &[(&str, &str)]) -> (String, String) {
        let repo = self.temp.path().join("remote");
        std::fs::create_dir_all(&repo).expect("repo dir");
        for (name, content) in files {
            std::fs::write(repo.join(name), content).expect("repo file");
        }
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "--quiet"]);
        // A repository a fetch may take an arbitrary commit out of, which
        // is what wormhole asks for and what GitHub allows.
        git(&["config", "uploadpack.allowAnySHA1InWant", "true"]);
        git(&["add", "-A"]);
        git(&["commit", "--quiet", "-m", "the role"]);
        (repo.display().to_string(), git(&["rev-parse", "HEAD"]))
    }

    /// A repository holding one role — the usual case.
    fn role_repo(&self, extra: &str) -> (String, String) {
        self.repo(&[
            ("wormhole.toml", &role_manifest(extra)),
            ("ROLE.md", "You are the fetched role.\n"),
        ])
    }

    /// A pin that is fetched and installed under a name, but not
    /// approved. `role add` needs a terminal to ask on, so the tests that
    /// are not about the asking arrange this state directly — it is also
    /// exactly the state a launch must refuse.
    fn fetch(&self, url: &str, sha: &str, name: &str) {
        let checkout = paths::checkout_dir(&self.data, sha);
        // One commit, one checkout — a second name for it fetches nothing.
        if !checkout.is_dir() {
            std::fs::create_dir_all(&checkout).expect("checkout dir");
            let out = Command::new("git")
                .args(["clone", "--quiet", url, "."])
                .current_dir(&checkout)
                .output()
                .expect("git clone");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::fs::remove_dir_all(checkout.join(".git")).expect("bookkeeping removed");
        }
        self.pointer(name, url, sha);
    }

    /// The pointer file `role add` would have written, in the shape it
    /// writes it, so a test can never install a stale one.
    fn pointer(&self, name: &str, url: &str, sha: &str) {
        let role = paths::role_dir(&self.config, name);
        std::fs::create_dir_all(&role).expect("role dir");
        let text = source::to_toml(&source::Source {
            url: url.to_owned(),
            sha: sha.to_owned(),
        })
        .expect("a pointer");
        std::fs::write(role.join(source::POINTER), text).expect("pointer written");
    }

    fn approve(&self, sha: &str) {
        std::fs::write(
            paths::approval_file(&self.data, sha),
            "approved by the test",
        )
        .expect("approval written");
    }

    fn checkout(&self, sha: &str) -> PathBuf {
        paths::checkout_dir(&self.data, sha)
    }
}

/// A manifest whose fetch could never work, so nothing in these tests
/// reaches a network even if a build were reached.
///
/// `extra` is appended, so it continues `[image]` — more image keys — or
/// opens a table of its own. Prepending it would make any image key in it
/// a second `[image]`, which TOML refuses.
fn role_manifest(extra: &str) -> String {
    let version = wormhole_core::manifest::VERSION;
    format!(
        "version = {version}\n[image]\n\
         base = \"file:///nothing/here.tar.gz\"\n\
         base_sha256 = \"{}\"\n{extra}",
        "0".repeat(64)
    )
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The whole point: a role that lives somewhere else starts here, and the
/// commit is what it starts from.
#[test]
fn a_role_fetched_from_a_repository_builds_like_any_other() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");
    box_.fetch(&url, &sha, "fetched");
    box_.approve(&sha);

    let output = box_.wormhole(&["build", "--role", "fetched"]);
    // The fetch of the *rootfs* cannot work, which is what proves the role
    // itself resolved: nothing reaches a rootfs fetch without a manifest.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&sha[..12]), "{stdout}");
    assert!(
        stderr(&output).contains("cannot fetch"),
        "{}",
        stderr(&output)
    );
}

/// A launch fetches nothing and asks nobody. A pointer whose checkout this
/// host does not hold is a refusal naming the command that fixes it —
/// never a silent re-fetch of bytes whose approval is equally gone.
#[test]
fn a_pin_with_no_checkout_refuses_and_names_the_command() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");
    box_.pointer("fetched", &url, &sha);

    let output = box_.wormhole(&["build", "--role", "fetched"]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = stderr(&output);
    assert!(refusal.contains("has not fetched"), "{refusal}");
    assert!(refusal.contains("wormhole role add"), "{refusal}");
}

/// Fetched is not approved. Somebody has to read what a stranger's role
/// runs, and until they have, it does not run.
#[test]
fn a_fetched_but_unapproved_pin_refuses_before_anything_is_built() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");
    box_.fetch(&url, &sha, "fetched");

    let output = box_.wormhole(&["build", "--role", "fetched"]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = stderr(&output);
    assert!(refusal.contains("not approved"), "{refusal}");
    // Nothing was built, and nothing reached a fetch of the rootfs.
    assert!(!box_.data.join("wormhole/images").exists(), "{refusal}");
}

/// A bare remote `--role` with no terminal to ask on is refused, not
/// answered by nobody. This is what keeps a scripted or scheduled start
/// from running a stranger's build shell unattended.
#[test]
fn a_bare_remote_ref_without_a_terminal_is_refused() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");

    let output = box_.wormhole(&["build", "--role", &format!("file://{url}@{sha}")]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = stderr(&output);
    assert!(refusal.contains("wormhole role add"), "{refusal}");
    assert!(!box_.data.join("wormhole/checkouts").exists(), "{refusal}");
}

/// A ref that names a branch instead of a commit is refused where it is
/// typed. Nothing fetches, so nothing can have floated.
#[test]
fn an_unpinned_ref_is_refused_before_anything_is_fetched() {
    let box_ = Sandbox::new();
    for reference in ["github:you/role", "github:you/role@main"] {
        let output = box_.wormhole(&["role", "add", reference]);
        assert!(!output.status.success(), "{reference}: {output:?}");
        let refusal = stderr(&output);
        assert!(
            refusal.contains("names no commit"),
            "{reference}: {refusal}"
        );
    }
    assert!(!box_.data.join("wormhole/checkouts").exists());
}

/// A repository that carries no manifest at its root is not a role. Said
/// at the fetch, where the reason is still known, and the half-made
/// checkout is not left behind for a later launch to trip over.
#[test]
fn a_repository_without_a_manifest_is_refused_and_leaves_no_checkout() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.repo(&[("README.md", "not a role\n")]);

    let output = box_.wormhole(&["role", "add", &format!("{url}@{sha}")]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("is not a role"),
        "{}",
        stderr(&output)
    );
    assert!(
        !box_.checkout(&sha).exists(),
        "a checkout survived a refusal"
    );
}

/// `role add` onto a name a person wrote by hand would replace their own
/// role with somebody else's under the name they trust. Refused, with the
/// directory named.
#[test]
fn adding_over_a_hand_written_role_is_refused() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");
    let mine = paths::role_dir(&box_.config, "mine");
    std::fs::create_dir_all(&mine).expect("role dir");
    std::fs::write(mine.join("wormhole.toml"), role_manifest("")).expect("my manifest");

    let output = box_.wormhole(&["role", "add", &format!("{url}@{sha}"), "--as", "mine"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("a directory you wrote"),
        "{}",
        stderr(&output)
    );
    assert!(std::fs::read_to_string(mine.join("wormhole.toml")).is_ok());
}

/// A name under `roles/` that is neither a recipe nor a pointer is not a
/// role. Said with the name still in hand, rather than surfacing later as
/// a missing file under a path the user never typed.
#[test]
fn a_role_name_that_is_not_a_role_is_refused_by_name() {
    let box_ = Sandbox::new();
    std::fs::create_dir_all(paths::role_dir(&box_.config, "hollow")).expect("role dir");

    let output = box_.wormhole(&["build", "--role", "hollow"]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = stderr(&output);
    assert!(refusal.contains("no role hollow"), "{refusal}");
    assert!(refusal.contains("wormhole role add"), "{refusal}");
}

/// Two roles pinned to one commit are one fetch and one approval: the pin
/// names the bytes, and the bytes are what was read and agreed to.
#[test]
fn two_names_for_one_commit_share_its_checkout_and_its_approval() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("[agent]\nrun = \"claude\"\n");
    box_.fetch(&url, &sha, "first");
    box_.fetch(&url, &sha, "second");
    box_.approve(&sha);

    for name in ["first", "second"] {
        let output = box_.wormhole(&["build", "--role", name]);
        assert!(
            stderr(&output).contains("cannot fetch"),
            "{name}: {}",
            stderr(&output)
        );
    }
    let fetched: Vec<_> = std::fs::read_dir(box_.data.join("wormhole/checkouts"))
        .expect("checkouts")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(fetched.len(), 1, "{fetched:?}");
}

/// A pointer someone hand-edited to name a branch would float a role
/// whose whole promise is that it does not. Refused on the way in.
#[test]
fn a_pointer_edited_to_float_is_refused() {
    let box_ = Sandbox::new();
    let role = paths::role_dir(&box_.config, "floating");
    std::fs::create_dir_all(&role).expect("role dir");
    std::fs::write(
        role.join(source::POINTER),
        "version = 1\nurl = \"https://x.test/r\"\nsha = \"main\"\n",
    )
    .expect("pointer written");

    let output = box_.wormhole(&["build", "--role", "floating"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("names no commit"),
        "{}",
        stderr(&output)
    );
}

/// The approval, said yes to on a real terminal.
///
/// Every other test here proves a refusal. This one proves the way
/// through: the screen a person reads carries the shell that would run on
/// their machine, `enter` is what keeps it, and afterwards the role is
/// installed and starts without asking again.
#[test]
fn approving_on_a_terminal_installs_the_role_and_never_asks_twice() {
    let box_ = Sandbox::new();
    // Carries something that executes, so the part that matters is on the
    // screen this test reads.
    let (url, sha) = box_.role_repo(
        "packages = [\"ripgrep\"]\nbuild = [\"echo built-by-a-stranger\"]\n\
         [agent]\nrun = \"claude\"\n",
    );

    let seen = on_a_terminal(
        box_.command(&[
            "role",
            "add",
            &format!("file://{url}@{sha}"),
            "--as",
            "smoke",
        ]),
        "enter start",
        b"\r",
    );

    // The screen carried what would execute, not just what would be
    // mounted. That is the whole reason it is shown.
    assert!(seen.contains("echo built-by-a-stranger"), "{seen}");
    assert!(seen.contains("ripgrep"), "{seen}");

    // The pointer is in config, small and readable; the commit is in data.
    let pointer =
        std::fs::read_to_string(paths::role_dir(&box_.config, "smoke").join(source::POINTER))
            .expect("pointer written");
    assert!(pointer.contains(&sha), "{pointer}");
    assert!(pointer.contains("version = 1"), "{pointer}");
    assert!(
        box_.checkout(&sha).join("wormhole.toml").is_file(),
        "the commit was not kept"
    );
    // `.git` is the fetch's bookkeeping, not the role.
    assert!(!box_.checkout(&sha).join(".git").exists());

    // And now it starts with no terminal, no network and no question.
    let output = box_.wormhole(&["build", "--role", "smoke"]);
    let refusal = stderr(&output);
    assert!(refusal.contains("cannot fetch"), "{refusal}");
    assert!(!refusal.contains("not approved"), "{refusal}");
}

// ---------------------------------------------------------------------
// Roles that live in a directory on this machine, and the verbs that
// manage every installed role whatever it was installed from.
// ---------------------------------------------------------------------

impl Sandbox {
    /// A role directory beside the workspace, with a manifest in it.
    fn role_dir(&self, name: &str, extra: &str) -> PathBuf {
        let dir = self.temp.path().join(name);
        std::fs::create_dir_all(&dir).expect("role dir");
        std::fs::write(dir.join("wormhole.toml"), role_manifest(extra)).expect("role manifest");
        dir
    }

    fn installed(&self, name: &str) -> PathBuf {
        paths::role_dir(&self.config, name)
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Naming a role you wrote used to mean `mkdir -p` and `ln -s` into
/// wormhole's own config directory, because `role add` parsed its
/// argument as a repository before anything else and asked a directory
/// for a commit.
#[test]
fn a_local_role_is_installed_by_its_directory() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "[agent]\nrun = \"claude\"\n");

    let added = box_.wormhole(&["role", "add", dir.to_str().expect("utf-8")]);
    assert!(added.status.success(), "{}", stderr(&added));

    let listed = box_.wormhole(&["role", "list"]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(stdout(&listed).contains("alphaca"), "{}", stdout(&listed));
}

/// The directory stays the role. Installing it must not take a copy, or
/// the name would quietly go on running last week's recipe.
#[test]
fn an_installed_local_role_still_follows_its_directory() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "[agent]\nrun = \"claude\"\n");
    assert!(
        box_.wormhole(&["role", "add", dir.to_str().expect("utf-8")])
            .status
            .success()
    );

    std::fs::write(
        dir.join("wormhole.toml"),
        role_manifest("[access]\ngrants = [\"~/.ssh\"]\n"),
    )
    .expect("edit the role");

    let shown = box_.wormhole(&["role", "show", "alphaca"]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    assert!(stdout(&shown).contains(".ssh"), "{}", stdout(&shown));
}

/// `--as` names it something else, and the same rule about what may name
/// a role applies however the role was found.
#[test]
fn a_local_role_can_be_named_and_a_name_that_cannot_be_typed_is_refused() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "");
    let path = dir.to_str().expect("utf-8");

    assert!(
        box_.wormhole(&["role", "add", path, "--as", "java"])
            .status
            .success()
    );
    assert!(box_.installed("java").exists());

    let refused = box_.wormhole(&["role", "add", path, "--as", "bad.name"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("cannot name a role"),
        "{}",
        stderr(&refused)
    );
}

/// A directory with no manifest in it is not a role, and saying so at the
/// install is the last moment the name the user typed is still known.
#[test]
fn a_directory_without_a_manifest_is_not_installable() {
    let box_ = Sandbox::new();
    let empty = box_.temp.path().join("empty");
    std::fs::create_dir_all(&empty).expect("dir");

    let refused = box_.wormhole(&["role", "add", empty.to_str().expect("utf-8")]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("wormhole.toml"),
        "{}",
        stderr(&refused)
    );
}

/// A bare word is never read as a path, so a role of that name in the
/// current folder is the likeliest thing meant — and used to be the one
/// thing the refusal never mentioned.
#[test]
fn a_bare_name_that_is_a_directory_here_says_so() {
    let box_ = Sandbox::new();
    let here = box_.workspace.join("alphaca");
    std::fs::create_dir_all(&here).expect("dir");
    std::fs::write(here.join("wormhole.toml"), role_manifest("")).expect("manifest");

    let refused = box_.wormhole(&["build", "--role", "alphaca"]);
    assert!(!refused.status.success());
    let said = stderr(&refused);
    assert!(said.contains("./alphaca"), "{said}");
    assert!(said.contains("role add"), "{said}");
}

/// One rule about what may name a role, checked by the writer and by the
/// reader. A directory placed here by hand under a name `role add` would
/// have refused is not a role wormhole will offer.
#[test]
fn a_role_named_something_unusable_is_not_listed() {
    let box_ = Sandbox::new();
    let roles = paths::roles_dir(&box_.config);
    std::fs::create_dir_all(roles.join("bad.name")).expect("roles dir");
    std::fs::write(roles.join("bad.name/wormhole.toml"), role_manifest("")).expect("manifest");

    let listed = box_.wormhole(&["role", "list"]);
    assert!(!stdout(&listed).contains("bad.name"), "{}", stdout(&listed));
}

/// `role list` is the only way to see what is installed without opening
/// the panel, and it says where each one points and whether it can start.
#[test]
fn role_list_says_where_each_role_points_and_whether_it_can_start() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "");
    box_.wormhole(&["role", "add", dir.to_str().expect("utf-8")]);
    let (url, sha) = box_.role_repo("");
    box_.pointer("java", &url, &sha);

    let listed = box_.wormhole(&["role", "list"]);
    let said = stdout(&listed);
    assert!(said.contains("alphaca"), "{said}");
    assert!(said.contains(dir.to_str().expect("utf-8")), "{said}");
    assert!(said.contains("java"), "{said}");
    assert!(said.contains(&url), "{said}");
    // Pinned, never fetched here: listed, and honest about not being ready.
    assert!(said.contains("role add"), "{said}");
}

/// Nothing installed is a sentence, not an empty screen.
#[test]
fn role_list_with_nothing_installed_says_so() {
    let box_ = Sandbox::new();
    let listed = box_.wormhole(&["role", "list"]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(stdout(&listed).contains("no roles"), "{}", stdout(&listed));
}

/// Removing a role takes the name back. It must never follow the link
/// into the directory the role is actually kept in.
#[test]
fn role_remove_takes_the_name_back_and_leaves_the_directory_alone() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "");
    box_.wormhole(&["role", "add", dir.to_str().expect("utf-8")]);

    let removed = box_.wormhole(&["role", "remove", "alphaca"]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(!box_.installed("alphaca").exists());
    assert!(
        dir.join("wormhole.toml").is_file(),
        "removing the name deleted the role"
    );
}

#[test]
fn removing_a_role_that_is_not_installed_says_so() {
    let box_ = Sandbox::new();
    let refused = box_.wormhole(&["role", "remove", "nothing"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("nothing"), "{}", stderr(&refused));
}

/// `role show` puts up the screen `role add` asks with, without
/// installing anything — a dry run for `--role`, and the only way to see
/// an approval screen again after the fact.
#[test]
fn role_show_previews_a_directory_without_installing_it() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "[access]\ngrants = [\"~/.ssh\"]\n");

    let shown = box_.wormhole(&["role", "show", dir.to_str().expect("utf-8")]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    assert!(stdout(&shown).contains(".ssh"), "{}", stdout(&shown));
    assert!(!box_.installed("alphaca").exists(), "show installed it");
}

/// The biggest thing a workspace could not say: "this project uses that
/// role". Before this, a project either wrote out a whole recipe or
/// everyone who cloned it typed `--role X` by hand, every time, forever.
#[test]
fn a_workspace_manifest_can_hand_its_box_to_a_role() {
    let box_ = Sandbox::new();
    let dir = box_.role_dir("alphaca", "[agent]\nrun = \"claude\"\n");
    std::fs::write(
        box_.workspace.join("wormhole.toml"),
        format!("version = 1\nrole = \"{}\"\n", dir.display()),
    )
    .expect("a workspace manifest naming a role");

    // No `--role` anywhere: the file is what picks the recipe.
    let output = box_.wormhole(&["build"]);
    let said = stdout(&output);
    assert!(said.contains("alphaca"), "{said}");
    // The role's own recipe is what a build then reaches for.
    assert!(
        stderr(&output).contains("cannot fetch"),
        "{}",
        stderr(&output)
    );
}

/// The flag is the user speaking and the file is a default, so `--role`
/// still wins over what the workspace names.
#[test]
fn an_explicit_role_beats_the_one_the_workspace_names() {
    let box_ = Sandbox::new();
    let named = box_.role_dir("named", "");
    let typed = box_.role_dir("typed", "");
    std::fs::write(
        box_.workspace.join("wormhole.toml"),
        format!("version = 1\nrole = \"{}\"\n", named.display()),
    )
    .expect("a workspace manifest naming a role");

    let said = stdout(&box_.wormhole(&["build", "--role", typed.to_str().expect("utf-8")]));
    assert!(said.contains("typed"), "{said}");
    assert!(!said.contains("named"), "{said}");
}

/// Not inheritance. A manifest that names a role and carries a recipe is
/// refused rather than merged — merging is where a key like this becomes
/// an override system nobody can predict.
#[test]
fn a_workspace_that_names_a_role_and_carries_a_recipe_is_refused() {
    let box_ = Sandbox::new();
    std::fs::write(
        box_.workspace.join("wormhole.toml"),
        format!(
            "version = {}\nrole = \"alphaca\"\n[image]\n\
             base = \"file:///nothing/here.tar.gz\"\nbase_sha256 = \"{}\"\n",
            wormhole_core::manifest::VERSION,
            "0".repeat(64)
        ),
    )
    .expect("a manifest that does both");

    let refused = box_.wormhole(&["build"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("carries no recipe of its own"),
        "{}",
        stderr(&refused)
    );
}

/// A launch still fetches nothing and asks nobody. A fresh clone whose
/// manifest names a pin this host has not approved refuses and names the
/// command, exactly as typing that ref at `--role` does.
#[test]
fn a_workspace_naming_an_unfetched_pin_refuses_and_names_the_command() {
    let box_ = Sandbox::new();
    let (url, sha) = box_.role_repo("");
    std::fs::write(
        box_.workspace.join("wormhole.toml"),
        format!("version = 1\nrole = \"{url}@{sha}\"\n"),
    )
    .expect("a workspace manifest naming a pin");

    let refused = box_.wormhole(&["build"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("wormhole role add"),
        "{}",
        stderr(&refused)
    );
}
