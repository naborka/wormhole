//! The claim: who holds a box, and who holds a build. An `flock` the
//! kernel ends with the process, so nothing here is ever stale.

use std::path::Path;

use nix::fcntl::{Flock, FlockArg};
use wormhole_core::{home, paths};

use crate::{fail, kept_boxes, read_record, report_problems};

/// Which box this run is, claimed for as long as this process lives.
///
/// `--id` names one outright. `--new` starts another. A bare `wormhole
/// box` resumes this workspace's most recently used box for this role and
/// falls through to a new one when every existing box is busy — so a
/// folder holds as many boxes as you make, and typing the same command
/// twice gets you back the same box rather than a stranger.
///
/// Whether a box is free is asked by taking its lock and never by reading
/// a list, so the answer cannot go stale between the reading and the
/// start. The kernel owns the claim: an `flock` ends when the process
/// holding it ends, however it ends — a box killed at any point leaves
/// nothing stale to reap.
///
/// The lock's file holds the pid of whoever took it, so a refusal can name
/// them. That text is a courtesy; the lock is the claim.
pub(crate) fn claim_named(
    data_home: &Path,
    workspace: &Path,
    wanted: &str,
) -> (String, Flock<std::fs::File>) {
    // An id names a box by its directory, and nothing else has to be
    // readable for that: a home whose record is corrupt is still a box
    // this can start. An alias lives *in* the record, so it can only be
    // looked up among the records that parse.
    let id = if paths::is_box_id(wanted) {
        let home = paths::home_dir(data_home, &paths::box_key(workspace, wanted));
        if !home.is_dir() {
            fail(&home::no_box(wanted));
        }
        if let Ok(record) = read_record(&home)
            && record.workspace != workspace
        {
            fail(&format!(
                "box {wanted} belongs to {}, not to this workspace",
                record.workspace.display()
            ));
        }
        wanted.to_owned()
    } else {
        let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
        report_problems(&problems);
        home::by_name(&kept, workspace, wanted)
            .unwrap_or_else(|| fail(&home::no_box(wanted)))
            .id
            .clone()
    };
    match claim_id(data_home, workspace, &id) {
        Some(lock) => (id, lock),
        None => fail(&format!(
            "box {id} is already running{}",
            holder(data_home, &paths::box_key(workspace, &id))
        )),
    }
}

/// The box a bare `wormhole box` should be: this workspace's most recently
/// used free one for this role, or another when every one is busy.
pub(crate) fn claim_free(
    data_home: &Path,
    workspace: &Path,
    new: bool,
    wanted: home::Wanted<'_>,
) -> (String, Flock<std::fs::File>) {
    let claim = |id: &str| claim_id(data_home, workspace, id);
    // A home that cannot be read is a box that cannot be resumed, and the
    // silent answer to that is a *new* box. Say so before starting one.
    let (kept, problems) = kept_boxes(data_home).unwrap_or_else(|e| fail(&e));
    report_problems(&problems);
    if !new {
        for record in home::resumable(&kept, workspace, wanted) {
            if let Some(lock) = claim(&record.id) {
                println!("box: {} (resumed)", record.id);
                return (record.id.clone(), lock);
            }
        }
    }

    // Every box is busy, or another was asked for. Ordinals are dense and
    // the loser of a race simply takes the next one, so two `--new` at the
    // same instant get two boxes rather than one refusal.
    let mut taken: Vec<String> = kept.iter().map(|record| record.id.clone()).collect();
    loop {
        let id = paths::box_id(workspace, home::free_ordinal(&taken, workspace));
        if let Some(lock) = claim(&id) {
            println!("box: {id} (new)");
            return (id, lock);
        }
        taken.push(id);
    }
}

/// Takes one box's claim, or `None` when another process holds it.
pub(crate) fn claim_id(
    data_home: &Path,
    workspace: &Path,
    id: &str,
) -> Option<Flock<std::fs::File>> {
    let file = paths::lock_file(data_home, &paths::box_key(workspace, id));
    try_lock(&file).unwrap_or_else(|e| fail(&e))
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
