//! The updater CORE: the half of the feature that decides, and never the half that acts on its
//! own. No UI, no threads, no global state, no network.
//!
//! # WHAT THIS MODULE IS AND WHAT IT DELIBERATELY IS NOT
//!
//! Everything here is a plain function or a plain struct a caller drives. The poll thread, the
//! `ureq` download, the UPDATES section on the Settings screen and the `Updater` field on `App`
//! are the OTHER half and are not in this file set. The split is not tidiness: it is the only way
//! the security-critical decisions get tests. `App::ui` cannot be called from a test at all
//! (`main.rs:2537` states why: `eframe::Frame` has no public constructor), and a decision written
//! inside a `ui` branch or inside a worker thread's closure is a decision no test can drive. So
//! the rule here is that nothing in this module reaches out: the caller hands in the bytes, the
//! clock, the paths and the fight state, and gets back a `Result` it has to look at.
//!
//! # THE ONE-WAY DOOR, WHICH IS WHY [`verify::KEYS`] IS A SLICE
//!
//! A released binary trusts the public keys it was compiled with and nothing else, forever. There
//! is no channel by which a shipped binary can be taught a new key, because teaching it one would
//! be an update, and an update is the thing the key authorises. Rotation therefore only works
//! FORWARDS: release N+1 carries key N+1 and is signed by key N, and only once the field is on
//! N+1 does signing switch. That is impossible unless the trusted set was a list from the first
//! release, so it is a list now, with one entry. See [`verify::KEYS`].
//!
//! # THE ORDER OF THE GATES IS THE DESIGN
//!
//! Each step is what makes the next step's inputs trustworthy, and nothing downstream of a failed
//! step runs, INCLUDING display. Nothing in this module parses a field of an unverified manifest
//! for any purpose. The type system carries that rule: [`manifest::judge`] takes a
//! [`verify::Verified`], and the only way to get one is [`verify::open`], which is the signature
//! gate.
//!
//! ```text
//! envelope version -> manifest signature -> channel -> published not older than last accepted
//!   -> format -> version comparison and yank/rollback rules -> artifact selection
//!   -> download with size enforced and sha256 streamed -> artifact signature (verify_stream)
//!   -> preflight smoke -> pointer flip
//! ```
//!
//! # WHERE THE OTHER HALF IS, NOW THAT IT EXISTS
//!
//! This tree's signature defect is code that compiles, is tested, and has zero production
//! callers, and `Cargo.toml` apologises twice for a dependency that arrived before the code that
//! used it. The note that stood here said this module had no caller and would get one with the
//! poll thread. It has one: `main.rs` calls `trampoline` on the launch path and `updater::run`
//! from `App::heartbeat`, and the Settings screen's UPDATES section draws
//! [`run::UpdateView`]. `grep -rn "updater::" src/` answers with those and not with comments.
//!
//! [`fetch`] and [`run`] are that other half and sit beside these four ON PURPOSE rather than
//! inside them. Everything in `install`, `launch`, `manifest` and `verify` still has no UI, no
//! threads and no network, which is the property the header above claims and the reason those
//! four are testable; putting a thread in one of them would make that claim false. The decision
//! spec puts `Updater` in this file, and the deviation is one line of module list against a
//! header that would otherwise have to be rewritten into something less true.
//!
//! Adding `pub mod updater;` to `lib.rs` is a change to the module set, which `lib.rs:27` calls
//! the contract fixed by decision D9. It is stated as one here and in the commit message.

pub mod fetch;
pub mod install;
pub mod launch;
pub mod manifest;
pub mod run;
pub mod verify;

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use semver::Version;

/// EVERY WAY THIS FEATURE IS ALLOWED TO SAY NO, IN ONE CLOSED SET.
///
/// # WHY ONE ENUM AND NOT `Result<_, String>`
///
/// The rest of this crate answers a failure with a sentence, and for a log folder that is the
/// right shape: there is one screen, it prints the sentence, and nobody has to branch on it. This
/// is different in three ways that each need a machine-readable cause.
///
///   * `update\state.json` records the cause so the updater can refuse to retry THAT version on
///     the timer. Retrying a signature failure on a loop turns a transient attack into a
///     persistent one and buries the message under a spinner.
///   * A corrupt download and a tampered download read as the same complaint to a careless
///     reader and must never be the same event: one is the CDN having a bad day, the other is
///     someone serving bytes the key never signed.
///   * A human pasting a refusal into a bug report should be pasting a stable word, not a
///     sentence that will be reworded next week. [`Refusal::code`] is that word.
///
/// [`fmt::Display`] is the sentence. It names the thing that was wrong AND what was expected,
/// because "signature check failed" sends a person to the wrong half of the pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The body served at the manifest URL is not JSON, or is JSON that is not an object.
    EnvelopeUnreadable(String),
    /// The body is an object but is not a signed envelope at all. This is exactly what the one
    /// placeholder object currently in the `grimoire-updates` bucket hits, and it is the correct
    /// refusal for it: no `envelope`, no `manifest` string, no `signature`.
    ManifestUnsigned { missing: &'static str },
    /// An envelope version this build does not know. The envelope is what says WHERE the
    /// signature is, so guessing is not an option and this is a refusal rather than a skip.
    EnvelopeVersion { saw: u64, knows: u64 },
    /// The signature does not check out against any compiled-in key. Includes the case of a
    /// signature made by a key that is not one of ours, which `minisign-verify` reports
    /// separately as a key-id mismatch.
    ManifestBadSignature(String),
    /// The signed text is not a manifest this build can read. Reached ONLY after the signature
    /// verified, which is the point: a publisher wrote this, not a stranger.
    ManifestUnreadable(String),
    /// The signed manifest names a different channel than the one that was asked for. The field
    /// is inside the signed bytes, so this catches a beta manifest served at the stable path.
    ManifestWrongChannel { asked: String, saw: String },
    /// A correctly signed manifest older than the newest one already accepted. The replay
    /// defence: a stale object from a cache cannot walk a client backwards.
    ManifestStale {
        saw: DateTime<Utc>,
        last: DateTime<Utc>,
    },
    /// A correctly signed manifest whose own `expires` stamp has passed.
    ///
    /// THE HALF OF THE REPLAY DEFENCE THAT NEEDS NOTHING STORED. [`Refusal::ManifestStale`] is the
    /// other half and it is empty on exactly the machines that need it most: a fresh install, a
    /// machine whose `state.json` was lost, and the first check after a channel switch all have no
    /// floor at all. A signed envelope that expires is refusable by a client that has never
    /// accepted anything, which is what bounds the life of a captured document.
    ManifestExpired {
        expires: DateTime<Utc>,
        now: DateTime<Utc>,
    },
    /// A manifest format newer than this build. The reader is told to fetch a new build by hand.
    ManifestFormatTooNew { saw: u64, knows: u64 },
    /// A manifest format this build does not know and that is not simply newer, which means the
    /// document was not written by this pipeline.
    ManifestFormatUnknown { saw: u64, knows: u64 },
    /// A version string in the manifest that is not a version. `field` names which one.
    VersionUnreadable { field: &'static str, saw: String },
    /// A lower version than the one running, with no `rollback` object authorising it.
    Downgrade { running: Version, offered: Version },
    /// A `rollback` object that does not describe this machine's situation. Both `to` and `from`
    /// must be present and `from` must equal the running version.
    RollbackMismatch {
        running: Version,
        to: Version,
        from: Version,
    },
    /// The version this manifest publishes is in its own `yanked` list.
    Yanked { version: Version },
    /// The running version is older than the oldest one allowed to jump straight to this release.
    TooOldToJump { running: Version, floor: Version },
    /// No artifact in the manifest matches what this build is. Names what it looked for, because
    /// "no artifact" alone cannot tell a Windows reader that only a macOS build was published.
    NoArtifact {
        kind: String,
        os: String,
        arch: String,
    },
    /// An artifact whose own `version` disagrees with the version the manifest publishes. Both
    /// numbers are inside the signed bytes, so this is a broken pipeline rather than an attack.
    ArtifactVersionMismatch { manifest: Version, artifact: String },
    /// The app artifact requires a data bundle at least this new and the manifest offers none new
    /// enough to install first.
    DataBundleMissing { requires: String },
    /// A data bundle that declares it needs an app newer than the one being offered.
    DataBundleTooNew { min_app: Version, offered: Version },
    /// A data bundle version that is not the `YYYY-MM-DD` shape the ordering rule depends on.
    DataVersionUnreadable { saw: String },
    /// An artifact whose `url` is not on the update host, over https.
    ///
    /// THE CLIENT'S OWN COPY OF A RULE THAT LIVED ONLY IN THE PIPELINE. `release.yml` already
    /// refuses to publish a manifest whose artifact url is not under the public base, and a
    /// defence that exists in the publisher and not in the client is a defence against the
    /// publisher's own mistakes and nothing else. The signature gate still holds the BYTES to what
    /// the manifest promised; what this stops is one signed document sending every client's
    /// address and user agent to a host of somebody else's choosing.
    ArtifactUrlNotOurs { url: String, expected: String },
    /// An artifact whose signed `size` is larger than [`fetch::MAX_ARTIFACT_BYTES`].
    ///
    /// ASKED BEFORE A BYTE IS REQUESTED, which is what the cap's own doc says of it and what was
    /// not true until this variant existed: the ceiling was only ever applied as `ureq`'s body
    /// limit, so a signed manifest carrying an absurd `size` wrote the whole of that limit to the
    /// reader's disk on every check before failing.
    ArtifactTooLarge { said: u64, cap: u64 },
    /// The stream ended short of, or ran past, the size the signed manifest states.
    ArtifactSizeMismatch { said: u64, got: u64 },
    /// The bytes on the server are not the bytes the signed manifest named. Corruption, not
    /// tampering: a tampered artifact fails the signature, and these are deliberately two causes.
    ArtifactHashMismatch { said: String, got: String },
    /// The artifact's own detached signature does not check out.
    ArtifactBadSignature(String),
    /// The `sha256` or `signature` text in the manifest is malformed, so the check cannot be set
    /// up at all. Distinct from a check that ran and said no.
    ArtifactCheckUnbuildable { field: &'static str, why: String },
    /// A file that verified when it was staged no longer verifies where it was installed.
    StagedFileChangedOnDisk { path: PathBuf, why: Box<Refusal> },
    /// An archive entry whose name or type would let it write outside the target directory.
    UnsafeArchiveEntry { entry: String, why: &'static str },
    /// An extracted data bundle that does not hold everything the loader needs. Checked BEFORE
    /// the swap, so a bundle missing a file never replaces a snapshot that works.
    BundleIncomplete { missing: String },
    /// A download was asked for while the reader is swinging, or standing in the camp with the
    /// next mob on its way. See [`may_start_download`].
    NotAQuietMoment { pulse: &'static str },
    /// A filesystem call this module made did not work.
    Io {
        doing: &'static str,
        path: PathBuf,
        why: String,
    },
    /// The staged binary would not start on this machine.
    PreflightFailed { exe: PathBuf, why: String },
    /// A rollback was asked for and there is no previous version on disk to roll back to.
    NoPreviousVersion,
    /// [`verify::KEYS`] is empty, or an entry in it is not a minisign public key. A build in this
    /// state can verify nothing, so it must say so rather than quietly refuse every update
    /// forever. A test holds the shipped constant to this.
    NoTrustAnchor { why: String },
}

impl Refusal {
    /// THE STABLE WORD FOR `update\state.json` AND FOR A BUG REPORT.
    ///
    /// Deliberately not derived from the variant name by a macro: the wire word is a promise to
    /// the file on disk and to whoever reads it, and renaming a Rust variant must not silently
    /// rewrite last week's recorded failures.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::EnvelopeUnreadable(_) => "EnvelopeUnreadable",
            Refusal::ManifestUnsigned { .. } => "ManifestUnsigned",
            Refusal::EnvelopeVersion { .. } => "EnvelopeVersion",
            Refusal::ManifestBadSignature(_) => "ManifestBadSignature",
            Refusal::ManifestUnreadable(_) => "ManifestUnreadable",
            Refusal::ManifestWrongChannel { .. } => "ManifestWrongChannel",
            Refusal::ManifestStale { .. } => "ManifestStale",
            Refusal::ManifestExpired { .. } => "ManifestExpired",
            Refusal::ManifestFormatTooNew { .. } => "ManifestFormatTooNew",
            Refusal::ManifestFormatUnknown { .. } => "ManifestFormatUnknown",
            Refusal::VersionUnreadable { .. } => "VersionUnreadable",
            Refusal::Downgrade { .. } => "Downgrade",
            Refusal::RollbackMismatch { .. } => "RollbackMismatch",
            Refusal::Yanked { .. } => "Yanked",
            Refusal::TooOldToJump { .. } => "TooOldToJump",
            Refusal::NoArtifact { .. } => "NoArtifact",
            Refusal::ArtifactVersionMismatch { .. } => "ArtifactVersionMismatch",
            Refusal::DataBundleMissing { .. } => "DataBundleMissing",
            Refusal::DataBundleTooNew { .. } => "DataBundleTooNew",
            Refusal::DataVersionUnreadable { .. } => "DataVersionUnreadable",
            Refusal::ArtifactUrlNotOurs { .. } => "ArtifactUrlNotOurs",
            Refusal::ArtifactTooLarge { .. } => "ArtifactTooLarge",
            Refusal::ArtifactSizeMismatch { .. } => "ArtifactSizeMismatch",
            Refusal::ArtifactHashMismatch { .. } => "ArtifactHashMismatch",
            Refusal::ArtifactBadSignature(_) => "ArtifactBadSignature",
            Refusal::ArtifactCheckUnbuildable { .. } => "ArtifactCheckUnbuildable",
            Refusal::StagedFileChangedOnDisk { .. } => "StagedFileChangedOnDisk",
            Refusal::UnsafeArchiveEntry { .. } => "UnsafeArchiveEntry",
            Refusal::BundleIncomplete { .. } => "BundleIncomplete",
            Refusal::NotAQuietMoment { .. } => "NotAQuietMoment",
            Refusal::Io { .. } => "Io",
            Refusal::PreflightFailed { .. } => "PreflightFailed",
            Refusal::NoPreviousVersion => "NoPreviousVersion",
            Refusal::NoTrustAnchor { .. } => "NoTrustAnchor",
        }
    }

    /// IS THIS REFUSAL A STATEMENT ABOUT THE BYTES, OR ABOUT THIS MACHINE?
    ///
    /// # WHY THE SET HAS TO BE SPLIT AT ALL
    ///
    /// `update\state.json` records refusals so a version that was refused is not fetched again on
    /// the timer, and retrying a signature failure on a loop turns a transient attack into a
    /// persistent one. That reasoning is exactly right for a refusal that is ABOUT THE BYTES: no
    /// amount of waiting changes whether a key signed them. It is exactly wrong for a refusal that
    /// is about the machine. [`Refusal::Io`] is a disk that filled, an antivirus that held the
    /// file open for a second, a folder that was not writable at that instant; recording one of
    /// those the same way makes a release permanently uninstallable on that machine, and the
    /// release pipeline refuses to republish the same version with different bytes, so there is no
    /// escape at all.
    ///
    /// # WHY THIS IS AN EXHAUSTIVE `match` AND NOT A LIST OF THE BAD ONES
    ///
    /// A new variant must not be able to default into the permanent bucket by nobody thinking
    /// about it. Written this way, adding a variant is a compile error here, and
    /// `every_refusal_says_whether_it_is_about_the_bytes` enumerates the whole set so the answer
    /// is written down rather than guessed.
    pub fn is_about_the_bytes(&self) -> bool {
        match self {
            /* THE DOCUMENT OR THE FILE IS WRONG, AND IT WILL BE JUST AS WRONG IN AN HOUR. Every
             * one of these was decided by looking at bytes that were fully in hand. */
            Refusal::EnvelopeUnreadable(_)
            | Refusal::ManifestUnsigned { .. }
            | Refusal::EnvelopeVersion { .. }
            | Refusal::ManifestBadSignature(_)
            | Refusal::ManifestUnreadable(_)
            | Refusal::ManifestWrongChannel { .. }
            | Refusal::ManifestStale { .. }
            | Refusal::ManifestFormatTooNew { .. }
            | Refusal::ManifestFormatUnknown { .. }
            | Refusal::VersionUnreadable { .. }
            | Refusal::Downgrade { .. }
            | Refusal::RollbackMismatch { .. }
            | Refusal::Yanked { .. }
            | Refusal::TooOldToJump { .. }
            | Refusal::NoArtifact { .. }
            | Refusal::ArtifactVersionMismatch { .. }
            | Refusal::DataBundleMissing { .. }
            | Refusal::DataBundleTooNew { .. }
            | Refusal::DataVersionUnreadable { .. }
            | Refusal::ArtifactUrlNotOurs { .. }
            | Refusal::ArtifactTooLarge { .. }
            | Refusal::ArtifactSizeMismatch { .. }
            | Refusal::ArtifactHashMismatch { .. }
            | Refusal::ArtifactBadSignature(_)
            | Refusal::ArtifactCheckUnbuildable { .. }
            | Refusal::UnsafeArchiveEntry { .. }
            | Refusal::BundleIncomplete { .. } => true,

            /* THE MACHINE, THE MOMENT, OR THE FILESYSTEM. None of these looked at the bytes and
             * found them wanting, so none of them is evidence about the release.
             *
             * `ManifestExpired` is here and its neighbours above are not, deliberately: it is the
             * one manifest refusal that can be true now and false later without the document
             * changing at all, because the thing that moved is this machine's clock. A client
             * whose clock was wrong by a week would otherwise write a permanent refusal for a
             * release that is perfectly current.
             *
             * `StagedFileChangedOnDisk` is the file on THIS disk disagreeing with a seal that the
             * same bytes already satisfied once, which is a disk, an antivirus or another writer,
             * and is answered by fetching them again.
             *
             * `NotAQuietMoment` and `NoTrustAnchor` are not failures of a release either: the
             * first is a fight that will end and the second is a property of this build. */
            Refusal::ManifestExpired { .. }
            | Refusal::StagedFileChangedOnDisk { .. }
            | Refusal::NotAQuietMoment { .. }
            | Refusal::Io { .. }
            | Refusal::PreflightFailed { .. }
            | Refusal::NoPreviousVersion
            | Refusal::NoTrustAnchor { .. } => false,
        }
    }

    /// HOW MANY TIMES A VERSION MAY BE FETCHED AGAIN AFTER THIS REFUSAL BEFORE IT IS PERMANENT.
    ///
    /// [`Refusal::is_about_the_bytes`] is the rule and this is the one exception to it, written
    /// here rather than folded into that answer so the exception is visible.
    ///
    /// `ArtifactHashMismatch` IS about the bytes and still gets one more try. Two things present
    /// as a hash mismatch: a CDN or a cable that corrupted the transfer, and TWO WRITERS staging
    /// the same file at once, which this app can produce on its own when a second copy is opened
    /// from a second folder. Both of those are gone on the next attempt and neither is evidence
    /// that a publisher served bytes the key never signed. A second mismatch over the same
    /// artifact is a statement about the artifact, and the count is what separates them.
    pub fn retries(&self) -> u32 {
        if !self.is_about_the_bytes() {
            return RETRY_WHILE_IT_KEEPS_HAPPENING;
        }
        match self {
            Refusal::ArtifactHashMismatch { .. } => 1,
            _ => 0,
        }
    }
}

/// The retry budget of a refusal that is not about the bytes: there is no count at which a full
/// disk becomes a statement about a release, so there is no count here either.
pub const RETRY_WHILE_IT_KEEPS_HAPPENING: u32 = u32::MAX;

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::EnvelopeUnreadable(why) => {
                write!(
                    f,
                    "the update server did not answer with a JSON object: {why}"
                )
            }
            Refusal::ManifestUnsigned { missing } => write!(
                f,
                "the update server answered with something that is not a signed manifest: it has \
                 no {missing} field"
            ),
            Refusal::EnvelopeVersion { saw, knows } => write!(
                f,
                "the update server sent envelope version {saw} and this build knows {knows}; \
                 download a new version by hand"
            ),
            Refusal::ManifestBadSignature(why) => write!(
                f,
                "the manifest is not signed by a key this build trusts, so nothing in it was \
                 read: {why}"
            ),
            Refusal::ManifestUnreadable(why) => {
                write!(
                    f,
                    "the signed manifest is not one this build can read: {why}"
                )
            }
            Refusal::ManifestWrongChannel { asked, saw } => write!(
                f,
                "the {asked} channel served a manifest that says it is for {saw}; this is a \
                 server mistake, not an update"
            ),
            Refusal::ManifestStale { saw, last } => write!(
                f,
                "the manifest published at {saw} is older than the one already accepted, \
                 published at {last}; a stale copy cannot replace a newer one"
            ),
            Refusal::ManifestExpired { expires, now } => write!(
                f,
                "the manifest served on this channel expired at {expires} and it is now {now}; \
                 the channel is serving an old document rather than the current one"
            ),
            Refusal::ManifestFormatTooNew { saw, knows } => write!(
                f,
                "the manifest is format {saw} and this build reads format {knows}; download a new \
                 version by hand"
            ),
            Refusal::ManifestFormatUnknown { saw, knows } => write!(
                f,
                "the manifest declares format {saw}, which this build does not know and is not \
                 newer than the {knows} it reads; it was not written by this pipeline"
            ),
            Refusal::VersionUnreadable { field, saw } => {
                write!(f, "the manifest's {field} is not a version: {saw:?}")
            }
            Refusal::Downgrade { running, offered } => write!(
                f,
                "the manifest offers {offered} and this app is {running}; going backwards needs a \
                 rollback instruction from the publisher and there is none"
            ),
            Refusal::RollbackMismatch { running, to, from } => write!(
                f,
                "the rollback instruction says {from} goes back to {to}, and this app is \
                 {running}; it is not about this machine"
            ),
            Refusal::Yanked { version } => write!(
                f,
                "{version} was withdrawn by the publisher and will not be installed"
            ),
            Refusal::TooOldToJump { running, floor } => write!(
                f,
                "this app is {running} and the release requires at least {floor} to update from; \
                 install {floor} first"
            ),
            Refusal::NoArtifact { kind, os, arch } => write!(
                f,
                "the manifest publishes nothing for this build: it was searched for a {kind} \
                 artifact for {os} {arch}"
            ),
            Refusal::ArtifactVersionMismatch { manifest, artifact } => write!(
                f,
                "the manifest publishes {manifest} and the file it points at says it is \
                 {artifact:?}; the release was built wrong"
            ),
            Refusal::DataBundleMissing { requires } => write!(
                f,
                "the new app needs the {requires} data bundle or newer, and the manifest offers \
                 none new enough to install first"
            ),
            Refusal::DataBundleTooNew { min_app, offered } => write!(
                f,
                "the data bundle needs app {min_app} or newer and the manifest offers {offered}"
            ),
            Refusal::DataVersionUnreadable { saw } => write!(
                f,
                "a data bundle version must be a YYYY-MM-DD date so that newer sorts after older; \
                 this one is {saw:?}"
            ),
            /* TWO SENTENCES FROM ONE VARIANT, BECAUSE AN OVERLONG BODY CANNOT BE COUNTED. The
             * check stops one byte past the promised length rather than reading an unbounded body
             * to find out how much more there is, so `got` is the count at which it stopped and
             * saying "the server sent {got}" would be a number this code never measured. */
            Refusal::ArtifactUrlNotOurs { url, expected } => write!(
                f,
                "the signed manifest sends this download to {url}, and this build only fetches \
                 artifacts from {expected}"
            ),
            Refusal::ArtifactTooLarge { said, cap } => write!(
                f,
                "the signed manifest says the download is {said} bytes and this build will not \
                 fetch more than {cap}; nothing was requested"
            ),
            Refusal::ArtifactSizeMismatch { said, got } if got < said => write!(
                f,
                "the signed manifest says the download is {said} bytes and the stream ended at \
                 {got}"
            ),
            Refusal::ArtifactSizeMismatch { said, .. } => write!(
                f,
                "the signed manifest says the download is {said} bytes and the stream still had \
                 more to give"
            ),
            Refusal::ArtifactHashMismatch { said, got } => write!(
                f,
                "the bytes on the server are not the bytes the signed manifest named: it says \
                 sha256 {said} and they hash to {got}"
            ),
            Refusal::ArtifactBadSignature(why) => write!(
                f,
                "the downloaded file is not signed by a key this build trusts: {why}"
            ),
            Refusal::ArtifactCheckUnbuildable { field, why } => write!(
                f,
                "the manifest's {field} is malformed, so the download could not even be checked: \
                 {why}"
            ),
            Refusal::StagedFileChangedOnDisk { path, why } => write!(
                f,
                "{} verified when it was downloaded and does not verify where it was installed: \
                 {why}",
                path.display()
            ),
            Refusal::UnsafeArchiveEntry { entry, why } => write!(
                f,
                "the data bundle holds an entry that would write outside the folder it is being \
                 extracted into ({why}): {entry:?}"
            ),
            Refusal::BundleIncomplete { missing } => write!(
                f,
                "the data bundle does not hold {missing}, so it was not installed over the \
                 snapshot that is already there"
            ),
            Refusal::NotAQuietMoment { pulse } => write!(
                f,
                "not now: the encounter is {pulse}, and a download waits until it is closed"
            ),
            Refusal::Io { doing, path, why } => {
                write!(f, "could not {doing} {}: {why}", path.display())
            }
            Refusal::PreflightFailed { exe, why } => write!(
                f,
                "the downloaded {} would not start on this machine, so it was not installed: \
                 {why}",
                exe.display()
            ),
            Refusal::NoPreviousVersion => {
                write!(f, "there is no previous version on disk to go back to")
            }
            Refusal::NoTrustAnchor { why } => write!(
                f,
                "this build has no usable update signing key compiled into it, so it can verify \
                 nothing: {why}"
            ),
        }
    }
}

/// A filesystem failure, named by what was being attempted, in one place so every call site says
/// the same kind of sentence.
pub(crate) fn io(doing: &'static str, path: &std::path::Path, e: &std::io::Error) -> Refusal {
    Refusal::Io {
        doing,
        path: path.to_path_buf(),
        why: e.to_string(),
    }
}

/// IS THIS A MOMENT WHEN A DOWNLOAD MAY START?
///
/// # ONLY [`crate::fights::Pulse::Closed`], AND NOT `Ingest::fight_is_live()`
///
/// `Ingest::fight_is_live()` (`ingest.rs:3119`) is `pulse().fighting()`, so it already answers
/// false during [`crate::fights::Pulse::Holding`]. Holding is the six seconds after a kill when
/// the reader is standing in the camp with the next mob on its way (`fights.rs:84-88`). Starting
/// a download there is still an interruption, so gating on `!fight_is_live()` would interrupt
/// exactly the case the hold window exists to describe. The gate is the three-state answer, and
/// only `Closed` passes.
///
/// # WHAT THIS GATES AND WHAT IT DOES NOT
///
/// It gates STARTING a download, and the other half gates the "Restart now" button the same way.
/// It does NOT gate the pointer flip, and that is the quiet virtue of the install design in
/// [`install`]: the flip is one rename of one small file, the running process is not touched by
/// it, and nothing restarts until the reader closes the app. The risky moment was designed out
/// rather than scheduled around. Nor does it abort a download already in flight when a fight
/// opens: the bytes already spent would be wasted and what is left to transfer is the small end
/// of it.
pub fn may_start_download(pulse: crate::fights::Pulse) -> bool {
    matches!(pulse, crate::fights::Pulse::Closed)
}

/// The name of a pulse, for a refusal a person reads.
pub(crate) fn pulse_word(pulse: crate::fights::Pulse) -> &'static str {
    match pulse {
        crate::fights::Pulse::Fighting => "still going",
        crate::fights::Pulse::Holding => "held open for the next mob",
        crate::fights::Pulse::Closed => "closed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::Pulse;

    /// DEFECT THIS PREVENTS: A REFUSAL WHOSE WIRE WORD DRIFTS WITH A RUST RENAME.
    ///
    /// `update\state.json` records `Refusal::code()` so the updater can decline to retry the same
    /// failing version on the timer, and a person pastes it into a bug report. Two variants
    /// sharing a code would make those two failures indistinguishable in the file, which is
    /// exactly the distinction the enum exists to draw between a corrupt download and a tampered
    /// one.
    ///
    /// WHAT MUTATION MAKES THIS RED: give `ArtifactHashMismatch` the code
    /// `"ArtifactBadSignature"`, which is the confusion this whole enum exists to prevent.
    #[test]
    fn every_refusal_has_its_own_wire_word_and_its_own_sentence() {
        let all = one_of_every_refusal();
        let mut seen: Vec<&'static str> = Vec::new();
        for r in &all {
            assert!(
                !seen.contains(&r.code()),
                "two refusals answer to the wire word {:?}; state.json could not tell them apart",
                r.code()
            );
            seen.push(r.code());
            let said = r.to_string();
            assert!(
                said.len() > 20 && !said.contains("Refusal"),
                "{:?} does not read as a sentence a person can act on: {said:?}",
                r.code()
            );
        }

        /* ONE VARIANT, TWO SENTENCES. A stream that ended short can say how short; one that had
         * more to give cannot, because the check stops one byte past the promised length rather
         * than reading the rest to count it. Both sentences must exist or the second case would
         * print a byte count nothing measured. */
        let short = Refusal::ArtifactSizeMismatch { said: 10, got: 4 };
        assert!(short.to_string().contains("ended at 4"), "{short}");
        let long = Refusal::ArtifactSizeMismatch { said: 10, got: 11 };
        assert!(long.to_string().contains("more to give"), "{long}");
    }

    /// DEFECT THIS PREVENTS: A FULL DISK MAKING A RELEASE PERMANENTLY UNINSTALLABLE ON A MACHINE.
    ///
    /// # THE FAILURE, IN THE ORDER IT HAPPENS
    ///
    /// `Worker::refuse` writes a refusal into `update\state.json` and `Worker::check` then skips
    /// that version for ever, which is right for a signature that did not check out and wrong for
    /// everything that is about the machine rather than about the bytes. The disk fills while
    /// staging, `copy_sealed`'s write fails, `Refusal::Io` is recorded, and from then on every
    /// check in every future session short-circuits to "0.2.0 was refused earlier (Io)". The
    /// reader frees forty gigabytes and it makes no difference, because the release pipeline
    /// refuses to republish a version with different bytes, so nothing can ever change the key
    /// that refusal was filed under.
    ///
    /// # WHY THIS TEST ENUMERATES ALL OF THEM RATHER THAN SPOT-CHECKING
    ///
    /// The danger is a NEW variant defaulting into the permanent bucket because nobody thought
    /// about which one it belongs in. [`Refusal::is_about_the_bytes`] is an exhaustive `match`, so
    /// adding a variant is a compile error there; this is the other half, which is that the answer
    /// given is the right one and is written down where a person reviews it.
    ///
    /// WHAT MUTATION MAKES THIS RED: move `Refusal::Io` (or `PreflightFailed`, or
    /// `StagedFileChangedOnDisk`, or `ManifestExpired`) into the `true` arm of
    /// `is_about_the_bytes`; or give `ArtifactHashMismatch` a retry budget of 0, which is the
    /// two-writers case becoming permanent on its first collision; or give
    /// `ArtifactBadSignature` a budget above 0, which is a tampered artifact being fetched again
    /// on a loop.
    #[test]
    fn every_refusal_says_whether_it_is_about_the_bytes() {
        let permanent = [
            "EnvelopeUnreadable",
            "ManifestUnsigned",
            "EnvelopeVersion",
            "ManifestBadSignature",
            "ManifestUnreadable",
            "ManifestWrongChannel",
            "ManifestStale",
            "ManifestFormatTooNew",
            "ManifestFormatUnknown",
            "VersionUnreadable",
            "Downgrade",
            "RollbackMismatch",
            "Yanked",
            "TooOldToJump",
            "NoArtifact",
            "ArtifactVersionMismatch",
            "DataBundleMissing",
            "DataBundleTooNew",
            "DataVersionUnreadable",
            "ArtifactUrlNotOurs",
            "ArtifactTooLarge",
            "ArtifactSizeMismatch",
            "ArtifactHashMismatch",
            "ArtifactBadSignature",
            "ArtifactCheckUnbuildable",
            "UnsafeArchiveEntry",
            "BundleIncomplete",
        ];
        let about_the_machine = [
            "ManifestExpired",
            "StagedFileChangedOnDisk",
            "NotAQuietMoment",
            "Io",
            "PreflightFailed",
            "NoPreviousVersion",
            "NoTrustAnchor",
        ];

        let all = one_of_every_refusal();
        assert_eq!(
            all.len(),
            permanent.len() + about_the_machine.len(),
            "a refusal was added or removed and this table was not updated, so the new one has \
             not been classified by anybody"
        );
        for r in &all {
            let code = r.code();
            let bytes = permanent.contains(&code);
            assert_ne!(
                bytes,
                about_the_machine.contains(&code),
                "{code} is in both tables or in neither"
            );
            assert_eq!(
                r.is_about_the_bytes(),
                bytes,
                "{code} answers the wrong side of the split: a refusal about the bytes is \
                 recorded for ever, and one about the machine has to be retriable or a full disk \
                 makes a release uninstallable on that machine"
            );
            let want = match code {
                /* The one exception, argued in `Refusal::retries`: two writers staging the same
                 * file and a bad CDN both present as a hash mismatch, and one retry is what tells
                 * them apart from bytes nobody signed. */
                "ArtifactHashMismatch" => 1,
                _ if bytes => 0,
                _ => RETRY_WHILE_IT_KEEPS_HAPPENING,
            };
            assert_eq!(r.retries(), want, "{code} has the wrong retry budget");
        }
    }

    /// One value of every variant, for the two tests that must see the whole set.
    fn one_of_every_refusal() -> Vec<Refusal> {
        let path = PathBuf::from("x");
        let v = |s: &str| Version::parse(s).expect("a literal version in a test");
        let when = |s: &str| {
            DateTime::parse_from_rfc3339(s)
                .expect("a literal stamp in a test")
                .with_timezone(&Utc)
        };
        let all = vec![
            Refusal::EnvelopeUnreadable("x".into()),
            Refusal::ManifestUnsigned { missing: "x" },
            Refusal::EnvelopeVersion { saw: 2, knows: 1 },
            Refusal::ManifestBadSignature("x".into()),
            Refusal::ManifestUnreadable("x".into()),
            Refusal::ManifestWrongChannel {
                asked: "stable".into(),
                saw: "beta".into(),
            },
            Refusal::ManifestStale {
                saw: when("2026-01-01T00:00:00Z"),
                last: when("2026-02-01T00:00:00Z"),
            },
            Refusal::ManifestExpired {
                expires: when("2026-02-01T00:00:00Z"),
                now: when("2026-03-01T00:00:00Z"),
            },
            Refusal::ManifestFormatTooNew { saw: 2, knows: 1 },
            Refusal::ManifestFormatUnknown { saw: 0, knows: 1 },
            Refusal::VersionUnreadable {
                field: "version",
                saw: "x".into(),
            },
            Refusal::Downgrade {
                running: v("0.2.0"),
                offered: v("0.1.0"),
            },
            Refusal::RollbackMismatch {
                running: v("0.2.0"),
                to: v("0.1.0"),
                from: v("0.3.0"),
            },
            Refusal::Yanked {
                version: v("0.2.0"),
            },
            Refusal::TooOldToJump {
                running: v("0.1.0"),
                floor: v("0.2.0"),
            },
            Refusal::NoArtifact {
                kind: "app".into(),
                os: "windows".into(),
                arch: "x86_64".into(),
            },
            Refusal::ArtifactVersionMismatch {
                manifest: v("0.2.0"),
                artifact: "0.1.9".into(),
            },
            Refusal::DataBundleMissing {
                requires: "2026-09-14".into(),
            },
            Refusal::DataBundleTooNew {
                min_app: v("0.3.0"),
                offered: v("0.2.0"),
            },
            Refusal::DataVersionUnreadable { saw: "x".into() },
            Refusal::ArtifactUrlNotOurs {
                url: "https://evil.example/app.exe".into(),
                expected: "https://updates.ragnarok.systems/".into(),
            },
            Refusal::ArtifactTooLarge {
                said: 3,
                cap: 2,
            },
            Refusal::ArtifactSizeMismatch { said: 2, got: 1 },
            Refusal::ArtifactHashMismatch {
                said: "a".into(),
                got: "b".into(),
            },
            Refusal::ArtifactBadSignature("x".into()),
            Refusal::ArtifactCheckUnbuildable {
                field: "sha256",
                why: "x".into(),
            },
            Refusal::StagedFileChangedOnDisk {
                path: path.clone(),
                why: Box::new(Refusal::NoPreviousVersion),
            },
            Refusal::UnsafeArchiveEntry {
                entry: "x".into(),
                why: "x",
            },
            Refusal::BundleIncomplete {
                missing: "gear-data.json".into(),
            },
            Refusal::NotAQuietMoment { pulse: "x" },
            Refusal::Io {
                doing: "rename into place",
                path: path.clone(),
                why: "access is denied".into(),
            },
            Refusal::PreflightFailed {
                exe: path,
                why: "it exited Some(101)".into(),
            },
            Refusal::NoPreviousVersion,
            Refusal::NoTrustAnchor { why: "x".into() },
        ];
        all
    }

    /// DEFECT THIS PREVENTS: A DOWNLOAD STARTING IN THE SIX SECONDS AFTER A KILL.
    ///
    /// The obvious gate is `!ingest.fight_is_live()`, and it is wrong: `fight_is_live` is
    /// `pulse().fighting()`, which is already false during `Pulse::Holding`. Holding is the
    /// reader standing in the camp with the next mob on its way, which is not a moment to start
    /// pulling ten megabytes.
    ///
    /// This is the truth table only. The test that drives a REAL `Ingest` through the REAL fold
    /// and asks the real `pulse()` is `a_download_waits_for_the_encounter_to_close` in
    /// `install.rs`, because a table that invents a `Pulse` value proves nothing about the code
    /// that produces one.
    ///
    /// WHAT MUTATION MAKES THIS RED: `matches!(pulse, Pulse::Closed | Pulse::Holding)`, which is
    /// what writing the gate as `!fighting()` amounts to.
    #[test]
    fn only_a_closed_encounter_is_a_moment_to_download() {
        assert!(may_start_download(Pulse::Closed));
        assert!(!may_start_download(Pulse::Holding));
        assert!(!may_start_download(Pulse::Fighting));
    }
}
