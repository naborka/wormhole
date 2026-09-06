//! Builds the in-box forwarder and hands its path to `include_bytes!`.
//! The helper runs inside whatever image a box uses, so it must be
//! static (no `PT_INTERP`): that is the only linkage that runs under
//! any libc, and baking it in is what keeps the §12 "nothing to
//! install" promise without caring how wormhole itself was linked.
//!
//! The musl target gives static for free, but a plain glibc toolchain
//! does not carry its std, and `cargo install` must not fail over that:
//! where musl is missing the forwarder is built for the outer target
//! with `+crt-static` instead. Either way the result is verified to
//! need no loader before it is embedded.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("cargo sets CARGO_CFG_TARGET_ARCH");
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let musl = format!("{arch}-unknown-linux-musl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"))
        .join("../wormhole-forward/Cargo.toml");
    // Its own target dir: cargo locks the one it builds into, and reusing
    // the outer build's would deadlock this script against its caller.
    let target_dir = out_dir.join("forward-target");
    println!("cargo:rerun-if-changed=../wormhole-forward/src/main.rs");
    println!("cargo:rerun-if-changed=../wormhole-forward/Cargo.toml");
    let (triple, static_flag) = if musl_std_installed(&musl) {
        (musl.clone(), None)
    } else {
        (target, Some("-C target-feature=+crt-static"))
    };
    let mut cargo = Command::new(env::var("CARGO").expect("cargo sets CARGO"));
    cargo
        .args(["build", "--release", "--target", &triple, "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        // The host's flags are for the host's target; a `-C target-cpu`
        // or linker choice meant for glibc would break the musl leg.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR");
    if let Some(flag) = static_flag {
        cargo.env("RUSTFLAGS", flag);
    }
    let output = cargo.output().expect("cannot run cargo for the forwarder");
    assert!(
        output.status.success(),
        "cannot build the in-box forwarder for {triple}:\n{}\n\
         the forwarder must be static; the surest way is the musl target:\n\
         rustup target add {musl}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = target_dir.join(&triple).join("release/wormhole-forward");
    let bytes = std::fs::read(&binary)
        .unwrap_or_else(|e| panic!("cannot read the built forwarder {}: {e}", binary.display()));
    assert_eq!(
        wormhole_core::broker::requires_loader(&bytes),
        Ok(false),
        "the forwarder built for {triple} asks the image for a dynamic loader\n\
         and would die inside any box whose image lacks it; build it static:\n\
         rustup target add {musl}"
    );
    println!("cargo:rustc-env=WORMHOLE_FORWARD_BIN={}", binary.display());
}

/// Whether the toolchain carries a std for the given target. `rustup
/// target add` creates the lib dir; a toolchain without the target has
/// none, and a bare triple rustc does not know fails the print itself.
fn musl_std_installed(triple: &str) -> bool {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let Ok(output) = Command::new(rustc)
        .args(["--print", "target-libdir", "--target", triple])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let libdir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    std::fs::read_dir(libdir).is_ok_and(|mut dir| dir.next().is_some())
}
