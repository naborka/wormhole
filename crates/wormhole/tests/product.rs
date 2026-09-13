//! Which product a start runs when the role offers more than one.
//!
//! `--run` names one. A terminal without it picks. Off a terminal a new
//! box refuses rather than guessing; a kept box resumes the product it
//! already runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use wormhole_core::{paths, source};

mod common;
use common::{a_record, drive_terminal, keep_box, on_a_refused_terminal, said, wormhole};

fn sandbox() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let data = temp.path().join("data");
    let role = temp.path().join("role");
    std::fs::create_dir_all(&workspace).expect("workspace");
    (temp, workspace, data, role)
}

fn write_role(dir: &Path, run: &str) {
    let version = wormhole_core::manifest::VERSION;
    let digest = "a".repeat(64);
    std::fs::create_dir_all(dir).expect("role");
    std::fs::write(
        dir.join("wormhole.toml"),
        format!(
            "version = {version}\n[agent]\nrun = {run}\n\
             [image]\nbase = \"file:///no/such/rootfs.tar.gz\"\nbase_sha256 = \"{digest}\"\n"
        ),
    )
    .expect("manifest");
}

fn box_cmd(data: &Path, cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command
        .args(args)
        .current_dir(cwd)
        .env("XDG_DATA_HOME", data);
    command
}

/// Several products and no `--run` is a question. Off a terminal nobody
/// can answer, so a new box refuses rather than opening a claude home
/// because that name came first.
#[test]
fn a_new_box_off_a_terminal_refuses_to_guess_the_product() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "[\"claude\", \"grok\"]");
    let role = role.canonicalize().expect("canonical");

    let output = wormhole(
        &data,
        &workspace,
        &["box", "--role", role.to_str().expect("utf-8"), "--new"],
    );
    assert!(!output.status.success(), "{}", said(&output));
    let said = said(&output);
    assert!(
        said.contains("this role runs claude, grok; pass --run claude|grok"),
        "{said}"
    );
    assert!(
        !data.join("wormhole/homes").exists(),
        "a home survived a refusal"
    );
}

/// `--run` is the answer a script gives, so a start that names the
/// product is not a question.
#[test]
fn naming_the_product_skips_the_question() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "[\"claude\", \"grok\"]");
    let role = role.canonicalize().expect("canonical");

    let output = wormhole(
        &data,
        &workspace,
        &[
            "box",
            "--role",
            role.to_str().expect("utf-8"),
            "--run",
            "grok",
            "--new",
        ],
    );
    let said = said(&output);
    assert!(!said.contains("pass --run"), "{said}");
    assert!(said.contains("(new)"), "{said}");
}

/// One product is not a choice. A start that would have asked for a list
/// of one must not refuse as if the list were several.
#[test]
fn a_single_product_is_not_a_question() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "\"claude\"");
    let role = role.canonicalize().expect("canonical");

    let output = wormhole(
        &data,
        &workspace,
        &["box", "--role", role.to_str().expect("utf-8"), "--new"],
    );
    let said = said(&output);
    assert!(!said.contains("pass --run"), "{said}");
    assert!(said.contains("(new)"), "{said}");
}

/// A kept grok box is still that box when the next start names no
/// product. Guessing claude would make a second home and call it
/// continuity.
#[test]
fn a_bare_start_resumes_the_product_the_box_already_runs() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "[\"claude\", \"grok\"]");
    let role = role.canonicalize().expect("canonical");
    let workspace = workspace.canonicalize().expect("canonical");
    let id = paths::box_id(&workspace, 0);
    let mut record = a_record(&workspace, &id, None);
    record.agent = Some("grok".to_owned());
    record.role = Some(role.display().to_string());
    record.source = Some(source::dir_source(&role));
    keep_box(&data, &record);

    let output = wormhole(
        &data,
        &workspace,
        &[
            "box",
            "--role",
            role.to_str().expect("utf-8"),
            "--",
            "/bin/true",
        ],
    );
    let said = said(&output);
    assert!(said.contains(&format!("box: {id} (resumed)")), "{said}");
    assert!(!said.contains("pass --run"), "{said}");
    assert!(!said.contains("run as:"), "{said}");
    let kept = common::kept_record(
        &data,
        &paths::home_dir(&data, &paths::box_key(&workspace, &id)),
    );
    assert_eq!(kept.agent.as_deref(), Some("grok"));
}

/// The same list the panel's `n` already shows. `q` backs out, and the
/// refusal names `--run` so the next start can skip the question.
#[test]
fn a_terminal_picks_the_product_and_q_backs_out() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "[\"claude\", \"grok\"]");
    let role = role.canonicalize().expect("canonical");

    let seen = on_a_refused_terminal(
        box_cmd(
            &data,
            &workspace,
            &["box", "--role", role.to_str().expect("utf-8"), "--new"],
        ),
        "run as:",
        b"q",
    );
    assert!(seen.contains("claude"), "{seen}");
    assert!(seen.contains("grok"), "{seen}");
    assert!(seen.contains("pass --run"), "{seen}");
    assert!(
        !data.join("wormhole/homes").exists(),
        "a home survived a cancelled pick"
    );
}

/// Down then enter is grok on a list that starts with claude. The pick
/// is what this start binds; a fetch that then fails is later.
#[test]
fn a_terminal_start_binds_the_product_that_was_picked() {
    let (_temp, workspace, data, role) = sandbox();
    write_role(&role, "[\"claude\", \"grok\"]");
    let role = role.canonicalize().expect("canonical");

    let (seen, _status) = drive_terminal(
        box_cmd(
            &data,
            &workspace,
            &["box", "--role", role.to_str().expect("utf-8"), "--new"],
        ),
        "run as:",
        b"j\r",
    );
    assert!(seen.contains("> grok"), "{seen}");
    assert!(seen.contains("(new)"), "{seen}");
    assert!(!seen.contains("pass --run"), "{seen}");
}
