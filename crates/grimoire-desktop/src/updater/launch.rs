//! The trampoline: which executable this process should actually be, decided before a window
//! exists.
//!
//! # THE MECHANISM THIS REPLACES, AND WHY IT WAS REJECTED
//!
//! The usual way to update a Windows program in place is the two-rename swap: rename the running
//! `grimoire-desktop.exe` out of the way, rename the staged one into its place. Windows permits
//! renaming a running image, so it works. It has one failure that cannot be recovered from:
//! between the two renames there is an instant in which NO FILE EXISTS at the path the Start Menu
//! shortcut points at. A power loss there leaves an install that cannot be launched, and
//! therefore cannot repair itself, because repair requires something to run. The owner's only
//! remedy is a manual reinstall they have no way of knowing they need. That gap cannot be closed
//! by ordering, by flushing, or by a breadcrumb, because every recovery path begins with running
//! a program.
//!
//! # WHAT IS DONE INSTEAD: A FIXED ENTRY POINT AND AN ATOMIC POINTER FLIP
//!
//! The file the shortcut names never moves, is never renamed and is never deleted. Updating is
//! one atomic rename of one small JSON file. The fixed entry point is not a second binary: it is
//! the SAME binary, behaving as a trampoline when it finds itself outside the managed directory.
//!
//! ```text
//! <where it was installed>\grimoire-desktop.exe                  the entry point, never touched
//! %LOCALAPPDATA%\eql-grimoire\app\current.json                   the pointer
//! %LOCALAPPDATA%\eql-grimoire\app\<version>\grimoire-desktop.exe what actually runs
//! ```
//!
//! The cost, named: every launch after the first update starts two processes and the first exits
//! within milliseconds. That is what Chrome does. It is visible in Task Manager for an instant
//! and nowhere else.
//!
//! # WHY THIS IS A FREE FUNCTION
//!
//! `App::ui` cannot be called from a test (`main.rs:2537`), and neither can `main`. A rule that
//! decides which executable runs is the last rule in this crate that should be reachable only by
//! running it, so it is a pure function over its inputs and `main` is three lines on top of it.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use semver::Version;

/// The pointer at `app\current.json`. The ONLY thing that reads it is the trampoline.
///
/// # WHY IT IS NOT `update\state.json`
///
/// `state.json` is everything the Settings screen shows: the last check, the last failure and its
/// cause, what is staged. It is written often and read by one screen. This is read on the launch
/// path of every process, before a window exists, and a parse failure in it means the app does not
/// start. Two files, so that a screen's bookkeeping can never make the app unlaunchable.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Current {
    /// WHICH SHAPE THIS DOCUMENT IS. See [`POINTER_VERSION`].
    ///
    /// FIRST FIELD, AND THE ONLY ONE THE READER IS ALLOWED TO BELIEVE BEFORE IT HAS CHECKED THIS
    /// ONE. The manifest got an envelope version and a `format` field for exactly this reason and
    /// the pointer got neither, which is the wrong way round: the manifest is read by a client
    /// that can be updated, and this file is read forever by a binary that by design never is.
    #[serde(default)]
    pub pointer: u64,
    /// The version `exe` is.
    pub version: String,
    /// What to run. Absolute, and checked to be inside the managed directory before it is run.
    pub exe: PathBuf,
    /// The length the SIGNED MANIFEST gave for `exe`, recorded at install time.
    ///
    /// # WHY THE POINTER CARRIES A SEAL AT ALL
    ///
    /// Because the `.minisig` beside every installed payload was write-only. It was written by
    /// `install::stage`, copied by `install::install_app`, and read by nothing: the artifact
    /// signature was checked exactly once, at install time, and every launch from then on exec'd
    /// whatever was at that path on trust. An attacker with user-level write to `%LOCALAPPDATA%`
    /// (malware, a merged roaming profile, a restored backup, a shared machine) could write
    /// `app\9.9.9\grimoire-desktop.exe` and a pointer naming it, and every double-click of the
    /// Start Menu shortcut would trampoline into it, forever, with the app appearing to start
    /// normally.
    ///
    /// These two numbers come out of the signed manifest at install time, so recording them is
    /// recording a publisher's claim rather than inventing one, and they make the check at launch
    /// one streamed pass instead of a parse of anything.
    ///
    /// `Option` BECAUSE A ROLLBACK CANNOT ALWAYS FILL THEM, and because a pointer this build did
    /// not write must not be silently treated as verified. Absent means "cannot be checked", and
    /// [`plan`] answers that with [`Plan::RunHere`], which is the safe direction: the entry point
    /// starts and the reader has a working app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The sha256 the signed manifest gave for `exe`. See [`Current::size`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The version that is still on disk for a rollback. Never pruned while it is named here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// When the flip happened. Written here and shown by the UPDATES section in the other half.
    pub installed: DateTime<Utc>,
    /// HOW MANY LAUNCHES STARTED AND NEVER DREW A FRAME.
    ///
    /// The trampoline increments this before `spawn`; the launched app clears it on its FIRST
    /// COMPLETED FRAME and not at startup, because a glow or wgpu failure happens during that
    /// frame and clearing earlier would call a crash a success.
    #[serde(default)]
    pub launches_failed: u32,
    /// When the last launch attempt began. Written by the trampoline, cleared by the first frame,
    /// and shown beside the failure count by the UPDATES section in the other half.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launch_started: Option<DateTime<Utc>>,
}

/// THE SHAPE OF `current.json` THIS BUILD WRITES AND READS.
///
/// # WHY A POINTER NEEDS A VERSION MORE THAN A MANIFEST DOES
///
/// The one thing that must read this file forever is the ENTRY POINT, which by design never moves,
/// is never renamed, is never deleted, and is therefore never updated. A payload three releases
/// from now that adds a required field, or changes what `installed` means, writes a shape the
/// permanently-old entry point cannot parse. `read_current` answers `None` for any parse failure
/// and `plan` maps that to [`Plan::RunHere`], which is SAFE and SILENT: every launch from then on
/// quietly runs the original version, the UPDATES section re-offers the new one, the updater
/// re-downloads and re-installs it and flips a pointer nothing will ever read, every session, and
/// the reader's only clue is that the version number never changes.
///
/// With a version in the file the same situation is a fact the Settings screen can print
/// ([`pointer_problem`]), which is the difference between a bug that is reported and one that is
/// not. It does not make an old reader able to read a new file, because nothing can; it makes the
/// old reader able to SAY SO.
///
/// THE RULE FOR CHANGING IT is the manifest's rule: a field added with `#[serde(default)]` that an
/// old reader may ignore never bumps this. A field an old reader must honour to be correct, or a
/// change to what an existing field means, always does.
pub const POINTER_VERSION: u64 = 1;

/// What this process should do about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Be the app. Every uncertain case answers this, because running the binary that is already
    /// here is always safe and bouncing to the wrong one is not.
    RunHere,
    /// Start this executable instead, pass the arguments along, and exit 0.
    Exec(PathBuf),
    /// The pointed-at version has failed to draw twice, or has been withdrawn. Rewrite the
    /// pointer to `to` and run that.
    Rollback { to: String, why: &'static str },
}

/// HOW MANY LAUNCHES THAT NEVER DREW A FRAME COUNT AS A BROKEN BUILD.
///
/// TWO, NOT ONE, AND THE REASON IS NOT TUNING. A single launch that starts and never draws can be
/// the machine restarting under the owner, a laptop lid, or a kill from an antivirus scanner that
/// then whitelists the file. Rolling back on one would undo a good update for a reason that had
/// nothing to do with it. Two consecutive launches that both started and neither drew is a
/// statement about the binary. This is a policy, not a measurement, and it is not dressed as one.
pub const FAILED_LAUNCHES_BEFORE_ROLLBACK: u32 = 2;

/// THE ENTRY POINT'S OWN PATH, PASSED DOWN TO THE PAYLOAD IT STARTS.
///
/// # WHY THE CHILD HAS TO BE TOLD, AND CANNOT WORK IT OUT
///
/// A managed payload's `current_exe()` is itself, inside the managed directory, and [`plan`]
/// answers [`Plan::RunHere`] for anything in there because that is the infinite-loop guard. So a
/// payload asked to restart after an update has no way to find the file the Start Menu shortcut
/// names: it would relaunch itself and run the version that was just replaced, and the pointer
/// that was flipped a second earlier would be ignored on every restart from then on. The
/// trampoline is the one process that knows both paths, and it is the only thing that sets this.
///
/// It is read by `run::entry_point`, which falls back to `current_exe()` when it is absent, which
/// is right for the case that matters most: a build nobody has updated IS the entry point.
///
/// A NAME AND NOT A FILE. It is an environment variable and not a field in `current.json` because
/// the entry point is a property of how THIS process was started, not of what is installed: two
/// copies of the app started from two different folders share one `current.json` and must not
/// share an answer to this.
pub const ENTRY_ENV: &str = "GRIMOIRE_ENTRY";

/// WHICH EXECUTABLE SHOULD THIS PROCESS BE?
///
/// `dev_build` is `cfg!(debug_assertions)` at the one production call site. It is an argument and
/// not a `cfg!` read inside this function for one reason: `cargo test` builds with debug
/// assertions on, so a branch that read the flag here would make every other branch untestable.
///
/// `yanked` is the withdrawn set from the newest manifest the client accepted, which the caller
/// has on hand from `update\state.json`. It is an argument because this function does not read
/// files; that is the whole of why it can be tested.
/// `keys` is [`super::verify::KEYS`] at the one production call site. It is an argument for the
/// same reason `dev_build` is: a test has to be able to drive the payload check with a key it
/// generated a moment ago, and a function that read the compiled-in list could only ever be tested
/// against a fixture nobody can regenerate.
pub fn plan(
    own_exe: &Path,
    app_dir: &Path,
    current: Option<&Current>,
    self_version: &Version,
    yanked: &[Version],
    dev_build: bool,
    keys: &[&str],
) -> Plan {
    /* A DEVELOPMENT BUILD NEVER TRAMPOLINES, for the same reason `data::SOURCE_TREE` is compiled
     * out of a release: a `cargo run` that silently launched an installed 0.2.0 instead of the
     * code just built would be a nightmare to diagnose, and the person diagnosing it would have
     * no reason to suspect it. */
    if dev_build {
        return Plan::RunHere;
    }

    /* THE INFINITE LOOP GUARD, AND IT IS A PROPERTY OF WHERE THE FILE IS RATHER THAN A FLAG THAT
     * CAN BE DROPPED. A managed payload is already the destination; if it bounced it would bounce
     * forever. `is_inside` answers TRUE when it cannot tell, because the cost of a wrong "yes" is
     * running the binary that is already here and the cost of a wrong "no" is a fork bomb. */
    if is_inside(own_exe, app_dir) {
        return Plan::RunHere;
    }

    let Some(c) = current else {
        return Plan::RunHere;
    };

    /* A POINTER SHAPE THIS BUILD DOES NOT KNOW IS RUN HERE, AND `pointer_problem` IS WHAT SAYS SO
     * ON THE SCREEN. See `POINTER_VERSION`. A future payload's pointer is unreadable to a
     * permanently-old entry point by construction; what must not happen is that being invisible. */
    if c.pointer != POINTER_VERSION {
        return Plan::RunHere;
    }

    /* THE POINTER MAY ONLY POINT INSIDE THE MANAGED DIRECTORY. `current.json` sits in the owner's
     * own LOCALAPPDATA, so this is not a defence against someone who already has write access
     * there, and it is not sold as one. It is a defence against a pointer that a half-finished
     * install, a merged profile or a restored backup left naming something that is not ours. */
    if !is_inside(&c.exe, app_dir) {
        return Plan::RunHere;
    }
    if same_file(&c.exe, own_exe) {
        return Plan::RunHere;
    }
    if !c.exe.is_file() {
        return Plan::RunHere;
    }
    let Ok(pointed) = Version::parse(&c.version) else {
        return Plan::RunHere;
    };

    /* A WITHDRAWN BUILD IS NOT RUN IF THERE IS ANYWHERE TO GO. With no previous on disk it is run
     * anyway: a withdrawn build that starts is better than no app at all, and the UPDATES section
     * is what tells the owner. */
    if yanked.contains(&pointed) {
        return match &c.previous {
            Some(to) => Plan::Rollback {
                to: to.clone(),
                why: "the publisher withdrew it",
            },
            None => Plan::RunHere,
        };
    }

    /* NOT STRICTLY NEWER IS RUN HERE. Equal means the pointer names what this binary already is,
     * and lower means a pointer left behind by something older than the entry point; in both
     * cases the file already loaded is the right one. */
    if pointed <= *self_version {
        return Plan::RunHere;
    }

    if c.launches_failed >= FAILED_LAUNCHES_BEFORE_ROLLBACK {
        if let Some(to) = &c.previous {
            return Plan::Rollback {
                to: to.clone(),
                why: "it started twice and never drew a frame",
            };
        }
        /* Nothing to roll back to. Running it is still the best of the available answers; the
         * alternative is an app that will not start at all. */
        return Plan::RunHere;
    }

    /* THE LAST GATE, AND THE ONLY ONE THAT ASKS ABOUT THE BYTES.
     *
     * Everything above this line is about the POINTER: that it is ours, that it names a file, that
     * the version parses, that it has not been withdrawn. None of that says anything about what is
     * IN the file, and until this check existed nothing did after install time. The `.minisig`
     * that travels beside every payload was written and copied and never read, so the updater's
     * whole promise, that the code this app runs was signed by the project key, was unenforced at
     * the one moment that matters.
     *
     * A FAILURE IS `RunHere` AND NOT A REFUSAL TO START. This is the launch path: the cost of a
     * wrong "no" is the entry point running, which is a working app one version behind and a
     * Settings screen that can say so, and the cost of a wrong "yes" is running somebody else's
     * program. Every other branch in this function makes the same trade for the same reason. */
    if let Err(why) = payload_is_signed(c, keys) {
        log::warn!(
            "not starting {}: {why}; running this build instead",
            c.exe.display()
        );
        return Plan::RunHere;
    }

    Plan::Exec(c.exe.clone())
}

/// ARE THE BYTES AT `c.exe` THE ONES A TRUSTED KEY SIGNED?
///
/// # WHAT EACH PIECE IS FOR
///
/// The `.minisig` beside the payload is the trust: it is a publisher's signature over those exact
/// bytes, made at release time, and checking it is the whole point of this function. The `size`
/// and `sha256` out of the pointer are the cheap half of the same pass ([`super::verify::Seal`]
/// checks all three in one read), and they are what makes a corrupted payload say "corrupted"
/// rather than "not signed by a key this build trusts", which sends a person to the wrong place.
///
/// # WHY A MISSING SEAL IS A FAILURE AND NOT A SKIP
///
/// A pointer with no `size` or `sha256` is one this build did not write. Treating it as verified
/// would mean an attacker could remove two fields to remove the check, which is a gate with a
/// published bypass. Treating it as unverified costs a launch of the entry point, which is a
/// working app.
fn payload_is_signed(c: &Current, keys: &[&str]) -> Result<(), String> {
    let (Some(size), Some(sha256)) = (c.size, c.sha256.as_deref()) else {
        return Err(
            "the pointer carries no size or hash for it, so there is nothing to check it against"
                .to_owned(),
        );
    };
    let sig_path = super::install::sig_of(&c.exe);
    let signature = std::fs::read_to_string(&sig_path).map_err(|e| {
        format!(
            "its signature at {} could not be read: {e}",
            sig_path.display()
        )
    })?;
    super::verify::check_file(
        &c.exe,
        super::verify::Seal {
            size,
            sha256,
            signature: &signature,
        },
        keys,
    )
    .map_err(|why| why.to_string())
}

/// WHAT IS WRONG WITH THE POINTER, IN WORDS, OR `None` WHEN NOTHING IS.
///
/// The Settings screen draws this. `plan` answers [`Plan::RunHere`] for a pointer it cannot use and
/// that answer is correct and silent; this is the half that is not silent. Without it the one
/// diagnosable symptom of a pointer an old entry point cannot read is that the version number
/// never changes however many times the updater installs something.
pub fn pointer_problem(current: Option<&Current>) -> Option<String> {
    let c = current?;
    if c.pointer != POINTER_VERSION {
        return Some(format!(
            "the file that says which version to start was written in a shape this build does not \
             know (pointer {} against the {POINTER_VERSION} this reads), so this copy is running \
             itself and ignoring it. Download the newest version by hand.",
            c.pointer
        ));
    }
    None
}

/// Is `child` inside `parent`?
///
/// ANSWERS TRUE WHEN IT CANNOT TELL, ON PURPOSE. Its one caller uses it as the loop guard, where
/// a wrong "yes" costs one process running the binary that is already loaded and a wrong "no"
/// costs an unbounded chain of processes. Canonicalising both sides is what makes `..`, a short
/// name, a junction and a case difference all come out the same; when a path does not exist or
/// the call fails, the raw comparison is tried and an inconclusive answer is "yes".
fn is_inside(child: &Path, parent: &Path) -> bool {
    match (child.canonicalize(), parent.canonicalize()) {
        (Ok(c), Ok(p)) => c.starts_with(p),
        _ => child.starts_with(parent) || !child.is_absolute(),
    }
}

/// Are these two paths the same file on disk?
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grimoire-updater-launch-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch folder");
        d
    }

    fn touch(p: &Path) {
        plant(p, b"MZ");
    }

    fn plant(p: &Path, bytes: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).expect("the parent");
        }
        std::fs::write(p, bytes).expect("plant a file");
    }

    fn sha256_of(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    struct Bench {
        root: PathBuf,
        app: PathBuf,
        entry: PathBuf,
        payload: PathBuf,
        /// REAL SIGNATURES OVER REAL BYTES, made fresh in this process.
        ///
        /// `plan` verifies the payload before it execs it, so a bench that planted `b"MZ"` and no
        /// `.minisig` would take every test in this file down the "it does not verify" branch and
        /// each one would pass for a reason that has nothing to do with what it says. The same
        /// argument `verify::probe` makes for not using frozen fixtures.
        signer: crate::updater::verify::probe::Signer,
    }

    impl Bench {
        fn new(tag: &str) -> Bench {
            let root = scratch(tag);
            let app = root.join("app");
            let entry = root.join("installed").join("grimoire-desktop.exe");
            let payload = app.join("0.2.0").join("grimoire-desktop.exe");
            /* THE ENTRY POINT IS NOT A MANAGED PAYLOAD AND CARRIES NO SIGNATURE. Nothing verifies
             * it: it is the file already running, and the trust in it is that it is the file the
             * reader installed by hand. */
            touch(&entry);
            let b = Bench {
                root,
                app,
                entry,
                payload,
                signer: crate::updater::verify::probe::Signer::new(),
            };
            b.install("0.2.0");
            b
        }

        /// Plant a signed payload for `version`, the way a real install leaves one.
        fn install(&self, version: &str) -> Vec<u8> {
            let bytes = format!("MZ the {version} payload").into_bytes();
            let exe = self.app.join(version).join("grimoire-desktop.exe");
            plant(&exe, &bytes);
            plant(
                &crate::updater::install::sig_of(&exe),
                self.signer.sign(&bytes).as_bytes(),
            );
            bytes
        }

        fn keys(&self) -> [&str; 1] {
            [self.signer.public_b64.as_str()]
        }

        fn current(&self, version: &str) -> Current {
            let exe = self.app.join(version).join("grimoire-desktop.exe");
            let bytes = std::fs::read(&exe).unwrap_or_default();
            Current {
                pointer: POINTER_VERSION,
                version: version.to_owned(),
                exe,
                size: Some(bytes.len() as u64),
                sha256: Some(sha256_of(&bytes)),
                previous: None,
                installed: Utc::now(),
                launches_failed: 0,
                last_launch_started: None,
            }
        }
        fn plan(&self, c: Option<&Current>, self_version: &str, yanked: &[&str]) -> Plan {
            let v = Version::parse(self_version).expect("a literal version");
            let y: Vec<Version> = yanked
                .iter()
                .map(|s| Version::parse(s).expect("a literal version"))
                .collect();
            plan(&self.entry, &self.app, c, &v, &y, false, &self.keys())
        }
    }

    /// DEFECT THIS PREVENTS: A FORK BOMB, AND EVERY OTHER WAY THE TRAMPOLINE CAN SEND A LAUNCH
    /// SOMEWHERE IT SHOULD NOT GO.
    ///
    /// `plan` decides what runs before a window exists, so every branch of it is a way the app can
    /// fail to start at all, which is the one failure a user cannot report from inside the app.
    /// The branches are listed here in the order they are written, because the ORDER is part of
    /// the rule: the position guard has to come before the pointer is read, or a managed payload
    /// would read the pointer that names itself and bounce forever.
    ///
    /// WHAT MUTATION MAKES THIS RED, branch by branch:
    ///   * drop the `dev_build` early return -- `runs_in_place_in_a_development_build` goes red.
    ///   * drop the `is_inside(own_exe, app_dir)` guard -- `a_managed_payload_never_bounces_again`
    ///     goes red, and a release build would spawn processes until the machine gave up.
    ///   * make `is_inside` answer `false` when it cannot tell -- same test, same outcome.
    ///   * drop the `c.exe.is_file()` check -- `a_pointer_at_a_missing_file_runs_here` goes red
    ///     and the app would exec something that is not there.
    ///   * change `pointed <= *self_version` to `<` -- `a_pointer_at_this_very_version_runs_here`
    ///     goes red and every launch would exec a copy of itself.
    #[test]
    fn a_development_build_and_a_managed_payload_both_run_in_place() {
        let b = Bench::new("runhere");

        /* A development build, whatever the pointer says. */
        let c = b.current("0.2.0");
        let v = Version::parse("0.1.0").expect("literal");
        let keys = b.keys();
        assert_eq!(
            plan(&b.entry, &b.app, Some(&c), &v, &[], true, &keys),
            Plan::RunHere,
            "a cargo run must never launch an installed build behind the developer's back"
        );
        /* The same inputs in a release build DO bounce, which is what makes the line above about
         * the flag and not about the fixture. */
        assert_eq!(
            plan(&b.entry, &b.app, Some(&c), &v, &[], false, &keys),
            Plan::Exec(b.payload.clone())
        );

        /* A managed payload is already the destination. This is the fork bomb guard. */
        assert_eq!(
            plan(&b.payload, &b.app, Some(&c), &v, &[], false, &keys),
            Plan::RunHere,
            "a payload that bounces would bounce forever"
        );

        /* AND A PAYLOAD THAT IS NOT THE ONE THE POINTER NAMES, which is the case the line above
         * cannot prove. When `own_exe` IS `current.exe`, the same-file guard answers as well, so
         * deleting the POSITION guard changes nothing there and a mutation run found exactly that
         * hole. Here an older payload is running while the pointer names a newer one: without the
         * position guard it would exec, and a managed payload must never bounce again whatever
         * the pointer says. */
        b.install("0.1.5");
        let older = b.app.join("0.1.5").join("grimoire-desktop.exe");
        assert_eq!(
            plan(
                &older,
                &b.app,
                Some(&c),
                &Version::parse("0.1.5").expect("literal"),
                &[],
                false,
                &keys
            ),
            Plan::RunHere,
            "a managed payload bounced to another managed payload"
        );
    }

    /// DEFECT THIS PREVENTS: AN APP THAT WILL NOT START BECAUSE THE POINTER IS WRONG.
    ///
    /// Every one of these is a state a half-finished install, a restored backup or a power loss
    /// can leave behind, and in every one of them the entry point binary is present, correct and
    /// runnable. Answering anything but `RunHere` turns a recoverable mess into an app that does
    /// not open.
    ///
    /// WHAT MUTATION MAKES THIS RED: any of the early `RunHere` returns replaced by a fall
    /// through to `Plan::Exec`.
    #[test]
    fn every_uncertain_pointer_runs_the_binary_that_is_already_here() {
        let b = Bench::new("uncertain");

        assert_eq!(
            b.plan(None, "0.1.0", &[]),
            Plan::RunHere,
            "no pointer at all"
        );

        let mut missing = b.current("0.3.0");
        missing.exe = b.app.join("0.3.0").join("grimoire-desktop.exe");
        assert_eq!(
            b.plan(Some(&missing), "0.1.0", &[]),
            Plan::RunHere,
            "a pointer at a version whose exe was never installed"
        );

        let mut unparsable = b.current("0.2.0");
        unparsable.version = "the newest one".to_owned();
        assert_eq!(
            b.plan(Some(&unparsable), "0.1.0", &[]),
            Plan::RunHere,
            "a version nobody can parse"
        );

        let same = b.current("0.2.0");
        assert_eq!(
            b.plan(Some(&same), "0.2.0", &[]),
            Plan::RunHere,
            "a pointer at this very version: exec would be a copy of this process"
        );
        assert_eq!(
            b.plan(Some(&same), "0.9.0", &[]),
            Plan::RunHere,
            "a pointer older than the entry point, left by a hand reinstall"
        );

        /* A pointer that names something outside the managed directory. */
        let outside = b.root.join("elsewhere").join("anything.exe");
        touch(&outside);
        let mut escaped = b.current("0.2.0");
        escaped.exe = outside;
        assert_eq!(
            b.plan(Some(&escaped), "0.1.0", &[]),
            Plan::RunHere,
            "the pointer may only name a file inside app\\"
        );

        /* And the honest case still bounces, so none of the above is passing by accident. */
        assert_eq!(
            b.plan(Some(&b.current("0.2.0")), "0.1.0", &[]),
            Plan::Exec(b.payload.clone())
        );
    }

    /// DEFECT THIS PREVENTS: THE TRAMPOLINE EXECUTING WHATEVER IS AT THE POINTED-AT PATH, FOREVER,
    /// ON TRUST.
    ///
    /// # THE ATTACK, WHICH NEEDS NO ELEVATION AND NO KEY
    ///
    /// `%LOCALAPPDATA%` is writable by the user. Anything running as that user (malware, a merged
    /// roaming profile, a restored backup, the other account on a shared machine) writes
    /// `app\9.9.9\grimoire-desktop.exe` and a `current.json` naming it, and before this check every
    /// branch of `plan` passed it: inside `app\`, a file, a version that parses, not yanked, newer
    /// than the entry point. From then on every double-click of the Start Menu shortcut
    /// trampolined into it and the app appeared to start normally. Overwriting the payload already
    /// installed works the same way and does not even need a new pointer.
    ///
    /// The material to answer this was already on disk and unused: `install::stage` writes a
    /// `.minisig` beside every payload and `install_app` copies it, and nothing in the crate read
    /// either of them. `grep minisig` found a write, a copy, and no reader.
    ///
    /// # WHY `RunHere` AND NOT A REFUSAL TO START
    ///
    /// This is the launch path. The cost of a wrong "no" here is the entry point running, which is
    /// a working app one version behind; the cost of a wrong "yes" is running somebody else's
    /// program with the reader's privileges. Every other uncertain branch of `plan` makes the same
    /// trade.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `payload_is_signed` call at the end of `plan`;
    /// treat a missing `size`/`sha256` as verified instead of unverified; or drop the `.minisig`
    /// read and check only the hash, which would accept any file an attacker can also write the
    /// pointer for.
    #[test]
    fn a_payload_whose_bytes_do_not_match_its_signature_is_never_started() {
        let b = Bench::new("payload-signature");

        /* The honest install bounces, which is what makes every refusal below about the bytes. */
        assert_eq!(
            b.plan(Some(&b.current("0.2.0")), "0.1.0", &[]),
            Plan::Exec(b.payload.clone())
        );

        /* ONE BYTE OF THE PAYLOAD CHANGED, THE SIGNATURE AND THE POINTER LEFT ALONE. This is the
         * overwrite-in-place case: the pointer is the one the installer wrote and the file under
         * it is not the one it named. */
        let honest = std::fs::read(&b.payload).expect("the payload");
        let mut tampered = honest.clone();
        tampered[3] ^= 0x40;
        plant(&b.payload, &tampered);
        assert_eq!(
            b.plan(Some(&b.current("0.2.0")), "0.1.0", &[]),
            Plan::RunHere,
            "a payload that does not match its own signature was started anyway"
        );

        /* A WHOLE VERSION PLANTED BY SOMEBODY WHO DOES NOT HAVE THE KEY: a real executable, a
         * pointer that names it, a seal computed over it, and a signature that is somebody else's.
         * Every field is self-consistent, which is exactly why the signature is the only thing
         * that can answer. */
        let theirs = crate::updater::verify::probe::Signer::new();
        let bytes = b"MZ a program the project never signed".to_vec();
        let exe = b.app.join("9.9.9").join("grimoire-desktop.exe");
        plant(&exe, &bytes);
        plant(
            &crate::updater::install::sig_of(&exe),
            theirs.sign(&bytes).as_bytes(),
        );
        let mut c = b.current("9.9.9");
        assert_eq!(
            c.sha256.as_deref(),
            Some(sha256_of(&bytes).as_str()),
            "the fixture must be internally consistent, or this test proves only that a hash \
             mismatch is caught"
        );
        assert_eq!(
            b.plan(Some(&c), "0.1.0", &[]),
            Plan::RunHere,
            "a payload signed by a key this build does not carry was started"
        );

        /* AND A POINTER WITH THE SEAL FIELDS REMOVED IS NOT A POINTER THAT SKIPS THE CHECK, which
         * would be a gate with a published bypass: delete two fields, delete the check. */
        plant(&exe, &bytes);
        plant(
            &crate::updater::install::sig_of(&exe),
            b.signer.sign(&bytes).as_bytes(),
        );
        assert_eq!(
            b.plan(Some(&b.current("9.9.9")), "0.1.0", &[]),
            Plan::Exec(exe.clone()),
            "a correctly signed payload must still start, or the refusals above are about the \
             fixture"
        );
        c = b.current("9.9.9");
        c.size = None;
        c.sha256 = None;
        assert_eq!(
            b.plan(Some(&c), "0.1.0", &[]),
            Plan::RunHere,
            "a pointer with no seal was treated as verified"
        );
    }

    /// DEFECT THIS PREVENTS: A FUTURE POINTER SHAPE TURNING EVERY LAUNCH INTO A SILENT NO-OP.
    ///
    /// # THE FAILURE THIS IS FOR
    ///
    /// The entry point is never updated, by design. A payload two releases from now that adds a
    /// required field to `Current` writes a shape it cannot parse; `read_current` answers `None`
    /// for any parse failure and `plan` maps that to `RunHere`, which is safe and says nothing. The
    /// visible result is an app that runs the original version forever while the updater
    /// re-downloads and re-installs the new one every session and flips a pointer nothing will ever
    /// read. The only clue a reader has is that the version number never changes.
    ///
    /// A version on the document does not make an old reader able to read a new file, because
    /// nothing can. It makes the old reader able to SAY SO, which is [`pointer_problem`] and is the
    /// difference between a bug that gets reported and one that does not.
    ///
    /// # THE ROUND TRIP IS HALF OF THE TEST
    ///
    /// The three fields that existed before this one must still deserialize, or every pointer
    /// written by a build between now and this change becomes unreadable, which is the exact
    /// failure being guarded against arriving by the guard itself.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `c.pointer != POINTER_VERSION` branch from `plan`;
    /// drop `#[serde(default)]` from `pointer`, `size` or `sha256`, which makes an older pointer
    /// unparsable; or have `pointer_problem` answer `None` for a shape this build cannot use.
    #[test]
    fn a_pointer_written_in_a_shape_this_build_does_not_know_runs_here_and_says_so() {
        let b = Bench::new("pointer-version");

        let mut future = b.current("0.2.0");
        future.pointer = POINTER_VERSION + 1;
        assert_eq!(
            b.plan(Some(&future), "0.1.0", &[]),
            Plan::RunHere,
            "a pointer shape this build does not know was acted on"
        );
        let said = pointer_problem(Some(&future)).expect("an unknown pointer shape is reportable");
        assert!(
            said.contains(&(POINTER_VERSION + 1).to_string()) && said.len() > 40,
            "the screen would have nothing to print that a person could act on: {said:?}"
        );
        assert_eq!(
            pointer_problem(Some(&b.current("0.2.0"))),
            None,
            "a pointer this build wrote must not be reported as a problem"
        );
        assert_eq!(pointer_problem(None), None, "no pointer is not a problem");

        /* THE THREE FIELDS THAT WERE THERE BEFORE ANY OF THIS, AND NOTHING ELSE. A pointer written
         * by a build older than this change must still parse, or this guard causes the failure it
         * is for. `pointer` reads as 0, which is not `POINTER_VERSION`, so such a file is run-here
         * and reported rather than acted on. */
        let old = serde_json::json!({
            "version": "0.2.0",
            "exe": b.payload,
            "installed": "2026-09-11T00:00:00Z",
        });
        let parsed: Current =
            serde_json::from_value(old).expect("a pointer written before these fields existed");
        assert_eq!(parsed.pointer, 0);
        assert_eq!(parsed.size, None);
        assert_eq!(b.plan(Some(&parsed), "0.1.0", &[]), Plan::RunHere);
        assert!(pointer_problem(Some(&parsed)).is_some());

        /* And what this build writes round-trips, which is the other direction of the same
         * question. */
        let mine = b.current("0.2.0");
        let text = serde_json::to_string(&mine).expect("a pointer serialises");
        let back: Current = serde_json::from_str(&text).expect("and reads back");
        assert_eq!(back.pointer, POINTER_VERSION);
        assert_eq!(back.size, mine.size);
        assert_eq!(back.sha256, mine.sha256);
    }

    /// DEFECT THIS PREVENTS: A BUILD THAT CANNOT DRAW LEAVING THE OWNER WITH NOTHING, AND A GOOD
    /// BUILD BEING ROLLED BACK BECAUSE THE MACHINE REBOOTED ONCE.
    ///
    /// The count is what separates the two, and one is not enough: a single launch that started
    /// and never drew can be a restart, a lid, or an antivirus kill that is then whitelisted.
    /// Rolling back on one would undo a good update for a reason that had nothing to do with it.
    ///
    /// AND A ROLLBACK WITH NOWHERE TO GO IS NOT A ROLLBACK. With no previous on disk, running the
    /// failing build is still better than refusing to start anything.
    ///
    /// WHAT MUTATION MAKES THIS RED: `>= 1` in place of
    /// `>= FAILED_LAUNCHES_BEFORE_ROLLBACK` (the one-failure case starts rolling back); `> 2`
    /// (two failures stop being enough); or returning `Rollback` when `previous` is `None`, which
    /// names a version that is not on disk.
    #[test]
    fn two_launches_that_never_drew_a_frame_roll_back_and_one_does_not() {
        let b = Bench::new("rollback");
        let with = |failed: u32, previous: Option<&str>| {
            let mut c = b.current("0.2.0");
            c.launches_failed = failed;
            c.previous = previous.map(str::to_owned);
            c
        };

        assert_eq!(
            b.plan(Some(&with(1, Some("0.1.0"))), "0.1.0", &[]),
            Plan::Exec(b.payload.clone()),
            "one failed launch is a machine restarting, not a broken build"
        );
        assert_eq!(
            b.plan(Some(&with(2, Some("0.1.0"))), "0.1.0", &[]),
            Plan::Rollback {
                to: "0.1.0".to_owned(),
                why: "it started twice and never drew a frame"
            }
        );
        assert_eq!(
            b.plan(Some(&with(9, None)), "0.1.0", &[]),
            Plan::RunHere,
            "a rollback with nowhere to go must not name a version that is not on disk"
        );
    }

    /// DEFECT THIS PREVENTS: A WITHDRAWN BUILD GOING ON RUNNING ON MACHINES THAT ALREADY TOOK IT.
    ///
    /// `yanked` is how a release that bricks is taken back, and the taking back has to happen at
    /// the one moment the app can change which binary it is: the launch. A client that only
    /// refused to INSTALL a yanked version would leave every machine that took it yesterday
    /// sitting on it.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `yanked.contains(&pointed)` branch; or place it
    /// after the `pointed <= *self_version` check, which would let a withdrawn build keep running
    /// whenever the entry point happened to be newer.
    #[test]
    fn a_withdrawn_build_is_left_at_the_next_launch_when_there_is_somewhere_to_go() {
        let b = Bench::new("yanked");
        let mut c = b.current("0.2.0");
        c.previous = Some("0.1.0".to_owned());
        assert_eq!(
            b.plan(Some(&c), "0.1.0", &["0.2.0"]),
            Plan::Rollback {
                to: "0.1.0".to_owned(),
                why: "the publisher withdrew it"
            }
        );
        /* Nothing to fall back to: it still runs, and the screen is what says so. */
        let mut alone = b.current("0.2.0");
        alone.previous = None;
        assert_eq!(b.plan(Some(&alone), "0.1.0", &["0.2.0"]), Plan::RunHere);
        /* And an unrelated withdrawal changes nothing. */
        assert_eq!(
            b.plan(Some(&c), "0.1.0", &["0.1.9"]),
            Plan::Exec(b.payload.clone())
        );
    }
}
