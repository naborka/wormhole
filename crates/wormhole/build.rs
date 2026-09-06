//! Builds the in-box forwarder for the musl target and hands its path to
//! `include_bytes!`. Musl because the helper runs inside whatever image a
//! box uses: static (no `PT_INTERP`) is the only linkage that runs under
//! any libc, and baking it in is what keeps the §12 "nothing to install"
//! promise without caring how wormhole itself was linked.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("cargo sets CARGO_CFG_TARGET_ARCH");
    let triple = format!("{arch}-unknown-linux-musl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"))
        .join("../wormhole-forward/Cargo.toml");
    // Its own target dir: cargo locks the one it builds into, and reusing
    // the outer build's would deadlock this script against its caller.
    let target_dir = out_dir.join("forward-target");
    println!("cargo:rerun-if-changed=../wormhole-forward/src/main.rs");
    println!("cargo:rerun-if-changed=../wormhole-forward/Cargo.toml");
    let output = Command::new(env::var("CARGO").expect("cargo sets CARGO"))
        .args(["build", "--release", "--target", &triple, "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        // The host's flags are for the host's target; a `-C target-cpu`
        // or linker choice meant for glibc would break the musl leg.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("cannot run cargo for the forwarder");
    assert!(
        output.status.success(),
        "cannot build the in-box forwarder for {triple}:\n{}\n\
         if the target is missing, install it: rustup target add {triple}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = target_dir.join(&triple).join("release/wormhole-forward");
    println!("cargo:rustc-env=WORMHOLE_FORWARD_BIN={}", binary.display());
}
