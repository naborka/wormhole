//! wormhole asks tmux for nothing: a box runs in the terminal it was
//! started in, a tmux pane or not, and tmux sees it as any other program.
//! Driven against a `tmux` first on `PATH` that records every call.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command};

use wormhole_core::{paths, registry};

mod common;
use common::{keep_box, shown};

const ID: &str = "0123456789ab";

struct Host {
    _temp: tempfile::TempDir,
    data: PathBuf,
    log: PathBuf,
}

impl Host {
    fn new() -> Host {
        let temp = tempfile::tempdir().expect("temp dir");
        let data = temp.path().canonicalize().expect("canonical");
        let tmux = common::a_dir(&data, "bin").join("tmux");
        std::fs::write(&tmux, "#!/bin/sh\necho \"$@\" >> \"$TMUX_LOG\"\n").expect("fake tmux");
        std::fs::set_permissions(&tmux, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Host {
            log: data.join("tmux.log"),
            data,
            _temp: temp,
        }
    }

    /// wormhole outside tmux, finding the recording `tmux` first.
    fn wormhole(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
        let path = std::env::var("PATH").unwrap_or_default();
        command
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CONFIG_HOME", self.data.join("config"))
            .env("TMUX_LOG", &self.log)
            .env(
                "PATH",
                format!("{}:{path}", self.data.join("bin").display()),
            )
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        command
    }

    /// The same, from a pane of a tmux server.
    fn in_tmux(&self) -> Command {
        let mut command = self.wormhole();
        command
            .env("TMUX", "/fake/tmux-1000/default,4242,0")
            .env("TMUX_PANE", "%1");
        command
    }

    /// Every call tmux got, one per line.
    fn asked(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

#[test]
fn a_box_runs_in_the_terminal_it_was_started_in() {
    let host = Host::new();
    let role = common::a_role(&host.data.join("role"), "");
    let mut start = host.wormhole();
    start
        .current_dir(common::a_dir(&host.data, "proj"))
        .args(["box", "--new", "--as", "api", "--role"])
        .arg(&role);

    let (seen, _) = common::drive_terminal(start, "", b"");

    assert_eq!(host.asked(), "");
    // As far as the role's base, which cannot be fetched.
    assert!(shown(&seen).contains("role-recipe"), "went on here: {seen}");
}

/// A new tab, the panel, Enter: the box runs in that tab.
#[test]
fn inside_tmux_the_panel_starts_a_box_in_its_own_tab() {
    let host = Host::new();
    let workspace = common::a_dir(&host.data, "proj");
    keep_box(&host.data, &common::a_record(&workspace, ID, None));

    // Enter asks where, Enter starts it there, where it fails at once for
    // want of a manifest; a key comes back, `q` leaves.
    let seen = shown(&common::on_a_terminal(
        host.in_tmux(),
        "join, or start",
        b"\r\r\rq",
    ));

    assert_eq!(host.asked(), "");
    assert!(
        seen.contains(&format!("wormhole box --id {ID}: exit status")),
        "{seen}"
    );
}

/// A process standing in for a box: alive until the test ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// An older wormhole recorded the pane a box ran in, and `attach` went there.
#[test]
fn attach_never_goes_to_a_pane_an_older_wormhole_recorded() {
    let host = Host::new();
    let init = Running(
        Command::new("sleep")
            .arg("300")
            .spawn()
            .expect("a stand-in init"),
    );
    let pid = init.0.id();
    let box_dir = paths::box_dir(&host.data, pid);
    std::fs::create_dir_all(&box_dir).expect("box dir");
    std::fs::write(box_dir.join("init.pid"), format!("{pid}\n")).expect("init.pid");
    let workspace = common::a_dir(&host.data, "proj");
    let entry = registry::Entry {
        pid,
        box_id: ID.to_owned(),
        key: None,
        workspace: workspace.clone(),
        image: "deadbeef".to_owned(),
        agent: Some("claude".to_owned()),
        name: None,
        alias: Some("api".to_owned()),
        started_unix: 1,
    };
    let older = registry::to_toml(&entry).expect("an entry")
        + "\n[tmux]\nsocket = \"/fake/tmux-1000/default\"\nid = \"%7\"\n";
    std::fs::write(box_dir.join("box.toml"), older).expect("box.toml");
    keep_box(&host.data, &common::a_record(&workspace, ID, Some("api")));

    let _ = host
        .in_tmux()
        .args(["attach", "api"])
        .output()
        .expect("attach runs");

    assert_eq!(host.asked(), "");
}
