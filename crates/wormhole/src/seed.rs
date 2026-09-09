//! What a start writes into the box home before the agent runs: the
//! instructions, the preflight hook, the agent's first-run answers and
//! its login.

use std::path::{Path, PathBuf};

use wormhole_core::manifest;

use crate::{DEFAULT_INSTRUCTIONS, fail, replace_file, replace_file_private};

/// Writes the agent's instructions into the kept home, freshly on every
/// start: the manifest's own directory (workspace or role) and the binary
/// are its source of truth, so nothing a past box wrote there can drift
/// away from them.
///
/// The text lands once, in the canonical `AGENTS.md` at the home root;
/// what goes at the path the agent actually reads is a pointer — an
/// import line, or a symlink for an agent with no import syntax. One
/// source of truth however many agents learn to read it.
pub(crate) fn seed_instructions(manifest: &manifest::Manifest, manifest_dir: &Path, home: &Path) {
    let Some((target, pointer)) = manifest::instructions_pointer(manifest) else {
        return;
    };
    let extra = manifest.agent.instructions.as_ref().map(|path| {
        std::fs::read_to_string(manifest_dir.join(path))
            .unwrap_or_else(|e| fail(&format!("cannot read instructions {path}: {e}")))
    });
    // The role's own file last, so it still wins where it disagrees with
    // the built-in instructions.
    seed_file(
        home,
        manifest::INSTRUCTIONS_SEED,
        &manifest::compose_instructions(DEFAULT_INSTRUCTIONS, extra.as_deref()),
    );
    match pointer {
        manifest::Pointer::Import(line) => seed_file(home, target, line),
        manifest::Pointer::Symlink => seed_symlink(home, target),
    }
}

/// Plants a symlink at `target` (relative to the box home) pointing at
/// the canonical instructions file, replacing whatever a past start left
/// there — a stale regular file would shadow the one source of truth.
fn seed_symlink(home: &Path, target: &str) {
    let link = home.join(target);
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", parent.display())));
    }
    let to = manifest::pointer_link(target);
    match std::fs::symlink_metadata(&link) {
        Ok(_) => std::fs::remove_file(&link)
            .unwrap_or_else(|e| fail(&format!("cannot replace {}: {e}", link.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => fail(&format!("cannot read {}: {e}", link.display())),
    }
    std::os::unix::fs::symlink(&to, &link)
        .unwrap_or_else(|e| fail(&format!("cannot link {}: {e}", link.display())));
}

/// Writes one seeded file into the kept home, creating its directory —
/// the single body behind every artifact a box start plants there.
fn seed_file(home: &Path, target: &str, content: &str) {
    let file = home.join(target);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", parent.display())));
    }
    std::fs::write(&file, content)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", file.display())));
}

/// Copies the preflight hook into the kept home, where the launch script
/// runs it — the manifest's directory (a role's especially) is not in the
/// box. Fresh on every start, and gone when the manifest names none, so
/// nothing stale survives a manifest change. A named hook that does not
/// exist stops the launch here, on the host, with the path in the error —
/// not in the box with a bare "not found".
pub(crate) fn seed_preflight(manifest: &manifest::Manifest, manifest_dir: &Path, home: &Path) {
    let seed = home.join(manifest::PREFLIGHT_SEED);
    let Some(path) = manifest.agent.preflight.as_ref() else {
        if let Err(e) = std::fs::remove_file(&seed)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            fail(&format!("cannot remove stale {}: {e}", seed.display()));
        }
        return;
    };
    let script = std::fs::read_to_string(manifest_dir.join(path))
        .unwrap_or_else(|e| fail(&format!("cannot read preflight hook {path}: {e}")));
    seed_file(home, manifest::PREFLIGHT_SEED, &script);
}

/// Answers the agent's first-run questions in its own config file —
/// merged, never overwritten, so logins and the agent's own choices in
/// the kept home survive. Which file and which format is the registry's
/// answer, not a string compared here. A login handed over carries the
/// host's account fields along, where the box has none of its own.
pub(crate) fn seed_agent_config(
    manifest: &manifest::Manifest,
    workspace: &Path,
    home: &Path,
    credentials: manifest::Credentials,
) {
    let Some(kind) = manifest::config_seed(manifest) else {
        return;
    };
    let file = home.join(kind.file());
    let existing = read_if_present(&file);
    let workspace = workspace.display().to_string();
    let config = match kind {
        manifest::ConfigSeed::ClaudeJson => {
            let host_login = match credentials {
                manifest::Credentials::None => None,
                _ => read_if_present(&host_home().join(".claude.json")),
            };
            wormhole_core::seed::claude_config(
                existing.as_deref(),
                &workspace,
                host_login.as_deref(),
            )
        }
        manifest::ConfigSeed::CodexToml => wormhole_core::seed::codex_config(
            existing.as_deref(),
            &workspace,
            manifest.agent.model.as_deref(),
        ),
    }
    .unwrap_or_else(|e| fail(&e));
    replace_file(&file, config).unwrap_or_else(|e| fail(&e));
}

/// A file's text, `None` when there is no such file.
fn read_if_present(file: &Path) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => fail(&format!("cannot read {}: {e}", file.display())),
    }
}

/// Hands the box the login this start asked for. Returns the host files a
/// `share` binds read-write, as `(host file, path in the box home)`; the
/// bind is the boundary's, the mount point is made here.
///
/// A `copy` is once: a box that already has the file keeps it. A host
/// with no login to give is refused by name.
pub(crate) fn seed_credentials(
    mode: manifest::Credentials,
    manifest: &manifest::Manifest,
    home: &Path,
) -> Vec<(String, String)> {
    if mode == manifest::Credentials::None {
        return Vec::new();
    }
    let host = host_home();
    let mut shared = Vec::new();
    for file in manifest::credential_files(manifest) {
        let source = host.join(file);
        if !source.is_file() {
            fail(&format!(
                "credentials = \"{mode}\", but the host has no {}; \
                 log in on the host first, or start with --credentials none",
                source.display()
            ));
        }
        let target = home.join(file);
        if mode == manifest::Credentials::Share {
            println!("credentials: sharing {} read-write", source.display());
            shared.push((source.display().to_string(), (*file).to_owned()));
        }
        if target.exists() {
            continue;
        }
        let seed = match mode {
            manifest::Credentials::Copy => {
                println!("credentials: copied {} into the box home", source.display());
                std::fs::read(&source)
                    .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", source.display())))
            }
            _ => Vec::new(),
        };
        replace_file_private(&target, seed).unwrap_or_else(|e| fail(&e));
    }
    shared
}

/// The host's CA bundle, when the manifest asks to trust it: the first of
/// the paths distros keep it at, followed through symlinks to the real
/// file so the mount cannot dangle. A host without any is an error the
/// user must see — they asked for a trust the box cannot get.
pub(crate) fn host_ca(manifest: &manifest::Manifest) -> Option<String> {
    if !manifest.access.host_ca {
        return None;
    }
    let candidates = [
        "/etc/ssl/certs/ca-certificates.crt", // Debian, Arch, Alpine
        "/etc/pki/tls/certs/ca-bundle.crt",   // Fedora, RHEL
        "/etc/ssl/ca-bundle.pem",             // openSUSE
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem", // CentOS
        "/etc/ssl/cert.pem",                  // FreeBSD, macOS
    ];
    let found = candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .unwrap_or_else(|| {
            fail("[access] host_ca is set, but no CA bundle exists at any known host path")
        });
    let real = std::fs::canonicalize(found)
        .unwrap_or_else(|e| fail(&format!("cannot resolve the host CA bundle {found}: {e}")));
    Some(real.display().to_string())
}

/// `~` in a manifest grant means the host's home, expanded here because a
/// home directory is not something the pure core can know.
pub(crate) fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => host_home().join(rest).display().to_string(),
        None => path.to_owned(),
    }
}

fn host_home() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => fail("HOME is not set, so `~` in a grant cannot be expanded"),
    }
}
