//! THE OTHER HALF: the poll thread, the settings block, and the state one screen reads.
//!
//! [`super`]'s header says the core is the half that DECIDES and never the half that acts. This is
//! the half that acts. Everything security-critical is still over there and is reached through it:
//! this file fetches bytes, hands them to [`super::verify::open`], hands the [`super::verify::Verified`]
//! it gets back to [`super::manifest::judge`], and hands the artifact to [`super::install::stage`]
//! and [`super::install::install_app`]. It adds no rule of its own to that chain, which is what
//! keeps the chain testable in a file with no threads in it.
//!
//! # WHAT IS DECIDED HERE, AND IT IS ONLY SCHEDULING
//!
//! When to check, whether the moment is quiet enough to download, what to show, and what the four
//! buttons do. Each of those is a free function or a method on a plain struct wherever a test
//! needs to drive it, because `App::ui` cannot be called from a test at all (`main.rs:2537`).
//!
//! # WHY THIS IS `run.rs` AND NOT `mod.rs`
//!
//! The decision spec puts `Updater`, `UpdaterSettings` and the poll thread in `updater/mod.rs`.
//! They are here instead, and the reason is that `mod.rs`'s own header states, at length, that the
//! module set it heads has no UI, no threads and no network, and names that split as the thing
//! that makes the rest testable. Moving a thread into the file that says it holds none would make
//! a load-bearing comment false; adding a submodule beside `install` and `verify` costs one line
//! and keeps it true. The split the spec actually argues for is preserved exactly.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use semver::Version;

use super::fetch::{self, Wire};
use super::install::{self, Layout, Spawn};
use super::manifest::{self, Artifact, Direction, Local, Outcome, KIND_APP, KIND_DATA};
use super::verify;
use super::Refusal;
use crate::fights::Pulse;

/* ============================================================== the constants == */

/// HOW OFTEN THE MANIFEST IS FETCHED. A POLICY, NOT A MEASUREMENT, AND IT SAYS SO.
///
/// Nothing about six hours was measured, because nothing here can measure it. It is short enough
/// that a reader who leaves the app open across a raid week hears about a fix the same day, and
/// long enough that this app is not a poller: `watcher::POLL_EVERY` is ninety seconds against a
/// service that expects to be polled, and an object store that publishes a release a month is not
/// that. If it ever needs a number, measure the request cost against the R2 free tier first and
/// put that measurement here. Declaring it a policy is the honest form; dressing it as a
/// measurement would not be.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// How long the worker sleeps between passes.
///
/// THE SAME QUARTER SECOND `watcher.rs:641-650` SLEEPS IN, AND FOR THE SAME REASON: a stop or a
/// button press is honoured within a quarter second rather than at the end of a six hour nap. The
/// watcher nests a slice loop inside its poll because its poll is the expensive part; here the
/// whole pass is a handful of comparisons unless something is actually due, so the pass IS the
/// slice and there is no inner loop to get wrong.
pub const TICK: Duration = Duration::from_millis(250);

/// THE LADDER A FAILED DOWNLOAD WAITS ON, IN SECONDS, FLATTENING AT ITS LAST RUNG FOR EVER AFTER.
///
/// # A POLICY, NOT A MEASUREMENT, AND IT SAYS SO
///
/// Nothing here was measured, because nothing here can measure a reader's connection or a bucket's
/// bad afternoon. It is the shape `chat::BACKOFF` already has in this crate and it is short at the
/// front and long at the back for the same two reasons: the common failure is a wifi hop or a
/// laptop lid and the reader is sitting there when it happens, and the uncommon failure is a
/// release whose artifact object is missing or 5xx, which only a new publish fixes, so a client
/// that keeps asking is part of why the host stays busy. The last rung is an hour against a
/// [`CHECK_EVERY`] of six, so a wedged download is retried a handful of times between checks
/// rather than fourteen thousand.
///
/// # THE DEFECT THIS EXISTS TO STOP, WHICH WAS REAL
///
/// The download had no clock at all. `Worker::tick` ran the download block on every 250 ms pass
/// and `download` put the offer back into `pending` on any wire failure, so a missing artifact
/// object meant four requests a second to `updates.ragnarok.systems`, from every client that
/// accepted that manifest, for as long as the app was open and the reader was out of combat, with
/// the Settings screen flipping red on each one. A mid-transfer reset at nine megabytes is the
/// same loop with nine megabytes of traffic per turn. The manifest check was gated by
/// `check_is_due` and the download was gated by the fight pulse and nothing else.
pub const DOWNLOAD_BACKOFF: [u64; 6] = [5, 30, 120, 600, 1800, 3600];

/// The channels this build offers on the Settings screen.
///
/// A LIST IN THE CODE AND NOT A TEXT BOX. The channel is half of a URL and the whole of a
/// promise about what gets installed, and a text box would let a typo point the client at a path
/// that publishes nothing and answer "could not check" forever. `beta` is here before anything is
/// published on it deliberately: the manifest's own `channel` field is inside the signed bytes and
/// is checked (`manifest::judge`), so an empty beta channel is a 404 and never a stable manifest
/// wearing a beta name.
pub const CHANNELS: [&str; 2] = ["stable", "beta"];

/// The wire word for a failure that never reached a decision.
///
/// NOT A [`Refusal`], AND THE DISTINCTION IS THE POINT. `Refusal` is the closed set of ways this
/// feature says no, and every one of them is something the client DECIDED after looking at bytes
/// it had. A connection that dropped is not a decision: nothing was read, nothing was judged, and
/// the right thing to do is try again on the next tick. Folding it into `Refusal` would put a
/// transient network blip in the same set as "somebody served bytes the signing key never signed",
/// which is the exact confusion that enum exists to prevent.
pub const NETWORK_CODE: &str = "Network";

/// The wire word for an offer this build cannot carry out.
///
/// The data-bundle half of the install is specified and partly built (the path guard, the
/// completeness probe and the directory swap are all in [`super::install`]) and its extraction
/// step is not. A manifest that offers a data bundle is therefore refused IN WORDS rather than
/// half-applied, and the app artifact beside it is refused with it, because the ordering rule in
/// the spec exists precisely so a new binary never lands on an old snapshot.
pub const UNSUPPORTED_CODE: &str = "UnsupportedArtifact";

/* ================================================================= the switch == */

/// Whether this module may touch a host at all. OFF until [`allow_updating`] is called, which
/// `main` does once and nothing else does.
///
/// WHY A SWITCH AND NOT A `cfg(test)`. `channel_art.rs:83-105` argues this in full and the argument
/// carries over word for word: several tests in `main.rs` build a whole `App`, and an `App` that
/// constructed an `Updater` would dial `updates.ragnarok.systems` from `cargo test`, which
/// `tests/live_twitch.rs:1-23` states as the thing this suite does not do. A `cfg(test)` would
/// stop that while CHANGING WHAT THE COMPILED-FOR-TEST CODE DOES, which is the shape of defect
/// that hides in exactly the path nobody runs twice. This is one production flag on one line of
/// `main`, and a test can read it.
///
/// IT GUARDS ONLY [`Updater::start`] AND NOT [`Updater::spawn`]. `spawn` takes the wire as an
/// argument and so can be driven from a test with a fake; `start` is the one that builds a real
/// `Https`. Putting the guard on the constructor that dials, rather than on both, is what lets the
/// worker's own behaviour be tested at all.
static UPDATING: AtomicBool = AtomicBool::new(false);

/// Let this module make requests. Called by `main` at startup, and by nothing else.
pub fn allow_updating() {
    UPDATING.store(true, Ordering::Relaxed);
}

/// Whether requests are allowed. `false` in every test binary, because none of them call
/// [`allow_updating`].
pub fn updating_allowed() -> bool {
    UPDATING.load(Ordering::Relaxed)
}

/* ================================================================ the settings == */

/// What the owner may decide about updates.
///
/// # `#[serde(default)]` IS ON THE CONTAINER AND MUST NEVER MOVE ONTO A FIELD
///
/// This is the trap `ingest.rs:647-658` writes out in full, and this struct walks straight into
/// it: field-level `#[serde(default)]` takes `bool::default()`, which is `false`, while
/// container-level `#[serde(default)]` takes `UpdaterSettings::default()`, which is `true` for two
/// of these three. The two spellings are indistinguishable in a diff. Written the wrong way, every
/// settings.json in the field (none of which has an `updater` block, because this is the first
/// build to write one) loads with updates switched OFF and nobody ever finds out, because the
/// symptom is the absence of a thing.
/// `a_settings_file_written_before_updates_existed_still_gets_them` is the test that catches it.
///
/// # NO FIELD HERE HAS ARRIVED AHEAD OF ITS READER
///
/// `enabled` gates the poll, `channel` builds the manifest URL, `auto_download` gates the
/// download. There is deliberately no `verify` toggle (verification is not optional and adding a
/// switch for it is out of scope permanently), no `check_on_launch` (the first check is driven by
/// the snapshot's state, see [`Updater::pump`]) and no `auto_apply` (the restart is always a
/// press). `reach.rs` would go red on a fourth field nothing read, and that floor is the reason to
/// mention it rather than the reason it is true.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UpdaterSettings {
    /// Whether the manifest is fetched at all. Off means the thread runs and does nothing, rather
    /// than not existing: a switch that can be turned back on inside one session is worth more
    /// than a thread saved.
    pub enabled: bool,
    /// `stable` or `beta`. See [`CHANNELS`] and [`fetch::channel_is_safe`].
    pub channel: String,
    /// Whether a known update is fetched without being asked for. Off still checks and still says
    /// what is available; it just waits for the Download button.
    pub auto_download: bool,
    /// Whether a staged update is installed without being asked for.
    ///
    /// ON BY DEFAULT, AND THE REASON IS WHAT INSTALLING DOES NOT DO. It copies the verified
    /// binary in beside the versions already installed and flips a pointer. It does not touch the
    /// running exe, it does not restart anything, and the reader keeps using the version he
    /// started. The new one runs at his next launch, whenever that is.
    ///
    /// SO THE PRESS IT REPLACED WAS ASKING PERMISSION FOR NOTHING. Somebody who has already said
    /// check for updates, and download them, and is then shown a button called Install, is being
    /// asked to confirm a copy into a folder he will never open. The owner put it plainly: the
    /// user should not be installing it.
    ///
    /// THE PREFLIGHT IS WHY THIS IS SAFE TO DO UNATTENDED. `install` spawns the staged binary and
    /// waits for it to come up before the pointer moves, so a build that cannot start never
    /// becomes the one that runs. Off still stages, and still says what is waiting.
    pub auto_install: bool,
}

impl Default for UpdaterSettings {
    /// HAND WRITTEN BECAUSE TWO OF THE THREE DEFAULTS ARE NOT THE ZERO VALUE, which is the same
    /// reason `TrackerSettings` writes its own (`ingest.rs:669`). A derived `Default` here would
    /// ship an app that never checks for updates and never downloads one.
    fn default() -> Self {
        UpdaterSettings {
            /* ON BY DEFAULT. An updater nobody switches on protects nobody: the reason this
             * feature exists is that a fix for a defect the reader has not noticed yet has to
             * reach them without them going looking for it. */
            enabled: true,
            channel: CHANNELS[0].to_owned(),
            /* ON BY DEFAULT, AND IT IS SAFE TO BE, because of what auto-download does NOT do.
             * It fetches and verifies bytes into a staging folder and stops. Nothing is installed,
             * nothing is restarted, and the download itself only starts while no encounter is
             * open (`may_start_download`). The press that changes the running app is still a
             * press. */
            auto_download: true,
            auto_install: true,
        }
    }
}

/* =============================================================== what is shown == */

/// WHERE THE UPDATER IS, AS ONE VALUE THE SETTINGS SCREEN PRINTS.
///
/// Every variant that names a version names a real one, and there is a separate variant for "this
/// session has not checked yet" so that the screen never prints a zero or a stamp that looks like
/// a measurement when nothing has been measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// The thread is running and has not finished a check yet. The honest state for the first
    /// seconds of every session, and the reason `last_check` is an `Option`.
    #[default]
    NeverChecked,
    /// Switched off in settings. Named rather than left looking like a check that never lands.
    Off,
    /// A fetch is in flight right now.
    Checking,
    /// The last check succeeded and named nothing newer.
    UpToDate,
    /// Something is available and is not being fetched. `why` says which of the two reasons it is,
    /// because "waiting because you turned auto-download off" and "waiting because you are in a
    /// fight" are different facts and only one of them is about a setting.
    Known {
        version: String,
        notes_url: Option<String>,
        why: &'static str,
    },
    /// Bytes are moving. `done` and `size` are the real counts out of
    /// [`super::verify::copy_sealed`]; `size` is the length the SIGNED manifest states, so the
    /// percentage is against a figure a key vouched for rather than a `Content-Length` header.
    Downloading {
        version: String,
        done: u64,
        size: u64,
    },
    /// Downloaded, hashed and signature-checked into staging. Nothing on disk has been replaced.
    Downloaded { version: String },
    /// Installed and verified again from disk, and `current.json` now points at it. The running
    /// process is untouched; this version starts at the next launch.
    Installed { version: String },
    /// The last attempt was refused, and this one is not retried on the timer.
    Refused {
        version: Option<String>,
        why: String,
    },
    /// THE CHANNEL COULD NOT BE REACHED, WHICH IS NOT A REFUSAL AND MUST NOT LOOK LIKE ONE.
    ///
    /// `settings::phase_line` states the invariant this variant exists to keep: "Wrong is a
    /// refusal and nothing else, so that a red square on this screen always means a decision was
    /// made against the update rather than that a download is slow". [`NETWORK_CODE`]'s own doc
    /// argues at length that a dropped connection is not a decision, and then `fail` set
    /// `Phase::Refused` for it anyway, so a reader playing offline, behind a captive portal, or
    /// during a two minute outage was shown a red error square and a sentence in the same visual
    /// language the screen reserves for a signature that did not check out.
    ///
    /// `since` IS WHEN IT STARTED FAILING, not when it last failed, so the screen can say how long
    /// rather than repeat "just now" every quarter second.
    Unreachable { since: DateTime<Utc>, why: String },
}

/// The last thing that went wrong, kept until something else goes wrong.
#[derive(Clone, Debug)]
pub struct Failure {
    pub when: DateTime<Utc>,
    /// The version it was about, when it was about one. A manifest that would not verify is about
    /// no version, because nothing in it may be read to find out which.
    pub version: Option<String>,
    /// [`Refusal::code`], or [`NETWORK_CODE`], or [`UNSUPPORTED_CODE`]. The stable word a person
    /// pastes into a bug report.
    pub code: String,
    /// The sentence, which names both what was wrong and what was expected.
    pub sentence: String,
}

/// EVERYTHING THE SETTINGS SCREEN DRAWS, cloned out from under the worker's mutex once a frame.
///
/// A SNAPSHOT AND NOT A HANDLE ONTO THE WORKER. The screen cannot call anything that blocks and
/// cannot hold a lock across a draw, exactly as `watcher::Watcher::status` and
/// `twitch_auth::Auth::view` already work.
#[derive(Clone, Debug, Default)]
pub struct UpdateView {
    pub phase: Phase,
    /// When the last check FINISHED, whatever it concluded. `None` until one has, which is what
    /// lets the screen say "not yet" instead of printing a stamp nothing produced.
    pub last_check: Option<DateTime<Utc>>,
    /// Survives across sessions, out of `state.json`, so "last checked" does not reset to nothing
    /// every launch.
    pub last_check_before: Option<DateTime<Utc>>,
    pub failure: Option<Failure>,
    /// The version `current.json` points at, when the updater has installed one. `None` on a build
    /// nobody has updated, which is every build until the first release.
    pub installed: Option<String>,
    /// Is there a previous data generation on disk for the "put the previous data back" button?
    /// Read from the filesystem on the worker thread, never from the UI thread.
    pub data_previous: bool,
    /// The thread would not start. Named, never unwrapped: see `channel_art.rs:324`.
    pub problem: Option<String>,
    /// WHAT IS WRONG WITH `current.json`, when something is. See [`super::launch::pointer_problem`].
    ///
    /// Separate from `problem` because they are different facts with different remedies: one is
    /// this session having no updater at all, the other is this build running itself and ignoring a
    /// pointer written by something newer, which needs a download by hand.
    pub pointer_note: Option<String>,
    /// Is there an offer that failed for a reason about this machine and can be tried again?
    ///
    /// The Settings screen draws a Try again button on this. The reader is the one who freed the
    /// disk or closed whatever held the file, so the reader is the one who knows the moment has
    /// passed; the backoff ladder is what stops the worker asking on its own.
    pub retry_available: bool,
}

/// What a press asks the worker to do.
///
/// A QUEUE AND NOT THREE `AtomicBool`s. `watcher::Watcher` uses a single `nudge` flag because it
/// has exactly one press; three flags would be three orderings to reason about and a press that
/// could overtake another. A `Vec` under a mutex, drained in order at the top of a pass, has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Check the manifest now rather than at the next tick.
    CheckNow,
    /// Fetch the known update now, whatever `auto_download` says. Still refused while an encounter
    /// is open, because [`super::install::stage`] asks the gate itself.
    Download,
    /// Preflight the staged payload, install it, and flip the pointer.
    Install,
    /// Put `data.previous\` back.
    RestorePreviousData,
}

/* ================================================================== the clocks == */

/// IS A CHECK DUE?
///
/// A FREE FUNCTION SO IT CAN BE TESTED, which is the rule this feature has to follow everywhere:
/// a decision written inside a worker's closure is a decision no test can drive.
///
/// `data_ready` is the snapshot having left `Data::Loading`. THE FIRST CHECK OF A SESSION IS TIED
/// TO THAT STATE AND NOT TO A TIMER, because the two jobs compete for exactly one resource: the
/// snapshot parse reads 24.7 MB of JSON off the same disk the download writes to, and a delay
/// invented to avoid that would be a number nobody measured. Waiting for the state that actually
/// matters needs no number at all.
pub fn check_is_due(
    enabled: bool,
    data_ready: bool,
    last: Option<Instant>,
    now: Instant,
    every: Duration,
) -> bool {
    if !enabled || !data_ready {
        return false;
    }
    match last {
        None => true,
        Some(t) => now.duration_since(t) >= every,
    }
}

/// MAY A DOWNLOAD START?
///
/// The fight gate is [`super::may_start_download`] and it is asked again inside
/// [`super::install::stage`], which is where it actually bites. This adds the two scheduling
/// questions on top of it: the owner has to have allowed it, and there has to be something to
/// fetch that has not already been refused.
pub fn may_fetch(pulse: Pulse, auto_download: bool, asked: bool, pending: bool) -> bool {
    pending && (auto_download || asked) && super::may_start_download(pulse)
}

/// IS A DOWNLOAD ATTEMPT DUE?
///
/// THE OTHER HALF OF [`may_fetch`], AND THE ONE THE DOWNLOAD DID NOT HAVE. `may_fetch` answers
/// whether this is a moment a download is ALLOWED; this answers whether enough time has passed
/// since the last one failed. A free function beside [`check_is_due`], for the same reason that one
/// is: a decision written inside the worker's loop is a decision no test can drive.
///
/// `failures` IS THE COUNT SINCE THE LAST SUCCESS OR THE LAST NEW OFFER, so the ladder walks
/// forwards while one thing keeps failing and starts again the moment anything changes. Zero
/// failures is always due, which is what makes the first attempt immediate.
pub fn download_is_due(
    failures: u32,
    last_attempt: Option<Instant>,
    now: Instant,
    ladder: &[u64],
) -> bool {
    let (Some(t), true) = (last_attempt, failures > 0) else {
        return true;
    };
    /* THE LADDER FLATTENS RATHER THAN RUNNING OFF ITS END. `saturating_sub(1)` indexes the first
     * rung for the first failure, and `min` holds everything past the last one at the last one. */
    let rung = ladder
        .get(usize::try_from(failures.saturating_sub(1)).unwrap_or(usize::MAX))
        .or_else(|| ladder.last())
        .copied()
        .unwrap_or(0);
    now.duration_since(t) >= Duration::from_secs(rung)
}

/* ============================================================ the state on disk == */

/// `update\state.json`: what one session needs to tell the next one.
///
/// A SEPARATE FILE FROM `settings.json` AND FROM `current.json`, and both separations are load
/// bearing. It is not settings because nothing in it is a preference the owner types; it is
/// machine bookkeeping written at precise moments, and routing it through the Settings screen's
/// 700 ms debounce (`settings.rs:1006`) would be wrong in both directions. It is not `current.json`
/// because that file is read on the launch path of every process before a window exists
/// (`launch.rs:46-50`): a parse failure there means the app does not start, so a screen's
/// bookkeeping must never share the file.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Persisted {
    /// The `published` stamp of the newest manifest accepted, PER CHANNEL.
    ///
    /// PER CHANNEL AND NOT ONE STAMP, because one stamp turns a legitimate act into a permanent
    /// refusal. `published` is the replay defence: a manifest older than the last one accepted is
    /// refused (`manifest::judge`). Switch from beta to stable with one shared stamp and the
    /// stable manifest, which is older than the beta one by construction, is refused as a replay
    /// forever. The two channels are two timelines and the file says so.
    pub accepted: BTreeMap<String, DateTime<Utc>>,
    /// When a check last finished, so the screen can say so on the next launch instead of showing
    /// "not yet" to somebody who checked an hour ago.
    pub last_check: Option<DateTime<Utc>>,
    /// The data bundle version the updater installed, for `manifest::Local::installed_data`.
    /// `None` means the snapshot came from somewhere else, which makes any offered bundle newer.
    pub installed_data: Option<String>,
    /// The withdrawn set out of the newest manifest accepted, for the trampoline to read at the
    /// next launch. `launch::plan` takes it as an argument precisely so it reads no files.
    pub yanked: Vec<String>,
    /// Versions that were refused, and will not be fetched again while the manifest keeps offering
    /// the same bytes.
    ///
    /// RETRYING A SIGNATURE FAILURE ON A LOOP TURNS A TRANSIENT ATTACK INTO A PERSISTENT ONE and
    /// buries the message under a spinner. The artifact's hash is stored beside the version
    /// because that is what makes the skip precise: a republished 0.2.0 with different bytes is a
    /// different artifact and is allowed another try, while the same bytes served again are not.
    ///
    /// ONLY REFUSALS THAT ARE ABOUT THE BYTES REACH THIS LIST. See [`Refusal::is_about_the_bytes`]
    /// and [`Persisted::attempts`] for the other half, which was the defect: a disk that filled
    /// once used to be written here and made that release permanently uninstallable on that
    /// machine, with no control anywhere that could clear it and a release pipeline that refuses
    /// to republish the same version with different bytes.
    pub refused: Vec<RefusedVersion>,
    /// How many times each version has been refused for a reason that was about THIS MACHINE.
    ///
    /// A COUNT AND NOT A CLOSED DOOR. A full disk, an antivirus holding the exe open for a second,
    /// a preflight that failed against a driver that was updated ten minutes later: none of these
    /// is evidence about a release, so none of them may be recorded the way a bad signature is.
    /// The count is kept so that the one refusal with a budget ([`Refusal::retries`], which gives
    /// `ArtifactHashMismatch` exactly one retry because two writers and a bad CDN look identical)
    /// can be counted across sessions, and so the screen can say how many times something has been
    /// tried.
    pub attempts: Vec<AttemptedVersion>,
    /// Versions the trampoline rolled AWAY from, which are not offered again on their own.
    ///
    /// # THE LOOP THIS CLOSES
    ///
    /// `install::roll_back` returns `away_from` and its doc says why: so the caller can decline to
    /// offer it again. The trampoline dropped it on the floor, and `grep away_from` found one
    /// production write and no production read. So a build that passed its preflight and then
    /// failed to draw on real launches (a GPU driver the three second smoke got away with, an
    /// antivirus that quarantined the exe after it was written, a hotkey conflict that only appears
    /// with the full session running) was rolled back at the third launch, re-offered by the next
    /// check, re-downloaded (10.8 MB), re-installed, and failed again the next session, forever.
    ///
    /// A version listed here is offered again only when the manifest names something STRICTLY
    /// ABOVE it, which is the publisher having shipped a fix.
    pub rolled_back_from: Vec<String>,
    /// The offer whose payload is already staged, verified, and waiting for the Install press.
    ///
    /// PERSISTED SO THAT CLOSING THE APP DOES NOT THROW THE DOWNLOAD AWAY. It was in memory only,
    /// so a reader who auto-downloaded 0.2.0, did not press Install and closed the app paid for
    /// the whole 10.8 MB again on the next launch, over a byte-identical file that was already on
    /// disk and already verified, every launch, forever. The whole artifact is kept rather than
    /// just the version because `install_app` needs the signed `size`, `sha256` and `signature`
    /// again, with no manifest and no network.
    pub staged: Option<StagedOffer>,
    /// WHEN THE WHOLE-DOCUMENT FIELDS WERE LAST SET BY A WRITER.
    ///
    /// Used only by [`Persisted::merged_over`], and only for the fields that cannot be merged by
    /// taking a maximum or a union: `yanked` and `installed_data` are pictures of a moment rather
    /// than accumulations, so the merge needs to know which of two writers' pictures is newer.
    /// Every other field in this struct is monotone and needs no stamp.
    pub stamped: Option<DateTime<Utc>>,
}

/// One version refused for a reason that was about this machine, and how many times.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AttemptedVersion {
    pub version: String,
    pub sha256: String,
    pub code: String,
    pub when: DateTime<Utc>,
    pub count: u32,
}

/// A payload on disk, waiting for a press.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StagedOffer {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    pub app: Artifact,
}

/// One version this client will not fetch again.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RefusedVersion {
    pub version: String,
    /// The `sha256` the signed manifest gave for the artifact, or an empty string when the failure
    /// happened before any artifact was named.
    pub sha256: String,
    pub code: String,
    pub when: DateTime<Utc>,
}

/// Where [`Persisted`] lives.
pub fn state_path(l: &Layout) -> PathBuf {
    l.root().join("update").join("state.json")
}

/// Read it, answering an unreadable or absent file with the default.
///
/// NEVER A `Result`, AND THAT IS THE SAME CALL `install::read_current` MAKES. The default is
/// "nothing has been accepted, nothing has been refused", which costs one extra manifest fetch and
/// re-refuses anything that is still bad. A `Result` here would invite a `?` on a path whose only
/// honest failure mode is to carry on.
pub fn read_state(l: &Layout) -> Persisted {
    std::fs::read_to_string(state_path(l))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// MERGE THIS WRITER'S PICTURE OVER WHAT IS ON DISK, AND WRITE THE RESULT.
///
/// Returns what is now on disk, which the caller must adopt: after a merge this writer's own
/// in-memory copy is out of date by definition.
///
/// # WHY A WHOLE-FILE WRITE WAS WRONG
///
/// `state.json` was read once at spawn and rewritten whole, with no re-read, no merge and no lock.
/// Nothing in this app stops two copies running: there is no single-instance guard anywhere in the
/// crate and [`super::launch::ENTRY_ENV`]'s own doc contemplates "two copies of the app started
/// from two different folders". Both get the same [`Layout::platform`] root. So: copy A is open all
/// evening; copy B checks, accepts a manifest that WITHDRAWS 0.2.0, and writes `yanked`; copy A
/// later refuses something and writes its own hours-old struct back, and the withdrawal is gone,
/// the replay floor walks backwards, and every refusal B recorded is erased. The withdrawn build
/// is then un-withdrawn on that machine and `launch::plan` will exec it at the next launch, which
/// is precisely the failure the yank mechanism exists to prevent.
///
/// # THE MERGE RULES, AND WHY EACH FIELD GETS THE ONE IT GETS
///
/// Most of this file is monotone, which is what makes the merge obvious rather than a guess:
/// `accepted` is a replay floor per channel and floors only rise, so the maximum wins; `refused`,
/// `attempts` and `rolled_back_from` are accumulations keyed by what they are about, so the union
/// wins and the higher attempt count wins inside it; `last_check` only moves forwards.
///
/// `yanked`, `installed_data` and `staged` are not accumulations, they are pictures of a moment,
/// and for those the newer [`Persisted::stamped`] wins. A writer that did not touch them keeps
/// whatever stamp it read, so it loses to a writer that did, which is exactly the case above.
pub fn write_state(l: &Layout, p: &Persisted) -> Result<Persisted, Refusal> {
    let path = state_path(l);
    install::with_lock(&path, || {
        let merged = p.merged_over(&read_state(l));
        write_state_inner(l, &merged)?;
        Ok(merged)
    })
}

/// Write it through a temp file and a rename, with the lock already held.
///
/// THE SAME DISCIPLINE `settings.rs:611` AND `install::write_current` USE. A torn `state.json`
/// would be read back as the default on the next launch, which silently forgets every refusal
/// recorded in it, and a forgotten refusal is a download this client already decided not to make.
fn write_state_inner(l: &Layout, p: &Persisted) -> Result<(), Refusal> {
    let path = state_path(l);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| super::io("create", d, &e))?;
    }
    let body = serde_json::to_vec_pretty(p).map_err(|e| Refusal::Io {
        doing: "encode the update state for",
        path: path.clone(),
        why: e.to_string(),
    })?;
    let tmp = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    {
        use std::io::Write as _;
        let mut f = std::fs::File::create(&tmp).map_err(|e| super::io("create", &tmp, &e))?;
        f.write_all(&body)
            .map_err(|e| super::io("write", &tmp, &e))?;
        f.sync_all().map_err(|e| super::io("flush", &tmp, &e))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| super::io("rename into place", &path, &e))
}

/// RECORD THAT THE TRAMPOLINE WENT BACK, so the version it fled is not offered again on its own.
///
/// A FREE FUNCTION HERE AND NOT A FEW LINES IN `main`. The trampoline is unreachable from a test
/// (`main` cannot be called from one), and this is the half of the rollback that decides something.
/// `main` calls it and nothing else does.
pub fn note_rolled_back(l: &Layout, away_from: &str) -> Result<(), Refusal> {
    let mut p = read_state(l);
    if p.rolled_back_from.iter().any(|v| v == away_from) {
        return Ok(());
    }
    p.rolled_back_from.push(away_from.to_owned());
    write_state(l, &p).map(|_| ())
}

impl Persisted {
    /// Has this exact artifact already been refused for good?
    pub fn has_refused(&self, version: &str, sha256: &str) -> bool {
        self.refused
            .iter()
            .any(|r| r.version == version && r.sha256 == sha256)
    }

    /// How many times has this exact artifact failed for a reason about this machine?
    pub fn attempts_at(&self, version: &str, sha256: &str) -> u32 {
        self.attempts
            .iter()
            .find(|r| r.version == version && r.sha256 == sha256)
            .map(|r| r.count)
            .unwrap_or(0)
    }

    /// Record a refusal in the half of the file it belongs in.
    ///
    /// # THE SPLIT IS [`Refusal::is_about_the_bytes`] AND THE BUDGET IS [`Refusal::retries`]
    ///
    /// Everything used to go into `refused`, which `check` then skips forever. That is right for a
    /// signature that did not check out and catastrophic for a disk that filled: the release
    /// pipeline refuses to republish a version with different bytes, so a version filed there
    /// could never be installed on that machine again, and there is no control anywhere in the
    /// UPDATES section that clears the list.
    ///
    /// A refusal with a budget moves to the permanent list once the budget is spent, which is what
    /// gives `ArtifactHashMismatch` its one retry: a collision between two copies of the app
    /// staging the same file and a corrupted transfer look identical, and a second mismatch over
    /// the same artifact is a statement about the artifact.
    ///
    /// THE LISTS ARE BOUNDED. Both are keyed by version and replaced rather than appended, so a
    /// manifest republished every six hours with a bad signature cannot grow this file forever.
    pub fn refuse(&mut self, version: &str, sha256: &str, r: &Refusal) {
        let code = r.code();
        let budget = r.retries();
        let spent = self.attempts_at(version, sha256).saturating_add(1);
        if spent > budget {
            self.attempts
                .retain(|a| !(a.version == version && a.sha256 == sha256));
            self.refused.retain(|x| x.version != version);
            self.refused.push(RefusedVersion {
                version: version.to_owned(),
                sha256: sha256.to_owned(),
                code: code.to_owned(),
                when: Utc::now(),
            });
            return;
        }
        self.attempts
            .retain(|a| !(a.version == version && a.sha256 == sha256));
        self.attempts.push(AttemptedVersion {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
            code: code.to_owned(),
            when: Utc::now(),
            count: spent,
        });
    }

    /// Forget everything recorded against an artifact that has just succeeded.
    pub fn forgive(&mut self, version: &str, sha256: &str) {
        self.attempts
            .retain(|a| !(a.version == version && a.sha256 == sha256));
    }

    /// Is this version one the trampoline went back FROM, with nothing newer published since?
    ///
    /// `newest` IS THE VERSION THE MANIFEST NOW PUBLISHES. A rolled-back version is offerable again
    /// the moment a manifest names something strictly above it, because that is the publisher
    /// having shipped a fix; until then, re-offering it is a 10.8 MB download, an install, two dead
    /// launches and a rollback, every session, for ever.
    pub fn was_rolled_back(&self, version: &Version) -> bool {
        self.rolled_back_from
            .iter()
            .filter_map(|v| Version::parse(v).ok())
            .any(|v| &v == version)
    }

    /// Mark the whole-document fields as having been set NOW. See [`Persisted::stamped`].
    fn touch(&mut self) {
        self.stamped = Some(Utc::now());
    }

    /// This writer's picture, merged over whatever is on disk. See [`write_state`].
    pub fn merged_over(&self, disk: &Persisted) -> Persisted {
        let mut out = self.clone();

        /* A REPLAY FLOOR ONLY EVER RISES, so the maximum is the merge and no stamp is needed. */
        for (channel, when) in &disk.accepted {
            let mine = out.accepted.get(channel).copied();
            /* `is_none() || is_some_and(..)` AND NOT `is_none_or(..)`, which reads better and is
             * stable since 1.82 against this workspace's 1.80 floor. */
            if mine.is_none() || mine.is_some_and(|m| m < *when) {
                out.accepted.insert(channel.clone(), *when);
            }
        }
        if disk.last_check > out.last_check {
            out.last_check = disk.last_check;
        }

        /* ACCUMULATIONS, KEYED BY WHAT THEY ARE ABOUT. Losing one of these loses a decision this
         * client already made, so the union is the only safe merge. */
        for r in &disk.refused {
            if !out
                .refused
                .iter()
                .any(|x| x.version == r.version && x.sha256 == r.sha256)
            {
                out.refused.push(r.clone());
            }
        }
        for a in &disk.attempts {
            match out
                .attempts
                .iter_mut()
                .find(|x| x.version == a.version && x.sha256 == a.sha256)
            {
                /* THE HIGHER COUNT WINS. Two writers that each failed once really have failed
                 * twice between them, and a budget that reset on every merge would be no budget. */
                Some(mine) if mine.count < a.count => *mine = a.clone(),
                Some(_) => {}
                None => out.attempts.push(a.clone()),
            }
        }
        /* A VERSION THIS MACHINE FLED IS STILL A VERSION THIS MACHINE FLED, whichever copy of the
         * app was running when it happened. */
        for v in &disk.rolled_back_from {
            if !out.rolled_back_from.contains(v) {
                out.rolled_back_from.push(v.clone());
            }
        }

        /* AND THE PICTURES, WHERE THE NEWER WRITER WINS OUTRIGHT. See `stamped`: a writer that did
         * not touch these keeps the stamp it read, so it loses to one that did. */
        if disk.stamped > out.stamped {
            out.yanked = disk.yanked.clone();
            out.installed_data = disk.installed_data.clone();
            out.staged = disk.staged.clone();
            out.stamped = disk.stamped;
        }
        out
    }
}

/* ================================================================== the worker == */

/// The pulse, as one byte the UI thread writes and the worker reads.
///
/// AN ATOMIC AND NOT A CHANNEL, because this is a level and not an event: the worker wants to know
/// what is true NOW, at the instant it is about to start a download, and a queue of past pulses
/// would answer a different question. Written once per heartbeat from [`Updater::pump`], read once
/// per pass.
fn pulse_byte(p: Pulse) -> u8 {
    match p {
        Pulse::Fighting => 0,
        Pulse::Holding => 1,
        Pulse::Closed => 2,
    }
}

fn byte_pulse(b: u8) -> Pulse {
    match b {
        0 => Pulse::Fighting,
        1 => Pulse::Holding,
        _ => Pulse::Closed,
    }
}

/// What the UI thread and the worker share, other than the view.
struct Live {
    stop: AtomicBool,
    /// Starts at `Fighting`.
    ///
    /// THE PESSIMISTIC START IS DELIBERATE. It is written by the first heartbeat, which runs
    /// before any check can be due (`check_is_due` also wants `data_ready`, which no heartbeat has
    /// set yet either), so in practice it is never read at this value. It is `Fighting` anyway
    /// because the one way this could be wrong is a download starting mid-pull, and a default that
    /// cannot cause that is worth having even where it cannot be reached.
    pulse: AtomicU8,
    data_ready: AtomicBool,
    settings: Mutex<UpdaterSettings>,
    asks: Mutex<Vec<Ask>>,
}

/// A poisoned mutex holds a perfectly good value; a panic on the worker must not take the UI down.
/// `watcher.rs:684` and `twitch_auth.rs:915` recover the same way.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// THE UPDATER: one field on `App`, one thread, and four buttons.
///
/// ONE PER PROCESS AND NOT ONE PER WINDOW. Each pop-out tool window builds its own `Ingest`
/// (`windows.rs:729`), which is why `TrackerSettings` had to move onto `Settings`; an updater
/// following that shape would mean several windows downloading the same ten megabytes at once.
/// This lives on `App` beside `watcher` (`main.rs:432`), the Settings screen is drawn in the main
/// window only (`main.rs:2292`), and a restart takes every pop-out with it, so the fight gate
/// covers them all at once.
pub struct Updater {
    shared: Arc<Mutex<UpdateView>>,
    live: Arc<Live>,
}

impl Drop for Updater {
    /// Ask the thread to stop at its next pass and DO NOT JOIN. A download can be mid-flight for
    /// up to [`fetch::DOWNLOAD_DEADLINE`] and the UI thread must not stall on that. Same as
    /// `watcher.rs:675`.
    fn drop(&mut self) {
        self.live.stop.store(true, Ordering::Relaxed);
    }
}

impl Updater {
    /// Spawn the worker with the production wire and the compiled-in keys.
    ///
    /// `None` WHEN THERE IS NOWHERE TO PUT THE FILES. `Layout::platform` is `None` on a platform
    /// with no local data directory, and an updater with nowhere to stage a download is not a
    /// degraded updater, it is no updater, and the Settings screen says so rather than showing a
    /// spinner that never resolves.
    pub fn start(ctx: &egui::Context, settings: &UpdaterSettings) -> Option<Updater> {
        if !updating_allowed() {
            /* NO THREAD AT ALL IN A TEST BINARY. Not a thread that refuses to fetch: a thread
             * would still write `state.json` under the owner's real `%LOCALAPPDATA%`, which is the
             * same class of accident `Settings::save` refuses under `cfg(test)` for. */
            return None;
        }
        let layout = Layout::platform()?;
        Some(Self::spawn(
            layout,
            Box::new(fetch::Https::default()),
            verify::KEYS,
            Some(ctx.clone()),
            settings,
            CHECK_EVERY,
        ))
    }

    /// The same worker with a caller-supplied wire, key list, clock and layout.
    ///
    /// WHAT THE TESTS USE, and what makes every branch below reachable without a network, without
    /// a six hour wait and without touching the owner's install. It is the shape
    /// `watcher::Watcher::spawn` already has, for the same reason.
    pub fn spawn(
        layout: Layout,
        wire: Box<dyn Wire>,
        keys: &'static [&'static str],
        ctx: Option<egui::Context>,
        settings: &UpdaterSettings,
        every: Duration,
    ) -> Updater {
        let persisted = read_state(&layout);
        let pointer = install::read_current(&layout);
        let shared = Arc::new(Mutex::new(UpdateView {
            last_check_before: persisted.last_check,
            installed: pointer.as_ref().map(|c| c.version.clone()),
            /* SAID ON THE SCREEN AND NOT ONLY IN A LOG. `launch::plan` answers `RunHere` for a
             * pointer shape it does not know, which is safe and silent, and silent is how an app
             * that reinstalls the same version every session for ever goes unreported. */
            pointer_note: super::launch::pointer_problem(pointer.as_ref()),
            data_previous: layout.data_previous().is_dir(),
            ..UpdateView::default()
        }));
        let live = Arc::new(Live {
            stop: AtomicBool::new(false),
            pulse: AtomicU8::new(pulse_byte(Pulse::Fighting)),
            data_ready: AtomicBool::new(false),
            settings: Mutex::new(settings.clone()),
            asks: Mutex::new(Vec::new()),
        });
        let me = Updater {
            shared: shared.clone(),
            live: live.clone(),
        };

        /* THE RUNNING VERSION IS `titlebar::version()`, WHICH IS `env!("CARGO_PKG_VERSION")`
         * (`titlebar.rs:274`) AND SO CANNOT DISAGREE WITH `Cargo.toml`. A build whose own version
         * will not parse as semver can compare nothing, so it says that in the view and runs a
         * thread that would answer every question wrongly. */
        let running = match Version::parse(crate::titlebar::version()) {
            Ok(v) => v,
            Err(e) => {
                lock(&shared).problem = Some(format!(
                    "this build's own version, {:?}, is not a version this can compare: {e}",
                    crate::titlebar::version()
                ));
                return me;
            }
        };

        let spawned = std::thread::Builder::new()
            .name("grimoire-update".to_owned())
            .spawn(move || {
                let mut w = Worker {
                    layout,
                    wire,
                    keys,
                    running,
                    ctx,
                    shared,
                    live,
                    persisted,
                    every,
                    pending: None,
                    staged: None,
                    last_check: None,
                    download_failures: 0,
                    last_download: None,
                    paint: crate::chat::Coalescer::new(Instant::now()),
                };
                w.recover_staged();
                w.run();
            });
        if let Err(e) = spawned {
            /* NO THREAD MEANS NO CHECKS, EVER, AND THE SCREEN HAS TO SAY SO. Left unsaid it reads
             * as a check that has not landed yet, which is `Phase::NeverChecked` and is a lie
             * after the first minute. `channel_art.rs:324`, `ingest.rs:2639` and `watcher.rs:653`
             * each answer this the same way: in words, never an unwrap. */
            let mut v = lock(&me.shared);
            v.problem = Some(format!("could not start the update thread: {e}"));
            v.phase = Phase::Off;
        }
        me
    }

    /// WHAT THE UI THREAD HANDS THE WORKER, ONCE PER HEARTBEAT.
    ///
    /// # THIS IS CALLED FROM `App::heartbeat` AND NEVER FROM `App::ui`
    ///
    /// eframe calls `ui` only while there is something to draw, and an app minimised to the tray
    /// with no tool window open draws nothing; `logic` is the callback that runs then. Two shipped
    /// defects came from exactly this (`Hotkeys::poll` and `Ingest::tail`, both written up at
    /// `main.rs:1709-1760`) and two source-text tests now stand over them. A third stands over
    /// this one.
    ///
    /// # IT IS CHEAP BY CONSTRUCTION
    ///
    /// Two atomic stores and one mutex that is never held across a network call or a draw. Nothing
    /// here blocks, because the heartbeat runs on the frame.
    pub fn pump(&self, pulse: Pulse, data_ready: bool, settings: &UpdaterSettings) {
        self.live.pulse.store(pulse_byte(pulse), Ordering::Relaxed);
        self.live.data_ready.store(data_ready, Ordering::Relaxed);
        /* COMPARED BEFORE IT IS WRITTEN, so the ordinary frame takes the lock and puts it down
         * again. `reconfigure` on `Ingest` (`ingest.rs:2664`) early-returns the same way and for
         * the same reason: the settings screen writes on a 700 ms debounce and this runs sixty
         * times a second. */
        let mut held = lock(&self.live.settings);
        if *held != *settings {
            *held = settings.clone();
        }
    }

    /// The snapshot the Settings screen draws. A clone of one small struct; safe every frame.
    pub fn view(&self) -> UpdateView {
        lock(&self.shared).clone()
    }

    /// Queue a press. Returns at once; the worker acts within [`TICK`].
    pub fn ask(&self, a: Ask) {
        lock(&self.live.asks).push(a);
    }
}

/// Everything the worker owns outright. None of it is shared, which is why none of it needs a
/// lock: the shared parts are `shared` and `live`.
struct Worker {
    layout: Layout,
    wire: Box<dyn Wire>,
    keys: &'static [&'static str],
    running: Version,
    ctx: Option<egui::Context>,
    shared: Arc<Mutex<UpdateView>>,
    live: Arc<Live>,
    persisted: Persisted,
    every: Duration,
    /// The offer the last accepted manifest made, and the app artifact chosen out of it.
    pending: Option<Pending>,
    /// The offer whose payload is staged and verified on disk, waiting for the Install press.
    ///
    /// THE WHOLE OFFER AND NOT JUST ITS VERSION, AND THAT IS A DEFECT FIX RATHER THAN A TIDY-UP.
    /// It held a `Version`, and `install` paired it with whatever was in `pending` to get the
    /// artifact back. Those two are written by different events: `download` sets this one, and a
    /// later `check` can replace `pending` with a NEWER offer while a payload for the older
    /// version is still sitting in staging. `install` would then have carried version 0.2.0 and
    /// 0.3.0's `size`, `sha256` and `signature` into the same call. It fails safe, because
    /// `install_app` re-verifies from disk and the seal would not match, but it fails with
    /// `StagedFileChangedOnDisk`, which tells a person their disk was tampered with when what
    /// actually happened is that a second release was published while they were reading.
    ///
    /// Holding the pair together makes the mismatch UNREPRESENTABLE, which is worth more than a
    /// test over it: there is no longer a state for a test to reach.
    staged: Option<Pending>,
    last_check: Option<Instant>,
    /// HOW MANY TIMES THE CURRENT OFFER HAS FAILED TO DOWNLOAD, and when the last attempt was.
    ///
    /// The pair is the whole of the download's clock; see [`download_is_due`] and
    /// [`DOWNLOAD_BACKOFF`] for what it stops, which was four requests a second to the update host
    /// for as long as the app was open. Both are cleared by a success and by a new offer, so the
    /// ladder is about one thing going wrong repeatedly rather than about the app's uptime.
    download_failures: u32,
    last_download: Option<Instant>,
    /// The repaint valve. A download reports progress once per 64 KiB chunk
    /// (`verify::CHUNK`), which for a ten megabyte artifact is many hundreds of events, and
    /// `chat.rs:1048-1053` records what happens when a burst like that is relayed one repaint per
    /// event. A manifest check does NOT go through it: `ytchat/surface.rs:25` records the decision
    /// not to copy this valve where the source does not burst, and a fetch every six hours is that
    /// case, so a check wakes the UI directly.
    paint: crate::chat::Coalescer,
}

/// An offer this client has accepted and not yet acted on.
struct Pending {
    version: Version,
    notes_url: Option<String>,
    app: Artifact,
}

impl Worker {
    /// TAKE BACK A PAYLOAD THIS MACHINE ALREADY DOWNLOADED AND VERIFIED.
    ///
    /// # THE WASTE THIS ENDS
    ///
    /// `staged` was in memory only and `Phase::Downloaded` did not survive a restart. A reader who
    /// auto-downloaded 0.2.0, did not press Install and closed the app paid for the whole 10.8 MB
    /// again on the next launch, writing it over a byte-identical file that was already on disk and
    /// already checked, and again the launch after that, for as long as they went on not pressing
    /// Install. The staging tree was never pruned either, because `prune` only walks `app\`.
    ///
    /// # THE FILE IS RE-VERIFIED AND NOT TRUSTED
    ///
    /// `check_file` is exactly the no-network, no-manifest re-verification this needs, and it is
    /// the same call `install_app` makes before it runs anything. A staged file that no longer
    /// matches its seal is deleted and forgotten rather than offered: between two sessions anything
    /// at all can have happened to a file in a user-writable folder.
    fn recover_staged(&mut self) {
        let Some(saved) = self.persisted.staged.clone() else {
            self.prune_staging(None);
            return;
        };
        let forget = |me: &mut Worker| {
            me.persisted.staged = None;
            me.persisted.touch();
            me.save_state();
            me.prune_staging(None);
        };
        let Ok(version) = Version::parse(&saved.version) else {
            forget(self);
            return;
        };
        let path = self.layout.staging(&version).join(install::exe_name());
        let seal = verify::Seal {
            size: saved.app.size,
            sha256: &saved.app.sha256,
            signature: &saved.app.signature,
        };
        if let Err(why) = verify::check_file(&path, seal, self.keys) {
            log::info!(
                "the payload staged for {version} is not the one that was staged ({why}); it will \
                 be fetched again"
            );
            forget(self);
            return;
        }
        log::info!("{version} was already staged and still verifies; it was not fetched again");
        self.prune_staging(Some(&version));
        self.staged = Some(Pending {
            version: version.clone(),
            notes_url: saved.notes_url.clone(),
            app: saved.app,
        });
        self.set_phase(Phase::Downloaded {
            version: version.to_string(),
        });
    }

    /// Remove every staging tree except the one version worth keeping.
    ///
    /// `prune` ONLY WALKS `app\`, so without this a superseded or abandoned staging tree has nobody
    /// at all to delete it and ten megabytes sit in `%LOCALAPPDATA%` for ever.
    fn prune_staging(&self, keep: Option<&Version>) {
        let root = self.layout.root().join("update").join("staging");
        let Ok(entries) = std::fs::read_dir(&root) else {
            return;
        };
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if keep.is_some_and(|k| k.to_string() == name) {
                continue;
            }
            let _ = std::fs::remove_dir_all(e.path());
        }
    }

    fn run(&mut self) {
        loop {
            if self.live.stop.load(Ordering::Relaxed) {
                return;
            }
            let settings = lock(&self.live.settings).clone();
            let asks: Vec<Ask> = std::mem::take(&mut lock(&self.live.asks));
            for a in asks {
                if self.live.stop.load(Ordering::Relaxed) {
                    return;
                }
                self.act(a, &settings);
            }
            if self.live.stop.load(Ordering::Relaxed) {
                return;
            }
            self.tick(&settings, false);
            std::thread::sleep(TICK);
        }
    }

    /// One pass. `asked` is true when a press, rather than the clock, brought us here.
    fn tick(&mut self, s: &UpdaterSettings, asked: bool) {
        if !s.enabled {
            /* SAID, NOT LEFT BLANK. `Phase::Off` is a different fact from `NeverChecked`, and a
             * screen that showed the second for a switched-off updater would be describing a
             * check that is coming. */
            self.set_phase(Phase::Off);
            return;
        }
        /* ONCE THE POINTER HAS FLIPPED THERE IS NOTHING LEFT TO ASK. Checking on after an install
         * would find the same manifest, judge the same version, and either say "up to date"
         * (overwriting the one line the reader needs, which is that a restart is waiting) or
         * offer the version already installed. */
        if matches!(self.phase(), Phase::Installed { .. }) {
            return;
        }
        let now = Instant::now();
        if asked
            || check_is_due(
                true,
                self.live.data_ready.load(Ordering::Relaxed),
                self.last_check,
                now,
                self.every,
            )
        {
            self.last_check = Some(now);
            self.check(s);
        }
        let pulse = byte_pulse(self.live.pulse.load(Ordering::Relaxed));
        /* THE DOWNLOAD HAS A CLOCK NOW, AND UNTIL IT DID THIS LINE WAS THE WHOLE DEFECT. `tick`
         * runs every 250 ms and `download` puts the offer back on any wire failure, so a missing
         * or briefly 5xx artifact object meant four requests a second, from every client that
         * accepted that manifest, for as long as the app was open. See `DOWNLOAD_BACKOFF`. */
        if self.staged.is_none()
            && may_fetch(pulse, s.auto_download, false, self.pending.is_some())
            && download_is_due(
                self.download_failures,
                self.last_download,
                now,
                &DOWNLOAD_BACKOFF,
            )
        {
            self.download(pulse);
        }
    }

    fn act(&mut self, a: Ask, s: &UpdaterSettings) {
        match a {
            Ask::CheckNow => {
                /* A PRESS OVERRIDES THE CLOCK AND NOT THE SWITCH. `tick` still refuses when
                 * `enabled` is false, because a Check now button that worked while the feature was
                 * off would make the switch a lie. The screen hides the button in that state; this
                 * is the half that does not depend on the screen being right. */
                self.tick(s, true);
            }
            Ask::Download => {
                let pulse = byte_pulse(self.live.pulse.load(Ordering::Relaxed));
                if may_fetch(pulse, s.auto_download, true, self.pending.is_some()) {
                    /* A PRESS IS NOT PACED BY THE LADDER, AND THAT IS THE POINT OF THE BUTTON. The
                     * ladder exists to stop the WORKER asking on its own; the reader is the one who
                     * freed the disk or reconnected the wifi and knows the moment has passed. One
                     * request per press is not a loop. */
                    self.download_failures = 0;
                    self.download(pulse);
                } else if self.pending.is_some() {
                    /* THE PRESS IS ANSWERED EVEN WHEN IT IS REFUSED. A button that does nothing
                     * visible is a button a person presses again. */
                    self.waiting_because(super::pulse_word(pulse));
                }
            }
            Ask::Install => self.install(),
            Ask::RestorePreviousData => self.restore_data(),
        }
    }

    /* ---------------------------------------------------------------- checking -- */

    fn check(&mut self, s: &UpdaterSettings) {
        if !fetch::channel_is_safe(&s.channel) {
            self.fail(
                None,
                "BadChannel",
                format!(
                    "{:?} is not a channel name this can put in a URL; pick one on the Settings \
                     screen",
                    s.channel
                ),
            );
            return;
        }
        self.set_phase(Phase::Checking);
        let url = fetch::manifest_url(&s.channel);
        let body = match self.wire.manifest(&url) {
            Ok(b) => b,
            Err(e) => {
                /* A DROPPED CONNECTION IS NOT A DECISION. It is recorded as a failure so the
                 * screen is honest about it, and it is NOT written into `persisted.refused`,
                 * because nothing was refused: the next tick tries again. */
                self.mark_checked();
                self.unreachable(format!("could not reach the update channel: {e}"));
                return;
            }
        };

        /* THE SIGNATURE GATE, AND NOTHING READS A FIELD BEFORE IT. `judge` takes a `Verified` and
         * `verify::open_with` is the only thing that makes one, so the ordering rule is a type
         * here and not a comment.
         *
         * `open_with(&body, self.keys)` AND NOT `open(&body)`, AND THIS WAS WRONG FOR ONE BUILD.
         * `open` is `open_with` against the compiled-in `verify::KEYS`, which is what production
         * passes in, so both spellings behave identically in the shipped binary and the mistake is
         * invisible there. What it destroys is the SEAM: a worker that ignores the key list it was
         * handed cannot be driven by a test signer, so every rule downstream of this line, the
         * channel check, the replay floor, the version rules, the artifact selection, becomes
         * untestable at the layer production actually uses. That is the same defect
         * `twitch_auth.rs:142-155` records in the other direction, and it was caught here by
         * `the_check_never_runs_on_the_thread_that_draws` refusing its own fixture. */
        let verified = match verify::open_with(&body, self.keys) {
            Ok(v) => v,
            Err(r) => {
                self.mark_checked();
                self.refuse(None, "", &r);
                return;
            }
        };

        let last = self.persisted.accepted.get(&s.channel).copied();
        let decision = match manifest::judge(
            &verified,
            &Local {
                running: &self.running,
                channel: &s.channel,
                os: manifest::this_os(),
                arch: manifest::this_arch(),
                last_accepted_published: last,
                /* THE CLOCK IS READ HERE AND NOT IN `judge`, for the reason that file gives: it
                 * reaches nothing, and a `Utc::now()` inside it would make the expiry rule the one
                 * rule over there that a test could only drive by waiting. This is the half that
                 * is allowed to read a clock. */
                now: Utc::now(),
                installed_data: self.persisted.installed_data.as_deref(),
            },
        ) {
            Ok(d) => d,
            Err(r) => {
                self.mark_checked();
                /* THE VERSION IS NAMED WHENEVER THE DOCUMENT VERIFIED, because at that point a
                 * publisher wrote it and the number in it is a fact about the release rather than
                 * a string an attacker chose. */
                let named = Some(verified.doc().version.clone());
                self.refuse(named, "", &r);
                return;
            }
        };

        /* ACCEPTED, SO THE REPLAY FLOOR MOVES. Written only here, after every gate, which is what
         * makes it a floor rather than a record of what was served. */
        let doc = verified.doc();
        self.persisted
            .accepted
            .insert(s.channel.clone(), doc.published);
        /* `yanked` IS A PICTURE AND NOT AN ACCUMULATION, so writing it is what stamps this
         * writer as the newest one to have set the whole-document fields. See
         * `Persisted::merged_over`: without the stamp, a second copy of the app writing its own
         * hours-old picture back would un-withdraw a build that had just been withdrawn. */
        self.persisted.yanked = doc.yanked.clone();
        self.persisted.touch();
        self.persisted.last_check = Some(Utc::now());
        if !decision.unknown_fields.is_empty() {
            /* KEPT AND IGNORED, WHICH IS THE STATED RULE. Logged rather than shown: a field this
             * build does not know is a fact about the pipeline and not a decision the reader can
             * make. */
            log::info!(
                "update manifest carried fields this build does not name: {}",
                decision.unknown_fields.join(", ")
            );
        }
        self.save_state();
        self.mark_checked();

        match decision.outcome {
            Outcome::UpToDate => {
                self.pending = None;
                if decision.running_withdrawn {
                    /* THE RUNNING VERSION HAS BEEN WITHDRAWN AND THERE IS NOTHING NEWER. There is
                     * nothing this half can do about it: the remedy is the trampoline falling back
                     * to `previous` at the next launch, which is why `yanked` was just written to
                     * `state.json`. Saying so is the whole of the job here. */
                    self.set_phase(Phase::Refused {
                        version: Some(self.running.to_string()),
                        why: format!(
                            "this version, {}, has been withdrawn and nothing newer is published \
                             yet. The previous version will be started at the next launch if one \
                             is still on disk.",
                            self.running
                        ),
                    });
                } else {
                    self.set_phase(Phase::UpToDate);
                }
            }
            Outcome::Offer(o) => {
                let app = match choose_app_step(&o.steps) {
                    Ok(a) => a,
                    Err(why) => {
                        self.pending = None;
                        self.fail(Some(o.version.to_string()), UNSUPPORTED_CODE, why);
                        return;
                    }
                };
                /* A VERSION THIS MACHINE ALREADY FLED IS NOT OFFERED AGAIN ON ITS OWN.
                 *
                 * THE LOOP THIS CLOSES. A build can pass the three second preflight and still fail
                 * to draw on real launches: a GPU driver the smoke run got away with, an antivirus
                 * that quarantines the exe after it is first written, a hotkey conflict that only
                 * appears with the full session running. The trampoline rolls it back at the third
                 * launch, and the next check used to offer it straight back: 10.8 MB, an install,
                 * two dead launches and another rollback, every session, for ever, because
                 * `away_from` was returned by `roll_back` and read by nobody.
                 *
                 * IT IS NOT A PERMANENT REFUSAL. The moment a manifest names something strictly
                 * above it, the publisher has shipped a fix and the newer version is offered
                 * normally, which is why this asks about the version being OFFERED rather than
                 * clearing the list. */
                if self.persisted.was_rolled_back(&o.version) {
                    self.pending = None;
                    self.set_phase(Phase::Refused {
                        version: Some(o.version.to_string()),
                        why: format!(
                            "{} was installed on this machine and started twice without drawing \
                             anything, so it was rolled back. It will not be fetched again until \
                             something newer than it is published.",
                            o.version
                        ),
                    });
                    return;
                }
                if self
                    .persisted
                    .has_refused(&o.version.to_string(), &app.sha256)
                {
                    /* THE SAME BYTES THAT WERE REFUSED BEFORE. Skipped without a download, which
                     * is what stops a failure retrying on a six hour loop. A republished version
                     * with different bytes has a different hash and is allowed another try. */
                    let code = self
                        .persisted
                        .refused
                        .iter()
                        .find(|r| r.version == o.version.to_string())
                        .map(|r| r.code.clone())
                        .unwrap_or_default();
                    self.pending = None;
                    self.set_phase(Phase::Refused {
                        version: Some(o.version.to_string()),
                        why: format!(
                            "{} was refused earlier ({code}) and the update channel is still \
                             offering the same bytes, so it has not been fetched again.",
                            o.version
                        ),
                    });
                    return;
                }
                /* A SECOND RELEASE WHILE A PAYLOAD IS ALREADY STAGED.
                 *
                 * The same version again, which is the ordinary case on a six hourly check: the
                 * staged payload IS that offer, so say so and stop rather than reporting the
                 * download as still to come.
                 *
                 * A DIFFERENT version: the staged one is superseded and its staging directory is
                 * removed here rather than left for the prune, because the prune only walks
                 * `app\` and a superseded staging tree has nobody else to delete it. */
                if let Some(s) = &self.staged {
                    if s.version == o.version {
                        self.pending = None;
                        self.set_phase(Phase::Downloaded {
                            version: o.version.to_string(),
                        });
                        return;
                    }
                    log::info!(
                        "{} was staged and {} has been published since; dropping the staged one",
                        s.version,
                        o.version
                    );
                    let _ = std::fs::remove_dir_all(self.layout.staging(&s.version));
                    self.staged = None;
                }
                let word = match o.direction {
                    Direction::Upgrade => "waiting",
                    /* NAMED, BECAUSE A DOWNGRADE IS NOT WHAT A READER EXPECTS AN UPDATER TO DO.
                     * It only happens when a signed manifest carries a `rollback` object naming
                     * this machine's exact version, and the screen should say which of the two
                     * this is. */
                    Direction::PublisherRollback => "published as a rollback",
                };
                /* A NEW OFFER STARTS THE LADDER AGAIN. The backoff is about one thing failing over
                 * and over, and a different version is a different thing; a client that had backed
                 * off to an hour on a broken 0.2.0 must not make the reader wait that hour for the
                 * 0.2.1 that fixes it. */
                if !self
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.version == o.version)
                {
                    self.download_failures = 0;
                    self.last_download = None;
                }
                self.pending = Some(Pending {
                    version: o.version.clone(),
                    notes_url: o.notes_url.clone(),
                    app,
                });
                let pulse = byte_pulse(self.live.pulse.load(Ordering::Relaxed));
                let why = if !lock(&self.live.settings).auto_download {
                    "automatic downloads are off"
                } else if !super::may_start_download(pulse) {
                    super::pulse_word(pulse)
                } else {
                    word
                };
                self.set_phase(Phase::Known {
                    version: o.version.to_string(),
                    notes_url: o.notes_url.clone(),
                    why,
                });
            }
        }
    }

    /* -------------------------------------------------------------- downloading -- */

    fn download(&mut self, pulse: Pulse) {
        let Some(p) = self.pending.take() else {
            return;
        };
        /* THE ATTEMPT IS STAMPED BEFORE IT IS MADE, not after it fails, so a transfer that takes
         * nine minutes and then resets does not get a free retry the instant it ends. */
        self.last_download = Some(Instant::now());
        /* AND THE TRY AGAIN BUTTON GOES AWAY WHILE THE THING IT WOULD RETRY IS IN FLIGHT. It is
         * set again by the failure that needs it, and cleared by a success below. */
        lock(&self.shared).retry_available = false;
        let size = p.app.size;
        self.set_phase(Phase::Downloading {
            version: p.version.to_string(),
            done: 0,
            size,
        });

        let body = match self.wire.artifact(&p.app.url) {
            Ok(b) => b,
            Err(e) => {
                /* NOT WRITTEN INTO `refused`: see `check`. A connection that dropped is a reason
                 * to try again, and the offer is put back so a LATER pass does: which pass is now
                 * `download_is_due`'s answer rather than "the next one, 250 ms from now". */
                self.download_failures = self.download_failures.saturating_add(1);
                self.unreachable(format!("could not fetch {}: {e}", p.app.url));
                self.pending = Some(p);
                return;
            }
        };

        /* THE PROGRESS VALVE. `copy_sealed` calls this once per 64 KiB, which for the measured
         * 10,874,880 byte release binary is 166 calls; each one takes a short lock, and only one
         * in every `REPAINT_EVERY` wakes the UI. Without the valve this is `chat.rs`'s recorded
         * defect exactly: one repaint per event, relayed from a thread that is not drawing. */
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        let paint = &mut self.paint;
        let label = p.version.to_string();
        let mut progress = |done: u64| {
            {
                let mut v = lock(&shared);
                v.phase = Phase::Downloading {
                    version: label.clone(),
                    done,
                    size,
                };
            }
            paint.dirtied();
            if paint.due(Instant::now(), crate::chat::REPAINT_EVERY) {
                if let Some(c) = &ctx {
                    wake(c);
                }
            }
        };

        let staged = install::stage(
            &self.layout,
            &p.version,
            &p.app,
            body,
            pulse,
            self.keys,
            &mut progress,
        );
        match staged {
            Ok(path) => {
                log::info!("update {} staged at {}", p.version, path.display());
                /* A SUCCESS CLEARS THE LADDER AND EVERYTHING RECORDED AGAINST THESE BYTES. The
                 * retry budget exists to tell a transient failure from a statement about the
                 * artifact, and an artifact that has now downloaded and verified has answered
                 * that question. */
                self.download_failures = 0;
                lock(&self.shared).retry_available = false;
                self.persisted
                    .forgive(&p.version.to_string(), &p.app.sha256);
                self.persisted.staged = Some(StagedOffer {
                    version: p.version.to_string(),
                    notes_url: p.notes_url.clone(),
                    app: p.app.clone(),
                });
                self.persisted.touch();
                self.save_state();
                /* THE WHOLE OFFER MOVES ACROSS, because `install` needs the artifact again: the
                 * preflight, the copy and the re-verification from disk all take the signed
                 * `size`, `sha256` and `signature` out of it, with no manifest and no network.
                 * It moves rather than being copied into a second field, so the version and the
                 * seal cannot come from two different releases. */
                self.set_phase(Phase::Downloaded {
                    version: p.version.to_string(),
                });
                self.staged = Some(p);
                /* AND STRAIGHT ON INTO THE INSTALL, because stopping here was asking permission
                 * for nothing. What follows copies the verified binary in beside the versions
                 * already installed and moves a pointer; the running app is untouched and keeps
                 * running, and the new version starts at the reader's next launch. The preflight
                 * inside `install` spawns the staged binary first and only flips the pointer once
                 * it comes up, so this cannot hand somebody a build that will not start.
                 *
                 * IT IS A SEPARATE SWITCH FROM `auto_download` AND NOT THE SAME ONE, because
                 * somebody who wants the bytes fetched but wants to choose his moment is asking
                 * for something coherent, and he is now the person the Install button is for. */
                if lock(&self.live.settings).auto_install {
                    self.install();
                }
            }
            Err(r) => {
                self.download_failures = self.download_failures.saturating_add(1);
                let version = p.version.to_string();
                self.refuse(Some(version.clone()), &p.app.sha256, &r);
                /* A REFUSAL THAT IS NOT ABOUT THE BYTES PUTS THE OFFER BACK, so the ladder retries
                 * it and the reader gets a Try again button. A full disk, an antivirus holding the
                 * file for a second, a folder that was not writable at that instant: none of those
                 * is a statement about the release, and dropping the offer on one of them was what
                 * made a release permanently unreachable on a machine that had one bad minute. */
                if !self.persisted.has_refused(&version, &p.app.sha256) {
                    /* AND THE SCREEN SAYS HOW MANY TIMES, which is the difference between "try
                     * again" and "this has been failing all evening". The count is the one kept in
                     * `state.json`, so it carries across sessions the way the failure does. */
                    let tries = self.persisted.attempts_at(&version, &p.app.sha256);
                    if let Phase::Refused { why, .. } = self.phase() {
                        self.set_phase(Phase::Refused {
                            version: Some(version),
                            why: format!("{why} (attempt {tries})"),
                        });
                    }
                    self.pending = Some(p);
                    lock(&self.shared).retry_available = true;
                }
            }
        }
    }

    /* ---------------------------------------------------------------- installing -- */

    fn install(&mut self) {
        let Some(p) = self.staged.take() else {
            return;
        };
        let (version, art, notes_url) = (p.version, p.app, p.notes_url);
        /* THE PREFLIGHT SPAWNS THE STAGED BINARY AND WAITS FOR IT, which is why this is on the
         * worker thread and behind a press. `install::Spawn` runs it with `GRIMOIRE_SMOKE_MS` set
         * to `data::LOAD_BUDGET`, and `App::new` does not construct an `Updater` when that switch
         * is set, so the preflight cannot race its own parent over `state.json`. */
        let pulse = byte_pulse(self.live.pulse.load(Ordering::Relaxed));
        match install::install_app(&self.layout, &version, &art, self.keys, &Spawn, pulse) {
            Ok(current) => {
                log::info!("update {} installed; it starts at the next launch", version);
                {
                    let mut v = lock(&self.shared);
                    v.installed = Some(current.version.clone());
                    v.retry_available = false;
                    v.phase = Phase::Installed {
                        version: current.version,
                    };
                }
                self.pending = None;
                self.persisted.forgive(&version.to_string(), &art.sha256);
                self.persisted.staged = None;
                self.persisted.touch();
                self.save_state();
                self.wake_now();
            }
            Err(r) => {
                /* THE STAGED PAYLOAD IS ALREADY GONE: `take` above dropped it, which is what
                 * stops a failed install being retried against the same bytes on every press.
                 * The refusal is recorded against the version, so the next check skips it too. */
                let version = version.to_string();
                self.refuse(Some(version.clone()), &art.sha256, &r);
                self.persisted.staged = None;
                self.persisted.touch();
                self.save_state();
                /* AND A FAILURE THAT WAS ABOUT THIS MACHINE IS FETCHED AGAIN RATHER THAN FILED
                 * AWAY. A preflight that failed against a driver, or an antivirus that held the
                 * exe open during the copy, says nothing about the release; the whole staging step
                 * is repeated because `take` above dropped the only handle on those bytes. */
                if !self.persisted.has_refused(&version, &art.sha256) {
                    self.pending = Some(Pending {
                        version: match Version::parse(&version) {
                            Ok(v) => v,
                            Err(_) => return,
                        },
                        notes_url,
                        app: art,
                    });
                    lock(&self.shared).retry_available = true;
                }
            }
        }
    }

    fn restore_data(&mut self) {
        match install::restore_previous_data(&self.layout) {
            Ok(()) => {
                /* THE BUTTON IS ITS OWN UNDO: `restore_previous_data` swaps the two generations
                 * rather than consuming one, so `data_previous` is still true afterwards and the
                 * button stays live. */
                let mut v = lock(&self.shared);
                v.data_previous = self.layout.data_previous().is_dir();
                v.failure = None;
                drop(v);
                self.persisted.installed_data = None;
                self.save_state();
                self.wake_now();
            }
            Err(r) => self.refuse(None, "", &r),
        }
    }

    /* -------------------------------------------------------------- the plumbing -- */

    fn phase(&self) -> Phase {
        lock(&self.shared).phase.clone()
    }

    fn set_phase(&mut self, p: Phase) {
        {
            let mut v = lock(&self.shared);
            if v.phase == p {
                /* NOTHING MOVED, SO NOTHING IS PAINTED. A six hour poll that woke the UI on every
                 * pass to redraw the same word would be a wake-up every quarter second. */
                return;
            }
            v.phase = p;
        }
        self.wake_now();
    }

    fn waiting_because(&mut self, why: &'static str) {
        let (version, notes_url) = match &self.pending {
            Some(p) => (p.version.to_string(), p.notes_url.clone()),
            None => return,
        };
        self.set_phase(Phase::Known {
            version,
            notes_url,
            why,
        });
    }

    fn mark_checked(&mut self) {
        let now = Utc::now();
        let mut v = lock(&self.shared);
        v.last_check = Some(now);
        v.last_check_before = Some(now);
    }

    /// A refusal: the closed-set cause, recorded in the half of `state.json` it belongs in, shown
    /// as a sentence.
    fn refuse(&mut self, version: Option<String>, sha256: &str, r: &Refusal) {
        if let Some(v) = &version {
            self.persisted.refuse(v, sha256, r);
            self.save_state();
        }
        self.fail(version, r.code(), r.to_string());
    }

    /// THE HOST DID NOT ANSWER, WHICH IS NOT A DECISION AND IS NOT DRAWN AS ONE.
    ///
    /// `Failure` is still recorded, because the code word is what a bug report wants and because
    /// the Last error line should say what happened. What changes is the PHASE: `Refused` is drawn
    /// with the red square that `settings::phase_line` reserves for "a decision was made against
    /// this update", and a captive portal is not that.
    ///
    /// `since` IS THE START OF THE OUTAGE AND NOT THE LAST ATTEMPT, so a reader who has been
    /// offline for an hour is told an hour rather than "just now" every time the ladder fires.
    fn unreachable(&mut self, sentence: String) {
        log::warn!("update check: {NETWORK_CODE}: {sentence}");
        let since = match self.phase() {
            Phase::Unreachable { since, .. } => since,
            _ => Utc::now(),
        };
        {
            let mut v = lock(&self.shared);
            v.failure = Some(Failure {
                when: Utc::now(),
                version: None,
                code: NETWORK_CODE.to_owned(),
                sentence: sentence.clone(),
            });
            v.phase = Phase::Unreachable {
                since,
                why: sentence,
            };
        }
        self.wake_now();
    }

    /// A failure that is shown but not recorded as a refusal.
    fn fail(&mut self, version: Option<String>, code: &str, sentence: String) {
        log::warn!("update check: {code}: {sentence}");
        {
            let mut v = lock(&self.shared);
            v.failure = Some(Failure {
                when: Utc::now(),
                version: version.clone(),
                code: code.to_owned(),
                sentence: sentence.clone(),
            });
            v.phase = Phase::Refused {
                version,
                why: sentence,
            };
        }
        self.wake_now();
    }

    fn save_state(&mut self) {
        match write_state(&self.layout, &self.persisted) {
            /* THE MERGED RESULT IS ADOPTED, NOT DISCARDED. `write_state` folds this writer's
             * picture over whatever another copy of the app has written since, and carrying on
             * with the pre-merge copy in memory would mean the next save undid the merge. */
            Ok(merged) => self.persisted = merged,
            Err(e) => {
                /* A STATE FILE THAT WOULD NOT WRITE IS NOT A FAILED UPDATE. It costs one extra
                 * manifest fetch next launch and a re-refusal of anything still bad, which is
                 * exactly what `read_state` returning the default already means. */
                log::warn!("could not write the update state: {e}");
            }
        }
    }

    /// Wake the UI now, outside the progress valve.
    ///
    /// `request_repaint_of(ViewportId::ROOT)` AND NOT THE BARE CALL. `chat.rs:1077-1082` states
    /// why: this thread is inside no viewport's callback, and the plain call targets whatever
    /// viewport egui last had in hand. `channel_art.rs:321` gets away with the bare form because
    /// its thread is started from inside the root's own pass; this one outlives frames and has no
    /// such excuse.
    fn wake_now(&self) {
        if let Some(c) = &self.ctx {
            wake(c);
        }
    }
}

fn wake(ctx: &egui::Context) {
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}

/// THE ONE ARTIFACT THIS BUILD KNOWS HOW TO INSTALL, out of an offer's steps.
///
/// # A DATA BUNDLE IS REFUSED IN WORDS, AND SO IS THE APP BESIDE IT
///
/// `manifest::judge` returns the steps IN INSTALL ORDER and puts a required data bundle first,
/// because a new binary landing on an old snapshot turns a working install into `Data::Failed` on
/// a file name the reader cannot act on. This build can stage and verify a bundle but cannot
/// extract one: [`super::install`] has the path guard, the completeness probe and the directory
/// swap, and the tar reader that would sit between them is not written. Taking the app step alone
/// would be exactly the ordering violation that list exists to prevent, so both are refused and
/// the sentence says which piece is missing.
///
/// The release pipeline publishes one artifact of kind `app` and says so in its own comment, so
/// this branch is unreachable from the channel as it stands today. It is written because the
/// manifest format allows the case and a silent half-install would be the worst possible answer to
/// it.
pub fn choose_app_step(steps: &[Artifact]) -> Result<Artifact, String> {
    if let Some(d) = steps.iter().find(|a| a.kind == KIND_DATA) {
        return Err(format!(
            "this release also publishes a data bundle ({}), and this build can verify one but \
             cannot unpack one yet. Installing the program without it would leave the new build \
             reading the old snapshot, so neither has been taken. Download the new version by \
             hand.",
            d.version
        ));
    }
    steps
        .iter()
        .find(|a| a.kind == KIND_APP)
        .cloned()
        .ok_or_else(|| {
            "this release offers nothing this build can install: there is no program in it."
                .to_owned()
        })
}

/// How a version directory is spelled on disk, for the Settings screen's file line.
pub fn installed_dir(l: &Layout, version: &str) -> Option<PathBuf> {
    Version::parse(version).ok().map(|v| l.version_dir(&v))
}

/// THE EXECUTABLE A RESTART SHOULD START, which is the ENTRY POINT and not this process.
///
/// # WHY AN ENVIRONMENT VARIABLE AND NOT `current_exe`
///
/// After the first update this process is a managed payload at
/// `%LOCALAPPDATA%\eql-grimoire\app\<version>\grimoire-desktop.exe`, and
/// `launch::plan` answers `RunHere` for anything inside that directory, which is the infinite-loop
/// guard. So relaunching `current_exe()` would start the version that is running now and the
/// pointer that was just flipped would be ignored. The entry point is the file the installer
/// wrote and the Start Menu shortcut names, it never moves, and the only thing that knows where it
/// is is the trampoline that bounced off it. It passes it along.
///
/// The fallback is `current_exe()`, which is right in the case that matters most: a build nobody
/// has updated yet IS the entry point.
pub fn entry_point() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(super::launch::ENTRY_ENV) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    std::env::current_exe().ok()
}

/// MAY THE READER BE OFFERED A RESTART RIGHT NOW?
///
/// # THE SAME GATE AS A DOWNLOAD, AND IT IS THE SECOND OF THE ONLY TWO THINGS GATED
///
/// [`super::may_start_download`] answers `true` only for [`Pulse::Closed`], and the reason it is
/// not `!Ingest::fight_is_live()` is written out there: `fight_is_live` is `pulse().fighting()`,
/// which is already false during [`Pulse::Holding`], and Holding is the six seconds after a kill
/// when the reader is standing in the camp with the next mob on its way (`fights.rs:84-88`).
/// Closing the app there is as much an interruption as closing it mid-pull, and it is worse than
/// an interrupted download because a download can be started again and a wipe cannot.
///
/// # THE POINTER FLIP IS NOT GATED, AND THAT IS THE DESIGN RATHER THAN AN OMISSION
///
/// `install::install_app` finishes with one atomic rename of one small file. The running process
/// is not touched by it, nothing restarts, and the new version starts whenever the reader next
/// opens the app. So the only moment that needs scheduling around is the one where a person is
/// asked to close the window, which is this one. The risky moment was designed out rather than
/// scheduled around.
///
/// Returns false unless something is actually installed, because a Restart now button with nothing
/// to restart into says an update happened when none did.
pub fn may_restart(pulse: Pulse, phase: &Phase) -> bool {
    matches!(phase, Phase::Installed { .. }) && super::may_start_download(pulse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::manifest::probe as mprobe;
    use crate::updater::verify::probe::Signer;
    use std::io::Read;
    use std::sync::atomic::AtomicUsize;

    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("grimoire-updater-run-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch folder");
        d
    }

    /// A key list with the lifetime [`Updater::spawn`] wants, out of a signer made in this test.
    ///
    /// LEAKED, AND ON PURPOSE. `spawn` takes `&'static [&'static str]` because the production
    /// value is the compiled-in `verify::KEYS`, and widening that signature to a lifetime
    /// parameter purely so a test could pass a borrowed slice would be the test shaping the
    /// production API. A handful of bytes per test process is the cheaper price.
    fn static_keys(signer: &Signer) -> &'static [&'static str] {
        let k: &'static str = Box::leak(signer.public_b64.clone().into_boxed_str());
        Box::leak(Box::new([k]))
    }

    /// A wire that answers from memory and records which thread asked it.
    struct Fake {
        /* `Arc` ON BOTH ANSWERS, because the wire is MOVED into the worker and a test that wants
         * to publish a second release while the first is staged has no other way to reach it. */
        body: Arc<Mutex<Result<String, String>>>,
        bytes: Arc<Mutex<Result<Vec<u8>, String>>>,
        /// EVERY THREAD THAT EVER CALLED THIS WIRE. The shared half of
        /// `the_check_never_runs_on_the_thread_that_draws`: the wire is moved into the worker and
        /// cannot be reached from the test afterwards, so it reports through an `Arc` the test
        /// kept a clone of.
        callers: Arc<Mutex<Vec<std::thread::ThreadId>>>,
        calls: Arc<AtomicUsize>,
        /// HOW MANY TIMES THE BODY OF AN ARTIFACT HAS BEEN ASKED FOR.
        ///
        /// Counted separately from `calls` because the two answer different questions and one of
        /// them is the critical defect in this file: the manifest check has a clock and the
        /// download did not, so a failing download asked four times a second. A test of that can
        /// only be a count over a window.
        art_calls: Arc<AtomicUsize>,
    }

    impl Fake {
        /// A wire answering one manifest and one payload, with the counters the caller keeps.
        fn new(body: &str, bytes: Result<Vec<u8>, String>) -> (Fake, Arc<AtomicUsize>) {
            let art_calls = Arc::new(AtomicUsize::new(0));
            (
                Fake {
                    body: Arc::new(Mutex::new(Ok(body.to_owned()))),
                    bytes: Arc::new(Mutex::new(bytes)),
                    callers: Arc::new(Mutex::new(Vec::new())),
                    calls: Arc::new(AtomicUsize::new(0)),
                    art_calls: art_calls.clone(),
                },
                art_calls,
            )
        }
    }

    impl Wire for Fake {
        fn manifest(&self, _url: &str) -> Result<String, String> {
            lock(&self.callers).push(std::thread::current().id());
            self.calls.fetch_add(1, Ordering::Relaxed);
            lock(&self.body).clone()
        }
        fn artifact(&self, _url: &str) -> Result<Box<dyn Read>, String> {
            lock(&self.callers).push(std::thread::current().id());
            self.art_calls.fetch_add(1, Ordering::Relaxed);
            match lock(&self.bytes).clone() {
                Ok(b) => Ok(Box::new(std::io::Cursor::new(b))),
                Err(e) => Err(e),
            }
        }
    }

    /// Spin until `f` answers true, or give up after a second.
    ///
    /// A DEADLINE AND NOT A SLEEP. The worker wakes every [`TICK`], and a fixed sleep long enough
    /// to be safe on a loaded machine is a fixed sleep every run of `cargo test` pays for. This
    /// returns the instant the condition holds, so the deadline costs nothing on a pass.
    ///
    /// THIRTY SECONDS IS A GIVE-UP POINT AND NOT A CLAIM ABOUT HOW LONG ANYTHING TAKES. It was one
    /// second, and one second went red on a `cargo test --lib` running eleven hundred other tests
    /// across every core on this machine: the worker had started its pass and had not been
    /// scheduled again. A deadline that a loaded machine can miss is a flaky test, and a flaky
    /// test in a suite this size is a test people learn to re-run rather than read.
    fn until(mut f: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        f()
    }

    /* ------------------------------------------------------------- the settings -- */

    /// DEFECT THIS PREVENTS: EVERY MACHINE IN THE FIELD LOADING WITH UPDATES SWITCHED OFF.
    ///
    /// `#[serde(default)]` on a CONTAINER takes `UpdaterSettings::default()`; on a FIELD it takes
    /// `bool::default()`, which is `false`. The two spellings are one word apart, look identical
    /// in a diff, and differ on exactly the fields whose default is not the zero value, which here
    /// is two of the three. No settings.json anywhere has an `updater` block, because this is the
    /// first build that writes one, so the wrong spelling would ship an updater that never checks
    /// and never downloads, on every machine, with no symptom but the absence of updates.
    ///
    /// The round trip goes through `Settings::save_to(&scratch)` and never `Settings::save()`,
    /// which refuses under `cfg(test)` on purpose (`settings.rs:580`) because a test once wrote
    /// its fixture over the owner's real settings file.
    ///
    /// WHAT MUTATION MAKES THIS RED: move `#[serde(default)]` off `struct UpdaterSettings` and
    /// onto its three fields; or replace the hand-written `impl Default` with a derived one; or
    /// drop `updater` from the hand-written `impl Default for Settings`, which does not compile,
    /// which is the good outcome.
    #[test]
    fn a_settings_file_written_before_updates_existed_still_gets_them() {
        let d = scratch("settings");
        let path = d.join("settings.json");

        /* A FILE FROM BEFORE THIS FEATURE: no `updater` key anywhere in it. */
        std::fs::write(&path, br#"{"always_on_top": true}"#).expect("write the old file");
        let old = crate::settings::Settings::load_from(&path).expect("the old file loads");
        assert!(
            old.updater.enabled,
            "a settings file with no updater block loaded with checking switched OFF; that is \
             `#[serde(default)]` on the fields instead of on the container"
        );
        assert!(
            old.updater.auto_download,
            "a settings file with no updater block loaded with downloads switched OFF"
        );
        assert_eq!(old.updater.channel, CHANNELS[0]);
        assert!(
            old.always_on_top,
            "the rest of the old file was lost, so this proves nothing about the updater block"
        );

        /* AND EVERY FIELD SURVIVES A WRITE AND A READ, including the two whose default is the
         * value being written over, which is the pair that a default-on-read would hide. */
        let mut s = crate::settings::Settings::default();
        s.updater.enabled = false;
        s.updater.channel = CHANNELS[1].to_owned();
        s.updater.auto_download = false;
        s.save_to(&path).expect("a scratch path is writable");
        let back = crate::settings::Settings::load_from(&path).expect("what was just written");
        assert_eq!(
            back.updater, s.updater,
            "the updater block did not round trip through serde"
        );

        let raw = std::fs::read_to_string(&path).expect("read it back");
        assert!(
            raw.contains("\"updater\""),
            "the updater block was not written to the file at all: {raw}"
        );

        /* AND A BLOCK NAMING ONLY ONE FIELD KEEPS THE DEFAULTS FOR THE OTHER TWO. */
        std::fs::write(&path, br#"{"updater":{"channel":"beta"}}"#).expect("write a part block");
        let part = crate::settings::Settings::load_from(&path).expect("a partial block loads");
        assert_eq!(part.updater.channel, "beta");
        assert!(
            part.updater.enabled && part.updater.auto_download,
            "a block naming only `channel` lost the other two to `bool::default()`"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /* --------------------------------------------------------------- the thread -- */

    /// DEFECT THIS PREVENTS: THE MANIFEST CHECK RUNNING ON THE THREAD THAT DRAWS.
    ///
    /// The whole reason there is a worker is that a ten second manifest fetch, and a ten megabyte
    /// download after it, never sit inside a frame. A [`Wire`] call written into [`Updater::pump`]
    /// or into the Settings section would be invisible to every test that did not look for it and
    /// would show up only as an app that freezes on a bad connection, which is the exact shape of
    /// the defect `Ingest::tail` and `Hotkeys::poll` both shipped.
    ///
    /// HOW IT PROVES IT RATHER THAN ASSERTING IT: the wire records the `ThreadId` of every caller.
    /// `pump` and `view` are called many times from THIS thread while the check runs, so a fetch
    /// written into either would put this thread's id in the list. The list must hold the worker's
    /// id and must not hold this one.
    ///
    /// WHAT MUTATION MAKES THIS RED: call `self.tick(&settings, true)` from inside `Updater::pump`
    /// (which is what "just check when the app asks" looks like written wrong), or move the
    /// `wire.manifest(..)` call into `Updater::view`.
    #[test]
    fn the_check_never_runs_on_the_thread_that_draws() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        /* A MANIFEST THAT PUBLISHES THE VERSION THIS BUILD ALREADY IS, so the check finishes and
         * stops rather than trying to fetch from `example.invalid`. */
        let mut doc = mprobe::sane();
        doc["version"] = serde_json::json!(crate::titlebar::version());
        doc["artifacts"] = serde_json::json!([mprobe::app(crate::titlebar::version())]);
        let body = signer.envelope(&mprobe::manifest_text(&doc));

        let callers = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let wire = Fake {
            body: Arc::new(Mutex::new(Ok(body))),
            bytes: Arc::new(Mutex::new(Err(
                "nothing is downloaded in this test".to_owned()
            ))),
            callers: callers.clone(),
            calls: calls.clone(),
            art_calls: Arc::new(AtomicUsize::new(0)),
        };

        let d = scratch("thread");
        let u = Updater::spawn(
            Layout::at(&d),
            Box::new(wire),
            keys,
            None,
            &UpdaterSettings::default(),
            CHECK_EVERY,
        );
        let here = std::thread::current().id();

        /* NOTHING MAY HAVE HAPPENED YET, and that is half the assertion: the first check waits for
         * the snapshot to finish loading, and only `pump` can say that it has. */
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "the wire was called before any heartbeat said the snapshot had finished loading"
        );

        let done = until(|| {
            u.pump(Pulse::Closed, true, &UpdaterSettings::default());
            let _ = u.view();
            matches!(u.view().phase, Phase::UpToDate)
        });
        assert!(
            done,
            "the worker never finished a check ({:?}), so this test proves nothing about where it \
             ran",
            u.view().phase
        );

        let who = lock(&callers).clone();
        assert!(!who.is_empty(), "the wire was never called at all");
        assert!(
            !who.contains(&here),
            "the update check ran on the thread that draws frames: {} of {} calls came from it",
            who.iter().filter(|t| **t == here).count(),
            who.len()
        );
        assert!(
            u.view().last_check.is_some(),
            "a finished check did not stamp a time, so the Settings screen would print nothing \
             where the last check goes"
        );
        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT THIS PREVENTS: THE TWO CONTROLS IN THE UPDATES SECTION DOING NOTHING UNTIL A RESTART.
    ///
    /// `pump` is the ONLY path by which the switch and the channel buttons reach the worker. The
    /// obvious alternative is to read the settings once in `Updater::spawn` and keep them, which
    /// compiles, passes every other test here, and leaves two controls on the Settings screen that
    /// tick and highlight and change nothing at all until the app is closed and opened again.
    ///
    /// # WHAT THIS TEST DELIBERATELY DOES NOT CLAIM
    ///
    /// `pump` compares before it writes, so an unchanged heartbeat allocates nothing. Its first
    /// half below covers that path, and it is honest to say that IT CANNOT FAIL ON THE GUARD
    /// ALONE: dropping `if *held != *settings` writes the same value sixty times a second, which
    /// is an allocation nothing here can observe and no behaviour at all. That guard is an
    /// efficiency choice with a comment and no test, and calling this a test of it would be
    /// exactly the kind of assertion that cannot fail. What the first half does prove is that a
    /// heartbeat carrying unchanged settings does not CORRUPT them, which a write that read the
    /// wrong field would.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the write from `pump` and read the settings only in
    /// `spawn`; or have `pump` write a `Default` instead of what it was handed.
    #[test]
    fn a_changed_setting_reaches_the_worker_without_a_restart() {
        let d = scratch("pump");
        let signer = Signer::new();
        let wire = Fake {
            body: Arc::new(Mutex::new(Err("not used in this test".to_owned()))),
            bytes: Arc::new(Mutex::new(Err("not used in this test".to_owned()))),
            callers: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(AtomicUsize::new(0)),
            art_calls: Arc::new(AtomicUsize::new(0)),
        };
        let off = UpdaterSettings {
            enabled: false,
            ..UpdaterSettings::default()
        };
        let u = Updater::spawn(
            Layout::at(&d),
            Box::new(wire),
            static_keys(&signer),
            None,
            &off,
            CHECK_EVERY,
        );
        let before = lock(&u.live.settings).clone();
        for _ in 0..64 {
            u.pump(Pulse::Closed, false, &off);
        }
        assert_eq!(
            lock(&u.live.settings).clone(),
            before,
            "sixty four identical heartbeats changed the stored settings"
        );
        let on = UpdaterSettings {
            channel: CHANNELS[1].to_owned(),
            ..off.clone()
        };
        u.pump(Pulse::Closed, false, &on);
        assert_eq!(
            lock(&u.live.settings).channel,
            CHANNELS[1],
            "a heartbeat carrying a changed channel never reached the worker, so the Settings \
             screen's channel buttons would do nothing until the app was restarted"
        );
        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A signed release, with an artifact whose seal really describes `payload`.
    ///
    /// REAL FIGURES OVER REAL BYTES AND NOT A FIXTURE. `stage` enforces the signed `size` exactly,
    /// streams the sha256 in the same pass, and checks the artifact's own signature with
    /// `verify_stream`; a placeholder for any of the three would be refused before the part of the
    /// worker this test is about was reached.
    fn signed_release(signer: &Signer, version: &str, published: &str, payload: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(payload);
        let sha: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let doc = serde_json::json!({
            "format": manifest::FORMAT,
            "channel": CHANNELS[0],
            "version": version,
            "published": published,
            /* FAR ENOUGH OUT THAT NO TEST IN THIS FILE FAILS ON A DATE. `judge` refuses a manifest
             * whose `expires` has passed, and these tests run against the real clock because the
             * worker is the half that reads one; a fixture that expired would turn every test here
             * into a test of the calendar. */
            "expires": "2099-01-01T00:00:00Z",
            "artifacts": [{
                "kind": KIND_APP,
                "os": manifest::this_os(),
                "arch": manifest::this_arch(),
                "version": version,
                /* A REAL ADDRESS ON THE REAL HOST. Nothing here fetches it (the wire is a `Fake`
                 * and ignores the url), but `judge` refuses an artifact that is not served from
                 * `fetch::HOST` over https, so a fixture pointing at `example.invalid` would be
                 * refused before the part of the worker each test is about was reached. */
                "url": format!("{}/grimoire/stable/{version}/grimoire-desktop.exe", fetch::HOST),
                "size": payload.len(),
                "sha256": sha,
                "signature": signer.sign(payload),
            }],
        });
        signer.envelope(&mprobe::manifest_text(&doc))
    }

    /// DEFECT THIS PREVENTS: AN INSTALL CARRYING ONE RELEASE'S VERSION AND ANOTHER'S SIGNATURE.
    ///
    /// # THE DEFECT THIS WAS WRITTEN FOR, WHICH WAS REAL AND IS FIXED
    ///
    /// `staged` held a `Version` and `install` fetched the artifact back out of `pending`. Those
    /// two fields are written by different events: `download` sets the first, and any later check
    /// replaces the second. A second release published while a payload sat in staging therefore
    /// left 0.2.0 in `staged` and 0.3.0's artifact in `pending`, and the Install press put 0.2.0's
    /// path and 0.3.0's `size`, `sha256` and `signature` into one call. It failed safe, because
    /// `install_app` re-verifies from disk, but it failed as `StagedFileChangedOnDisk`, which
    /// tells a person their disk was tampered with when what actually happened is that a release
    /// was published while they were reading the screen.
    ///
    /// `staged` is a whole `Pending` now, so that pairing is unrepresentable. What is left to test
    /// is the reconciliation this test drives: the superseded payload is dropped, its staging tree
    /// is removed, and the bytes that end up on disk are the NEW release's.
    ///
    /// # IT DRIVES THE REAL WORKER, NOT A HELPER
    ///
    /// A real thread, the real check order, real minisign signatures over real bytes, and the
    /// pump called from this thread exactly as `App::heartbeat` calls it. The only fake is the
    /// wire, which is what `Updater::spawn` takes an argument for.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `if let Some(s) = &self.staged` reconciliation in
    /// `check`, which leaves the older payload staged and its directory on disk for ever; or drop
    /// the `remove_dir_all` inside it, which leaks a staging tree per superseded release; or make
    /// `staged` a `Version` again, which does not compile, which is the point of the change.
    #[test]
    fn a_release_published_while_one_is_staged_replaces_it_and_its_bytes() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("supersede");
        let l = Layout::at(&d);
        let v = |s: &str| Version::parse(s).expect("a literal version");
        /* STAGING IS WHAT THIS MEASURES, SO AUTO-INSTALL IS PINNED OFF. With it on, the default,
         * a successful stage runs straight into the install and the staging folder this test is
         * about is emptied before it can look. Turning it off here is not weakening the test: the
         * install path has its own, and a test that measured two behaviours at once would report
         * a change in either as a failure of both. */
        let s = UpdaterSettings {
            auto_install: false,
            ..UpdaterSettings::default()
        };

        /* TWO PAYLOADS OF DIFFERENT LENGTHS, so that reading the file back proves WHICH release is
         * on disk rather than merely that a file is. */
        let two = b"the 0.2.0 payload".to_vec();
        let three = b"the 0.3.0 payload, which is longer".to_vec();
        assert_ne!(two.len(), three.len());

        let body = Arc::new(Mutex::new(Ok(signed_release(
            &signer,
            "0.2.0",
            "2026-09-14T18:02:11Z",
            &two,
        ))));
        let bytes = Arc::new(Mutex::new(Ok(two.clone())));
        let u = Updater::spawn(
            l.clone(),
            Box::new(Fake {
                body: body.clone(),
                bytes: bytes.clone(),
                callers: Arc::new(Mutex::new(Vec::new())),
                calls: Arc::new(AtomicUsize::new(0)),
                art_calls: Arc::new(AtomicUsize::new(0)),
            }),
            keys,
            None,
            &s,
            CHECK_EVERY,
        );

        let downloaded = |u: &Updater, want: &str| -> bool {
            matches!(u.view().phase, Phase::Downloaded { ref version } if version == want)
        };

        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                downloaded(&u, "0.2.0")
            }),
            "0.2.0 never reached the staging folder; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(
            std::fs::read(l.staging(&v("0.2.0")).join(install::exe_name())).expect("staged 0.2.0"),
            two,
            "the staged file is not the payload the manifest named"
        );

        /* A SECOND RELEASE, PUBLISHED LATER. The `published` stamp moves forward, or the replay
         * floor written by the first check would refuse this one and the test would be measuring
         * that instead. */
        *lock(&body) = Ok(signed_release(
            &signer,
            "0.3.0",
            "2026-09-20T09:00:00Z",
            &three,
        ));
        *lock(&bytes) = Ok(three.clone());
        u.ask(Ask::CheckNow);

        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                downloaded(&u, "0.3.0")
            }),
            "0.3.0 never replaced 0.2.0; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(
            std::fs::read(l.staging(&v("0.3.0")).join(install::exe_name())).expect("staged 0.3.0"),
            three,
            "the staged file is not the new release's payload"
        );
        assert!(
            !l.staging(&v("0.2.0")).exists(),
            "the superseded release is still staged at {}; nothing else ever deletes it, because \
             the prune only walks the app folder",
            l.staging(&v("0.2.0")).display()
        );

        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /* ----------------------------------------------------------------- the gates -- */

    /// DEFECT THIS PREVENTS: BEING ASKED TO CLOSE THE APP IN THE MIDDLE OF A PULL.
    ///
    /// `Restart now` closes the window. Offering it while the reader is swinging, or in the six
    /// seconds after a kill with the next mob incoming, is worse than an interrupted download: a
    /// download can be started again and a wipe cannot. The obvious gate is
    /// `!ingest.fight_is_live()` and it is WRONG, because `fight_is_live` is `pulse().fighting()`
    /// and is already false during `Pulse::Holding`.
    ///
    /// THE FIGHT STATE IS DRIVEN THROUGH THE REAL FOLD AND NEVER SET BY HAND. `still_going`
    /// (`ingest.rs:1990`) has already shipped one false positive of exactly this kind
    /// (`ingest.rs:1969-1988`, regression test `a_pull_that_ended_stops_saying_in_combat`), so a
    /// test that assigned `live_open` would be proving something about the test and nothing about
    /// the code that answers.
    ///
    /// WHAT MUTATION MAKES THIS RED: write `may_restart` as `!matches!(pulse, Pulse::Fighting)`,
    /// which is `!fight_is_live()` spelled out and lets the prompt through during Holding; or drop
    /// the `Phase::Installed` test, which offers a restart with nothing to restart into.
    #[test]
    fn a_live_fight_holds_back_the_restart_prompt() {
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

        let ready = Phase::Installed {
            version: "0.2.0".to_owned(),
        };
        let cases = [
            ("restart-fighting", None, Pulse::Fighting, false),
            (
                "restart-holding",
                Some(COMBAT_SECONDS + 1),
                Pulse::Holding,
                false,
            ),
            (
                "restart-closed",
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
                may_restart(ing.pulse(), &ready),
                may,
                "{tag}: the restart prompt disagrees with the encounter"
            );
            /* AND THE SAME ENCOUNTER GATES A DOWNLOAD THE SAME WAY, which is the other half of the
             * rule and the reason both read the one function. */
            assert_eq!(
                may_fetch(ing.pulse(), true, false, true),
                may,
                "{tag}: the download gate and the restart gate disagree about one encounter"
            );
        }

        /* NOTHING INSTALLED IS NOT A REASON TO OFFER A RESTART, whatever the encounter is doing. */
        for p in [Pulse::Closed, Pulse::Holding, Pulse::Fighting] {
            assert!(
                !may_restart(p, &Phase::UpToDate),
                "a restart was offered with nothing installed to restart into"
            );
            assert!(
                !may_restart(
                    p,
                    &Phase::Downloaded {
                        version: "0.2.0".to_owned()
                    }
                ),
                "a restart was offered for a payload that has not been installed yet"
            );
        }
    }

    /// DEFECT THIS PREVENTS: THE FIRST CHECK RACING THE SNAPSHOT PARSE, AND THE SECOND NEVER COMING.
    ///
    /// Two halves, each failing in its own direction. Without `data_ready` the check fires on the
    /// first heartbeat, which is while 24.7 MB of JSON is being read off the same disk the
    /// download writes to. Without the elapsed comparison a session that runs for a week checks
    /// once.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `!data_ready` clause; or write the interval test as
    /// `>` rather than `>=`, which makes the interval one tick longer than it says and is the
    /// classic off-by-one here; or return `true` when `enabled` is false, which would make the
    /// Settings switch decorative.
    #[test]
    fn the_first_check_waits_for_the_snapshot_and_the_next_one_waits_for_the_clock() {
        let now = Instant::now();
        let every = Duration::from_secs(60);

        assert!(
            !check_is_due(true, false, None, now, every),
            "a check was due while the snapshot was still loading"
        );
        assert!(
            check_is_due(true, true, None, now, every),
            "the first check never becomes due once the snapshot has landed"
        );
        assert!(
            !check_is_due(false, true, None, now, every),
            "a check was due with the feature switched off"
        );

        let then = now - every;
        assert!(
            check_is_due(true, true, Some(then), now, every),
            "a check exactly one interval old is not due, so the interval is longer than it says"
        );
        assert!(
            !check_is_due(true, true, Some(now), now, every),
            "a check that just happened is due again at once, which is a poll loop"
        );
    }

    /// DEFECT THIS PREVENTS: A DOWNLOAD THAT IGNORES THE SWITCH, OR A BUTTON THAT DOES NOTHING.
    ///
    /// `auto_download` off has to mean off, the Download button has to work anyway, and neither
    /// may start one while an encounter is open. Three conditions, and each is a different
    /// sentence on the Settings screen.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `asked` term (the button stops working whenever the
    /// switch is off); drop `auto_download` (the switch stops working); drop `pending` (a download
    /// is started with nothing to fetch).
    #[test]
    fn the_download_switch_the_button_and_the_encounter_all_have_to_agree() {
        assert!(may_fetch(Pulse::Closed, true, false, true));
        assert!(
            !may_fetch(Pulse::Closed, false, false, true),
            "a download started with automatic downloads switched off"
        );
        assert!(
            may_fetch(Pulse::Closed, false, true, true),
            "the Download button does nothing while automatic downloads are off, which makes the \
             button a lie"
        );
        assert!(
            !may_fetch(Pulse::Fighting, true, true, true),
            "a pressed button started a download mid-pull"
        );
        assert!(
            !may_fetch(Pulse::Holding, true, true, true),
            "a pressed button started a download in the camp with the next mob incoming"
        );
        assert!(
            !may_fetch(Pulse::Closed, true, true, false),
            "a download was started with nothing on offer to fetch"
        );
    }

    /// DEFECT THIS PREVENTS: FOUR REQUESTS A SECOND TO THE UPDATE HOST, FROM EVERY CLIENT, FOR AS
    /// LONG AS THE APP IS OPEN.
    ///
    /// # THE LOOP, WHICH NEEDED NOTHING TO GO WRONG BUT A MISSING OBJECT
    ///
    /// `Worker::tick` runs every [`TICK`], which is 250 ms, and the download block was gated by
    /// `self.staged.is_none() && may_fetch(..)` and by nothing else: no clock, no backoff, no
    /// attempt count. `download` puts the offer back into `pending` on any wire-level failure. So
    /// a versioned artifact object that was missing, evicted, or briefly 5xx turned into four GETs
    /// a second to `updates.ragnarok.systems` from every client that had accepted that manifest,
    /// for as long as the app was open and the reader was out of combat, with the Settings screen
    /// flipping red on each one. A mid-transfer reset at nine megabytes is the same loop carrying
    /// nine megabytes a turn. `chat::BACKOFF` and `watcher::POLL_EVERY` both exist in this crate
    /// and neither was reused here; the manifest check had `check_is_due` and the download had
    /// nothing.
    ///
    /// # THE TEST IS A COUNT OVER A WINDOW, BECAUSE THAT IS WHAT THE DEFECT IS
    ///
    /// It drives the real worker on a real thread with a wire that always fails the artifact, and
    /// counts how many times the body was asked for over a window several ticks long. The first
    /// rung of [`DOWNLOAD_BACKOFF`] is five seconds, so inside a window of well under that the
    /// honest answer is exactly one. Without the clock the same window is dozens.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `download_is_due` term from `tick`; have
    /// `download` clear `download_failures` on a failure; or stamp `last_download` only on
    /// success, which is the same loop with an extra branch.
    #[test]
    fn a_download_that_keeps_failing_is_not_asked_for_four_times_a_second() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("download-backoff");
        let s = UpdaterSettings::default();

        let payload = b"the 0.2.0 payload".to_vec();
        let (wire, asked) = Fake::new(
            &signed_release(&signer, "0.2.0", "2026-09-14T18:02:11Z", &payload),
            Err("the object is not there (404)".to_owned()),
        );
        let u = Updater::spawn(Layout::at(&d), Box::new(wire), keys, None, &s, CHECK_EVERY);

        /* The first attempt happens at once, which is what `download_is_due` answers for a
         * failure count of zero and is the behaviour a reader wants from the first try. */
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                asked.load(Ordering::Relaxed) >= 1
            }),
            "the artifact was never fetched at all, so this test is measuring nothing; the phase \
             was {:?}",
            u.view().phase
        );

        /* AND THEN NOTHING FOR THE LENGTH OF THE FIRST RUNG. The window is deliberately far
         * shorter than that rung (which is DOWNLOAD_BACKOFF[0] seconds) and far longer than
         * TICK, so an unclocked loop has many chances to ask again and a clocked one has none. */
        let window = TICK * 8;
        assert!(
            window < Duration::from_secs(DOWNLOAD_BACKOFF[0]),
            "the window has to sit inside the first rung, or this test proves nothing"
        );
        let began = Instant::now();
        while began.elapsed() < window {
            u.pump(Pulse::Closed, true, &s);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            asked.load(Ordering::Relaxed),
            1,
            "the download was retried {} times inside {window:?}, which is a request every {TICK:?} \
             to the update host from every client that accepted this manifest",
            asked.load(Ordering::Relaxed)
        );

        /* AND A FAILURE IS NOT DRAWN AS A DECISION AGAINST THE UPDATE. A missing object is the
         * host's problem and the screen says so in the language it reserves for things in flight,
         * not in the language it reserves for a signature that did not check out. */
        assert!(
            matches!(u.view().phase, Phase::Unreachable { .. }),
            "a wire failure left the phase at {:?}, which the Settings screen draws with the red \
             square it reserves for refusals",
            u.view().phase
        );

        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT THIS PREVENTS: A LADDER THAT DOES NOT CLIMB, OR ONE THAT NEVER LETS GO.
    ///
    /// The rule itself, away from threads: the first attempt is always due, each failure moves one
    /// rung, and the ladder flattens at its last rung rather than running off the end of the array
    /// and panicking or answering zero.
    ///
    /// WHAT MUTATION MAKES THIS RED: return `true` whatever the count; index the ladder with
    /// `failures` instead of `failures - 1` (the first failure would wait the second rung and the
    /// last rung would be unreachable); drop the `or_else(last)` and a client that failed seven
    /// times would ask again immediately, for ever.
    #[test]
    fn the_download_ladder_climbs_and_then_flattens() {
        let now = Instant::now();
        let ladder = [5u64, 30, 120];

        assert!(
            download_is_due(0, None, now, &ladder),
            "the first attempt is not due, so a new offer would never be fetched at all"
        );
        assert!(
            download_is_due(0, Some(now), now, &ladder),
            "a success followed by a new offer has to be due at once"
        );

        let at = |ago: u64| now - Duration::from_secs(ago);
        assert!(!download_is_due(1, Some(at(4)), now, &ladder));
        assert!(download_is_due(1, Some(at(5)), now, &ladder));
        assert!(!download_is_due(2, Some(at(29)), now, &ladder));
        assert!(download_is_due(2, Some(at(30)), now, &ladder));

        /* PAST THE END OF THE LADDER IT HOLDS AT THE LAST RUNG. A count that walked off the array
         * would either panic or answer "due", and "due" is the loop this whole thing removes. */
        assert!(!download_is_due(9, Some(at(119)), now, &ladder));
        assert!(download_is_due(9, Some(at(120)), now, &ladder));
        assert!(!download_is_due(u32::MAX, Some(at(1)), now, &ladder));
    }

    /// DEFECT THIS PREVENTS: A DISK THAT FILLED ONCE MAKING A RELEASE PERMANENTLY UNINSTALLABLE.
    ///
    /// # THE FAILURE, END TO END
    ///
    /// `Worker::refuse` wrote EVERY `Refusal` into `persisted.refused`, and `check` then skips that
    /// version for ever. `Io` and `PreflightFailed` are environmental and say nothing about the
    /// bytes, yet they were recorded exactly like `ArtifactBadSignature`. So: the disk fills while
    /// staging, `copy_sealed`'s write fails, the refusal is written to `update\state.json`, the
    /// reader frees forty gigabytes and reopens the app, and every check from then on
    /// short-circuits to "0.2.0 was refused earlier (Io) and the update channel is still offering
    /// the same bytes". There is no control anywhere in the UPDATES section that clears the list,
    /// and the release pipeline explicitly refuses to republish a version with different bytes, so
    /// there is no escape at all. An antivirus that locks the exe for one second during the copy
    /// does the same thing.
    ///
    /// # WHY THE HASH MISMATCH IS THE INTERESTING CASE
    ///
    /// It IS about the bytes and it still gets one retry, because two copies of this app staging
    /// the same file at once produce exactly that refusal (see `install::stage`, which now names
    /// the partial file after the process writing it). One collision must not be permanent; a
    /// second mismatch over the same artifact is a statement about the artifact.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `Persisted::refuse` write everything to `refused` again;
    /// give `ArtifactHashMismatch` a budget of zero; or give `Io` a finite budget, and the second
    /// full disk becomes permanent.
    #[test]
    fn a_full_disk_does_not_make_a_release_uninstallable_for_ever() {
        let disk_full = Refusal::Io {
            doing: "write the download to",
            path: PathBuf::from("<the staged file>"),
            why: "there is not enough space on the disk".to_owned(),
        };

        let mut p = Persisted::default();
        for n in 1..=20 {
            p.refuse("0.2.0", "aa", &disk_full);
            assert!(
                !p.has_refused("0.2.0", "aa"),
                "a full disk was written into the permanent list on attempt {n}, so this release \
                 can never be installed on this machine again"
            );
            assert_eq!(p.attempts_at("0.2.0", "aa"), n);
        }
        assert_eq!(
            p.attempts.len(),
            1,
            "twenty attempts wrote twenty rows; state.json grows without limit"
        );

        /* A SUCCESS FORGETS ALL OF IT, because the question the count exists to answer has been
         * answered. */
        p.forgive("0.2.0", "aa");
        assert_eq!(p.attempts_at("0.2.0", "aa"), 0);

        /* THE ONE WITH A BUDGET. First mismatch: retriable, because two writers and a bad CDN look
         * identical from here. Second: permanent, because it is now a statement about the bytes. */
        let mismatch = Refusal::ArtifactHashMismatch {
            said: "a".repeat(64),
            got: "b".repeat(64),
        };
        let mut q = Persisted::default();
        q.refuse("0.2.0", "aa", &mismatch);
        assert!(
            !q.has_refused("0.2.0", "aa"),
            "one hash mismatch was made permanent, so two copies of the app colliding once puts a \
             release out of reach"
        );
        q.refuse("0.2.0", "aa", &mismatch);
        assert!(
            q.has_refused("0.2.0", "aa"),
            "a second mismatch over the same artifact is about the artifact and must stick"
        );

        /* AND A SIGNATURE FAILURE IS PERMANENT ON THE FIRST ONE, which is the rule this split
         * exists to preserve rather than to weaken. */
        let mut r = Persisted::default();
        r.refuse("0.2.0", "aa", &Refusal::ArtifactBadSignature("x".into()));
        assert!(r.has_refused("0.2.0", "aa"));
    }

    /// DEFECT THIS PREVENTS: A BUILD THAT CANNOT DRAW BEING REINSTALLED EVERY SESSION, FOR EVER.
    ///
    /// # THE CYCLE
    ///
    /// 0.2.0 installs and passes the three second preflight, then fails to draw on real launches:
    /// a GPU driver the smoke run got away with, an antivirus that quarantines the exe after it is
    /// first written, a hotkey conflict that only appears with the full session running. Two dead
    /// launches, the trampoline rolls back, 0.1.0 opens. `install::roll_back` returns `away_from`
    /// and its doc says it exists "so the caller can write it into `state.json` and decline to
    /// offer it again"; the trampoline wrote `current = Some(back.now)` and dropped it, and
    /// `grep -rn away_from` found one production write and zero production reads. So the next
    /// check offers 0.2.0 straight back: 10.8 MB, an install, two dead launches and another
    /// rollback, next session, and the session after that.
    ///
    /// # IT IS NOT A PERMANENT REFUSAL, AND THAT IS THE OTHER HALF
    ///
    /// A version this machine fled is offerable again the moment the manifest names something
    /// strictly above it, because that is the publisher having shipped a fix. A test that only
    /// proved the refusal would be green for an implementation that bricked the channel.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `was_rolled_back` branch from `check`; have
    /// `note_rolled_back` write nothing; or have `was_rolled_back` answer true for every version,
    /// and the 0.2.1 case below stops being offered.
    #[test]
    fn a_version_this_machine_rolled_back_from_is_not_offered_again_on_its_own() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("rolled-back");
        let l = Layout::at(&d);
        let s = UpdaterSettings::default();

        /* The trampoline's half, which is a free function precisely because `main` cannot be
         * called from a test. */
        note_rolled_back(&l, "0.2.0").expect("a scratch path is writable");
        assert!(
            read_state(&l).was_rolled_back(&Version::parse("0.2.0").expect("literal")),
            "the rollback was not recorded, so nothing below is being tested"
        );
        note_rolled_back(&l, "0.2.0").expect("recording it twice is not an error");
        assert_eq!(
            read_state(&l).rolled_back_from.len(),
            1,
            "every launch after a rollback would add a row"
        );

        let payload = b"the 0.2.0 payload".to_vec();
        let (wire, asked) = Fake::new(
            &signed_release(&signer, "0.2.0", "2026-09-14T18:02:11Z", &payload),
            Ok(payload.clone()),
        );
        let body = wire.body.clone();
        let bytes = wire.bytes.clone();
        let u = Updater::spawn(l.clone(), Box::new(wire), keys, None, &s, CHECK_EVERY);

        let refused = |u: &Updater| matches!(u.view().phase, Phase::Refused { .. });
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                refused(&u)
            }),
            "the version this machine rolled back from was offered again; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(
            asked.load(Ordering::Relaxed),
            0,
            "10.8 MB was fetched again for a build this machine has already run and fled"
        );
        let Phase::Refused { why, .. } = u.view().phase else {
            panic!("checked above")
        };
        assert!(
            why.contains("rolled back"),
            "the screen does not say why nothing is being offered: {why:?}"
        );

        /* AND A FIX IS TAKEN. The publisher names something above it and the machine moves. */
        let next = b"the 0.2.1 payload".to_vec();
        *lock(&body) = Ok(signed_release(
            &signer,
            "0.2.1",
            "2026-09-20T09:00:00Z",
            &next,
        ));
        *lock(&bytes) = Ok(next.clone());
        u.ask(Ask::CheckNow);
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                matches!(u.view().phase, Phase::Downloaded { ref version } if version == "0.2.1")
            }),
            "a release published above the rolled-back one was not taken, so a machine that rolls \
             back once never updates again; the phase was {:?}",
            u.view().phase
        );

        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT THIS PREVENTS: TWO COPIES OF THE APP ERASING EACH OTHER'S BOOKKEEPING, AND A
    /// WITHDRAWN BUILD BECOMING UN-WITHDRAWN.
    ///
    /// # THE SEQUENCE
    ///
    /// `state.json` was read once into `Worker::persisted` at spawn and rewritten WHOLE, with no
    /// re-read, no merge and no lock. Nothing stops two copies of the app running: there is no
    /// single-instance guard in the crate and `launch::ENTRY_ENV`'s own doc contemplates "two
    /// copies of the app started from two different folders", and both get the same layout root.
    /// So copy A is open all evening; copy B checks, accepts a manifest that withdraws 0.2.0 and
    /// writes `yanked: ["0.2.0"]`; copy A later refuses something and writes its own hours-old
    /// struct back. `yanked` is empty again, the replay floor has walked backwards, and every
    /// refusal B recorded is gone. `launch::plan` will then happily exec the withdrawn build at
    /// the next launch, which is the exact failure the yank mechanism exists to prevent.
    ///
    /// # THE MERGE RULES ARE NOT ARBITRARY AND THE TEST SAYS WHICH IS WHICH
    ///
    /// Monotone fields merge by maximum or union, because losing one loses a decision. The fields
    /// that are a picture of a moment rather than an accumulation merge by
    /// [`Persisted::stamped`], because for those there is no union that is meaningful: a publisher
    /// un-withdrawing a version is a legitimate act and a union would make it impossible.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `write_state` write `p` instead of the merge; take the
    /// minimum for `accepted`; drop the `stamped` comparison so the writer always wins; or drop
    /// `install::with_lock` from `write_state`, which the concurrent half below catches.
    #[test]
    fn two_copies_of_the_app_do_not_erase_each_others_state() {
        let d = scratch("state-merge");
        let l = Layout::at(&d);
        let when = |s: &str| {
            s.parse::<DateTime<Utc>>()
                .expect("a literal stamp in a test")
        };

        /* COPY B: accepts a manifest that withdraws 0.2.0. */
        let mut b = read_state(&l);
        b.accepted
            .insert("stable".into(), when("2026-09-20T00:00:00Z"));
        b.yanked = vec!["0.2.0".into()];
        b.touch();
        b.refuse("0.9.9", "bb", &Refusal::ArtifactBadSignature("x".into()));
        write_state(&l, &b).expect("copy B writes");

        /* COPY A: a picture read hours ago, with a refusal of its own to record. `stamped` is
         * `None` on it, because it never touched the whole-document fields. */
        let mut a = Persisted {
            accepted: [("stable".to_owned(), when("2026-09-14T00:00:00Z"))]
                .into_iter()
                .collect(),
            ..Persisted::default()
        };
        a.refuse("0.8.8", "aa", &Refusal::ArtifactBadSignature("x".into()));
        let merged = write_state(&l, &a).expect("copy A writes");

        assert_eq!(
            merged.yanked,
            vec!["0.2.0".to_owned()],
            "the withdrawal recorded by the other copy was erased, so launch::plan will start a \
             build the publisher has taken back"
        );
        assert_eq!(
            merged.accepted.get("stable").copied(),
            Some(when("2026-09-20T00:00:00Z")),
            "the replay floor walked backwards, which is the defence against a stale manifest \
             being undone by the other copy of the app"
        );
        assert!(
            merged.has_refused("0.9.9", "bb") && merged.has_refused("0.8.8", "aa"),
            "a refusal was lost, so this client will fetch bytes it already decided against"
        );
        /* And the file on disk says the same thing as the value handed back, or the worker's own
         * memory and the next reader's would disagree. */
        let disk = read_state(&l);
        assert_eq!(disk.yanked, merged.yanked);
        assert_eq!(disk.refused.len(), 2);

        /* AND THE NEWER PICTURE WINS RATHER THAN THE LAST WRITER. Copy A now un-withdraws 0.2.0
         * deliberately, which is a legitimate act, and its stamp is newer. */
        let mut later = merged.clone();
        later.yanked = Vec::new();
        later.touch();
        let after = write_state(&l, &later).expect("copy A writes again");
        assert!(
            after.yanked.is_empty(),
            "a publisher un-withdrawing a version could never reach this machine"
        );

        /* THE LOCK, DRIVEN BY THE ONLY THING THAT CAN DRIVE IT: several writers at once. Threads
         * stand in for processes and the lock cannot tell the difference, because it is a file
         * created with `create_new`. Each writer records a refusal only it knows about, and all
         * of them have to survive. */
        let writers: Vec<_> = (0..6)
            .map(|n| {
                let root = d.clone();
                std::thread::spawn(move || {
                    let mine = Layout::at(root);
                    let mut p = read_state(&mine);
                    p.refuse(
                        &format!("1.0.{n}"),
                        "cc",
                        &Refusal::ArtifactBadSignature("x".into()),
                    );
                    write_state(&mine, &p).expect("a writer writes");
                })
            })
            .collect();
        for w in writers {
            w.join().expect("a writer finished");
        }
        let all = read_state(&l);
        for n in 0..6 {
            assert!(
                all.has_refused(&format!("1.0.{n}"), "cc"),
                "1.0.{n}'s refusal was lost to another writer, which is the whole-file overwrite \
                 this lock and merge exist to remove"
            );
        }

        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT THIS PREVENTS: PAYING FOR THE SAME TEN MEGABYTES ON EVERY LAUNCH.
    ///
    /// `Worker::staged` was in memory only and `Phase::Downloaded` did not survive a restart, and
    /// nothing at startup looked in `update\staging\<version>\` for a payload that was already
    /// downloaded, hashed and signature-checked. A reader who auto-downloaded and did not press
    /// Install paid for the whole artifact again on the next launch, writing it over a
    /// byte-identical file that was already there, every launch, for as long as they went on not
    /// pressing Install. The staging tree was never pruned either, because `prune` only walks
    /// `app\`.
    ///
    /// THE FILE IS RE-VERIFIED AND NOT TRUSTED. `check_file` is the same no-network, no-manifest
    /// pass `install_app` makes before it runs anything, and a staged file that no longer matches
    /// its seal is deleted rather than offered: a user-writable folder between two sessions can
    /// hold anything.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `recover_staged` call from the worker's start; do
    /// not persist `staged` on a successful download; or have `recover_staged` trust the file
    /// instead of checking it, and the tampered half below goes green when it should not.
    #[test]
    fn a_payload_already_staged_is_not_downloaded_again_next_session() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("staged-survives");
        let l = Layout::at(&d);
        /* STAGING IS WHAT THIS MEASURES, SO AUTO-INSTALL IS PINNED OFF. With it on, the default,
         * a successful stage runs straight into the install and the staging folder this test is
         * about is emptied before it can look. Turning it off here is not weakening the test: the
         * install path has its own, and a test that measured two behaviours at once would report
         * a change in either as a failure of both. */
        let s = UpdaterSettings {
            auto_install: false,
            ..UpdaterSettings::default()
        };
        let manifest = signed_release(
            &signer,
            "0.2.0",
            "2026-09-14T18:02:11Z",
            b"the 0.2.0 payload",
        );

        let (wire, first) = Fake::new(&manifest, Ok(b"the 0.2.0 payload".to_vec()));
        let u = Updater::spawn(l.clone(), Box::new(wire), keys, None, &s, CHECK_EVERY);
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                matches!(u.view().phase, Phase::Downloaded { .. })
            }),
            "0.2.0 never staged; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(first.load(Ordering::Relaxed), 1);
        drop(u);

        /* THE SECOND SESSION. Same layout, same manifest, a wire that would fail loudly if it were
         * asked for the body at all. */
        let (wire, again) = Fake::new(
            &manifest,
            Err("the second session must not fetch this".to_owned()),
        );
        let u = Updater::spawn(l.clone(), Box::new(wire), keys, None, &s, CHECK_EVERY);
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                matches!(u.view().phase, Phase::Downloaded { ref version } if version == "0.2.0")
            }),
            "the staged payload was not recovered; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(
            again.load(Ordering::Relaxed),
            0,
            "the payload was fetched again over a byte-identical file that was already on disk and \
             already verified"
        );
        drop(u);

        /* AND A STAGED FILE THAT IS NO LONGER WHAT WAS STAGED IS NOT OFFERED. Between two sessions
         * anything at all can happen to a file in a user-writable folder, so the recovery is a
         * re-verification and not a memory. */
        let staged = l
            .staging(&Version::parse("0.2.0").expect("literal"))
            .join(install::exe_name());
        std::fs::write(&staged, b"something else entirely").expect("the swap");
        let (wire, third) = Fake::new(&manifest, Ok(b"the 0.2.0 payload".to_vec()));
        let u = Updater::spawn(l.clone(), Box::new(wire), keys, None, &s, CHECK_EVERY);
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                third.load(Ordering::Relaxed) >= 1
            }),
            "a staged file that no longer verifies was offered as if it were the download; the \
             phase was {:?}",
            u.view().phase
        );

        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT: AN UPDATE THAT SITS THERE WAITING FOR A PRESS NOBODY SHOULD HAVE TO MAKE.
    ///
    /// The first cut staged a verified payload, set `Phase::Downloaded`, and stopped until
    /// somebody found the Settings screen and clicked Install. The owner's words on seeing it:
    /// the user should not be installing it. Nor should he: installing copies the binary in
    /// beside the versions already there and moves a pointer, the running app is untouched, and
    /// the new one starts at his next launch. Asking permission for that buys nothing and means
    /// a fix for a defect he has not noticed reaches him only if he goes looking.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `auto_install` branch after a successful stage
    /// in `download`, or defaulting `auto_install` to false. Either leaves the run parked on
    /// `Phase::Downloaded` and this never reaches `Installed`.
    #[test]
    fn a_verified_download_installs_itself_without_being_asked() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("auto-install");
        let l = Layout::at(&d);
        /* THE SHIPPED DEFAULTS, DELIBERATELY, because what is being measured is what happens to
         * somebody who changed nothing. */
        let s = UpdaterSettings::default();
        assert!(
            s.auto_install,
            "the shipped default must install without being asked"
        );
        let manifest = signed_release(
            &signer,
            "0.2.0",
            "2026-09-14T18:02:11Z",
            b"the 0.2.0 payload",
        );

        let (wire, _seen) = Fake::new(&manifest, Ok(b"the 0.2.0 payload".to_vec()));
        let u = Updater::spawn(l.clone(), Box::new(wire), keys, None, &s, CHECK_EVERY);

        /* IT MUST LEAVE `Downloaded` ON ITS OWN, and that is the whole assertion. Where it lands
         * after that is decided by the preflight, which spawns the staged binary and waits for it:
         * this test hands over the bytes `the 0.2.0 payload`, which Windows refuses to run, so the
         * honest terminal state here is a refusal that SAYS SO. Feeding it a real executable would
         * measure the preflight, which has its own tests, rather than the question here, which is
         * whether anybody had to press anything. */
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                matches!(
                    u.view().phase,
                    Phase::Installed { .. } | Phase::Refused { .. }
                )
            }),
            "the run parked after staging and waited to be asked: {:?}",
            u.view().phase
        );

        match u.view().phase {
            Phase::Installed { .. } => {}
            Phase::Refused { ref why, .. } => assert!(
                why.contains("would not start"),
                "the install was attempted but refused for a reason that is not the preflight: {why}"
            ),
            ref other => panic!("staging led somewhere that is not the install path: {other:?}"),
        }
    }

    /// DEFECT THIS PREVENTS: A SIGNED `size` OF TEN TERABYTES WRITING 128 MiB TO THE DISK ON EVERY
    /// CHECK.
    ///
    /// `fetch::MAX_ARTIFACT_BYTES` said of itself that it "is what stops a correctly signed
    /// manifest with a wrong `size` from filling a disk, and it is checked before a single byte is
    /// read". Neither half was true: the constant was only ever applied as `ureq`'s body limit,
    /// which is enforced while bytes stream, and `art.size` was compared to it nowhere in the
    /// crate. The result was bounded and failed closed, but 128 MiB was written to the reader's
    /// disk first, on every check that re-offered the same artifact, and the comment said it could
    /// not happen.
    ///
    /// THE ASSERTION THAT MATTERS IS THAT THE WIRE WAS NEVER ASKED, because "it fails" was already
    /// true and is not what was wrong.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the size comparison from `manifest::fetchable`, and
    /// the artifact counter goes from zero to one.
    #[test]
    fn an_absurd_signed_size_is_refused_before_the_body_is_requested() {
        let signer = Signer::new();
        let keys = static_keys(&signer);
        let d = scratch("absurd-size");
        let s = UpdaterSettings::default();

        /* A REAL SIGNED MANIFEST WITH ONE FIELD MOVED. The signature is made over the document
         * AFTER the size is changed, so this is a publisher's own claim and not a tampered
         * document, which is the case the check is for. */
        let payload = b"the 0.2.0 payload".to_vec();
        let mut doc: serde_json::Value = {
            let envelope = signed_release(&signer, "0.2.0", "2026-09-14T18:02:11Z", &payload);
            let v: serde_json::Value = serde_json::from_str(&envelope).expect("an envelope");
            serde_json::from_str(v["manifest"].as_str().expect("the manifest string"))
                .expect("a manifest")
        };
        doc["artifacts"][0]["size"] = serde_json::json!(fetch::MAX_ARTIFACT_BYTES + 1);
        let body = signer.envelope(&mprobe::manifest_text(&doc));

        let (wire, asked) = Fake::new(&body, Ok(vec![0u8; 64]));
        let u = Updater::spawn(Layout::at(&d), Box::new(wire), keys, None, &s, CHECK_EVERY);
        assert!(
            until(|| {
                u.pump(Pulse::Closed, true, &s);
                matches!(u.view().phase, Phase::Refused { .. })
            }),
            "a signed size above the ceiling was not refused; the phase was {:?}",
            u.view().phase
        );
        assert_eq!(
            asked.load(Ordering::Relaxed),
            0,
            "the body was requested for an artifact whose own signed size is above the ceiling, \
             so the cap is enforced by writing it to the disk rather than by not asking"
        );
        assert_eq!(
            u.view().failure.map(|f| f.code),
            Some("ArtifactTooLarge".to_owned())
        );

        drop(u);
        let _ = std::fs::remove_dir_all(&d);
    }

    /* ------------------------------------------------------------ what is offered -- */

    /// DEFECT THIS PREVENTS: A NEW BINARY BEING INSTALLED ON TOP OF AN OLD SNAPSHOT.
    ///
    /// `manifest::judge` returns the steps in install order and puts a required data bundle FIRST,
    /// which is the whole reason `requires_data` exists. This build can verify a bundle and cannot
    /// unpack one, so the only two honest answers are "install both" and "install neither"; taking
    /// the app step alone is the ordering violation that list exists to prevent, and it turns a
    /// working install into `Data::Failed` on a file name the reader cannot act on.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `choose_app_step` pick the `app` entry and ignore the
    /// data one, which is what `steps.iter().find(|a| a.kind == KIND_APP)` on its own does.
    #[test]
    fn an_offer_this_build_cannot_carry_out_in_full_is_refused_in_full() {
        let art = |v: &serde_json::Value| -> Artifact {
            serde_json::from_value(v.clone()).expect("a probe artifact parses")
        };
        let app = art(&mprobe::app("0.2.0"));
        let data = art(&mprobe::data("2026-09-14"));

        let alone = choose_app_step(std::slice::from_ref(&app)).expect("an app on its own");
        assert_eq!(alone.kind, KIND_APP);
        assert_eq!(alone.version, "0.2.0");

        let both = choose_app_step(&[data, app])
            .expect_err("a release carrying a data bundle was half applied");
        assert!(
            both.contains("2026-09-14") && both.contains("by hand"),
            "the refusal names neither the bundle nor what to do instead: {both}"
        );

        let neither = choose_app_step(&[]).expect_err("an offer with no program in it was taken");
        assert!(
            neither.contains("no program"),
            "the refusal does not say what was missing: {neither}"
        );
    }

    /// DEFECT THIS PREVENTS: A REFUSED VERSION BEING FETCHED AGAIN EVERY SIX HOURS FOREVER.
    ///
    /// Retrying a signature failure on a loop turns a transient attack into a persistent one and
    /// buries the message under a spinner. The skip is keyed on the version AND the artifact hash,
    /// so a republished 0.2.0 with different bytes is a different artifact and gets another try,
    /// which is the half a version-only key gets wrong: a genuine bad build, fixed and republished
    /// under the same number, would be refused forever.
    ///
    /// The list is bounded too, and that is not decoration: a manifest republished every six hours
    /// with a bad signature would otherwise grow `state.json` without limit.
    ///
    /// WHAT MUTATION MAKES THIS RED: compare only `r.version` in `has_refused`; or drop the
    /// `retain` in `refuse`, which lets one version accumulate rows without end.
    #[test]
    fn a_refusal_is_remembered_by_its_bytes_and_not_just_by_its_number() {
        let mut p = Persisted::default();
        assert!(!p.has_refused("0.2.0", "aa"));
        p.refuse("0.2.0", "aa", &Refusal::ArtifactBadSignature("x".into()));
        assert!(p.has_refused("0.2.0", "aa"), "the refusal was not recorded");
        assert!(
            !p.has_refused("0.2.0", "bb"),
            "0.2.0 republished with different bytes is still refused, so a fixed build could never \
             reach this machine"
        );
        assert!(!p.has_refused("0.3.0", "aa"));

        for _ in 0..10 {
            p.refuse("0.2.0", "aa", &Refusal::ArtifactBadSignature("x".into()));
        }
        assert_eq!(
            p.refused.len(),
            1,
            "ten refusals of one version wrote ten rows; state.json grows without limit"
        );

        /* AND A REFUSAL THAT WAS ABOUT THIS MACHINE DOES NOT GO IN THAT LIST AT ALL. See
         * `a_full_disk_does_not_make_a_release_uninstallable_for_ever` below for the whole of
         * that rule; this is the boundary between the two halves of `refuse`. */
        let mut m = Persisted::default();
        m.refuse(
            "0.2.0",
            "aa",
            &Refusal::Io {
                doing: "write the download to",
                path: PathBuf::from("x"),
                why: "there is not enough space on the disk".into(),
            },
        );
        assert!(
            !m.has_refused("0.2.0", "aa"),
            "a full disk was recorded the way a bad signature is, which makes that release \
             permanently uninstallable on this machine"
        );
        assert_eq!(m.attempts_at("0.2.0", "aa"), 1);

        /* AND IT SURVIVES THE FILE. A refusal forgotten at the next launch is a download this
         * client already decided not to make, paid for again on every launch. */
        let d = scratch("state");
        let l = Layout::at(&d);
        write_state(&l, &p).expect("a scratch path is writable");
        let back = read_state(&l);
        assert!(
            back.has_refused("0.2.0", "aa"),
            "the refusal did not survive being written and read back"
        );
        assert!(
            read_state(&Layout::at(d.join("nothing-here")))
                .refused
                .is_empty(),
            "a missing state file is not answered with the default, so a first launch would fail \
             rather than start with nothing recorded"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// DEFECT THIS PREVENTS: SWITCHING CHANNELS REFUSING EVERY MANIFEST AFTERWARDS.
    ///
    /// `published` is the replay defence: a manifest older than the newest one accepted is refused
    /// (`manifest::judge`). Keep ONE stamp across both channels and the first switch from beta
    /// back to stable refuses the stable manifest as a replay, because a stable release is older
    /// than the beta that preceded it by construction. The symptom is an app that says it could
    /// not check and never recovers, on one of the two actions this section offers.
    ///
    /// WHAT MUTATION MAKES THIS RED: make `Persisted::accepted` a single `Option<DateTime<Utc>>`
    /// rather than a map keyed by channel.
    #[test]
    fn the_replay_floor_is_per_channel_so_switching_back_is_not_a_replay() {
        let d = scratch("channels");
        let l = Layout::at(&d);
        let t = |s: &str| {
            DateTime::parse_from_rfc3339(s)
                .expect("a literal stamp")
                .with_timezone(&Utc)
        };
        let mut p = Persisted::default();
        p.accepted
            .insert(CHANNELS[1].to_owned(), t("2026-09-20T00:00:00Z"));
        p.accepted
            .insert(CHANNELS[0].to_owned(), t("2026-09-14T00:00:00Z"));
        write_state(&l, &p).expect("a scratch path is writable");

        let back = read_state(&l);
        assert_eq!(
            back.accepted.get(CHANNELS[0]).copied(),
            Some(t("2026-09-14T00:00:00Z")),
            "the stable floor was overwritten by the beta one, so every stable manifest would be \
             refused as a replay"
        );
        assert_eq!(
            back.accepted.get(CHANNELS[1]).copied(),
            Some(t("2026-09-20T00:00:00Z"))
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
