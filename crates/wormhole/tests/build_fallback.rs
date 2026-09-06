//! `cargo install` must work on a toolchain that has no musl std — a
//! plain glibc rustup or a distro rustc. This runs the real compiled
//! build script with a target arch whose musl std cannot exist, and
//! holds it to the fallback promise: build for the outer target with
//! `+crt-static` and still embed a static binary.

use std::path::PathBuf;

/// The build script binary cargo compiled for this package, found next
/// to this test's own target directory.
fn build_script() -> PathBuf {
    let mut dir = std::env::current_exe().expect("test binary path");
    // target/<profile>/deps/<test> -> target/<profile>
    dir.pop();
    dir.pop();
    let build = dir.join("build");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&build).expect("target build dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("wormhole-") {
            continue;
        }
        let script = path.join("build-script-build");
        let Ok(meta) = script.metadata() else {
            continue;
        };
        let time = meta.modified().expect("mtime");
        if newest.as_ref().is_none_or(|(t, _)| *t < time) {
            newest = Some((time, script));
        }
    }
    newest.expect("no compiled build script under target/*/build").1
}

fn host_triple() -> String {
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("run rustc -vV");
    let text = String::from_utf8(out.stdout).expect("utf8");
    text.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .expect("rustc -vV prints host")
        .to_owned()
}

#[test]
fn without_musl_std_the_build_falls_back_to_a_static_host_build() {
    let out_dir = tempfile::tempdir().expect("tempdir");
    // An arch no toolchain has a musl std for, so the fallback path is
    // taken on every machine, including one where musl is the host.
    let output = std::process::Command::new(build_script())
        .env("CARGO_CFG_TARGET_ARCH", "wormholefake")
        .env("TARGET", host_triple())
        .env("OUT_DIR", out_dir.path())
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run build script");
    assert!(
        output.status.success(),
        "the build script must fall back when the musl std is missing:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let binary = stdout
        .lines()
        .find_map(|l| l.strip_prefix("cargo:rustc-env=WORMHOLE_FORWARD_BIN="))
        .expect("the build script must still hand over a forwarder path");
    let bytes = std::fs::read(binary).expect("the forwarder the script points at must exist");
    assert_eq!(
        wormhole_core::broker::requires_loader(&bytes),
        Ok(false),
        "the fallback forwarder must be a static ELF"
    );
}
