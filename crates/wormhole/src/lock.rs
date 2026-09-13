//! The claim: who holds a box, and who holds a build. An `flock` the
//! kernel ends with the process, so nothing here is ever stale.

use std::path::Path;

use nix::fcntl::{Flock, FlockArg};
use wormhole_core::manifest::Resume;
use wormhole_core::{home, paths};

use crate::{fail, kept_boxes, report_problems};

/// A box this process holds the claim on. Its key is found, never rebuilt
/// from where the start stands.
pub(crate) struct Claim {
    pub(crate) target: home::Target,
    pub(crate) lock: Flock<std::fs::File>,
}

/// Which box this run is, claimed for as long as this process lives.
///
/// `--id` names one outright, from any workspace. `--new` starts another.
/// A bare `wormhole box` resumes the most recently used free box for this
/// role and falls through to a new one when every one is busy — so a
/// folder holds as many boxes as you make, and typing the same command
/// twice gets you back the same box rather than a stranger.
///
/// Whether a box is free is asked by taking its lock and never by reading
/// a list, so the answer cannot go stale between the reading and the
/// start. The kernel owns the claim: an `flock` ends when the process
/// holding it ends, however it ends — a box killed at any point leaves
/// nothing stale to reap.
///
/// Claimed before anything is resolved, so a refusal costs nothing. The
/// lock's file holds the pid of whoever took it, so a refusal can name
/// them. That text is a courtesy; the lock is the claim.
pub(crate) fn claim_named(data_home: &Path, here: &Path, wanted: &str) -> Claim {
    let kept = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&kept.problems);
    let target = home::target(&kept.records, &kept.keys, here, wanted).unwrap_or_else(|e| fail(&e));
    match claim_key(data_home, &target.key) {
        Some(lock) => Claim { target, lock },
        None => fail(&format!(
            "box {} is already running{}",
            target.id,
            holder(data_home, &target.key)
        )),
    }
}

/// The box a bare `wormhole box` should be: the most recently used free
/// one for this role, as far as the role lets a start look, or another
/// when none is free.
pub(crate) fn claim_free(
    data_home: &Path,
    here: &Path,
    new: bool,
    wanted: home::Wanted<'_>,
    resume: Resume,
) -> Claim {
    // A home that cannot be read is a box that cannot be resumed, and the
    // silent answer to that is a *new* box. Say so before starting one.
    let kept = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&kept.problems);
    if !new {
        for record in home::resumable(&kept.records, here, wanted, resume) {
            if let Some(lock) = claim_key(data_home, &record.key) {
                if record.serves(here) {
                    println!("box: {} (resumed)", record.id);
                } else {
                    println!(
                        "box: {} (resumed; last used in {})",
                        record.id,
                        record.workspace.display()
                    );
                }
                return Claim {
                    target: home::Target {
                        id: record.id.clone(),
                        key: record.key.clone(),
                        record: Some(record.clone()),
                    },
                    lock,
                };
            }
        }
    }

    // Every box is busy, or another was asked for. Ordinals are dense and
    // the loser of a race simply takes the next one, so two `--new` at the
    // same instant get two boxes rather than one refusal. A home whose
    // record cannot be read still holds its id.
    let mut taken: Vec<String> = kept
        .keys
        .iter()
        .filter_map(|key| paths::key_id(key))
        .map(str::to_owned)
        .collect();
    loop {
        let id = paths::box_id(here, home::free_ordinal(&taken, here));
        let key = paths::box_key(here, &id);
        if let Some(lock) = claim_key(data_home, &key) {
            println!("box: {id} (new)");
            return Claim {
                target: home::Target {
                    id,
                    key,
                    record: None,
                },
                lock,
            };
        }
        taken.push(id);
    }
}

/// Takes one box's claim, or `None` when another process holds it.
fn claim_key(data_home: &Path, key: &str) -> Option<Flock<std::fs::File>> {
    try_lock(&paths::lock_file(data_home, key)).unwrap_or_else(|e| fail(&e))
}

/// Who holds a box's claim, for a refusal that names them. Empty when the
/// file says nothing — the lock is the claim, this is only the courtesy.
///
/// Takes the key rather than the workspace and the id, because a box
/// whose record cannot be read has a key and nothing else.
pub(crate) fn holder(data_home: &Path, key: &str) -> String {
    let pid = std::fs::read_to_string(paths::lock_file(data_home, key)).unwrap_or_default();
    match pid.trim() {
        "" => String::new(),
        pid => format!(" (pid {pid})"),
    }
}

/// Takes an exclusive lock on `file` without waiting, stamping it with our
/// pid so a refusal can name who holds it. `Ok(None)` means someone else
/// has it.
///
/// One body behind every claim wormhole makes on shared host state. The
/// kernel ends the lock when its holder ends, so nothing needs reaping.
pub(crate) fn try_lock(file: &Path) -> Result<Option<Flock<std::fs::File>>, String> {
    match Flock::lock(open_lock(file)?, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => Ok(Some(stamp(lock, file))),
        Err((_, nix::errno::Errno::EWOULDBLOCK)) => Ok(None),
        Err((_, e)) => Err(format!("cannot claim {}: {e}", file.display())),
    }
}

/// The same claim, waited for instead of refused — and saying so, because
/// a wait nobody explains looks like a hang.
///
/// The two answers to "somebody else holds this" are not one answer: a box
/// already running cannot be joined, so its claim is a refusal, while a
/// build already running produces exactly what this process is waiting
/// for, so its claim is a queue.
pub(crate) fn wait_for_lock(
    file: &Path,
    waiting_for: &str,
) -> Result<Flock<std::fs::File>, String> {
    let held = match Flock::lock(open_lock(file)?, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => return Ok(stamp(lock, file)),
        Err((handle, nix::errno::Errno::EWOULDBLOCK)) => handle,
        Err((_, e)) => return Err(format!("cannot claim {}: {e}", file.display())),
    };
    println!("{waiting_for}; waiting for it to finish");
    match Flock::lock(held, FlockArg::LockExclusive) {
        Ok(lock) => Ok(stamp(lock, file)),
        Err((_, e)) => Err(format!("cannot claim {}: {e}", file.display())),
    }
}

/// The file behind a claim, created if it is not there. Never truncated on
/// open: its content belongs to whoever holds the lock, and opening is not
/// holding.
fn open_lock(file: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(file)
        .map_err(|e| format!("cannot open {}: {e}", file.display()))
}

/// Writes our pid into a claim we now hold, so a refusal elsewhere can name
/// us. A courtesy, not the claim: the lock is that.
fn stamp(mut lock: Flock<std::fs::File>, file: &Path) -> Flock<std::fs::File> {
    let pid = std::process::id().to_string();
    let wrote = lock
        .set_len(0)
        .and_then(|()| std::io::Write::write_all(&mut *lock, pid.as_bytes()));
    if let Err(e) = wrote {
        eprintln!(
            "wormhole: cannot record the lock holder in {}: {e}",
            file.display()
        );
    }
    lock
}
