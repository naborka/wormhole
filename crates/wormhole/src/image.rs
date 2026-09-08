//! Turns a manifest's rootfs tarball into an extracted image directory,
//! once. Fetching uses `curl` and extraction uses `tar`, so the host's CA
//! store, proxy settings and tar quirks are theirs to handle, not ours.
//! The digest is checked here, in process, before anything is extracted.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use wormhole_core::manifest::Manifest;
use wormhole_core::paths;

/// The extracted, unmodified rootfs for this manifest, fetched once and
/// reused. The digest names it, so identical bytes are never fetched twice.
pub fn ensure_base(manifest: &Manifest, data_home: &Path) -> Result<PathBuf, String> {
    let base = paths::base_dir(data_home, &manifest.image.base_sha256);
    if base.is_dir() {
        return Ok(base);
    }

    let partial = paths::partial(&base);
    remove(&partial)?;
    create(&partial)?;

    let tarball = partial.join("rootfs.tar");
    println!("fetching {}", manifest.image.base);
    fetch(&manifest.image.base, &tarball)?;
    verify(&tarball, &manifest.image.base_sha256, "rootfs")?;
    extract(&tarball, &partial)?;
    remove_file(&tarball)?;

    finish(&partial, &base)
}

/// One artifact's bytes on the host, fetched once and proved before
/// anything can use them.
///
/// This is the whole point of an artifact: the fetch happens here, where
/// the machine's own resolver, proxy and trust store already work, and
/// what crosses into the build box is bytes whose digest the recipe
/// already named. A network that intercepts TLS cannot break a build that
/// opens no connection, and cannot substitute bytes whose digest is fixed.
pub fn ensure_artifact(url: &str, sha256: &str, data_home: &Path) -> Result<PathBuf, String> {
    let file = paths::artifact_file(data_home, sha256);
    if file.is_file() {
        return Ok(file);
    }

    let partial = paths::partial(&file);
    let _ = fs::remove_file(&partial);
    create(&paths::artifacts_dir(data_home))?;
    println!("fetching {url}");
    fetch(url, &partial)?;
    verify(&partial, sha256, "artifact")?;
    // Read-only and not executable, decided here rather than left to
    // curl's umask: bytes off the network are not something a build can
    // run by accident, and the recipe copying one to run it is the
    // deliberate step that says otherwise.
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o444))
        .map_err(|e| format!("cannot seal {}: {e}", partial.display()))?;
    finish(&partial, &file)
}

/// Copies a directory tree to `target`, replacing whatever was there.
/// Reflinked where the filesystem supports it, so a copy costs almost
/// nothing and a box never writes into the image it started from.
pub fn copy(from: &Path, target: &Path) -> Result<(), String> {
    remove(target)?;
    if let Some(parent) = target.parent() {
        create(parent)?;
    }
    run(
        Command::new("cp")
            .args(["--archive", "--reflink=auto"])
            .arg(from)
            .arg(target),
        "copy the image",
    )
}

/// Renames a finished directory into place. Until this succeeds nothing
/// can mistake a half-built image for a usable one.
pub fn finish(partial: &Path, final_dir: &Path) -> Result<PathBuf, String> {
    if let Some(parent) = final_dir.parent() {
        create(parent)?;
    }
    fs::rename(partial, final_dir)
        .map_err(|e| format!("cannot move into {}: {e}", final_dir.display()))?;
    Ok(final_dir.to_owned())
}

/// Makes a path not exist, for the callers with nowhere to put a failure:
/// a throwaway root copy nobody waits on, and the detached reap behind it.
pub fn discard(path: &Path) {
    let _ = remove(path);
}

fn fetch(url: &str, target: &Path) -> Result<(), String> {
    run(
        Command::new("curl")
            .args(["--fail", "--silent", "--show-error", "--location"])
            .arg("--output")
            .arg(target)
            .arg(url),
        &format!("fetch {url}"),
    )
}

/// The only thing standing between a manifest and arbitrary code in the
/// box, so it reads the file in chunks and compares the whole digest.
/// One body for the base and every artifact: they are held to one rule,
/// and a second copy of it would drift.
fn verify(file: &Path, expected: &str, what: &str) -> Result<(), String> {
    let bytes = fs::File::open(file).map_err(|e| format!("cannot read the fetched {what}: {e}"))?;
    let mut hasher = Sha256::new();
    let mut reader = std::io::BufReader::new(bytes);
    std::io::copy(&mut reader, &mut hasher)
        .map_err(|e| format!("cannot hash the fetched {what}: {e}"))?;
    let found = format!("{:x}", hasher.finalize());
    if found == expected {
        Ok(())
    } else {
        Err(format!(
            "{what} digest does not match the manifest\n  expected {expected}\n  found    {found}"
        ))
    }
}

/// `./dev` is skipped because its device nodes cannot be created without
/// privileges, and the box mounts its own `/dev` anyway.
fn extract(tarball: &Path, into: &Path) -> Result<(), String> {
    run(
        Command::new("tar")
            .arg("--extract")
            .arg("--auto-compress")
            .arg("--no-same-owner")
            .arg("--exclude=./dev/*")
            .arg("--file")
            .arg(tarball)
            .arg("--directory")
            .arg(into),
        "extract the rootfs",
    )
}

pub fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("cannot {what}: {e}; is it installed?"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(format!("cannot {what}: {}", detail.trim()))
}

pub fn create(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

/// Makes a path not exist. Absence is success — every caller wants the
/// path gone, not the deleting.
///
/// Whatever the path is: `gc` hands this a directory holding a gigabyte
/// and a lock file holding nothing, and a deleter that only knew about
/// directories would leave one of them behind for no reason a caller
/// could see.
pub fn remove(path: &Path) -> Result<(), String> {
    let gone = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match gone {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot clear {}: {e}", path.display())),
    }
}

fn remove_file(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| format!("cannot remove {}: {e}", path.display()))
}
