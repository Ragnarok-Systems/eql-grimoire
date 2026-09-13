//! Where the updater's files live, and the order the files move in.
//!
//! # TWO ROOTS, AND THE SPLIT IS DELIBERATE
//!
//! `settings.json` and the snapshot the reader curates stay in `dirs::config_dir()`
//! (`%APPDATA%`, which roams). Everything the updater writes goes in `dirs::data_local_dir()`
//! (`%LOCALAPPDATA%`, which does not). A 10.4 MB executable and a 29 MB data tree must never
//! enter a roaming profile: on a domain machine that is a login-time copy of both, every login.
//!
//! ```text
//! %LOCALAPPDATA%\eql-grimoire\
//!   app\
//!     current.json                       the pointer the trampoline reads; nothing else reads it
//!     0.2.0\grimoire-desktop.exe         a managed payload
//!     0.2.0\grimoire-desktop.exe.minisig
//!     0.1.0\grimoire-desktop.exe         the previous version, kept for rollback
//!   data\                                the installed data bundle
//!   data.previous\                       one generation back
//!   update\
//!     staging\0.2.0\grimoire-desktop.exe partial and verified downloads live here
//! ```
//!
//! Everything under `app\` and `update\staging\` is on one volume BY CONSTRUCTION, which is what
//! makes every rename below a rename and not a copy-and-delete.
//!
//! # THE ORDER IS THE DESIGN, AND EVERY STEP NAMES THE FAILURE IT GUARDS
//!
//! Power loss, step by step: staging leaves a `.part` or a staged tree the next check deletes and
//! redownloads; the preflight leaves nothing; the copy leaves an `.incoming` that the next
//! attempt overwrites and the prune removes; the re-verify leaves an installed-but-unverified
//! payload that `current.json` does not point at, so nothing runs it; the pointer flip is atomic;
//! the prune leaves stale directories the next prune removes. AT NO POINT does the path the Start
//! Menu shortcut names stop existing, and at no point is there a state that requires a human to
//! repair.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::Utc;
use semver::Version;

use super::launch::{Current, POINTER_VERSION};
use super::manifest::{Artifact, KIND_DATA};
use super::verify::Seal;
use super::{io, pulse_word, Refusal};

/// The one file name every managed payload has. Not the artifact's URL basename: the URL carries
/// a version and a platform so that a bucket listing reads, and the installed file must not,
/// because the trampoline's pointer is what says which version a payload is.
pub fn exe_name() -> String {
    format!("grimoire-desktop{}", std::env::consts::EXE_SUFFIX)
}

/// The one file name a staged data bundle has.
pub const BUNDLE_NAME: &str = "bundle.tar.gz";

/// The environment variable that makes a launch draw for a moment and exit.
///
/// DECLARED IN `main.rs` AND REPEATED HERE, WITH A TEST THAT HOLDS THE TWO TOGETHER. `main.rs`
/// owns it as a private `const` (`SMOKE_ENV`), and a `pub` copy over there would be API for one
/// caller. A source-text test in this file asserts `main.rs` still names this exact string, so a
/// rename there goes red here rather than silently turning every preflight into a launch with no
/// deadline that never exits.
pub const SMOKE_ENV: &str = "GRIMOIRE_SMOKE_MS";

/// Where the updater's files are.
#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    /// A layout rooted anywhere, which is what lets every rule below be tested against a temp
    /// directory instead of the owner's real install.
    pub fn at(root: impl Into<PathBuf>) -> Layout {
        Layout { root: root.into() }
    }

    /// `%LOCALAPPDATA%\eql-grimoire`, or `None` on a platform with no local data directory.
    pub fn platform() -> Option<Layout> {
        dirs::data_local_dir().map(|d| Layout::at(d.join(crate::settings::APP_DIR)))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn app_dir(&self) -> PathBuf {
        self.root.join("app")
    }
    pub fn current_path(&self) -> PathBuf {
        self.app_dir().join("current.json")
    }
    /// The directory a version's payload lives in.
    ///
    /// TAKES A PARSED [`Version`] AND NOT A STRING, WHICH IS THE PATH GUARD. A semver's only
    /// characters are digits, letters, `.`, `-` and `+`, so a parsed version cannot carry a path
    /// separator, a `..`, or a drive prefix. Making the type the guard means there is no
    /// sanitiser here for a later caller to forget to call.
    pub fn version_dir(&self, v: &Version) -> PathBuf {
        self.app_dir().join(v.to_string())
    }
    pub fn exe(&self, v: &Version) -> PathBuf {
        self.version_dir(v).join(exe_name())
    }
    pub fn staging(&self, v: &Version) -> PathBuf {
        self.root.join("update").join("staging").join(v.to_string())
    }
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn data_previous(&self) -> PathBuf {
        self.root.join("data.previous")
    }
    pub fn data_incoming(&self) -> PathBuf {
        self.root.join("data.incoming")
    }
}

/// The name a staged artifact is written under.
fn staged_name(a: &Artifact) -> String {
    if a.kind == KIND_DATA {
        BUNDLE_NAME.to_owned()
    } else {
        exe_name()
    }
}

/// The signature file that travels beside a payload.
pub fn sig_of(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".minisig");
    PathBuf::from(s)
}

/* ============================================================== durability == */

/// Write a file and flush it to the DEVICE before returning.
///
/// # WHY THIS EXISTS RATHER THAN `std::fs::write`
///
/// Everything in this module is a rename, and a rename is only atomic with respect to what is
/// already on the platter. [`write_current`] has always said so about the pointer: "a rename that
/// lands ahead of the bytes it names is the power-loss case this whole design exists to remove".
/// The same sentence is true of every other file here and was not being obeyed for any of them,
/// including the 10.8 MB one the pointer names. On NTFS a metadata operation can be journalled
/// ahead of the data it refers to, so the state after a crash was a directory entry, a size, and
/// extents that were never written.
///
/// The re-verification at step 8 does not close that gap, and it is worth saying why: it reads
/// back through the page cache, so it passes on bytes that have not reached the disk.
fn write_durably(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    use std::io::Write as _;
    let mut f = std::fs::File::create(path).map_err(|e| io("create", path, &e))?;
    f.write_all(bytes).map_err(|e| io("write", path, &e))?;
    f.sync_all().map_err(|e| io("flush", path, &e))
}

/// Copy a file and flush the copy to the device before returning. See [`write_durably`].
///
/// STREAMED THROUGH A 64 KiB BUFFER, because the one caller is copying a payload measured at
/// 10,874,880 bytes on 2026-09-11 and a read-it-all-then-write-it would hold that in memory for no
/// reason. `std::io::copy` picks its own buffer and is what this would be without the `sync_all`.
fn copy_durably(from: &Path, to: &Path) -> Result<(), Refusal> {
    let mut src = std::fs::File::open(from).map_err(|e| io("open", from, &e))?;
    let mut dst = std::fs::File::create(to).map_err(|e| io("create", to, &e))?;
    std::io::copy(&mut src, &mut dst).map_err(|e| io("copy into", to, &e))?;
    dst.sync_all().map_err(|e| io("flush", to, &e))
}

/// Ask the filesystem to flush a DIRECTORY's own entries, where the platform has such a call.
///
/// BEST EFFORT, AND THE RETURN TYPE SAYS SO. Flushing a file guarantees its contents; on a
/// crash-consistent filesystem the directory entry that names it is a separate write. Opening a
/// directory as a file works on Unix and fails on Windows without `FILE_FLAG_BACKUP_SEMANTICS`,
/// which `std::fs` does not expose, so on Windows this does nothing and the rename's own ordering
/// guarantees are what is left. Reporting a failure would be reporting that this platform does not
/// offer the call, which is not a failure of the update.
fn sync_dir(dir: &Path) {
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

/// A temp file beside `path` that no other writer, in this process or another, will also pick.
///
/// A FIXED `.tmp` NAME IS SHARED BY EVERY WRITER. The one case [`with_lock`] cannot rule out is a
/// holder slow enough to have its lock taken, and with one shared name that case was two writers
/// creating, filling and renaming ONE file, so the second rename found nothing and the write
/// failed. With a name per writer the same case is a last-rename-wins lost update, which is the
/// cost `LOCK_PATIENCE` already names and accepts.
pub(crate) fn temp_beside(path: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".tmp-{}-{n}", std::process::id()));
    PathBuf::from(s)
}

/* =================================================================== locking == */

/// HOW LONG A WRITER WAITS FOR ANOTHER WRITER BEFORE IT TAKES THE LOCK ANYWAY.
///
/// A POLICY, NOT A MEASUREMENT. The section it guards is a read of a file measured in hundreds of
/// bytes, a serialize, a flush and a rename, so a lock file that has carried ONE holder's token for
/// five seconds, timed on the waiter's own monotonic clock, belongs to a process that died holding
/// it rather than one that is busy. Never the wall clock and never the file's modification time:
/// either moves when the system clock is stepped, and a step forward once made a live holder's lock
/// look five seconds old. Waiting forever would let a crashed copy of
/// the app stop every future copy from recording anything; failing instead would turn the same
/// crash into a permanent inability to write the bookkeeping. Taking it is the only answer that
/// recovers on its own, and the cost of taking it wrongly is exactly the unsynchronised write this
/// lock exists to remove, which is to say no worse than not having it.
pub const LOCK_PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

/// Run `f` with nobody else in this app writing `path`.
///
/// # WHY THERE IS A LOCK AT ALL, WHICH IS NOT ABOUT THREADS
///
/// Nothing in this crate stops two copies of the app running: there is no single-instance guard,
/// and [`super::launch::ENTRY_ENV`]'s own doc contemplates "two copies of the app started from two
/// different folders". Both get the same [`Layout::platform`] root, so both write the same
/// `current.json` and the same `update\state.json`. Every writer here is read-modify-write, and
/// two of those interleaved lose one side's changes entirely: a withdrawal recorded by one copy
/// erased by the other's stale in-memory picture is the yank mechanism failing at the one moment
/// it exists for.
///
/// # A LOCK FILE, NOT AN OS FILE LOCK
///
/// `std::fs` has no advisory locking and the crates that add it would be a new dependency for one
/// call site. `create_new` is a single atomic filesystem operation on both platforms this builds
/// for, which is the whole of what a lock needs. The holder writes a token of its own into the lock
/// file, and the lock is released by deleting the file if it still carries that token, including on
/// the error path, which is what the guard struct below is for: an early `?` inside `f` must not
/// leave a lock behind.
pub fn with_lock<T>(path: &Path, f: impl FnOnce() -> Result<T, Refusal>) -> Result<T, Refusal> {
    with_lock_patient(path, LOCK_PATIENCE, f)
}

/// [`with_lock`] with the patience as an argument, so a test can hold it to milliseconds instead
/// of sleeping through five real seconds.
fn with_lock_patient<T>(
    path: &Path,
    patience: std::time::Duration,
    f: impl FnOnce() -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    let lock = {
        let mut s = path.as_os_str().to_owned();
        s.push(".lock");
        PathBuf::from(s)
    };
    if let Some(d) = lock.parent() {
        std::fs::create_dir_all(d).map_err(|e| io("create", d, &e))?;
    }

    let token = lock_token();
    /* When this waiter was first denied the lock's name, while it still is. See `denied_for`. */
    let mut denied_since: Option<std::time::Instant> = None;
    /* The token this waiter last read in the lock file, and when, on the monotonic clock, it first
     * read that same token. Cleared whenever the token changes or the file is gone. */
    let mut seen: Option<(Vec<u8>, std::time::Instant)> = None;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                /* THE TOKEN GOES IN BEFORE THE SECTION RUNS. A holder that cannot write it does not
                 * run the section with a lock whose release could never recognise it: the file is
                 * removed and the failure returned. A holder that dies mid-write leaves a partial
                 * token, which a waiter treats like any other token that never changes. */
                use std::io::Write as _;
                if let Err(e) = file.write_all(token.as_bytes()) {
                    drop(file);
                    let _ = std::fs::remove_file(&lock);
                    return Err(io("write", &lock, &e));
                }
                break;
            }
            /* A NAME STILL BEING RELEASED, NOT A REFUSAL. On Windows, deleting a file sets its
             * delete disposition on a handle and then closes it, and in between the name is delete
             * pending: `create_new` on it is "Access is denied" (os error 5), not `AlreadyExists`.
             * A waiter that arrived while the previous holder's release was mid-flight returned
             * that as an I/O error and its section never ran. Measured: 8 threads racing
             * `create_new` against `remove_file` for 5 s were denied 1,475 times in 40,263.
             *
             * BOUNDED, because the same error is also a folder nobody may write: see `denied_for`.
             * Windows only: elsewhere a denied create is a permission, never a release. */
            Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => {
                seen = None;
                if denied_for(&mut denied_since, patience) {
                    return Err(io("create", &lock, &e));
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                /* THE SAME TOKEN FOR THE WHOLE PATIENCE ON THIS WAITER'S MONOTONIC CLOCK, AND
                 * NOTHING ELSE, SAYS THE HOLDER IS DEAD.
                 *
                 * Not how long this waiter has waited: measured from the waiter's own start, a
                 * queue of live writers each holding the lock briefly kept a late waiter out past
                 * the patience with no holder being slow, and the waiter took the lock from one that
                 * was still inside (Linux CI, `failed_launches_accumulate_and_a_drawn_frame_clears_
                 * them`). A busy queue keeps writing new tokens; only a dead holder leaves one that
                 * stays.
                 *
                 * And not the lock file's modification time, which is what replaced that and was
                 * wrong in its own way: its age is two readings of the wall clock, so a clock
                 * stepped forward took the lock from live holders (15 of 15 runs of the live-queue
                 * test on WSL with root stepping the clock two seconds every 150 ms, 0 of 15 left
                 * alone), and a clock stepped back left a dead holder's lock with an age that could
                 * not be read, which was never stale, so it was never taken. */
                match std::fs::read(&lock) {
                    Ok(now) => {
                        denied_since = None;
                        let same_since = seen
                            .as_ref()
                            .filter(|(was, _)| *was == now)
                            .map(|(_, since)| *since);
                        match same_since {
                            Some(since) if since.elapsed() >= patience => {
                                /* TAKEN, NOT WAITED FOR FOR EVER. See `LOCK_PATIENCE`: the
                                 * alternative is that one crash disables this bookkeeping
                                 * permanently.
                                 *
                                 * AND ROUND AGAIN TO CREATE IT, never straight into the section.
                                 * Breaking here ran the write with no lock file at all, so any
                                 * other writer walked in beside it.
                                 *
                                 * STILL NOT PERFECT, SAID PLAINLY: a holder that finishes between
                                 * the read above and this remove, followed by a new holder creating
                                 * the name in that same gap, loses its lock to this remove. That
                                 * needs a holder to have sat on one token for the whole patience
                                 * first; a queue of live holders cannot reach it. */
                                let _ = std::fs::remove_file(&lock);
                                seen = None;
                                continue;
                            }
                            Some(_) => {}
                            None => seen = Some((now, std::time::Instant::now())),
                        }
                    }
                    /* Released between the create and the read: straight round to create it. */
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        seen = None;
                        continue;
                    }
                    /* Delete pending on Windows reads as denied too, for the reason given above. */
                    Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => {
                        seen = None;
                        if denied_for(&mut denied_since, patience) {
                            return Err(io("read", &lock, &e));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => return Err(io("read", &lock, &e)),
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(io("create", &lock, &e)),
        }
    }

    /* THE RELEASE HAPPENS ON EVERY PATH OUT, INCLUDING A PANIC. `f` is arbitrary caller code and
     * the whole file dance below it is full of `?`; a lock leaked by an early return would be
     * taken by the next writer only after `LOCK_PATIENCE`, once per failure, for ever.
     *
     * AND ONLY OF A LOCK THAT IS STILL THIS HOLDER'S. A holder slow past the patience has had its
     * lock taken, and the name now belongs to the taker; deleting it unread let the next writer in
     * beside the taker, turning one wrong take into two. */
    struct Held {
        lock: PathBuf,
        token: String,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            if std::fs::read(&self.lock).is_ok_and(|b| b == self.token.as_bytes()) {
                let _ = std::fs::remove_file(&self.lock);
            }
        }
    }
    let _held = Held { lock, token };
    f()
}

/// Has this waiter been denied the lock's name for the whole patience? Starts the count on the
/// first denial; the caller clears `since` when the name answers anything else.
///
/// THE SAME ERROR IS ALSO A FOLDER NOBODY MAY WRITE, which is why a delete pending name is waited
/// for only so long: denied for the whole patience, counted from this waiter's first denial, it is
/// that, and the caller returns it.
fn denied_for(since: &mut Option<std::time::Instant>, patience: std::time::Duration) -> bool {
    since.get_or_insert_with(std::time::Instant::now).elapsed() >= patience
}

/// A token no other holder, in this process or another, will also write into a lock file.
///
/// THREE PARTS, EACH COVERING WHAT THE OTHERS DO NOT. The process id tells two live processes
/// apart; the process-wide counter tells two holders in one process apart; the nonce tells this
/// process from an earlier one that had the same id and died holding a lock. The nonce is the
/// nanoseconds since this process first asked for a token, on the monotonic clock, hashed with
/// `RandomState`'s per-process random keys, because the standard library exposes no absolute
/// monotonic reading and a wall clock reading is exactly what this lock no longer trusts.
fn lock_token() -> String {
    use std::hash::BuildHasher as _;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static FIRST: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let since = FIRST
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos();
    let nonce = std::collections::hash_map::RandomState::new().hash_one((n, since));
    format!("{}-{n}-{nonce:016x}", std::process::id())
}

/* ================================================================= staging == */

/// STEPS 1 TO 5: DOWNLOAD AND STAGE.
///
/// `body` is whatever the caller has: an HTTP response body on the other half's worker thread, a
/// file in a test. Nothing in here dials anything, which is what makes the whole of the check
/// order testable.
///
/// 1. Write to `<name>.part`. A partially written file is never mistaken for a complete one,
///    because nothing ever reads a `.part`.
/// 2. Stream it, enforcing the signed `size` exactly, feeding each chunk to sha256 and to the
///    file in the same loop, so the artifact is never held in memory.
/// 3. sha256 against the signed manifest. A mismatch deletes the `.part` and refuses, with a
///    cause of its own so corruption is never reported as tampering.
/// 4. The artifact's own signature. A failure deletes and refuses.
/// 5. Write the signature text beside it FIRST, then rename `.part` into place. The order is the
///    guard: a crash between the two leaves a signature with no payload, which is inert, rather
///    than a verified-looking payload with no proof.
///
/// THE FIGHT GATE IS HERE AND NOT AT THE CALL SITE. A gate written at a call site is a gate the
/// second call site will not have; see [`super::may_start_download`] for why only `Pulse::Closed`
/// passes.
pub fn stage<R: Read>(
    l: &Layout,
    version: &Version,
    art: &Artifact,
    body: R,
    pulse: crate::fights::Pulse,
    keys: &[&str],
    progress: &mut dyn FnMut(u64),
) -> Result<PathBuf, Refusal> {
    if !super::may_start_download(pulse) {
        return Err(Refusal::NotAQuietMoment {
            pulse: pulse_word(pulse),
        });
    }
    let dir = l.staging(version);
    std::fs::create_dir_all(&dir).map_err(|e| io("create", &dir, &e))?;
    let final_path = dir.join(staged_name(art));
    /* THE PARTIAL FILE IS NAMED AFTER THE PROCESS WRITING IT, AND THE RENAME IS THE ARBITRATION.
     *
     * THE DEFECT THIS FIXES. The name was fixed, so two copies of the app offered the same version
     * both called `File::create` on it and both write loops interleaved into one file. Nothing
     * stops two copies running (see `with_lock` above), and `auto_download` is on by default, so
     * both would reach here on their own within a quarter second of each other. The result is two
     * hash mismatches, both recorded, and a reader told "the bytes on the server are not the bytes
     * the signed manifest named" for a release that is on that machine permanently unreachable,
     * because the release pipeline refuses to republish a version with different bytes.
     *
     * With a per-writer name, each loop owns its file, each is checked in full, and the rename at
     * the end is the arbitration: the loser overwrites a complete verified file with another
     * complete verified file, which is the same bytes twice. That is also why the hash mismatch
     * gets one retry rather than being permanent on its first occurrence
     * (`Refusal::retries`): a collision and a bad CDN look identical from here. */
    let part = {
        let mut s = final_path.as_os_str().to_owned();
        s.push(format!(".{}.part", std::process::id()));
        PathBuf::from(s)
    };

    let seal = Seal {
        size: art.size,
        sha256: &art.sha256,
        signature: &art.signature,
    };
    let answer = (|| -> Result<(), Refusal> {
        let f = std::fs::File::create(&part).map_err(|e| io("create", &part, &e))?;
        let mut w = std::io::BufWriter::new(f);
        super::verify::copy_sealed(body, Some(&mut w), seal, keys, progress)?;
        /* FLUSH BEFORE THE RENAME. A BufWriter dropped without a flush swallows its own error,
         * and the bytes it is holding are exactly the tail the sha256 already accepted.
         *
         * AND THEN TO THE DEVICE, which the flush alone does not do: `BufWriter::flush` moves the
         * bytes from this program's buffer into the operating system's, and the rename below can
         * still be journalled ahead of them. See `write_durably` for the whole of that argument;
         * it is the same discipline, applied to the file that actually has to exist. */
        use std::io::Write as _;
        w.flush().map_err(|e| io("finish writing", &part, &e))?;
        w.into_inner()
            .map_err(|e| io("finish writing", &part, e.error()))?
            .sync_all()
            .map_err(|e| io("flush", &part, &e))
    })();
    if let Err(why) = answer {
        /* ANY FAILURE DELETES THE PARTIAL FILE BEFORE ANYTHING IS REPORTED. A staged file left
         * behind by a refused download is a file the next attempt might treat as progress. */
        let _ = std::fs::remove_file(&part);
        return Err(why);
    }

    write_durably(&sig_of(&final_path), art.signature.as_bytes())?;
    std::fs::rename(&part, &final_path).map_err(|e| io("rename into place", &final_path, &e))?;
    sync_dir(&dir);
    Ok(final_path)
}

/* =============================================================== preflight == */

/// Does this binary start on this machine?
///
/// A TRAIT, BECAUSE THE PRODUCTION ANSWER SPAWNS A PROCESS AND A TEST MUST NOT. The production
/// implementation is [`Spawn`] and it is four lines on top of [`smoke_command`], which is itself
/// asserted by a test: this crate has already shipped a defect where a test double and the real
/// wire disagreed (`twitch_auth.rs:142-155`), and the defence is to keep the production side thin
/// enough to check.
pub trait Preflight {
    fn smoke(&self, exe: &Path) -> Result<(), String>;
}

/// The command a preflight runs.
///
/// THE DEADLINE IS `data::LOAD_BUDGET` AND NOT A NEW NUMBER. That constant is the app's own
/// measured budget for being fully up (`data/mod.rs:152`, with the measurements in its doc), so
/// reusing it means this feature introduces no figure nobody measured.
pub fn smoke_command(exe: &Path) -> std::process::Command {
    let mut c = std::process::Command::new(exe);
    c.env(SMOKE_ENV, crate::data::LOAD_BUDGET.as_millis().to_string());
    c
}

/// HOW MUCH LONGER THAN ITS OWN CLOCK THE PARENT GIVES A PREFLIGHT.
///
/// A POLICY, NOT A MEASUREMENT, AND IT IS NOT DRESSED AS ONE. The child's deadline is
/// `data::LOAD_BUDGET`, which the child starts counting once it is running; the parent's has to
/// cover process creation, paging in a binary measured at 10,874,880 bytes on 2026-09-11, the
/// child's own budget, and a clean exit, on a machine that is also running a game. Four times the
/// child's budget is a ceiling above all of that and is not a prediction of any of it. If it ever
/// needs a real number, measure a cold preflight on the slowest machine a reader has and put that
/// measurement here.
pub const PREFLIGHT_GRACE: u32 = 4;

/// The production preflight: run it, and require exit code 0 WITHIN A DEADLINE.
///
/// # THE DEFECT THIS SHAPE FIXES, WHICH WEDGED THE WHOLE FEATURE
///
/// This was `smoke_command(exe).output()`, which blocks until the child exits and has no deadline
/// and no kill. The child's only exit path is `App::smoke`, and until the change that accompanies
/// this one, that was called from `App::ui` only. eframe calls `ui` while the window is visible
/// and `logic` otherwise, so a preflight whose window never becomes visible, which is what
/// happens when the reader presses Install with the game fullscreen-exclusive, over RDP, or when
/// the new window opens minimized, never reached its own clock and never exited. `output()` then
/// blocked the update worker for ever: no further checks, no further presses serviced, the
/// Settings screen frozen, and on quitting Grimoire an orphan process holding a log tail, a
/// watcher poll and a hotkey registration, with no window the reader could find.
///
/// The child half of that fix is in `App::logic`. This is the half that does not depend on the
/// child being right: a payload that does not exit is a REFUSAL, which is the correct answer to
/// "does this binary start on this machine", rather than a worker that never runs again.
///
/// # STDERR GOES TO A FILE AND NOT TO A PIPE
///
/// `output()` reads the pipes for you. Waiting on a child with a deadline means not reading them
/// while it runs, and a child that fills the pipe buffer would then block on the write, which is
/// the same hang this function exists to remove, arriving by a different road. A file has no such
/// limit, and the tail of it is what the refusal quotes.
///
/// TWO CONDITIONS THE OTHER HALF MUST HOLD UP FOR THIS TO BE SAFE, recorded here because this is
/// where they bite. When the smoke switch is set, `App::new` must NOT construct the `Updater`: a
/// preflight that polled the network or wrote `state.json` would race its own parent. And the
/// hotkey conflicts a second instance reports are non-fatal (`hotkeys.rs:29`, one failure never
/// stops the rest), so the run still exits 0.
pub struct Spawn;

impl Preflight for Spawn {
    fn smoke(&self, exe: &Path) -> Result<(), String> {
        let log = {
            let mut s = exe.as_os_str().to_owned();
            s.push(".preflight.log");
            PathBuf::from(s)
        };
        let mut cmd = smoke_command(exe);
        if let Ok(f) = std::fs::File::create(&log) {
            cmd.stderr(std::process::Stdio::from(f));
        }
        cmd.stdin(std::process::Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Err(format!("it would not start at all: {e}")),
        };

        let budget = crate::data::LOAD_BUDGET * PREFLIGHT_GRACE;
        let answer = match wait_within(&mut child, budget) {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => {
                let tail: String = std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" / ");
                Err(format!("it exited {:?}; {tail}", status.code()))
            }
            Err(why) => Err(why),
        };
        let _ = std::fs::remove_file(&log);
        answer
    }
}

/// Wait for a child, and kill it if it outstays `budget`.
///
/// A FREE FUNCTION BECAUSE IT IS THE PART WORTH A TEST. Driving [`Spawn`] itself from a test means
/// spawning the real app; this is the rule that matters and it can be pointed at any command at
/// all. The poll interval is a twentieth of a second: the preflight takes seconds, so the cost of
/// asking is nothing and a spin would be a core.
pub fn wait_within(
    child: &mut std::process::Child,
    budget: std::time::Duration,
) -> Result<std::process::ExitStatus, String> {
    let end = std::time::Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(e) => return Err(format!("it could not be waited for: {e}")),
        }
        if std::time::Instant::now() >= end {
            /* KILLED, AND THEN REAPED. A `kill` without the `wait` leaves a zombie on Unix and a
             * handle on Windows, and the point of this branch is that nothing is left behind. */
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "it did not exit within {:.1}s, so it was stopped; a preflight that draws no \
                 window never reaches its own deadline",
                budget.as_secs_f32()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/* ================================================================ installing == */

/// STEPS 6 TO 10: PREFLIGHT, COPY, RE-VERIFY, FLIP, PRUNE.
///
/// Returns the pointer that was written. AFTER STEP 9 THE UPDATE IS DONE: nothing is restarted,
/// nothing is at risk, and the app keeps running the version it is on until the reader closes it.
/// That is why there is no fight gate on this call and only on [`stage`]: the risky moment was
/// designed out rather than scheduled around.
pub fn install_app(
    l: &Layout,
    version: &Version,
    art: &Artifact,
    keys: &[&str],
    pre: &dyn Preflight,
    pulse: crate::fights::Pulse,
) -> Result<Current, Refusal> {
    /* THE FIGHT GATE IS ASKED HERE AND NOT ONLY AT THE BUTTON, for the reason `stage` gives about
     * its own: a gate written at a call site is a gate the second call site will not have. The
     * preflight below opens a real, undecorated, visible window for `data::LOAD_BUDGET` and
     * registers global hotkeys while it is up, so pressing Install mid-pull puts a second Grimoire
     * window over a fullscreen game and takes focus. The pointer flip that follows needs no gate
     * at all (nothing restarts), but the step that draws does. */
    if !super::may_start_download(pulse) {
        return Err(Refusal::NotAQuietMoment {
            pulse: pulse_word(pulse),
        });
    }

    let staged = l.staging(version).join(exe_name());
    let seal = Seal {
        size: art.size,
        sha256: &art.sha256,
        signature: &art.signature,
    };

    /* 6. THE BYTES ABOUT TO BE EXECUTED ARE THE BYTES THAT WERE SIGNED. THIS IS THE FIRST
     * STATEMENT IN THIS FUNCTION AND IT MUST STAY THAT WAY.
     *
     * THE DEFECT THIS FIXES. The only verification the staged file had was `copy_sealed` during
     * the download. Auto-download is on by default, so the payload lands in
     * `%LOCALAPPDATA%\eql-grimoire\update\staging\<version>\` unattended and then sits there until
     * the reader happens to press Install, which can be hours. `%LOCALAPPDATA%` is writable by the
     * user with no elevation, so any process running as that user can replace the staged file in
     * that window. The preflight then SPAWNED it: the attacker's code ran, with this app's
     * privileges and its inherited environment, for as long as it liked. Restoring the real bytes
     * before step 7 made every later check pass, the pointer flip, and nothing anywhere recorded
     * that anything had happened. The `check_file` at step 8 was, and is, a check on the COPY;
     * against a swap it was a post-mortem rather than a gate, because the code had already run.
     *
     * ONE STREAMED PASS OVER A FILE MEASURED AT 10,874,880 BYTES, with no manifest and no network:
     * the `.minisig` beside the payload and the seal out of the offer are all it uses. The second
     * `check_file` at step 8 is NOT made redundant by this one and neither replaces the other:
     * this proves what is about to run, that one proves the copy landed intact. */
    super::verify::check_file(&staged, seal, keys)?;

    /* 7. WOULD IT START AT ALL? A missing runtime, a GPU driver the new eframe rejects, an
     * antivirus quarantine. Catching it here means the pointer never flips, which is strictly
     * better than flipping it and finding out at the next launch. */
    pre.smoke(&staged).map_err(|why| Refusal::PreflightFailed {
        exe: staged.clone(),
        why,
    })?;

    /* 8. THE SLOW, INTERRUPTIBLE COPY HAPPENS TO A NAME NOTHING LAUNCHES, and the rename is the
     * only step that makes it real. A rename within one directory on NTFS is atomic, so a
     * half-copied executable is never launchable.
     *
     * `copy_durably` AND NOT `std::fs::copy`, which is the difference between a file that exists
     * and a file that is on the disk. See that function: the pointer written at step 10 is flushed
     * and this was not, so a power loss ten seconds after an install left NTFS holding a directory
     * entry, a size, and extents nobody wrote, with `current.json` naming it. `plan` sees a file,
     * execs it, Windows refuses the image, and on a first update there is no previous to fall back
     * to, so the reader double-clicks the shortcut twice and nothing at all happens. */
    let dir = l.version_dir(version);
    std::fs::create_dir_all(&dir).map_err(|e| io("create", &dir, &e))?;
    let installed = l.exe(version);
    let incoming = {
        let mut s = installed.as_os_str().to_owned();
        s.push(".incoming");
        PathBuf::from(s)
    };
    copy_durably(&staged, &incoming)?;
    std::fs::rename(&incoming, &installed).map_err(|e| io("rename into place", &installed, &e))?;
    copy_durably(&sig_of(&staged), &sig_of(&installed))?;
    sync_dir(&dir);

    /* 9. RE-VERIFY FROM DISK: size, sha256, signature, with no manifest and no network. This is
     * why the `.minisig` travels with the binary. It is the same call as step 6 against a
     * different file, and it answers a different question: step 6 asked whether the bytes about to
     * run are the signed ones, and this asks whether the copy that just landed is intact. */
    super::verify::check_file(&installed, seal, keys)?;

    /* 10. THE FLIP. Temp file, flush, rename: the same discipline `settings.rs:611` uses and for
     * the same reason. `current.json` is either entirely the old pointer or entirely the new one,
     * and a torn temp file is never named `current.json`. */
    let was = read_current(l);
    let next = Current {
        pointer: POINTER_VERSION,
        version: version.to_string(),
        exe: installed,
        /* THE SEAL TRAVELS INTO THE POINTER, which is what lets the trampoline check the payload
         * before it execs it. Both numbers are inside the signed manifest, so recording them is
         * recording a publisher's claim rather than inventing one. See `launch::plan`. */
        size: Some(art.size),
        sha256: Some(art.sha256.clone()),
        previous: was.as_ref().map(|c| c.version.clone()),
        installed: Utc::now(),
        launches_failed: 0,
        last_launch_started: None,
    };
    write_current(l, &next)?;

    /* 11. TIDY. Exactly two versions are kept, because the second one is the rollback and a third
     * has no job. Failures here are not failures of the update, which is already done. */
    let _ = std::fs::remove_dir_all(l.staging(version));
    let _ = prune(l, &next);
    Ok(next)
}

/// Remove every version directory that is neither the current one nor its rollback.
pub fn prune(l: &Layout, keep: &Current) -> Result<Vec<String>, Refusal> {
    let app = l.app_dir();
    let Ok(entries) = std::fs::read_dir(&app) else {
        return Ok(Vec::new());
    };
    let mut removed = Vec::new();
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name == keep.version || keep.previous.as_deref() == Some(name.as_str()) {
            continue;
        }
        match std::fs::remove_dir_all(e.path()) {
            Ok(()) => removed.push(name),
            /* A version directory that will not delete is a locked file, not a broken update, and
             * the next prune will get it. Reporting it as a failed install would be a lie. */
            Err(_) => continue,
        }
    }
    removed.sort();
    Ok(removed)
}

/* ================================================================ the pointer == */

/// Read `app\current.json`.
///
/// `None` FOR EVERY KIND OF FAILURE, INCLUDING AN UNPARSABLE FILE, and that is the right answer
/// rather than a lazy one: the caller is the trampoline, `None` means [`super::launch::Plan::RunHere`],
/// and running the binary that is already loaded is the safe answer to "I cannot tell". A
/// `Result` here would invite a `?` on the launch path, which is an app that refuses to start
/// because a JSON file is malformed.
pub fn read_current(l: &Layout) -> Option<Current> {
    let raw = std::fs::read_to_string(l.current_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write `app\current.json` through a temp file and a rename, with nobody else writing it.
///
/// THE LOCK IS NOT ABOUT THREADS, it is about the second copy of the app. See [`with_lock`].
pub fn write_current(l: &Layout, c: &Current) -> Result<(), Refusal> {
    with_lock(&l.current_path(), || write_current_inner(l, c))
}

/// [`write_current`] with the lock already held by the caller, which is what the read-modify-write
/// callers below need: taking it twice would be a writer waiting on itself.
fn write_current_inner(l: &Layout, c: &Current) -> Result<(), Refusal> {
    let path = l.current_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| io("create", d, &e))?;
    }
    let body = serde_json::to_vec_pretty(c).map_err(|e| Refusal::Io {
        doing: "encode the pointer for",
        path: path.clone(),
        why: e.to_string(),
    })?;
    let tmp = temp_beside(&path);
    /* FLUSHED TO THE DEVICE BEFORE THE RENAME. The rename is what makes the new pointer real,
     * and a rename that lands ahead of the bytes it names is the power-loss case this whole
     * design exists to remove. A failed write removes its own temp file: every writer's name is
     * its own now, so nothing else would ever overwrite it. */
    if let Err(e) = write_durably(&tmp, &body) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io("rename into place", &path, &e));
    }
    if let Some(d) = path.parent() {
        sync_dir(d);
    }
    Ok(())
}

/// The trampoline's own bookkeeping, written immediately before `spawn`.
///
/// # THE READ AND THE WRITE ARE ONE CRITICAL SECTION
///
/// THE DEFECT THIS FIXES. Three shortcut double-clicks in quick succession each read a count the
/// previous one had not written yet, so three launches recorded one failure between them, or the
/// third computed its plan against a count two others had already moved. The count is what the
/// automatic rollback reads, so getting it wrong in either direction is either a broken build that
/// is never rolled back or a healthy one that is. The lock is what makes "increment" mean
/// increment across processes; see [`with_lock`].
pub fn note_launch_started(l: &Layout) -> Result<(), Refusal> {
    with_lock(&l.current_path(), || {
        let Some(mut c) = read_current(l) else {
            return Ok(());
        };
        c.launches_failed = c.launches_failed.saturating_add(1);
        c.last_launch_started = Some(Utc::now());
        write_current_inner(l, &c)
    })
}

/// Cleared by the launched app ON ITS FIRST COMPLETED FRAME.
///
/// NOT AT STARTUP. A glow or wgpu failure happens DURING that first frame, so clearing the count
/// before it would call a crash a success and the automatic rollback would never fire.
///
/// THE RACE, AND WHY IT DOES NOT MATTER. The parent writes the count before `spawn` and the child
/// clears it on its first frame. The parent has exited long before a frame is drawn, and both
/// writes are temp-plus-rename, so the worst case is a lost clear and one spurious increment. Two
/// increments are needed to act, so a lost clear costs nothing.
pub fn note_first_frame(l: &Layout) -> Result<(), Refusal> {
    with_lock(&l.current_path(), || {
        let Some(mut c) = read_current(l) else {
            return Ok(());
        };
        if c.launches_failed == 0 && c.last_launch_started.is_none() {
            return Ok(());
        }
        c.launches_failed = 0;
        c.last_launch_started = None;
        write_current_inner(l, &c)
    })
}

/// What a rollback did.
#[derive(Clone, Debug)]
pub struct RolledBack {
    /// The pointer now in force.
    pub now: Current,
    /// The version that was left behind, so the caller can write it into `state.json` and decline
    /// to offer it again until a manifest names something above it.
    pub away_from: String,
}

/// Point `current.json` back at `previous`.
///
/// `previous` BECOMES `None`, WHICH IS NOT AN OVERSIGHT. After a rollback the version just left
/// is the last thing this machine should return to, so it must not sit in the field whose whole
/// job is naming where to go next. It is returned instead, for the caller to record.
pub fn roll_back(l: &Layout) -> Result<RolledBack, Refusal> {
    with_lock(&l.current_path(), || {
        let Some(c) = read_current(l) else {
            return Err(Refusal::NoPreviousVersion);
        };
        let Some(previous) = c.previous.clone() else {
            return Err(Refusal::NoPreviousVersion);
        };
        let Ok(to) = Version::parse(&previous) else {
            return Err(Refusal::NoPreviousVersion);
        };
        let exe = l.exe(&to);
        if !exe.is_file() {
            /* A pointer naming a version that is not on disk is the same situation as having no
             * previous at all, and saying so is better than writing a pointer at nothing. */
            return Err(Refusal::NoPreviousVersion);
        }
        /* THE SEAL FOR THE VERSION BEING GONE BACK TO IS NOT IN THIS FILE, because the pointer
         * only ever carries the seal of the version it names and this one names a different
         * version now. It is read back off disk instead: `sealed_from_disk` recomputes the size
         * and the hash of the payload that is actually there, and the `.minisig` beside it is what
         * the signature check at launch uses, so the answer is still "these bytes are the ones a
         * key signed" rather than "these bytes are what some file says they are". */
        let (size, sha256) = match sealed_from_disk(&exe) {
            Some((s, h)) => (Some(s), Some(h)),
            None => (None, None),
        };
        let now = Current {
            pointer: POINTER_VERSION,
            version: previous,
            exe,
            size,
            sha256,
            previous: None,
            installed: c.installed,
            launches_failed: 0,
            last_launch_started: None,
        };
        write_current_inner(l, &now)?;
        Ok(RolledBack {
            now,
            away_from: c.version,
        })
    })
}

/// The size and sha256 of a file that is already on disk.
///
/// USED ONLY WHERE THE SIGNED FIGURES ARE NOT TO HAND, which is the rollback: the pointer carries
/// the seal of the version it names, and a rollback names a different one. This is NOT a substitute
/// for the manifest's numbers and cannot be: a hash computed from the same bytes that are being
/// checked proves only that nothing changed BETWEEN now and the next launch. What makes that worth
/// having is the `.minisig` beside the payload, which is a publisher's signature over those exact
/// bytes and is what `launch::plan` actually verifies; these two numbers are the cheap part of the
/// check, not the trust in it.
fn sealed_from_disk(exe: &Path) -> Option<(u64, String)> {
    use sha2::{Digest, Sha256};
    let size = std::fs::metadata(exe).ok()?.len();
    let mut f = std::fs::File::open(exe).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).ok()?;
    let sha = h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    });
    Some((size, sha))
}

/* =================================================================== the data == */

/// What a tar entry is, in the only terms the path guard cares about.
///
/// OUR OWN ENUM AND NOT `tar::EntryType`, so that the guard is a pure function with no archive
/// dependency behind it. The extractor maps one onto the other in the half that reads archives;
/// `tar` and `flate2` are not manifest entries yet, because a dependency that arrives before its
/// caller is a defect this tree's `Cargo.toml` apologises for twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Hardlink,
    Other,
}

/// WHERE ONE ARCHIVE ENTRY IS ALLOWED TO LAND, OR A REFUSAL.
///
/// THIS IS THE ONE PLACE IN THE FEATURE WHERE ATTACKER-CHOSEN STRINGS BECOME FILESYSTEM PATHS,
/// and the organisation has already found and fixed a ZIP-SLIP remote code execution in another
/// tree, so this is not a hypothetical. Extraction is never `Archive::unpack`: every entry comes
/// through here, and an entry that fails ANY of these aborts the whole extraction rather than
/// being skipped, because an archive with one hostile entry is a hostile archive.
///
/// The rules, and what each one stops:
///   * empty, or a `..` component: climbing out of the target directory.
///   * an absolute path or a root component: writing to `\Windows\System32` outright.
///   * a drive prefix or a verbatim prefix (`C:`, `\\?\`, `\\server\share`): the same, in the
///     spellings `Path::is_absolute` alone does not always catch on a cross-platform build.
///   * a literal backslash anywhere: on Unix that is a filename character, so `..\..\evil` is one
///     component there and three on Windows. Our bundles never contain one.
///   * a colon anywhere: `file:stream` is an NTFS alternate data stream, which is a write to a
///     place a directory listing does not show.
///   * a symlink or hardlink entry: the path is checked, and then the LINK TARGET would not be.
///     There is nothing in a snapshot bundle that needs one.
///   * and finally the joined result must still start with the target, which is the check that
///     does not care whether the list above was complete.
pub fn safe_entry_path(target: &Path, entry: &str, kind: EntryKind) -> Result<PathBuf, Refusal> {
    let no = |why: &'static str| Refusal::UnsafeArchiveEntry {
        entry: entry.to_owned(),
        why,
    };
    match kind {
        EntryKind::File | EntryKind::Directory => {}
        EntryKind::Symlink => return Err(no("it is a symbolic link")),
        EntryKind::Hardlink => return Err(no("it is a hard link")),
        EntryKind::Other => return Err(no("it is not a plain file or directory")),
    }
    if entry.is_empty() {
        return Err(no("it has no name"));
    }
    if entry.contains('\\') {
        return Err(no("it contains a backslash"));
    }
    if entry.contains(':') {
        return Err(no("it contains a colon"));
    }
    let rel = Path::new(entry);
    if rel.is_absolute() {
        return Err(no("it is an absolute path"));
    }
    let mut out = target.to_path_buf();
    for c in rel.components() {
        match c {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return Err(no("it climbs out with ..")),
            std::path::Component::RootDir => return Err(no("it starts at the root")),
            std::path::Component::Prefix(_) => return Err(no("it names a drive or a share")),
        }
    }
    /* THE CHECK THAT DOES NOT DEPEND ON THE LIST ABOVE BEING COMPLETE. Lexical, because the file
     * does not exist yet and `canonicalize` would fail; the component walk above is what makes a
     * lexical answer sound, since nothing that survives it can contain a `..`. */
    if !out.starts_with(target) || out == target {
        return Err(no(
            "it does not land inside the folder being extracted into",
        ));
    }
    Ok(out)
}

/// Does an extracted tree hold everything the loader needs?
///
/// THE SAME PROBE `data::Snapshot::locate` USES, asked before the swap rather than after. A bundle
/// missing one file would otherwise be installed and then fail at load time on a file name the
/// reader cannot act on, having already replaced a snapshot that worked.
pub fn bundle_is_complete(dir: &Path) -> Result<(), Refusal> {
    for f in crate::data::FILES {
        if !dir.join(f).is_file() {
            return Err(Refusal::BundleIncomplete {
                missing: (*f).to_owned(),
            });
        }
    }
    if !dir.join(crate::data::ATLAS_DIR).is_dir() {
        return Err(Refusal::BundleIncomplete {
            missing: crate::data::ATLAS_DIR.to_owned(),
        });
    }
    Ok(())
}

/// Put `data.incoming\` in place, keeping one generation back.
///
/// # THE GAP BETWEEN THE TWO RENAMES IS ACCEPTABLE HERE AND WAS NOT ACCEPTABLE FOR THE BINARY
///
/// That gap is exactly what made the two-rename executable swap unusable (see
/// [`super::launch`]), and the difference is not the odds, it is what goes missing. A missing
/// `data\` produces `Data::Absent`, which is a DESIGNED state: it prints the candidate list and
/// tells the reader where to put the files. A missing executable produces nothing at all, because
/// every recovery path begins with running a program. On top of that this gap self-heals, which
/// is [`heal_missing_data`]; a missing exe has no such recovery.
pub fn swap_data(l: &Layout) -> Result<(), Refusal> {
    let incoming = l.data_incoming();
    bundle_is_complete(&incoming)?;
    let previous = l.data_previous();
    if previous.exists() {
        std::fs::remove_dir_all(&previous).map_err(|e| io("remove", &previous, &e))?;
    }
    let live = l.data_dir();
    if live.exists() {
        std::fs::rename(&live, &previous).map_err(|e| io("set aside", &live, &e))?;
    }
    std::fs::rename(&incoming, &live).map_err(|e| io("move into place", &incoming, &e))
}

/// THE SELF-HEAL, RUN AT STARTUP. Returns whether it did anything.
///
/// If `data\` is absent and `data.previous\` is there, the previous generation is put back. This
/// is what closes the window in [`swap_data`], and it is the reason that window is acceptable at
/// all.
pub fn heal_missing_data(l: &Layout) -> Result<bool, Refusal> {
    let live = l.data_dir();
    let previous = l.data_previous();
    if live.exists() || !previous.exists() {
        return Ok(false);
    }
    std::fs::rename(&previous, &live).map_err(|e| io("put back", &previous, &e))?;
    Ok(true)
}

/// The reader's own "put the previous data back" button.
///
/// A BUTTON AND NOT AN AUTOMATIC ACTION, deliberately. Rolling the snapshot back on its own would
/// fight the reader's `data_root` override, which the loader treats as an instruction rather than
/// a guess (`data/mod.rs:423`).
pub fn restore_previous_data(l: &Layout) -> Result<(), Refusal> {
    let previous = l.data_previous();
    if !previous.is_dir() {
        return Err(Refusal::NoPreviousVersion);
    }
    let live = l.data_dir();
    let aside = l.data_incoming();
    if aside.exists() {
        std::fs::remove_dir_all(&aside).map_err(|e| io("remove", &aside, &e))?;
    }
    if live.exists() {
        std::fs::rename(&live, &aside).map_err(|e| io("set aside", &live, &e))?;
    }
    std::fs::rename(&previous, &live).map_err(|e| io("put back", &previous, &e))?;
    /* The generation just displaced becomes the one to go back to, so the button is its own
     * undo rather than a one-way door. */
    std::fs::rename(&aside, &previous).map_err(|e| io("keep", &aside, &e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::Pulse;
    use crate::updater::manifest::{this_arch, this_os, KIND_APP};
    use crate::updater::verify::probe::Signer;
    use sha2::{Digest, Sha256};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grimoire-updater-install-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch folder");
        d
    }

    fn v(s: &str) -> Version {
        Version::parse(s).expect("a literal version")
    }

    /// An app artifact whose seal really describes `bytes`.
    fn sealed(signer: &Signer, version: &str, bytes: &[u8]) -> Artifact {
        let mut h = Sha256::new();
        h.update(bytes);
        let sha = h
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Artifact {
            kind: KIND_APP.to_owned(),
            os: this_os().to_owned(),
            arch: this_arch().to_owned(),
            version: version.to_owned(),
            url: "https://example.invalid/app".to_owned(),
            size: bytes.len() as u64,
            sha256: sha,
            signature: signer.sign(bytes),
            requires_data: None,
            min_app: None,
            extra: serde_json::Map::new(),
        }
    }

    struct Yes;
    impl Preflight for Yes {
        fn smoke(&self, _exe: &Path) -> Result<(), String> {
            Ok(())
        }
    }
    struct No;
    impl Preflight for No {
        fn smoke(&self, _exe: &Path) -> Result<(), String> {
            Err("it exited Some(101)".to_owned())
        }
    }

    /// A connection that delivers `upto` bytes and then drops, which is the commonest failure a
    /// download has and the one no signature can distinguish from an attack on its own.
    struct Dropped<'a> {
        bytes: &'a [u8],
        upto: usize,
        sent: usize,
    }
    impl Read for Dropped<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.sent >= self.upto {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "the connection was reset",
                ));
            }
            let n = buf.len().min(self.upto - self.sent).min(4);
            buf[..n].copy_from_slice(&self.bytes[self.sent..self.sent + n]);
            self.sent += n;
            Ok(n)
        }
    }

    /// DEFECT THIS PREVENTS: A REFUSED DOWNLOAD LEAVING BYTES BEHIND THAT LOOK LIKE PROGRESS.
    ///
    /// Nothing ever reads a `.part`, so a half-written payload cannot be installed. What it CAN
    /// do is sit on the disk forever, and worse, a `.part` that a later version of this code
    /// decided to resume would resume bytes that failed a signature check. Deleting it before the
    /// refusal is reported is the only ordering in which no caller can see both.
    ///
    /// AND A DROPPED CONNECTION IS ITS OWN CAUSE. It is the commonest failure a download has and
    /// the one that must never be dressed up as either of the other two: a reset connection
    /// reported as a bad signature sends a reader hunting an attacker, and reported as a size
    /// mismatch it blames the publisher for the train going into a tunnel.
    ///
    /// WHAT MUTATION MAKES THIS RED: move the `remove_file(&part)` after the `return Err`, or
    /// delete it; rename the `.part` into place before the checks rather than after; or answer a
    /// read error with `break` instead of a named `Refusal::Io`, which turns every dropped
    /// connection into an `ArtifactSizeMismatch`.
    #[test]
    fn a_refused_download_leaves_nothing_staged() {
        let l = Layout::at(scratch("staging"));
        let signer = Signer::new();
        let keys = [signer.public_b64.as_str()];
        let body = b"a payload that is exactly this long".to_vec();
        let art = sealed(&signer, "0.2.0", &body);
        let ver = v("0.2.0");

        /* Corrupt bytes with an honest seal. */
        let mut corrupt = body.clone();
        corrupt[0] ^= 1;
        let no = stage(
            &l,
            &ver,
            &art,
            &corrupt[..],
            Pulse::Closed,
            &keys,
            &mut |_| {},
        )
        .expect_err("corrupt bytes");
        assert_eq!(no.code(), "ArtifactHashMismatch");
        let left: Vec<String> = std::fs::read_dir(l.staging(&ver))
            .expect("the staging folder")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "a refused download left {left:?} behind in staging"
        );

        /* A CONNECTION THAT DROPS HALFWAY, which is what actually happens on a train. It is a
         * failure of the network and must read as one: reporting a reset connection as a bad
         * signature would send a reader hunting an attacker, and reporting it as a size mismatch
         * would blame the publisher. Nothing is left staged either way. */
        let no = stage(
            &l,
            &ver,
            &art,
            Dropped {
                bytes: &body,
                upto: 12,
                sent: 0,
            },
            Pulse::Closed,
            &keys,
            &mut |_| {},
        )
        .expect_err("the connection dropped");
        assert_eq!(no.code(), "Io", "a dropped connection read as {no}");
        assert!(
            no.to_string().contains("reset"),
            "the refusal must carry what the network said, got {no}"
        );
        let left: Vec<String> = std::fs::read_dir(l.staging(&ver))
            .expect("the staging folder")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "a dropped download left {left:?} behind in staging"
        );

        /* AND THE PARTIAL FILE IS THIS PROCESS'S OWN, WHICH IS WHAT MAKES TWO WRITERS SAFE.
         *
         * Nothing stops two copies of the app running, both get the same staging folder, and
         * `auto_download` is on by default, so two `File::create` calls on one fixed name and two
         * write loops interleaving into it is a thing this app could do to itself. Both hashes
         * then fail, both are recorded, and the reader is told the bytes on the server are not the
         * bytes the manifest named, about a release the pipeline will not republish. With a
         * per-writer name each loop owns its file and the rename is the arbitration.
         *
         * WHAT MUTATION MAKES THIS RED: put the fixed `.part` name back. */
        /* THE NAME IS OBSERVED WHILE THE DOWNLOAD IS RUNNING, through the progress callback, which
         * is the only moment the partial file exists: `stage` deletes it on a failure and renames
         * it on a success, so there is nothing to look at afterwards either way. This is the
         * production name and not one the test constructed. */
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        stage(&l, &ver, &art, &body[..], Pulse::Closed, &keys, &mut |_| {
            if seen.borrow().is_empty() {
                *seen.borrow_mut() = std::fs::read_dir(l.staging(&ver))
                    .expect("the staging folder")
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
            }
        })
        .expect("the honest download stages");
        let partial = seen
            .borrow()
            .iter()
            .find(|n| n.ends_with(".part"))
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "no partial file was on disk while it was being written: {:?}",
                    seen.borrow()
                )
            });
        assert!(
            partial.contains(&std::process::id().to_string()),
            "the partial file is named {partial:?}, which every copy of this app would open and \
             write into at once; two interleaved write loops both fail their hash and both are \
             recorded against a release the pipeline will not republish"
        );

        /* The honest one lands, with its signature beside it, and the signature is written
         * FIRST so a crash never leaves a payload with no proof. */
        let p = stage(&l, &ver, &art, &body[..], Pulse::Closed, &keys, &mut |_| {})
            .expect("the honest download stages");
        assert_eq!(
            p.file_name().and_then(|n| n.to_str()),
            Some(exe_name().as_str()),
            "the file that lands is named after the payload and not after the writer; only the \
             PARTIAL name carries the process id"
        );
        assert_eq!(std::fs::read(&p).expect("the staged file"), body);
        assert_eq!(
            std::fs::read_to_string(sig_of(&p)).expect("the signature beside it"),
            art.signature
        );
    }

    /// DEFECT THIS PREVENTS: THE UPDATER PULLING TEN MEGABYTES THROUGH THE MIDDLE OF A PULL.
    ///
    /// # THIS DRIVES THE REAL FOLD AND NOT A `Pulse` VALUE A TEST INVENTED
    ///
    /// `still_going` has already shipped one false positive (`ingest.rs:1969-1988`: it tested only
    /// `last.ended != "the log stopped"`, which the aggregator makes almost always true, so
    /// `IN COMBAT` burned over a corpse after every kill). A test that set `live_open` by hand
    /// would have passed against that defect and against its fix, and would prove nothing about
    /// the code that actually answers. So this plants a log, boots an `Ingest` off it exactly as
    /// `App::new` does, and asks the real `pulse()`.
    ///
    /// The three logs are the three states, separated by the log's own clock: a pull whose last
    /// line is a blow, the same pull with chat past the combat window, and the same pull with
    /// chat past the hold window as well.
    ///
    /// WHAT MUTATION MAKES THIS RED: gate `stage` on `!ingest.fight_is_live()` (that is
    /// `!pulse().fighting()`, so the HOLDING case starts downloading); or delete the gate.
    #[test]
    fn a_download_waits_for_the_encounter_to_close() {
        use crate::fights::{probe, COMBAT_SECONDS, HOLD_SECONDS};

        let at = |s: i64| format!("[Wed Jul 15 23:{:02}:{:02} 2026]", 10 + s / 60, s % 60);
        let blow = |s: i64| {
            format!(
                "{} You slash a dry bone skeleton for 20 points of damage.",
                at(s)
            )
        };
        let chat = |s: i64| format!("{} Losumyda says, 'oom'", at(s));
        let log = |after: Option<i64>| {
            let mut s = format!("{}\n{}\n", blow(0), blow(1));
            if let Some(gap) = after {
                s.push_str(&format!("{}\n", chat(1 + gap)));
            }
            s
        };

        let l = Layout::at(scratch("fight-gate"));
        let signer = Signer::new();
        let keys = [signer.public_b64.as_str()];
        let body = b"never downloaded".to_vec();
        let art = sealed(&signer, "0.2.0", &body);

        let cases = [
            ("upd-fighting", None, Pulse::Fighting, false),
            (
                "upd-holding",
                Some(COMBAT_SECONDS + 1),
                Pulse::Holding,
                false,
            ),
            (
                "upd-closed",
                Some(COMBAT_SECONDS + HOLD_SECONDS + 2),
                Pulse::Closed,
                true,
            ),
        ];
        for (tag, after, want, may) in cases {
            let dir = probe::planted(tag, &log(after));
            let ing = probe::booted(&dir);
            assert_eq!(
                ing.pulse(),
                want,
                "{tag}: the real fold answered {:?}, so this case is not testing what it says",
                ing.pulse()
            );
            assert_eq!(
                super::super::may_start_download(ing.pulse()),
                may,
                "{tag}: the gate disagrees with the encounter"
            );
            let got = stage(
                &l,
                &v("0.2.0"),
                &art,
                &body[..],
                ing.pulse(),
                &keys,
                &mut |_| {},
            );
            if may {
                got.expect("a closed encounter is a moment to download");
            } else {
                let no = got.expect_err("a download started during a fight");
                assert_eq!(no.code(), "NotAQuietMoment", "{tag}");
            }
        }
    }

    /// DEFECT THIS PREVENTS: A BINARY THAT WILL NOT START BEING MADE THE ONE THAT RUNS.
    ///
    /// The preflight is the difference between finding out now, with the pointer untouched, and
    /// finding out at the next launch with the pointer already flipped. Its failure must leave
    /// `current.json` exactly as it was, which is the assertion below: not just that the call
    /// returns an error, but that nothing moved.
    ///
    /// WHAT MUTATION MAKES THIS RED: ignore the preflight result (`let _ = pre.smoke(..)`), or
    /// move the pointer write ahead of it.
    #[test]
    fn a_payload_that_will_not_start_never_becomes_the_pointer() {
        let l = Layout::at(scratch("preflight"));
        let signer = Signer::new();
        let keys = [signer.public_b64.as_str()];
        let body = b"MZ a payload".to_vec();
        let art = sealed(&signer, "0.2.0", &body);
        let ver = v("0.2.0");
        stage(&l, &ver, &art, &body[..], Pulse::Closed, &keys, &mut |_| {}).expect("staged");

        let no =
            install_app(&l, &ver, &art, &keys, &No, Pulse::Closed).expect_err("it does not start");
        assert_eq!(no.code(), "PreflightFailed");
        assert!(
            read_current(&l).is_none(),
            "the pointer was written for a build that would not start"
        );

        /* AND THE INSTALL IS AS MUCH A MOMENT AS THE DOWNLOAD IS. The preflight opens a real,
         * visible window for `data::LOAD_BUDGET`, so pressing Install mid-pull puts a second
         * Grimoire over a fullscreen game. The gate is asked here and not only at the button, for
         * the reason `stage` gives about its own. */
        let no = install_app(&l, &ver, &art, &keys, &Yes, Pulse::Fighting)
            .expect_err("an install started during a fight");
        assert_eq!(no.code(), "NotAQuietMoment");
        assert!(
            read_current(&l).is_none(),
            "the pointer moved for an install that was refused"
        );

        /* And the same inputs with a preflight that passes DO flip the pointer, so the assertion
         * above is about the preflight and not about the fixture. */
        let c = install_app(&l, &ver, &art, &keys, &Yes, Pulse::Closed).expect("it installs");
        assert_eq!(c.version, "0.2.0");
        assert_eq!(read_current(&l).expect("a pointer").version, "0.2.0");
    }

    /// DEFECT THIS PREVENTS: AN INSTALL THAT CANNOT BE UNDONE, AND A ROLLBACK THAT POINTS AT
    /// NOTHING.
    ///
    /// The previous version is the whole of the recovery story for a build that starts and cannot
    /// draw, so two things have to hold: the prune must never delete it, and a rollback must
    /// refuse rather than write a pointer at a directory that is not there.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `keep.previous` arm of the prune (the rollback
    /// target is deleted the moment it is named); carry `previous` forward in `roll_back` (the
    /// machine bounces back to the build it just fled); or drop the `exe.is_file()` check (the
    /// pointer names a version nobody installed).
    #[test]
    fn the_previous_version_survives_the_prune_and_is_where_a_rollback_goes() {
        let l = Layout::at(scratch("rollback"));
        let signer = Signer::new();
        let keys = [signer.public_b64.as_str()];

        let mut installed = Vec::new();
        for ver in ["0.1.0", "0.2.0", "0.3.0"] {
            let body = format!("payload for {ver}").into_bytes();
            let art = sealed(&signer, ver, &body);
            let vv = v(ver);
            stage(&l, &vv, &art, &body[..], Pulse::Closed, &keys, &mut |_| {}).expect("staged");
            installed
                .push(install_app(&l, &vv, &art, &keys, &Yes, Pulse::Closed).expect("installed"));
        }
        let now = installed.last().expect("three installs");
        assert_eq!(now.version, "0.3.0");
        assert_eq!(now.previous.as_deref(), Some("0.2.0"));

        let mut left: Vec<String> = std::fs::read_dir(l.app_dir())
            .expect("app dir")
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec!["0.2.0".to_owned(), "0.3.0".to_owned()],
            "exactly two versions are kept: the current one and its rollback"
        );

        let back = roll_back(&l).expect("there is somewhere to go");
        assert_eq!(back.away_from, "0.3.0");
        assert_eq!(back.now.version, "0.2.0");
        assert_eq!(
            back.now.previous, None,
            "the version just fled must not be where the next rollback goes"
        );
        assert_eq!(back.now.launches_failed, 0);
        assert_eq!(read_current(&l).expect("a pointer").version, "0.2.0");

        /* And with nowhere left to go it refuses rather than writing a pointer at nothing. */
        assert_eq!(
            roll_back(&l).expect_err("no previous").code(),
            "NoPreviousVersion"
        );
    }

    /// DEFECT THIS PREVENTS: A CRASH DURING A LAUNCH BEING COUNTED AS A SUCCESS.
    ///
    /// The count is what the automatic rollback reads. It is incremented before `spawn` and
    /// cleared on the FIRST COMPLETED FRAME, not at startup, because a glow or wgpu failure
    /// happens during that frame.
    ///
    /// AND THE COUNT SURVIVES SEVERAL PROCESSES DOING IT AT ONCE. The read and the write are one
    /// critical section under a lock file, because nothing stops two copies of the app running and
    /// three shortcut double-clicks in quick succession each used to read a count the previous one
    /// had not written yet. A count that loses increments is a broken build that is never rolled
    /// back; a count that gains them is a healthy build that is.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `note_first_frame` run unconditionally at startup in
    /// the caller (this test cannot see that); have `note_launch_started` overwrite rather than
    /// increment, which caps the count at one and means the rollback never fires; or drop the
    /// `with_lock` around either of them, which loses increments the moment two writers overlap.
    #[test]
    fn failed_launches_accumulate_and_a_drawn_frame_clears_them() {
        let l = Layout::at(scratch("launch-count"));
        let c = Current {
            pointer: POINTER_VERSION,
            version: "0.2.0".to_owned(),
            exe: l.exe(&v("0.2.0")),
            size: None,
            sha256: None,
            previous: Some("0.1.0".to_owned()),
            installed: Utc::now(),
            launches_failed: 0,
            last_launch_started: None,
        };
        write_current(&l, &c).expect("the first pointer");

        note_launch_started(&l).expect("a launch begins");
        assert_eq!(read_current(&l).expect("pointer").launches_failed, 1);
        note_launch_started(&l).expect("and another");
        let two = read_current(&l).expect("pointer");
        assert_eq!(two.launches_failed, 2);
        assert!(two.last_launch_started.is_some());
        assert_eq!(
            two.previous.as_deref(),
            Some("0.1.0"),
            "the rollback target must survive the bookkeeping"
        );

        note_first_frame(&l).expect("a frame was drawn");
        let clear = read_current(&l).expect("pointer");
        assert_eq!(clear.launches_failed, 0);
        assert_eq!(clear.last_launch_started, None);

        /* THREE WRITERS, EIGHT INCREMENTS EACH, TWENTY FOUR EXPECTED.
         *
         * THREADS STAND IN FOR PROCESSES AND THE LOCK CANNOT TELL THE DIFFERENCE: it is a file
         * created with `create_new`, so two threads race it exactly as two processes do.
         *
         * THE NUMBERS ARE SMALL ON PURPOSE, AND THAT IS NOT TIMIDITY. Each critical section ends
         * in an `fsync`, and `LOCK_PATIENCE` takes the lock from a holder that has had it for five
         * seconds, because in production that is a process that died. A test that queued hundreds
         * of `fsync`s behind one lock, on a machine already running the whole suite across every
         * core, could reach that patience honestly and lose an update for a reason that is not a
         * defect. Twenty four overlapping read-modify-writes is far past what an unlocked version
         * survives (measured against the unlocked code below, which loses several every run) and
         * nowhere near what a loaded machine cannot finish inside the patience. */
        let writers: Vec<_> = (0..3)
            .map(|_| {
                let root = l.root().to_path_buf();
                std::thread::spawn(move || {
                    let mine = Layout::at(root);
                    for _ in 0..8 {
                        note_launch_started(&mine).expect("a launch begins");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("a writer finished");
        }
        assert_eq!(
            read_current(&l).expect("pointer").launches_failed,
            24,
            "increments were lost, so two copies of the app writing this file at once can make a \
             broken build look healthy or a healthy one look broken"
        );
    }

    /// DEFECT THIS PREVENTS: THE LOCK TAKEN FROM A HOLDER THAT IS ALIVE AND WORKING.
    ///
    /// Patience used to be measured from when the WAITER started waiting. A queue of writers each
    /// holding the lock briefly keeps a late waiter out for longer than that without any one holder
    /// being slow, and the waiter then took the lock from a live holder: two writers inside one
    /// critical section, both writing the same temp file, and one rename failing with "No such file
    /// or directory". Measured on the Linux CI runner under a loaded suite, as a failure of
    /// `failed_launches_accumulate_and_a_drawn_frame_clears_them`. What says a holder died is ONE
    /// TOKEN IN ITS LOCK FILE FOR THE WHOLE PATIENCE, which a busy queue keeps replacing and a dead
    /// holder never does.
    ///
    /// Here patience is one second and each of 64 sections holds the lock for 25 ms, so the late
    /// writers wait well past the patience behind holders whose token never stays for more than
    /// 25 ms. Run while the wall clock is stepped, this is also the test that caught the file time
    /// rule taking the lock from live holders.
    ///
    /// WHAT MUTATION MAKES THIS RED: measure patience from the waiter's own start again, or judge a
    /// lock by its file's modification time while the clock is stepped.
    #[test]
    fn a_queue_of_live_holders_never_has_the_lock_taken_from_under_it() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let path = scratch("live-queue").join("guarded.json");
        let patience = std::time::Duration::from_secs(1);
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let writers: Vec<_> = (0..8)
            .map(|_| {
                let (path, inside, most) = (path.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        with_lock_patient(&path, patience, || {
                            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                            most.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            inside.fetch_sub(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .expect("the section ran");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("a writer finished");
        }
        assert_eq!(
            most.load(Ordering::SeqCst),
            1,
            "two writers were inside the section at once: a waiter took the lock from a holder that \
             was alive and working"
        );
    }

    /// DEFECT THIS PREVENTS: A LOCK IN THE MIDDLE OF BEING RELEASED REFUSED THE SECTION OUTRIGHT.
    ///
    /// On Windows, deleting a file sets its delete disposition on a handle and then closes that
    /// handle, and between the two the name is DELETE PENDING: `create_new` on it fails with
    /// "Access is denied" (os error 5), not `AlreadyExists`. `with_lock_patient` treated every
    /// error but `AlreadyExists` as final, so a waiter that tried to create the lock while the
    /// previous holder's release was mid-flight returned an I/O error and its section never ran.
    /// Measured: 8 threads racing `create_new` against `remove_file` for 5 s saw os error 5 on 1,475
    /// of 40,263 attempts, and the Windows gate on 2026-09-12 failed
    /// `a_queue_of_live_holders_never_has_the_lock_taken_from_under_it` with it.
    ///
    /// THE MOMENT IS HELD OPEN, NOT RACED FOR: a handle with the disposition set keeps the name
    /// delete pending until the handle closes, 100 ms later.
    ///
    /// WHAT MUTATION MAKES THIS RED: returning a denied `create_new` at once again.
    #[cfg(windows)]
    #[test]
    fn a_lock_mid_release_is_waited_for_not_refused() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };
        /// `DELETE`, the standard access right a handle needs to set its delete disposition.
        const DELETE: u32 = 0x0001_0000;
        /// Read, write and delete sharing, so the pending name is the only thing in the way.
        const SHARE_ALL: u32 = 0x7;

        let path = scratch("mid-release").join("guarded.json");
        let lock = PathBuf::from(format!("{}.lock", path.display()));
        std::fs::write(&lock, b"").expect("the lock the previous holder is releasing");
        let releasing = std::fs::OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(SHARE_ALL)
            .open(&lock)
            .expect("a handle that may delete the lock");
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: the handle is open and owned by `releasing` for the whole call, and the buffer is
        // a live FILE_DISPOSITION_INFO whose size is passed with it.
        unsafe {
            SetFileInformationByHandle(
                HANDLE(releasing.as_raw_handle()),
                FileDispositionInfo,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        }
        .expect("the lock is marked for deletion");
        let denied = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map(drop)
            .map_err(|e| e.kind());
        assert_eq!(
            denied,
            Err(std::io::ErrorKind::PermissionDenied),
            "the name is not delete pending, so what follows proves nothing"
        );

        let released = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(releasing);
        });
        let ran = with_lock_patient(&path, std::time::Duration::from_secs(5), || Ok(()));
        released.join().expect("the release finished");
        assert!(
            ran.is_ok(),
            "a lock in the middle of being released refused the section: {ran:?}"
        );
    }

    /// DEFECT THIS PREVENTS: A TAKEN LOCK THAT NOBODY HOLDS.
    ///
    /// The taker used to delete the stale lock and walk into the section WITHOUT creating its own,
    /// so for the whole of its write there was no lock file and any other writer went straight in
    /// beside it; its guard then deleted whatever lock file was there on the way out, which by then
    /// could belong to the next holder.
    ///
    /// WHAT MUTATION MAKES THIS RED: `break` out of the wait after removing the stale lock instead
    /// of going round again to create one.
    #[test]
    fn a_dead_holders_lock_is_taken_and_the_taker_really_holds_it() {
        let path = scratch("dead-holder").join("guarded.json");
        let lock = {
            let mut s = path.as_os_str().to_owned();
            s.push(".lock");
            PathBuf::from(s)
        };
        std::fs::write(&lock, b"").expect("a lock nobody will ever release");
        let patience = std::time::Duration::from_millis(100);
        std::thread::sleep(patience * 3);
        let held = with_lock_patient(&path, patience, || Ok(lock.exists())).expect("taken");
        assert!(
            held,
            "the stale lock was removed and the section ran with no lock file at all"
        );
        assert!(!lock.exists(), "the taker did not release the lock it took");
    }

    /// The lock file `with_lock_patient` guards `path` with.
    fn lock_beside(path: &Path) -> PathBuf {
        let mut s = path.as_os_str().to_owned();
        s.push(".lock");
        PathBuf::from(s)
    }

    /// DEFECT THIS PREVENTS: THE LOCK TAKEN FROM A LIVE HOLDER BECAUSE THE WALL CLOCK MOVED.
    ///
    /// A lock was judged stale by `SystemTime::now()` minus the lock file's modification time, which
    /// is two readings of the wall clock. A clock stepped forward (a time sync, a resume from sleep,
    /// an operator) makes a lock written a moment ago look old, and the waiter took it from a holder
    /// that was still inside. Measured on WSL: `a_queue_of_live_holders_never_has_the_lock_taken_
    /// from_under_it` failed 15 of 15 runs while root stepped the clock two seconds forward and back
    /// every 150 ms, and 0 of 15 with the clock left alone. A modification time of 1970 is the same
    /// jump, held still so that it cannot be missed.
    ///
    /// THE HOLDER STAYS WELL INSIDE THE PATIENCE: two seconds of patience, 400 ms in the section. A
    /// holder that sat on one token past the patience would be taken by design, alive or not, so
    /// only a holder inside it says anything about the clock.
    ///
    /// WHAT MUTATION MAKES THIS RED: judge a lock stale by its file's modification time again.
    #[test]
    fn a_live_holder_whose_lock_file_looks_ancient_is_never_taken_from() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let path = scratch("ancient-mtime").join("guarded.json");
        let lock = lock_beside(&path);
        let patience = std::time::Duration::from_secs(2);
        let held_for = std::time::Duration::from_millis(400);
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let (entered, holding) = std::sync::mpsc::channel();
        let holder = {
            let (path, lock, inside, most) =
                (path.clone(), lock.clone(), inside.clone(), most.clone());
            std::thread::spawn(move || {
                with_lock_patient(&path, patience, || {
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(&lock)
                        .and_then(|f| f.set_modified(std::time::UNIX_EPOCH))
                        .expect("the lock file's time set to 1970");
                    entered.send(()).expect("the waiter is listening");
                    std::thread::sleep(held_for);
                    inside.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
                .expect("the holder's section ran");
            })
        };
        holding.recv().expect("the holder is inside");
        with_lock_patient(&path, patience, || {
            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            inside.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("the waiter's section ran");
        holder.join().expect("the holder finished");
        assert_eq!(
            most.load(Ordering::SeqCst),
            1,
            "a waiter took the lock from a live holder whose lock file's time said 1970"
        );
    }

    /// DEFECT THIS PREVENTS: A DEAD HOLDER'S LOCK NEVER TAKEN BECAUSE ITS TIME IS IN THE FUTURE.
    ///
    /// The other half of the same wall clock rule. A holder that died just before the clock was
    /// stepped back leaves a lock whose modification time is ahead of now; `elapsed()` on that is an
    /// error, the age read as unknown, and an unknown age was never stale, so every writer after it
    /// waited for ever. That is one crash and one clock correction disabling the bookkeeping for
    /// good, which is the outcome `LOCK_PATIENCE` exists to rule out. A token that has not changed
    /// for the whole patience is a dead holder whatever the file's time says, and it is not taken any
    /// sooner than that patience on the waiter's own clock.
    ///
    /// WHAT MUTATION MAKES THIS RED: judge a lock stale by its file's modification time again (the
    /// waiter never returns), or take a lock the first time its token is read (taken early).
    #[test]
    fn a_dead_holders_token_is_taken_after_the_patience_whatever_its_file_time_says() {
        let path = scratch("future-mtime").join("guarded.json");
        let lock = lock_beside(&path);
        std::fs::write(&lock, b"a holder that died").expect("a lock nobody will ever release");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&lock)
            .and_then(|f| {
                f.set_modified(
                    std::time::SystemTime::now() + std::time::Duration::from_secs(86_400),
                )
            })
            .expect("the lock file's time set a day ahead");
        let patience = std::time::Duration::from_millis(300);
        let (done, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let held = with_lock_patient(&path, patience, || {
                Ok(std::fs::read(lock_beside(&path)).unwrap_or_default())
            });
            let _ = done.send((started.elapsed(), held));
        });
        let (waited, held) = answer
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the waiter never took a dead holder's lock whose time is in the future");
        let held = held.expect("taken");
        assert!(
            waited >= patience,
            "the lock was taken after {waited:?}, before the {patience:?} patience had passed"
        );
        assert!(
            held != b"a holder that died",
            "the section ran under the dead holder's lock rather than one of its own"
        );
    }

    /// DEFECT THIS PREVENTS: A HOLDER WHOSE LOCK WAS TAKEN DELETING THE TAKER'S LOCK ON ITS WAY OUT.
    ///
    /// A holder slow past the patience has its lock taken, and the taker creates its own under the
    /// same name. The release deleted whatever file had that name, so the slow holder's exit removed
    /// the lock the taker was holding and the next writer walked in beside the taker: one wrong take
    /// became two. The take is played inside the section here, the lock replaced by one carrying
    /// another holder's token, which is what a taker's remove and create leave behind.
    ///
    /// WHAT MUTATION MAKES THIS RED: remove the lock on release without reading whose token it
    /// carries.
    #[test]
    fn a_holder_whose_lock_was_taken_leaves_the_takers_lock_alone() {
        let path = scratch("taken-from").join("guarded.json");
        let lock = lock_beside(&path);
        with_lock_patient(&path, std::time::Duration::from_secs(5), || {
            std::fs::remove_file(&lock).expect("the taker removes the stale lock");
            std::fs::write(&lock, b"the taker's own token").expect("and creates its own");
            Ok(())
        })
        .expect("the section ran");
        assert_eq!(
            std::fs::read(&lock).ok().as_deref(),
            Some(&b"the taker's own token"[..]),
            "the holder whose lock was taken deleted the taker's lock on release"
        );
    }

    /// DEFECT THIS PREVENTS: THE BYTES THAT RUN NOT BEING THE BYTES THAT WERE CHECKED.
    ///
    /// # THE WINDOW, WHICH IS HOURS AND NOT MICROSECONDS
    ///
    /// `auto_download` is on by default, so a verified payload lands in
    /// `%LOCALAPPDATA%\eql-grimoire\update\staging\<version>\` unattended and sits there until the
    /// reader happens to press Install. That folder is writable by the user with no elevation.
    /// Until this change `install_app` ran the PREFLIGHT first, which SPAWNS the staged file: any
    /// process running as the same user could replace it in that window and its code would run
    /// with this app's privileges and inherited environment, for as long as it liked. Putting the
    /// real bytes back before the copy made every later check pass, the pointer flip, and nothing
    /// anywhere record that anything had happened. The existing `check_file` was on the COPY, so
    /// against a swap it was a post-mortem and not a gate.
    ///
    /// # THE DOUBLE RECORDS WHETHER IT WAS CALLED, WHICH IS THE WHOLE ASSERTION
    ///
    /// It is not enough that the call returns an error: the defect was that the tampered file was
    /// EXECUTED before the error. A `Preflight` that records its own invocation is the only way to
    /// say "and nothing ran", which is the thing being fixed.
    ///
    /// WHAT MUTATION MAKES THIS RED: move the first `check_file` back below the `pre.smoke` call,
    /// and the recorder goes from empty to one entry.
    #[test]
    fn the_staged_bytes_are_checked_before_anything_is_allowed_to_run_them() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        /// A preflight that records that it was asked, and otherwise says yes.
        struct Counted(Arc<AtomicUsize>);
        impl Preflight for Counted {
            fn smoke(&self, _exe: &Path) -> Result<(), String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let l = Layout::at(scratch("swapped-staging"));
        let signer = Signer::new();
        let keys = [signer.public_b64.as_str()];
        let body = b"MZ the payload the publisher signed".to_vec();
        let art = sealed(&signer, "0.2.0", &body);
        let ver = v("0.2.0");
        let staged = stage(&l, &ver, &art, &body[..], Pulse::Closed, &keys, &mut |_| {})
            .expect("the honest download stages");

        /* SOMEBODY ELSE'S PROGRAM, WRITTEN OVER THE VERIFIED ONE. The signature beside it is left
         * exactly as the download wrote it, which is the realistic case: an attacker with write
         * access to this folder has no reason to touch a file nobody reads, and could not produce
         * a valid one for these bytes anyway. */
        std::fs::write(&staged, b"MZ a program nobody signed").expect("the swap");

        let ran = Arc::new(AtomicUsize::new(0));
        let no = install_app(&l, &ver, &art, &keys, &Counted(ran.clone()), Pulse::Closed)
            .expect_err("a swapped staging file was installed");
        assert_eq!(
            ran.load(Ordering::SeqCst),
            0,
            "the preflight SPAWNED the staged file before anything checked it, so the swapped \
             program ran with this app's privileges and the refusal that followed was a \
             post-mortem"
        );
        assert_eq!(no.code(), "StagedFileChangedOnDisk");
        assert!(
            read_current(&l).is_none(),
            "the pointer was written for bytes that did not verify"
        );

        /* AND THE HONEST BYTES STILL INSTALL, so the refusal above is about the swap and not about
         * the fixture. The preflight is reached exactly once. */
        std::fs::write(&staged, &body).expect("put the real payload back");
        install_app(&l, &ver, &art, &keys, &Counted(ran.clone()), Pulse::Closed)
            .expect("the payload the manifest named installs");
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    /// DEFECT THIS PREVENTS: A PREFLIGHT THAT NEVER EXITS WEDGING THE UPDATE WORKER FOR EVER.
    ///
    /// # THE FAILURE, WHICH NEEDED NO ATTACKER
    ///
    /// The preflight's child exits from `App::smoke`, which ran only from `App::ui`. eframe calls
    /// `ui` while the window is visible and `logic` otherwise, so a preflight launched while
    /// EverQuest is fullscreen-exclusive, or over RDP, or into a minimized window, never reached
    /// its own clock. `Spawn::smoke` was `Command::output()`, which blocks until the child exits:
    /// no deadline, no kill. The worker thread stopped for ever, the Settings screen froze on
    /// `Downloaded`, the Install button stayed live and inert, and quitting Grimoire left an orphan
    /// process holding a log tail, a watcher poll and a hotkey registration with no window the
    /// reader could find.
    ///
    /// The child half of the fix is that `App::logic` runs the smoke clock too. This is the half
    /// that does not depend on the child being right.
    ///
    /// # THE COMMAND IS A REAL PROCESS THAT REALLY DOES NOT EXIT
    ///
    /// A fake `Child` cannot be built, and the point of this rule is what it does to a live one:
    /// it has to be killed and reaped. The spelling differs per platform because there is no
    /// portable "sleep" binary, and five seconds against a half second budget is what makes the
    /// mutation visible: with the deadline removed the call returns `Ok` five seconds later and
    /// the assertion below goes red rather than hanging the suite.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the deadline branch in `wait_within` and it returns
    /// `Ok` instead of the refusal; delete the `child.kill()` and the process is still alive
    /// afterwards.
    #[test]
    fn a_preflight_that_does_not_exit_is_stopped_and_refused() {
        let mut hang = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "ping", "-n", "6", "127.0.0.1"]);
            c
        } else {
            let mut c = std::process::Command::new("sleep");
            c.arg("5");
            c
        };
        hang.stdout(std::process::Stdio::null());
        hang.stderr(std::process::Stdio::null());
        let mut child = hang.spawn().expect("a command that does not exit at once");

        let began = std::time::Instant::now();
        let why = wait_within(&mut child, std::time::Duration::from_millis(500))
            .expect_err("a child that outstays its deadline must be a refusal");
        assert!(
            why.contains("did not exit"),
            "the refusal must say what actually happened, got {why:?}"
        );
        assert!(
            began.elapsed() < std::time::Duration::from_secs(4),
            "the wait was not bounded: it took {:?}",
            began.elapsed()
        );
        assert!(
            matches!(child.try_wait(), Ok(Some(_))),
            "the child outlived the refusal, which is the orphan process this exists to prevent"
        );

        /* AND A CHILD THAT DOES EXIT IS WAITED FOR NORMALLY, so the branch above is about the
         * deadline and not about every child being killed. */
        let mut quick = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "exit", "0"]);
            c
        } else {
            std::process::Command::new("true")
        };
        quick.stdout(std::process::Stdio::null());
        let mut child = quick.spawn().expect("a command that exits at once");
        let status = wait_within(&mut child, std::time::Duration::from_secs(30))
            .expect("a child that exits is not a failure");
        assert!(status.success());
    }

    /// DEFECT THIS PREVENTS: A POINTER THAT IS ON THE DISK NAMING AN EXECUTABLE THAT IS NOT.
    ///
    /// # WHY THIS IS A SOURCE-TEXT TEST, SAID PLAINLY
    ///
    /// Durability cannot be observed from inside the process that asked for it: every read after a
    /// `write` comes back through the page cache whether or not a single byte reached the platter,
    /// which is the same reason the `check_file` at step 9 does not close this gap. The only
    /// honest test is a floor over the call actually being made, which is the instrument
    /// `the_preflight_uses_the_environment_variable_main_actually_reads` already uses in this file.
    /// It is a floor and not a proof, and saying so is better than a test that looks like a proof.
    ///
    /// THE DEFECT IT STANDS OVER. `write_current` flushed the 200 byte pointer with `sync_all` and
    /// its comment said why: "a rename that lands ahead of the bytes it names is the power-loss
    /// case this whole design exists to remove". The 10.8 MB file that pointer NAMES was copied
    /// with `std::fs::copy` and renamed with no flush at all. After a power loss NTFS holds the
    /// directory entry and a size, and extents nobody wrote; `plan` sees a file, execs it, Windows
    /// refuses the image, and on a first update there is no previous version, so the reader
    /// double-clicks the shortcut twice and nothing happens, with no message anywhere.
    ///
    /// WHAT MUTATION MAKES THIS RED: put `std::fs::copy` back in `install_app`; drop the
    /// `sync_all` out of `write_durably` or `copy_durably`; or drop the `sync_all` on the staged
    /// `.part` before its rename.
    #[test]
    fn every_file_this_module_renames_is_flushed_to_the_device_first() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("updater")
                .join("install.rs"),
        )
        .expect("this module's own source");
        let production = src.split("mod tests").next().expect("the production half");

        assert!(
            !production.contains("std::fs::copy("),
            "a payload is being copied with std::fs::copy, which returns before the bytes are on \
             the device; the rename that follows can be journalled ahead of them"
        );
        for (what, needle) in [
            ("the durable write", "fn write_durably"),
            ("the durable copy", "fn copy_durably"),
        ] {
            let body = production
                .split(needle)
                .nth(1)
                .unwrap_or_else(|| panic!("{what} is gone"));
            let body = body.split("\nfn ").next().unwrap_or(body);
            assert!(
                body.contains("sync_all()"),
                "{what} no longer flushes to the device, so every rename in this module can land \
                 ahead of the bytes it names"
            );
        }
        let staging = production
            .split("fn stage<R: Read>")
            .nth(1)
            .expect("the staging half");
        assert!(
            staging.contains("sync_all()"),
            "the staged payload is renamed into place without being flushed first"
        );
    }

    /// DEFECT THIS PREVENTS: A HOSTILE DATA BUNDLE WRITING OUTSIDE THE FOLDER IT IS UNPACKED INTO.
    ///
    /// This is the one place in the feature where a string an attacker chose becomes a filesystem
    /// path, and this organisation has already found and fixed a ZIP-SLIP remote code execution in
    /// another tree. The list is long because each spelling is a different hole, and the final
    /// `starts_with` is there because the list will never be provably complete.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete any single arm. The `..` arm and the `starts_with`
    /// guard cover each other, so deleting BOTH is the only way to make the classic traversal
    /// pass, which is exactly why both are written.
    #[test]
    fn an_archive_entry_may_only_land_inside_the_folder_it_is_extracted_into() {
        let target = Path::new("C:").join("target");
        let ok = |e: &str| {
            safe_entry_path(&target, e, EntryKind::File)
                .unwrap_or_else(|why| panic!("{e:?} should be allowed: {why}"))
        };
        assert_eq!(ok("gear-data.json"), target.join("gear-data.json"));
        assert_eq!(
            ok("atlas-wiki/freeport.json"),
            target.join("atlas-wiki").join("freeport.json")
        );
        assert_eq!(ok("./gear-data.json"), target.join("gear-data.json"));

        for hostile in [
            "",
            "..",
            "../evil",
            "atlas-wiki/../../evil",
            "/etc/passwd",
            "C:/Windows/System32/evil.dll",
            "C:evil",
            r"..\..\evil",
            r"\\server\share\evil",
            r"\\?\C:\evil",
            "gear-data.json:hidden",
        ] {
            let no = safe_entry_path(&target, hostile, EntryKind::File)
                .expect_err("a hostile entry was allowed");
            assert_eq!(
                no.code(),
                "UnsafeArchiveEntry",
                "{hostile:?} was accepted as a place to write"
            );
        }

        for kind in [EntryKind::Symlink, EntryKind::Hardlink, EntryKind::Other] {
            assert!(
                safe_entry_path(&target, "gear-data.json", kind).is_err(),
                "{kind:?} entries must be refused: the path is checked and the link target is not"
            );
        }
    }

    /// DEFECT THIS PREVENTS: AN INCOMPLETE BUNDLE REPLACING A SNAPSHOT THAT WORKED, AND THE SWAP
    /// LEAVING THE READER WITH NO DATA AT ALL.
    ///
    /// The swap is two renames with a real gap between them, which is the same gap that made the
    /// two-rename BINARY swap unusable. It is acceptable here only because the thing that can go
    /// missing is content the app is designed to be without and because the gap self-heals, and
    /// both of those have to be true in code rather than in a comment.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `bundle_is_complete` call from `swap_data` (a
    /// bundle missing a file replaces a working one); or have `heal_missing_data` return early
    /// when `data.previous` exists, which is the case it is FOR.
    #[test]
    fn a_bundle_is_checked_before_it_replaces_one_and_a_lost_swap_heals_itself() {
        let l = Layout::at(scratch("data-swap"));
        let plant = |dir: &Path, mark: &str, whole: bool| {
            std::fs::create_dir_all(dir.join(crate::data::ATLAS_DIR)).expect("the atlas dir");
            let files: Vec<&str> = if whole {
                crate::data::FILES.to_vec()
            } else {
                crate::data::FILES[1..].to_vec()
            };
            for f in files {
                std::fs::write(dir.join(f), mark.as_bytes()).expect("a snapshot file");
            }
        };

        plant(&l.data_dir(), "the old snapshot", true);
        plant(&l.data_incoming(), "half a snapshot", false);
        let no = swap_data(&l).expect_err("an incomplete bundle");
        assert_eq!(no.code(), "BundleIncomplete");
        assert_eq!(
            std::fs::read_to_string(l.data_dir().join(crate::data::FILES[0]))
                .expect("the old snapshot is untouched"),
            "the old snapshot"
        );

        let _ = std::fs::remove_dir_all(l.data_incoming());
        plant(&l.data_incoming(), "the new snapshot", true);
        swap_data(&l).expect("a whole bundle installs");
        assert_eq!(
            std::fs::read_to_string(l.data_dir().join(crate::data::FILES[0])).expect("live"),
            "the new snapshot"
        );
        assert_eq!(
            std::fs::read_to_string(l.data_previous().join(crate::data::FILES[0]))
                .expect("one generation back"),
            "the old snapshot"
        );

        /* THE POWER-LOSS CASE: the first rename landed and the second did not. */
        std::fs::rename(l.data_dir(), l.data_previous().with_extension("gone"))
            .expect("simulate the gap");
        let _ = std::fs::remove_dir_all(l.data_previous().with_extension("gone"));
        assert!(
            heal_missing_data(&l).expect("the heal runs"),
            "it did nothing"
        );
        assert_eq!(
            std::fs::read_to_string(l.data_dir().join(crate::data::FILES[0])).expect("live again"),
            "the old snapshot",
            "the generation that was set aside is what comes back"
        );
        assert!(
            !heal_missing_data(&l).expect("the heal runs again"),
            "a healthy install must not be touched"
        );
    }

    /// DEFECT THIS PREVENTS: THE PREFLIGHT'S ENVIRONMENT VARIABLE DRIFTING AWAY FROM THE ONE
    /// `main.rs` READS.
    ///
    /// `main.rs` owns `SMOKE_ENV` as a private constant and this module repeats the string. If the
    /// two came apart, `smoke_command` would launch the new binary with NO deadline: a full app
    /// that opens a window, never exits, and blocks the install behind an `output()` call that
    /// waits forever. Nothing about that failure looks like a renamed constant.
    ///
    /// A SOURCE-TEXT TEST, LIKE THE TWO IN `main.rs` THAT HOLD THE HEARTBEAT TOGETHER
    /// (`main.rs:4033`, `main.rs:4493`). It is a floor, not a type check, and it is the only
    /// check available across a private constant.
    ///
    /// WHAT MUTATION MAKES THIS RED: change [`SMOKE_ENV`] here, or rename the variable in
    /// `main.rs`.
    #[test]
    fn the_preflight_uses_the_environment_variable_main_actually_reads() {
        let main = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("main.rs is beside this module");
        assert!(
            main.contains(&format!("{SMOKE_ENV:?}")),
            "main.rs does not name {SMOKE_ENV:?}, so the preflight would launch a full app with \
             no deadline and wait for it forever"
        );

        let c = smoke_command(Path::new("x"));
        let set: Vec<_> = c
            .get_envs()
            .filter(|(k, _)| *k == std::ffi::OsStr::new(SMOKE_ENV))
            .collect();
        assert_eq!(
            set.len(),
            1,
            "the production command must set it exactly once"
        );
        assert_eq!(
            set[0].1.and_then(|v| v.to_str()),
            Some(crate::data::LOAD_BUDGET.as_millis().to_string().as_str()),
            "the deadline must be the app's own measured load budget and not a new number"
        );
    }
}
