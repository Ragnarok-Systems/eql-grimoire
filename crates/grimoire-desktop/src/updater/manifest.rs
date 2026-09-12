//! The manifest document, and every rule that decides whether it offers this machine anything.
//!
//! Nothing here fetches, writes or spawns. [`judge`] takes a [`crate::updater::verify::Verified`]
//! and a description of this machine and answers.

use chrono::{DateTime, Utc};
use semver::Version;

use super::verify::Verified;
use super::Refusal;

/// The manifest format this build reads.
///
/// MIRRORS `grimoire_corpus::FORMAT_VERSION` AND ITS RULE: a reader refuses what it does not
/// know. A HIGHER format is a refusal in words telling the reader to fetch a new build by hand,
/// because a document written for a newer reader may mean something different by a field this one
/// thinks it understands, and that is worse than not reading it.
///
/// THE PUBLISHER'S HALF OF THE RULE, which belongs in the release workflow as a comment: adding
/// an OPTIONAL field never bumps this. Changing what an existing field means, or adding a field a
/// client must honour for safety, always does.
///
/// IT WENT FROM 1 TO 2 WHEN [`Manifest::expires`] ARRIVED, which is the second half of that rule
/// being obeyed rather than a version number moving for tidiness. A client that did not know about
/// `expires` would read a format-2 document, find a field it does not name, keep it in
/// [`Manifest::extra`] and install from an envelope whose publisher had already declared it dead.
/// That is exactly the case the bump exists for: an old reader must refuse the document, not read
/// it and ignore the field that makes it safe.
pub const FORMAT: u64 = 2;

/// The artifact kinds this build knows. An artifact of any other kind is SKIPPED, not refused:
/// that is how a future macOS or arm64 artifact lands in the same manifest without breaking a
/// Windows client shipped today.
pub const KIND_APP: &str = "app";
/// The versioned snapshot bundle. See decision D6 (`data/mod.rs:1-6`): the snapshot is loaded at
/// run time and deliberately not `include_bytes!`, so it updates on its own clock.
pub const KIND_DATA: &str = "data";

/// The `os` and `arch` value an artifact uses to say "every one of them".
pub const ANY: &str = "any";

/// WHAT THIS BUILD IS, in the manifest's spelling.
///
/// Read from `cfg!` rather than from `std::env::consts`, because `consts::OS` is `"windows"` and
/// `consts::ARCH` is `"x86_64"` today but they are documented as "not guaranteed to be stable"
/// in their exact spelling, and a manifest field is a wire format. A test pins these two to the
/// strings the pipeline writes.
pub fn this_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// See [`this_os`].
pub fn this_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

/// The signed document.
///
/// # UNKNOWN FIELDS ARE KEPT AND IGNORED; AN UNKNOWN `format` IS REFUSED
///
/// `extra` collects every key this struct does not name, exactly as `Settings` does. A client
/// that DROPPED what it did not understand could not be debugged from the machine it is running
/// on, and a client that ERRORED on it would make every additive change a breaking change.
///
/// # WHY `#[serde(default)]` IS ON ONE FIELD HERE AND ON THE CONTAINER IN `TrackerSettings`
///
/// The trap written out at `ingest.rs:647-658` is real and it is narrower than "never put default
/// on a field": field-level default takes `FieldType::default()` while container-level takes
/// `Struct::default()`, and the two differ exactly where the struct's hand-written `Default` is
/// not the zero value. `TrackerSettings` has such a `Default`, so the attribute must go on its
/// container. `Manifest` has NO `Default` and must never get one, because a defaulted manifest is
/// a manifest nobody published: `format` in particular is required and not defaulted, so that a
/// document missing it is refused rather than read as format 0. The fields that DO carry a default
/// here (`yanked`, `expires`, and the optional ones) each take a field default that is exactly
/// what a container default would give them, `Vec::new()` and `None`, which is precisely the
/// condition under which field-level is safe. `expires` is the one whose absence is not harmless,
/// and [`judge`] refuses it by name after the `format` gate rather than letting serde read it as
/// nothing.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// See [`FORMAT`]. Required, and deliberately not defaulted.
    pub format: u64,
    /// Checked against the channel that was asked for. The field is INSIDE the signed bytes,
    /// which is its whole job: a CDN misconfiguration or a mistaken upload that serves the beta
    /// manifest at the stable path cannot do it silently.
    pub channel: String,
    /// The app version this manifest publishes.
    pub version: String,
    /// RFC3339 UTC. The replay defence: the client stores the `published` of the last manifest it
    /// accepted and refuses any manifest older than that, so a correctly signed but stale
    /// document served from a cache cannot walk a client backwards.
    pub published: DateTime<Utc>,
    /// RFC3339 UTC. THE HALF OF THE REPLAY DEFENCE THAT NEEDS NOTHING STORED ON THIS MACHINE.
    ///
    /// `published` is refused against `last_accepted_published`, and that floor is `None` on a
    /// fresh install, after a reinstall, when `update\state.json` is lost or torn, and on the
    /// first check of a newly selected channel, because the two channels are two independent
    /// timelines. Those are precisely the machines a replay reaches. Without an expiry a correctly
    /// signed envelope is an indefinitely re-servable document, and re-serving one needs only
    /// write access to the bucket or a position that can answer its URL, which is a strictly
    /// weaker capability than the signing key.
    ///
    /// # WHY IT IS AN `Option` HERE AND REQUIRED IN [`judge`]
    ///
    /// Required at the serde level would mean a format-1 document, which has no such field,
    /// failing to PARSE, and a parse failure is reported as [`Refusal::ManifestUnreadable`]: a
    /// sentence about a Rust type, for a document whose real problem is that it was written for an
    /// older reader. The ordering rule says `format` is asked before any field is believed, and
    /// that is only possible if the fields of a format this build does not read can still be
    /// absent. So the check is in `judge`, AFTER the format gate, where a format-2 document with
    /// no expiry is refused by name and a format-1 document is told it is a format-1 document.
    #[serde(default)]
    pub expires: Option<DateTime<Utc>>,
    /// Release notes, for the reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    /// The oldest version that may jump straight to this one. Today the client refuses the jump
    /// in words rather than attempting it; the field exists so that a settings or data migration
    /// which is not backward compatible has a lever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_from: Option<String>,
    /// Versions that are never installed. How a release that bricks is withdrawn from machines
    /// that already took it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub yanked: Vec<String>,
    /// The ONLY way a client installs a version lower than the one it is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<Rollback>,
    /// May be empty. An entry whose `kind`, `os` or `arch` this build does not recognise is
    /// skipped rather than refused.
    pub artifacts: Vec<Artifact>,
    /// Every key this build does not name, kept so the Settings screen can show what was actually
    /// served rather than what this struct could hold.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The publisher saying "go back", which is the only authority for a downgrade.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rollback {
    /// The version to install.
    pub to: String,
    /// The version this instruction is about. Must equal the running version, so that a rollback
    /// published for 0.2.0 does not move a machine sitting on 0.1.9.
    pub from: String,
    /// For the reader. Kept because a rollback with no stated reason is a rollback nobody can
    /// support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One published file.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Artifact {
    /// [`KIND_APP`] or [`KIND_DATA`]; anything else is skipped.
    pub kind: String,
    /// [`this_os`] or [`ANY`].
    pub os: String,
    /// [`this_arch`] or [`ANY`].
    pub arch: String,
    /// For an app artifact, the same semver the manifest publishes. For a data bundle, the
    /// `YYYY-MM-DD` date the bundle was built, which is a date and not a semver on purpose: a
    /// snapshot has no API and nothing about it is major, minor or patch.
    pub version: String,
    pub url: String,
    /// Enforced EXACTLY by the download. Not a ceiling: the honest length is published and
    /// signed, so this replaces the fixed byte cap `channel_art.rs:418` uses.
    pub size: u64,
    /// 64 lowercase hex, checked while the bytes are being written to disk.
    pub sha256: String,
    /// The detached minisign signature of the file itself, carried inline so the download costs
    /// one GET, and ALSO published as a `.minisig` beside the artifact for hand verification.
    ///
    /// NOT REDUNDANT WITH THE MANIFEST SIGNATURE. A staged artifact sits on disk between the
    /// download and the next launch, and at install time the only things present are the file and
    /// this signature: no manifest, no network.
    pub signature: String,
    /// On an app artifact: the lowest data bundle version that app can read, so the client
    /// installs the bundle FIRST when both are offered. A new binary landing on an old snapshot
    /// turns a working install into `Data::Failed` on a file name the reader cannot act on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_data: Option<String>,
    /// On a data artifact: the lowest app that can read the bundle, so a client refuses a bundle
    /// it is too old for instead of breaking a snapshot that works.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app: Option<String>,
    /// See [`Manifest::extra`].
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Artifact {
    /// Does this entry describe a file for this build?
    ///
    /// `ANY` on either axis matches, which is what a platform-independent data bundle uses. An
    /// `os` or `arch` this build has never heard of simply does not match, which is the skip the
    /// forward-compatibility rule asks for.
    fn is_for(&self, kind: &str, os: &str, arch: &str) -> bool {
        self.kind == kind
            && (self.os == os || self.os == ANY)
            && (self.arch == arch || self.arch == ANY)
    }
}

/// What this machine is, for [`judge`].
#[derive(Clone, Copy, Debug)]
pub struct Local<'a> {
    /// `env!("CARGO_PKG_VERSION")`, parsed. The one place the running version exists is
    /// `titlebar::version()` (`titlebar.rs:274`), which is `env!` and not `option_env!`, so it
    /// cannot disagree with `Cargo.toml`.
    pub running: &'a Version,
    /// The channel this client asked for, which the manifest must agree it is.
    pub channel: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    /// The `published` of the newest manifest this client has ever accepted. `None` on a client
    /// that has never accepted one, which is the only state in which any stamp is acceptable.
    pub last_accepted_published: Option<DateTime<Utc>>,
    /// NOW, HANDED IN RATHER THAN READ, exactly as `last_accepted_published` is. This module
    /// reaches nothing, which is the property that makes every rule in it testable; a
    /// `Utc::now()` inside [`judge`] would make the expiry rule the one rule here that a test
    /// could only drive by waiting.
    pub now: DateTime<Utc>,
    /// The data bundle version currently installed, if the updater installed one. `None` means
    /// the snapshot came from somewhere else (the folder beside the exe, the reader's own
    /// `data_root`), in which case any offered bundle is newer by the rule in section 5.
    pub installed_data: Option<&'a str>,
}

/// Which way an offer moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Forwards, or sideways for a data-only refresh.
    Upgrade,
    /// Backwards, authorised by a `rollback` object in the signed manifest.
    PublisherRollback,
}

/// Something to install, and what to install first.
#[derive(Clone, Debug)]
pub struct Offer {
    /// The app version this manifest publishes. Equal to the running version for a data-only
    /// refresh, which is a real release: a wiki rescrape moves 7.1 MB and needs no new binary.
    pub version: Version,
    pub direction: Direction,
    pub notes_url: Option<String>,
    /// IN INSTALL ORDER. A required data bundle comes first, and the app's pointer flip does not
    /// happen until it has landed.
    pub steps: Vec<Artifact>,
}

/// The answer, with the two things that are true whatever the answer is.
#[derive(Clone, Debug)]
pub struct Decision {
    /// The manifest lists the RUNNING version as withdrawn. True whether or not there is
    /// something to install, because the remedy differs: a newer release is the better fix, and
    /// only when there is none does the trampoline fall back to `previous`.
    pub running_withdrawn: bool,
    /// Every key the served document carried that this build does not name, manifest and
    /// artifacts together, so the Settings screen can be honest about what arrived.
    pub unknown_fields: Vec<String>,
    pub outcome: Outcome,
}

/// The two shapes of answer.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Nothing to do.
    UpToDate,
    /// Something to install.
    Offer(Offer),
}

/// Parse a version field, naming which one when it is not a version.
fn version(field: &'static str, saw: &str) -> Result<Version, Refusal> {
    Version::parse(saw).map_err(|_| Refusal::VersionUnreadable {
        field,
        saw: saw.to_owned(),
    })
}

/// A data bundle version is a `YYYY-MM-DD` date.
///
/// # WHY A DATE AND WHY THE SHAPE IS CHECKED
///
/// The only question ever asked of a bundle version is "is this one newer than that one", and an
/// ISO-8601 date answers it with a string comparison, which is the entire reason the format was
/// chosen. That is only true while both sides ARE that shape: `"2026-9-14"` sorts after
/// `"2026-10-01"`, so a single sloppy value would silently invert the ordering and install an old
/// snapshot over a new one. Refusing the shape is what makes the cheap comparison honest.
fn data_version(saw: &str) -> Result<&str, Refusal> {
    let bad = || Refusal::DataVersionUnreadable {
        saw: saw.to_owned(),
    };
    let b = saw.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(bad());
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return Err(bad());
    }
    Ok(saw)
}

/// MAY THIS BUILD FETCH THIS ARTIFACT AT ALL, BEFORE ANYTHING IS ASKED FOR?
///
/// # BOTH OF THESE ARE CLIENT-SIDE COPIES OF RULES THAT EXISTED ONLY ELSEWHERE
///
/// The url rule lived in `release.yml`, whose publish guard refuses a manifest naming anything
/// outside the project's own prefix and says in its own comment that a manifest may not send
/// clients anywhere else. The client never asked. A defence that exists in the pipeline and not in
/// the client protects against the pipeline's own mistakes and against nothing else: anyone able
/// to produce one signed manifest could point every client at a host of their choosing and collect
/// each one's address and user agent on a six hourly schedule. The signature gate still holds the
/// BYTES to what was promised, so this is not what stops unsigned code running; it is what stops
/// the download being somebody else's business.
///
/// The size rule is what [`super::fetch::MAX_ARTIFACT_BYTES`] already claimed of itself and did
/// not have: the constant was only ever `ureq`'s body limit, enforced while bytes stream, so a
/// signed manifest with a wrong `size` wrote 128 MiB to the reader's disk on every check before
/// failing.
///
/// # WHY HERE AND NOT AT THE CALL SITE
///
/// Same reason the fight gate is inside [`super::install::stage`]: a rule written where the caller
/// happens to be today is a rule the second caller will not have. This is the half of the feature
/// that has tests, and every artifact a client can ever act on comes back through it.
fn fetchable(a: &Artifact) -> Result<(), Refusal> {
    let expected = format!("{}/", super::fetch::HOST);
    /* THE PREFIX COMPARISON IS AGAINST "https://host/" WITH THE SLASH, which is not decoration:
     * without it `https://updates.ragnarok.systems.evil.example/x` starts with the host string and
     * would pass. */
    if !a.url.starts_with(&expected) {
        return Err(Refusal::ArtifactUrlNotOurs {
            url: a.url.clone(),
            expected,
        });
    }
    if a.size > super::fetch::MAX_ARTIFACT_BYTES {
        return Err(Refusal::ArtifactTooLarge {
            said: a.size,
            cap: super::fetch::MAX_ARTIFACT_BYTES,
        });
    }
    Ok(())
}

/// Every key the served document carried that this build does not name.
fn unknown_fields(m: &Manifest) -> Vec<String> {
    let mut out: Vec<String> = m.extra.keys().cloned().collect();
    for (i, a) in m.artifacts.iter().enumerate() {
        out.extend(a.extra.keys().map(|k| format!("artifacts[{i}].{k}")));
    }
    out.sort();
    out
}

/// DOES THIS SIGNED MANIFEST OFFER THIS MACHINE ANYTHING?
///
/// # THE ARGUMENT IS A [`Verified`] AND THAT IS THE ORDERING RULE
///
/// Nothing downstream of a failed signature check may run, including display. That is not a
/// comment here: [`Verified`] has no public constructor other than
/// [`crate::updater::verify::open`], so a caller cannot reach these rules with a document nobody
/// signed.
///
/// # THE ORDER OF THE CHECKS
///
/// channel, then `published`, then `format`, then the version rules, then artifact selection.
/// Each one is cheap and each one makes the next one's inputs mean what they say. `channel` is
/// first among these because a manifest for the wrong channel is a SERVER mistake and a reader
/// told "you are up to date" would never find out.
pub fn judge(v: &Verified, local: &Local<'_>) -> Result<Decision, Refusal> {
    let m = v.doc();

    if m.channel != local.channel {
        return Err(Refusal::ManifestWrongChannel {
            asked: local.channel.to_owned(),
            saw: m.channel.clone(),
        });
    }

    if let Some(last) = local.last_accepted_published {
        if m.published < last {
            return Err(Refusal::ManifestStale {
                saw: m.published,
                last,
            });
        }
    }

    if m.format != FORMAT {
        return Err(if m.format > FORMAT {
            Refusal::ManifestFormatTooNew {
                saw: m.format,
                knows: FORMAT,
            }
        } else {
            Refusal::ManifestFormatUnknown {
                saw: m.format,
                knows: FORMAT,
            }
        });
    }

    /* THE FLOOR THAT DOES NOT DEPEND ON THIS MACHINE HAVING SEEN ANYTHING BEFORE.
     *
     * The `published` check above is empty on a fresh install, after a reinstall, when
     * `state.json` is lost or torn, and on the first check of a newly selected channel. Those are
     * precisely the machines a replayed envelope reaches, so that check alone leaves the defence
     * open exactly where it is needed. The publisher's own `expires` closes it with nothing stored
     * on the client at all.
     *
     * IT IS ASKED AFTER `format`, because it is a format-2 field and a format-1 document has to be
     * told it is a format-1 document rather than told it forgot a field it never had. A format-2
     * document with no expiry is a broken publish and says so by name.
     *
     * IT IS ITS OWN REFUSAL AND NOT A FOLD INTO `ManifestStale`: stale means "this channel served
     * you something older than you already have" and expired means "this channel is serving an old
     * document to everybody", and only the second is a reason to look at the channel rather than
     * at this machine. It is also the one manifest refusal that is not recorded as permanent
     * (`Refusal::is_about_the_bytes`), because a clock that is a week out must not file a
     * perfectly good release away for ever. */
    let Some(expires) = m.expires else {
        return Err(Refusal::ManifestUnreadable(format!(
            "a format {FORMAT} manifest must carry an expires stamp, and this one has none; \
             without it a copy of this document is valid for ever on any machine that has not \
             accepted one before"
        )));
    };
    if expires < local.now {
        return Err(Refusal::ManifestExpired {
            expires,
            now: local.now,
        });
    }

    let offered = version("version", &m.version)?;
    let mut yanked = Vec::new();
    for y in &m.yanked {
        yanked.push(version("yanked", y)?);
    }
    let running_withdrawn = yanked.contains(local.running);
    let unknown = unknown_fields(m);
    let done = |outcome| {
        Ok(Decision {
            running_withdrawn,
            unknown_fields: unknown.clone(),
            outcome,
        })
    };

    /* A WITHDRAWN RELEASE IS NEVER INSTALLED, WHICHEVER DIRECTION IT LIES IN. Asked before the
     * comparison, because a manifest that publishes a version and withdraws it in the same
     * breath is a publisher mid-retraction and neither answer below is the right one. */
    if yanked.contains(&offered) {
        return Err(Refusal::Yanked { version: offered });
    }

    match offered.cmp(local.running) {
        std::cmp::Ordering::Less => {
            let Some(r) = &m.rollback else {
                return Err(Refusal::Downgrade {
                    running: local.running.clone(),
                    offered,
                });
            };
            let to = version("rollback.to", &r.to)?;
            let from = version("rollback.from", &r.from)?;
            if to != offered || &from != local.running {
                return Err(Refusal::RollbackMismatch {
                    running: local.running.clone(),
                    to,
                    from,
                });
            }
            done(Outcome::Offer(Offer {
                version: offered.clone(),
                direction: Direction::PublisherRollback,
                notes_url: m.notes_url.clone(),
                steps: steps_for(m, &offered, local)?,
            }))
        }
        std::cmp::Ordering::Equal => {
            /* THE APP HAS NOT MOVED AND THE SNAPSHOT STILL MIGHT HAVE. A wiki rescrape is a real
             * release with no new binary: 7.1 MB of bundle against a 10.9 MB exe that did not
             * change. Answering "up to date" here would make a data-only release undeliverable. */
            match data_step(m, &offered, local, None)? {
                Some(a) => {
                    fetchable(&a)?;
                    done(Outcome::Offer(Offer {
                        version: offered,
                        direction: Direction::Upgrade,
                        notes_url: m.notes_url.clone(),
                        steps: vec![a],
                    }))
                }
                None => done(Outcome::UpToDate),
            }
        }
        std::cmp::Ordering::Greater => {
            if let Some(floor) = &m.minimum_from {
                let floor = version("minimum_from", floor)?;
                if local.running < &floor {
                    return Err(Refusal::TooOldToJump {
                        running: local.running.clone(),
                        floor,
                    });
                }
            }
            done(Outcome::Offer(Offer {
                version: offered.clone(),
                direction: Direction::Upgrade,
                notes_url: m.notes_url.clone(),
                steps: steps_for(m, &offered, local)?,
            }))
        }
    }
}

/// The app artifact, with a data bundle in front of it when the app says it needs one.
fn steps_for(m: &Manifest, offered: &Version, local: &Local<'_>) -> Result<Vec<Artifact>, Refusal> {
    let app = m
        .artifacts
        .iter()
        .find(|a| a.is_for(KIND_APP, local.os, local.arch))
        .ok_or_else(|| Refusal::NoArtifact {
            kind: KIND_APP.to_owned(),
            os: local.os.to_owned(),
            arch: local.arch.to_owned(),
        })?;

    /* THE ARTIFACT MUST AGREE WITH THE DOCUMENT THAT CARRIES IT. Both are inside the signed
     * bytes, so a disagreement is not an attack, it is a broken pipeline: the installer names the
     * directory after the manifest's version and would file bytes labelled 0.1.9 under 0.2.0.
     * Cheap to check and impossible to notice later. */
    let labelled = version("artifact version", &app.version)?;
    if &labelled != offered {
        return Err(Refusal::ArtifactVersionMismatch {
            manifest: offered.clone(),
            artifact: app.version.clone(),
        });
    }

    let mut steps = Vec::new();
    if let Some(data) = data_step(m, offered, local, app.requires_data.as_deref())? {
        steps.push(data);
    }
    steps.push(app.clone());
    for a in &steps {
        fetchable(a)?;
    }
    Ok(steps)
}

/// The data bundle to install, if any.
///
/// Two callers with two questions, which is why `requires` is an option rather than two
/// functions. With a `requires` the question is "does the app that is about to be installed need
/// a newer snapshot than the one on disk"; without one it is "is the offered snapshot newer than
/// the one on disk", which is the whole of a data-only release.
fn data_step(
    m: &Manifest,
    offered: &Version,
    local: &Local<'_>,
    requires: Option<&str>,
) -> Result<Option<Artifact>, Refusal> {
    let installed = match local.installed_data {
        Some(v) => Some(data_version(v)?),
        None => None,
    };
    if let Some(need) = requires {
        let need = data_version(need)?;
        if installed.is_some_and(|have| have >= need) {
            return Ok(None);
        }
    }

    let Some(bundle) = m
        .artifacts
        .iter()
        .find(|a| a.is_for(KIND_DATA, local.os, local.arch))
    else {
        return match requires {
            Some(need) => Err(Refusal::DataBundleMissing {
                requires: need.to_owned(),
            }),
            None => Ok(None),
        };
    };
    let have_on_offer = data_version(&bundle.version)?;

    if let Some(need) = requires {
        if have_on_offer < need {
            return Err(Refusal::DataBundleMissing {
                requires: need.to_owned(),
            });
        }
    } else if installed.is_some_and(|have| have >= have_on_offer) {
        return Ok(None);
    }

    if let Some(min_app) = &bundle.min_app {
        let min_app = version("min_app", min_app)?;
        if offered < &min_app {
            return Err(Refusal::DataBundleTooNew {
                min_app,
                offered: offered.clone(),
            });
        }
    }
    Ok(Some(bundle.clone()))
}

#[cfg(test)]
pub(crate) mod probe {
    //! Manifests built as JSON, so a test can remove a required field, add one this build has
    //! never heard of, or write a version nobody could parse. A builder over the Rust struct
    //! could do none of those, which is the half of the parser worth testing.

    use serde_json::{json, Value};

    /// A manifest that offers 0.2.0 on stable with one Windows app artifact and nothing else
    /// unusual. Every other fixture in these tests is this one with a field moved.
    ///
    /// `expires` IS FAR IN THE FUTURE AND `NOW` IS FIXED, so that no test in this file starts
    /// failing on a date. [`NOW`] is the clock every fixture is judged against and it sits between
    /// `published` and `expires` on purpose: a fixture that relied on the real clock would go red
    /// the day the window closed, which is a test measuring the calendar.
    pub(crate) fn sane() -> Value {
        json!({
            "format": super::FORMAT,
            "channel": "stable",
            "version": "0.2.0",
            "published": "2026-09-14T18:02:11Z",
            "expires": "2026-10-29T18:02:11Z",
            "notes_url": "https://example.invalid/notes",
            "artifacts": [ app("0.2.0") ],
        })
    }

    /// The clock the fixtures above are judged against: after `published`, before `expires`.
    pub(crate) fn now() -> super::DateTime<super::Utc> {
        "2026-09-15T00:00:00Z"
            .parse::<super::DateTime<super::Utc>>()
            .expect("a literal stamp")
    }

    /// One app artifact for this build. The seal fields are placeholders: every test that cares
    /// about them replaces them with real ones computed over real bytes.
    ///
    /// THE URL IS A REAL ONE ON THE REAL HOST AND NOT `example.invalid`, because [`judge`] now
    /// refuses an artifact that is not served from [`super::super::fetch::HOST`] over https.
    /// Nothing here fetches it; what the fixture has to be is a url the client would accept, or
    /// every test in this file would be measuring the url rule instead of the rule it names.
    pub(crate) fn app(version: &str) -> Value {
        json!({
            "kind": "app",
            "os": super::this_os(),
            "arch": super::this_arch(),
            "version": version,
            "url": format!("{}/grimoire/stable/{version}/grimoire-desktop.exe", crate::updater::fetch::HOST),
            "size": 3,
            "sha256": "0".repeat(64),
            "signature": "untrusted comment: placeholder\n",
        })
    }

    /// One data bundle.
    pub(crate) fn data(version: &str) -> Value {
        json!({
            "kind": "data",
            "os": "any",
            "arch": "any",
            "version": version,
            "url": format!("{}/grimoire/stable/{version}/bundle.tar.gz", crate::updater::fetch::HOST),
            "size": 3,
            "sha256": "0".repeat(64),
            "signature": "untrusted comment: placeholder\n",
        })
    }

    pub(crate) fn manifest_text(v: &Value) -> String {
        serde_json::to_string(v).expect("a JSON value serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::probe;
    use super::*;
    use crate::updater::verify::{open_with, probe::Signer};
    use serde_json::json;

    struct Bench {
        signer: Signer,
        running: Version,
    }

    impl Bench {
        fn new(running: &str) -> Bench {
            Bench {
                signer: Signer::new(),
                running: Version::parse(running).expect("a literal version"),
            }
        }
        fn keys(&self) -> [&str; 1] {
            [self.signer.public_b64.as_str()]
        }
        fn local(&self) -> Local<'_> {
            Local {
                running: &self.running,
                channel: "stable",
                os: this_os(),
                arch: this_arch(),
                last_accepted_published: None,
                now: probe::now(),
                installed_data: None,
            }
        }
        /// Sign a manifest and put it through the production path: verify, THEN judge.
        ///
        /// THE VERIFY STEP'S REFUSAL IS PROPAGATED AND NOT UNWRAPPED, because `open_with` is
        /// where the manifest is PARSED as well as checked: a document missing a required field
        /// gets as far as the signature and no further, and a helper that unwrapped here would
        /// turn the whole malformed-manifest half of these tests into a panic.
        fn ask(&self, doc: &serde_json::Value, local: &Local<'_>) -> Result<Decision, Refusal> {
            let body = self.signer.envelope(&probe::manifest_text(doc));
            let v = open_with(&body, &self.keys())?;
            judge(&v, local)
        }
    }

    fn offered(d: &Decision) -> &Offer {
        match &d.outcome {
            Outcome::Offer(o) => o,
            Outcome::UpToDate => panic!("expected an offer, got up to date"),
        }
    }

    /// DEFECT THIS PREVENTS: THE WIRE SPELLING OF THIS BUILD DRIFTING AWAY FROM THE PIPELINE'S.
    ///
    /// `os` and `arch` are matched as strings against what a GitHub workflow wrote months
    /// earlier. If this build started calling itself `win32`, every artifact would stop matching
    /// and the app would report "the manifest publishes nothing for this build" forever, which
    /// looks exactly like a publisher who has not shipped yet.
    ///
    /// WHAT MUTATION MAKES THIS RED: return `std::env::consts::OS` and `ARCH`, or change either
    /// literal.
    #[test]
    fn this_build_names_itself_the_way_the_pipeline_does() {
        assert!(
            ["windows", "macos", "linux"].contains(&this_os()),
            "an os spelling the pipeline does not write: {}",
            this_os()
        );
        assert!(
            ["x86_64", "aarch64"].contains(&this_arch()),
            "an arch spelling the pipeline does not write: {}",
            this_arch()
        );
        /* The gate this feature is actually built for. */
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            assert_eq!(this_os(), "windows");
            assert_eq!(this_arch(), "x86_64");
        }
    }

    /// DEFECT THIS PREVENTS: A MALFORMED MANIFEST BEING READ AS A SENSIBLE ONE.
    ///
    /// A missing `format` read as 0, a missing `artifacts` read as empty, a `published` read as
    /// the epoch: each of those turns a broken publish into a confident wrong answer. `format` is
    /// the one that matters most, because it is the field that decides whether any of the others
    /// mean what this build thinks.
    ///
    /// WHAT MUTATION MAKES THIS RED: put `#[serde(default)]` on the `Manifest` container, which
    /// is the attribute placement `TrackerSettings` needs and this struct must not have.
    #[test]
    fn a_manifest_missing_a_required_field_is_refused_with_the_field_named() {
        let b = Bench::new("0.1.0");
        for missing in ["format", "channel", "version", "published", "artifacts"] {
            let mut doc = probe::sane();
            doc.as_object_mut().expect("an object").remove(missing);
            let no = b
                .ask(&doc, &b.local())
                .expect_err("a manifest missing a required field");
            assert_eq!(
                no.code(),
                "ManifestUnreadable",
                "{missing} was quietly defaulted"
            );
            assert!(
                no.to_string().contains(missing),
                "the refusal must name {missing}, got {no}"
            );
        }
        /* And the untouched fixture is accepted, so the refusals above are about the field and
         * not about the fixture. */
        b.ask(&probe::sane(), &b.local())
            .expect("the intact fixture is read");
    }

    /// DEFECT THIS PREVENTS: A FIELD THIS BUILD DOES NOT KNOW BEING DROPPED, OR BEING FATAL.
    ///
    /// Dropping it means a machine cannot be debugged from the manifest it actually received.
    /// Erroring on it means every additive change to the format is a breaking change, which
    /// defeats the point of having a `format` number at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `#[serde(flatten)] extra` field (unknown keys are
    /// then dropped and the re-serialize loses them), or add
    /// `#[serde(deny_unknown_fields)]`.
    #[test]
    fn an_unknown_field_survives_a_parse_and_a_re_serialize() {
        let b = Bench::new("0.1.0");
        let mut doc = probe::sane();
        doc["a_field_from_the_future"] = json!({"nested": [1, 2, 3]});
        doc["artifacts"][0]["signed_by_a_second_key"] = json!("yes");

        let body = b.signer.envelope(&probe::manifest_text(&doc));
        let v = open_with(&body, &b.keys()).expect("it verifies");
        let back = serde_json::to_value(v.doc()).expect("the parsed manifest re-serializes");
        assert_eq!(
            back["a_field_from_the_future"], doc["a_field_from_the_future"],
            "an unknown manifest field was dropped"
        );
        assert_eq!(
            back["artifacts"][0]["signed_by_a_second_key"],
            json!("yes"),
            "an unknown artifact field was dropped"
        );

        let d = judge(&v, &b.local()).expect("an unknown field is not fatal");
        assert_eq!(
            d.unknown_fields,
            vec![
                "a_field_from_the_future".to_owned(),
                "artifacts[0].signed_by_a_second_key".to_owned()
            ],
            "the screen is told what arrived that this build does not name"
        );
    }

    /// DEFECT THIS PREVENTS: A FORMAT THIS BUILD CANNOT READ BEING READ ANYWAY.
    ///
    /// A newer format may mean something different by a field this build thinks it understands, so
    /// a higher number is a refusal in words and not a best effort. A number that is neither this
    /// one nor higher means the document was not written by this pipeline at all, and the two get
    /// different causes because the remedies differ: update by hand, versus report a broken
    /// publish.
    ///
    /// WHAT MUTATION MAKES THIS RED: `if m.format > FORMAT` in place of `!=`, which lets format 0
    /// through.
    #[test]
    fn a_format_this_build_does_not_read_is_refused() {
        let b = Bench::new("0.1.0");
        let with = |f: u64| {
            let mut doc = probe::sane();
            doc["format"] = json!(f);
            doc
        };
        assert_eq!(
            b.ask(&with(FORMAT + 1), &b.local())
                .expect_err("a newer format")
                .code(),
            "ManifestFormatTooNew"
        );
        assert_eq!(
            b.ask(&with(0), &b.local())
                .expect_err("a format this pipeline never writes")
                .code(),
            "ManifestFormatUnknown"
        );
    }

    /// DEFECT THIS PREVENTS: THE BETA MANIFEST BEING SERVED AT THE STABLE PATH AND NOBODY NOTICING.
    ///
    /// The channel the client asked for is a URL segment, which a CDN rule or a mistaken upload
    /// can get wrong. The channel inside the signed bytes is the publisher's own statement, so
    /// comparing them is the only check that can catch it, and it must be a refusal rather than a
    /// shrug: a reader silently moved onto beta would not know.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the channel comparison in `judge`.
    #[test]
    fn a_manifest_for_another_channel_is_refused() {
        let b = Bench::new("0.1.0");
        let mut doc = probe::sane();
        doc["channel"] = json!("beta");
        let no = b
            .ask(&doc, &b.local())
            .expect_err("stable asked, beta served");
        assert_eq!(no.code(), "ManifestWrongChannel");
        assert!(
            no.to_string().contains("stable") && no.to_string().contains("beta"),
            "the refusal must name both channels, got {no}"
        );
    }

    /// DEFECT THIS PREVENTS: A CORRECTLY SIGNED, PERFECTLY VALID, MONTHS-OLD MANIFEST BEING
    /// REPLAYED TO WALK A CLIENT BACKWARDS.
    ///
    /// Nothing in the signature can catch this: the publisher really did sign it, and every field
    /// in it really was true once. The only thing that makes it wrong is that the client has
    /// already seen a newer one. A cache serving a stale object does it by accident and a network
    /// attacker does it on purpose, and the defence is the same.
    ///
    /// EQUAL IS ACCEPTED, deliberately: a client that refused the manifest it already holds would
    /// refuse every re-check of an unchanged channel.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `published` comparison, or write it as `<=`.
    #[test]
    fn a_manifest_older_than_the_last_one_accepted_is_refused() {
        let b = Bench::new("0.1.0");
        let last = DateTime::parse_from_rfc3339("2026-09-14T18:02:11Z")
            .expect("a literal stamp")
            .with_timezone(&Utc);
        let mut local = b.local();
        local.last_accepted_published = Some(last);

        /* The same stamp is fine: this is the manifest already accepted, served again. */
        b.ask(&probe::sane(), &local)
            .expect("re-serving the accepted manifest is not an attack");

        let mut old = probe::sane();
        old["published"] = json!("2026-08-01T00:00:00Z");
        old["version"] = json!("0.9.0");
        old["artifacts"][0]["version"] = json!("0.9.0");
        let no = b.ask(&old, &local).expect_err("a replayed manifest");
        assert_eq!(no.code(), "ManifestStale");

        /* And a NEWER one is taken, so the rule is about direction and not about refusing
         * everything once a stamp has been stored. */
        let mut newer = probe::sane();
        newer["published"] = json!("2026-10-01T00:00:00Z");
        b.ask(&newer, &local).expect("a newer manifest is accepted");
    }

    /// DEFECT THIS PREVENTS: A CAPTURED ENVELOPE BEING A VALID DOCUMENT FOR EVER ON EVERY FRESH
    /// INSTALL.
    ///
    /// # WHY THE STALE CHECK ABOVE IS NOT ENOUGH, WHICH IS THE WHOLE OF THIS
    ///
    /// `last_accepted_published` is `None` on a client that has never accepted a manifest: a fresh
    /// install, a reinstall, a machine whose `update\state.json` was lost or torn, and the first
    /// check after a channel switch, because the two channels are two independent timelines. Those
    /// are exactly the machines a replay reaches. Anyone who can answer the manifest URL, which
    /// needs the bucket credentials rather than the signing key, can serve an envelope the
    /// publisher signed months ago for a release with a since-fixed defect, and every one of those
    /// clients accepts it: the signature is genuine, the channel matches, and the version is above
    /// what they are running.
    ///
    /// Clients that DO hold a floor are not downgraded but are frozen on `ManifestStale` and will
    /// never see the real fix. Neither outcome produces a signal anybody can act on.
    ///
    /// # THE CLOCK IS AN ARGUMENT, LIKE THE FLOOR
    ///
    /// `judge` reads nothing, which is the property that makes the rest of this file testable, so
    /// `now` is handed in beside `last_accepted_published`. It is also why the fixture's stamps are
    /// literals rather than offsets from the real clock: a test that computed them from today
    /// would go red on a date rather than on a defect.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `expires < local.now` comparison; write it as
    /// `>`; or let a missing `expires` through instead of refusing it, which is the same document
    /// with the field removed.
    #[test]
    fn a_manifest_that_has_expired_is_refused_by_a_client_that_has_accepted_nothing() {
        let b = Bench::new("0.1.0");
        let fresh = b.local();
        assert_eq!(
            fresh.last_accepted_published, None,
            "this test is about the machine with no stored floor; with one, the stale check would \
             be what refused the document and this would prove nothing"
        );

        /* The honest document, judged by a clock inside its window. */
        b.ask(&probe::sane(), &fresh)
            .expect("a live manifest is accepted");

        /* THE SAME BYTES, JUDGED A DAY AFTER THEY DIED. Nothing about the document changed, which
         * is the point: the signature still checks out and every field in it was true once. */
        let mut late = fresh;
        late.now = "2026-10-30T00:00:00Z"
            .parse()
            .expect("a literal stamp in a test");
        let no = b
            .ask(&probe::sane(), &late)
            .expect_err("an expired manifest");
        assert_eq!(no.code(), "ManifestExpired");
        assert!(
            no.to_string().contains("2026-10-29"),
            "the refusal must name the stamp the publisher set, got {no}"
        );

        /* A format-2 document with no expiry at all is a broken publish and is refused by name,
         * rather than being read as one that never dies. */
        let mut none = probe::sane();
        none.as_object_mut().expect("an object").remove("expires");
        let no = b.ask(&none, &fresh).expect_err("no expiry at all");
        assert_eq!(no.code(), "ManifestUnreadable");
        assert!(
            no.to_string().contains("expires"),
            "the refusal must name the missing field, got {no}"
        );
    }

    /// DEFECT THIS PREVENTS: ONE SIGNED MANIFEST SENDING EVERY CLIENT TO A HOST OF SOMEBODY ELSE'S
    /// CHOOSING, AND A WRONG `size` WRITING 128 MiB TO THE READER'S DISK ON EVERY CHECK.
    ///
    /// # NEITHER OF THESE RUNS UNSIGNED CODE, AND BOTH MATTER ANYWAY
    ///
    /// `copy_sealed` still holds the bytes to the signed size, hash and signature, so an artifact
    /// fetched from anywhere at all is still refused unless the key signed it. What the url rule
    /// buys is everything short of that: a capability weaker than a full key compromise (one
    /// signed manifest, from a compromised workflow run or a hand-signed document) could otherwise
    /// point every client at an arbitrary server on a six hourly schedule, handing it each one's
    /// address and the user agent `fetch.rs` deliberately pins. It also keeps every install coming
    /// from the project's own bucket, so a defect anywhere downstream in the verify path is not
    /// remotely reachable from a server of the attacker's choosing.
    ///
    /// The size rule is the check `fetch::MAX_ARTIFACT_BYTES` claimed in its own doc and did not
    /// have: the constant was only ever `ureq`'s body limit, which is enforced while bytes stream.
    ///
    /// # THE HOST PREFIX IS COMPARED WITH ITS TRAILING SLASH
    ///
    /// Without it, `https://updates.ragnarok.systems.evil.example/x` starts with the host string
    /// and passes, which is the classic prefix hole and is why that case is in the table.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the prefix comparison in `fetchable`; drop the
    /// trailing slash from `expected`; accept `http` by comparing only the host part; or delete
    /// the size comparison.
    #[test]
    fn an_artifact_may_only_be_fetched_from_the_update_host_and_may_not_be_absurdly_large() {
        let b = Bench::new("0.1.0");
        let with_url = |u: &str| {
            let mut doc = probe::sane();
            doc["artifacts"][0]["url"] = json!(u);
            doc
        };

        for hostile in [
            "https://evil.example/app.exe",
            "http://updates.ragnarok.systems/grimoire/stable/0.2.0/grimoire-desktop.exe",
            "https://updates.ragnarok.systems.evil.example/grimoire/stable/app.exe",
            "https://updates.ragnarok.systems@evil.example/app.exe",
            "//updates.ragnarok.systems/app.exe",
            "file:///C:/Windows/System32/evil.exe",
        ] {
            let no = b
                .ask(&with_url(hostile), &b.local())
                .expect_err("an artifact url off the update host was accepted");
            assert_eq!(
                no.code(),
                "ArtifactUrlNotOurs",
                "{hostile:?} was refused for the wrong reason: {no}"
            );
            assert!(
                no.to_string().contains(hostile),
                "the refusal must name the address it would have fetched from, got {no}"
            );
        }

        /* AND THE HONEST URL STILL PASSES, so none of the above is a fixture that would fail
         * whatever it said. */
        let good = format!(
            "{}/grimoire/stable/0.2.0/grimoire-desktop.exe",
            crate::updater::fetch::HOST
        );
        b.ask(&with_url(&good), &b.local())
            .expect("the project's own bucket is where releases come from");

        /* THE SIZE THE MANIFEST STATES IS THE ONE THING THAT DECIDES HOW MUCH IS EVER WRITTEN, so
         * it is refused BEFORE a request rather than while bytes arrive. */
        let mut huge = probe::sane();
        huge["artifacts"][0]["size"] = json!(crate::updater::fetch::MAX_ARTIFACT_BYTES + 1);
        let no = b
            .ask(&huge, &b.local())
            .expect_err("a signed size above the ceiling");
        assert_eq!(no.code(), "ArtifactTooLarge");

        let mut at_the_cap = probe::sane();
        at_the_cap["artifacts"][0]["size"] = json!(crate::updater::fetch::MAX_ARTIFACT_BYTES);
        b.ask(&at_the_cap, &b.local())
            .expect("the ceiling itself is allowed; it is a cap and not a limit one below");
    }

    /// DEFECT THIS PREVENTS: THE APP DOWNGRADING ITSELF.
    ///
    /// The live bucket holds a placeholder whose version is `0.0.0-smoketest`. Under semver a
    /// pre-release sorts below its own release, which sorts below 0.1.0, so a correct comparator
    /// answers "older" and a hand-rolled one written on string order or on the numeric triple
    /// alone answers "newer" or "equal". That is the exact reason `semver` is a dependency rather
    /// than twenty lines of `split('.')`.
    ///
    /// WHAT MUTATION MAKES THIS RED: return an offer on `Ordering::Less` without consulting
    /// `m.rollback`; or compare `m.version` and the running version as strings.
    #[test]
    fn a_lower_version_is_never_offered_without_the_publisher_saying_so() {
        let b = Bench::new("0.1.0");
        for lower in ["0.0.0-smoketest", "0.0.9", "0.1.0-rc.1"] {
            let mut doc = probe::sane();
            doc["version"] = json!(lower);
            doc["artifacts"][0]["version"] = json!(lower);
            let no = b
                .ask(&doc, &b.local())
                .expect_err("a lower version with no rollback instruction");
            assert_eq!(
                no.code(),
                "Downgrade",
                "{lower} was offered to a 0.1.0 client"
            );
        }
        /* The same version is not an offer either. */
        let mut same = probe::sane();
        same["version"] = json!("0.1.0");
        same["artifacts"][0]["version"] = json!("0.1.0");
        let d = b.ask(&same, &b.local()).expect("the running version");
        assert!(matches!(d.outcome, Outcome::UpToDate));
    }

    /// DEFECT THIS PREVENTS: A ROLLBACK INSTRUCTION MEANT FOR SOMEBODY ELSE'S MACHINE MOVING THIS
    /// ONE.
    ///
    /// `rollback` is the only authority for going backwards, so it is the one field an attacker
    /// would most like to see loosely checked. `from` must be the running version: a rollback
    /// published to get 0.2.0 machines off a bad build must not drag a 0.1.9 machine down with
    /// it, and a `to` that disagrees with the manifest's own `version` is a broken publish.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop either half of the `to`/`from` comparison.
    #[test]
    fn a_rollback_moves_only_the_machines_it_names() {
        let b = Bench::new("0.2.0");
        let with = |to: &str, from: &str| {
            let mut doc = probe::sane();
            doc["version"] = json!("0.1.9");
            doc["artifacts"][0]["version"] = json!("0.1.9");
            doc["rollback"] = json!({"to": to, "from": from, "reason": "0.2.0 will not start"});
            doc
        };
        let d = b
            .ask(&with("0.1.9", "0.2.0"), &b.local())
            .expect("a rollback that names this machine");
        let o = offered(&d);
        assert_eq!(o.direction, Direction::PublisherRollback);
        assert_eq!(o.version, Version::parse("0.1.9").expect("literal"));

        for (to, from) in [("0.1.8", "0.2.0"), ("0.1.9", "0.1.9"), ("0.1.9", "0.3.0")] {
            let no = b
                .ask(&with(to, from), &b.local())
                .expect_err("a rollback that does not describe this machine");
            assert_eq!(
                no.code(),
                "RollbackMismatch",
                "rollback to {to} from {from} was obeyed by a 0.2.0 machine"
            );
        }
    }

    /// DEFECT THIS PREVENTS: A WITHDRAWN RELEASE BEING INSTALLED, AND A MACHINE ALREADY ON ONE
    /// NOT BEING TOLD.
    ///
    /// `yanked` is how a release that bricks is taken back from machines that already took it. Two
    /// separate things have to happen: the version is never installed, and a client sitting on it
    /// learns that it is, so the trampoline can fall back at the next launch.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `yanked.contains(&offered)` check; or compute
    /// `running_withdrawn` from the offered version rather than the running one.
    #[test]
    fn a_withdrawn_version_is_never_installed_and_a_machine_on_one_is_told() {
        let b = Bench::new("0.1.0");
        let mut doc = probe::sane();
        doc["yanked"] = json!(["0.2.0"]);
        assert_eq!(
            b.ask(&doc, &b.local())
                .expect_err("the manifest publishes and withdraws the same version")
                .code(),
            "Yanked"
        );

        let mut doc = probe::sane();
        doc["yanked"] = json!(["0.1.0"]);
        let d = b
            .ask(&doc, &b.local())
            .expect("a withdrawn RUNNING version is not a refusal, it is a reason to move");
        assert!(
            d.running_withdrawn,
            "the client was not told it is on a withdrawn build"
        );
        assert_eq!(
            offered(&d).version,
            Version::parse("0.2.0").expect("literal")
        );

        let d = b
            .ask(&probe::sane(), &b.local())
            .expect("nothing withdrawn");
        assert!(!d.running_withdrawn);
    }

    /// DEFECT THIS PREVENTS: A MIGRATION THAT CANNOT BE SKIPPED BEING SKIPPED.
    ///
    /// WHAT MUTATION MAKES THIS RED: write the comparison as `local.running <= &floor`, which
    /// refuses the machine that is exactly at the floor and is the whole point of the field.
    #[test]
    fn a_client_too_old_to_jump_is_told_which_release_to_take_first() {
        let mut doc = probe::sane();
        doc["minimum_from"] = json!("0.1.5");

        let b = Bench::new("0.1.0");
        let no = b.ask(&doc, &b.local()).expect_err("too old to jump");
        assert_eq!(no.code(), "TooOldToJump");
        assert!(no.to_string().contains("0.1.5"), "got {no}");

        let b = Bench::new("0.1.5");
        b.ask(&doc, &b.local())
            .expect("exactly at the floor may jump");
        let b = Bench::new("0.1.9");
        b.ask(&doc, &b.local()).expect("above the floor may jump");
    }

    /// DEFECT THIS PREVENTS: A MANIFEST THAT PUBLISHES NOTHING FOR THIS MACHINE BEING READ AS AN
    /// OFFER, AND A FUTURE PLATFORM BREAKING TODAY'S CLIENT.
    ///
    /// The two halves pull in opposite directions and both are required. An artifact for a
    /// platform this build has never heard of must be SKIPPED, or the day a macOS build is added
    /// every Windows client refuses the manifest. No matching artifact at all must be a refusal
    /// that NAMES what it looked for, or the reader is left with "no update" and no way to tell
    /// that from "no release yet".
    ///
    /// WHAT MUTATION MAKES THIS RED: refuse on the first unrecognised `os`; or return
    /// `Outcome::UpToDate` when no artifact matches.
    #[test]
    fn an_artifact_for_another_platform_is_skipped_and_none_at_all_is_named() {
        let b = Bench::new("0.1.0");

        let mut doc = probe::sane();
        let mut stranger = probe::app("0.2.0");
        stranger["os"] = json!("solaris");
        stranger["arch"] = json!("sparc");
        let mut future_kind = probe::app("0.2.0");
        future_kind["kind"] = json!("installer");
        doc["artifacts"] = json!([stranger, future_kind, probe::app("0.2.0")]);
        let d = b.ask(&doc, &b.local()).expect("ours is in there");
        assert_eq!(offered(&d).steps.len(), 1);
        assert_eq!(offered(&d).steps[0].os, this_os());

        let mut none = probe::sane();
        none["artifacts"] = json!([]);
        let no = b
            .ask(&none, &b.local())
            .expect_err("nothing for this build");
        assert_eq!(no.code(), "NoArtifact");
        assert!(
            no.to_string().contains(this_os()) && no.to_string().contains(this_arch()),
            "the refusal must say what it looked for, got {no}"
        );
    }

    /// DEFECT THIS PREVENTS: AN ARTIFACT FILED UNDER A VERSION IT DOES NOT CLAIM TO BE.
    ///
    /// The installer names the directory after the manifest's `version`. An app artifact labelled
    /// 0.1.9 inside a manifest publishing 0.2.0 would be installed as 0.2.0, and `current.json`
    /// would then point a rollback at a version that never existed. Both numbers are inside the
    /// signed bytes, so this is a broken pipeline rather than an attack, and it is exactly the
    /// kind of thing nobody notices for three releases.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `labelled != offered` comparison.
    #[test]
    fn an_artifact_must_claim_the_version_the_manifest_publishes() {
        let b = Bench::new("0.1.0");
        let mut doc = probe::sane();
        doc["artifacts"][0]["version"] = json!("0.1.9");
        let no = b.ask(&doc, &b.local()).expect_err("a mislabelled artifact");
        assert_eq!(no.code(), "ArtifactVersionMismatch");
    }

    /// DEFECT THIS PREVENTS: A NEW BINARY LANDING ON AN OLD SNAPSHOT.
    ///
    /// The loader reads a record shape and the bundle carries it, so an app that needs the newer
    /// bundle and does not get it turns a working install into `Data::Failed` on a file name the
    /// reader cannot act on. The fix is ordering, not a version check at load time: the bundle
    /// installs FIRST and the pointer flip waits for it.
    ///
    /// WHAT MUTATION MAKES THIS RED: push the app step before the data step; or return `Ok(None)`
    /// from `data_step` when `installed` is `None` (a machine whose snapshot the updater has
    /// never touched is exactly the machine that needs the bundle).
    #[test]
    fn a_required_data_bundle_is_installed_before_the_app() {
        let b = Bench::new("0.1.0");
        let mut doc = probe::sane();
        doc["artifacts"][0]["requires_data"] = json!("2026-09-14");
        doc["artifacts"] = json!([doc["artifacts"][0], probe::data("2026-09-14")]);

        /* Nothing installed by the updater yet: the bundle is needed. */
        let d = b.ask(&doc, &b.local()).expect("an offer");
        let steps = &offered(&d).steps;
        assert_eq!(steps.len(), 2, "both the bundle and the app");
        assert_eq!(
            steps[0].kind, KIND_DATA,
            "the bundle must be installed first"
        );
        assert_eq!(steps[1].kind, KIND_APP);

        /* Already holding that bundle, or a newer one: the app alone. */
        for have in ["2026-09-14", "2026-10-01"] {
            let mut local = b.local();
            local.installed_data = Some(have);
            let d = b.ask(&doc, &local).expect("an offer");
            assert_eq!(
                offered(&d).steps.len(),
                1,
                "holding {have} still refetched it"
            );
            assert_eq!(offered(&d).steps[0].kind, KIND_APP);
        }

        /* Holding an older one: the bundle again. */
        let mut local = b.local();
        local.installed_data = Some("2026-01-01");
        assert_eq!(
            offered(&b.ask(&doc, &local).expect("an offer")).steps.len(),
            2
        );

        /* And an app that needs a bundle the manifest does not publish is refused rather than
         * installed on top of whatever is there. */
        let mut missing = probe::sane();
        missing["artifacts"][0]["requires_data"] = json!("2026-09-14");
        let no = b.ask(&missing, &b.local()).expect_err("no bundle offered");
        assert_eq!(no.code(), "DataBundleMissing");

        /* A bundle that is offered but is older than the app needs is the same refusal, because
         * installing it would be a wasted 7 MB that still leaves the app unable to read it. */
        let mut stale = probe::sane();
        stale["artifacts"][0]["requires_data"] = json!("2026-09-14");
        stale["artifacts"] = json!([stale["artifacts"][0], probe::data("2026-01-01")]);
        assert_eq!(
            b.ask(&stale, &b.local())
                .expect_err("too old a bundle")
                .code(),
            "DataBundleMissing"
        );
    }

    /// DEFECT THIS PREVENTS: A DATA-ONLY RELEASE BEING UNDELIVERABLE.
    ///
    /// A wiki rescrape moves 7.1 MB of bundle and no binary, so the manifest publishes the same
    /// app version it published last week. A client that answered "up to date" on
    /// `Ordering::Equal` and stopped there could never receive one, and the whole reason the data
    /// is a separate bundle (decision D6) is that it updates on its own clock.
    ///
    /// WHAT MUTATION MAKES THIS RED: return `Outcome::UpToDate` unconditionally on
    /// `Ordering::Equal`.
    #[test]
    fn a_data_only_release_is_offered_to_a_client_on_the_current_version() {
        let b = Bench::new("0.2.0");
        let mut doc = probe::sane();
        doc["artifacts"] = json!([probe::app("0.2.0"), probe::data("2026-09-14")]);

        let mut local = b.local();
        local.installed_data = Some("2026-01-01");
        let d = b.ask(&doc, &local).expect("a newer bundle on the same app");
        let o = offered(&d);
        assert_eq!(o.version, Version::parse("0.2.0").expect("literal"));
        assert_eq!(o.steps.len(), 1);
        assert_eq!(o.steps[0].kind, KIND_DATA);

        /* Already on it: nothing to do. */
        let mut local = b.local();
        local.installed_data = Some("2026-09-14");
        let d = b.ask(&doc, &local).expect("nothing new");
        assert!(matches!(d.outcome, Outcome::UpToDate));
    }

    /// DEFECT THIS PREVENTS: A BUNDLE INSTALLED ONTO AN APP THAT CANNOT READ IT.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `min_app` comparison.
    #[test]
    fn a_bundle_that_needs_a_newer_app_is_refused() {
        let b = Bench::new("0.2.0");
        let mut doc = probe::sane();
        let mut bundle = probe::data("2026-09-14");
        bundle["min_app"] = json!("0.3.0");
        doc["artifacts"] = json!([probe::app("0.2.0"), bundle]);
        let mut local = b.local();
        local.installed_data = Some("2026-01-01");
        assert_eq!(
            b.ask(&doc, &local)
                .expect_err("the bundle needs 0.3.0")
                .code(),
            "DataBundleTooNew"
        );
    }

    /// DEFECT THIS PREVENTS: A SLOPPY DATE SILENTLY INVERTING THE BUNDLE ORDERING.
    ///
    /// The comparison is a string comparison, which is correct for `YYYY-MM-DD` and wrong for
    /// anything else: `"2026-9-14"` sorts AFTER `"2026-10-01"`, so one unpadded month would
    /// install an old snapshot over a new one and nothing would say so. Refusing the shape is
    /// what makes the cheap comparison honest.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `data_version` return `Ok(saw)` unconditionally.
    #[test]
    fn a_bundle_version_that_is_not_a_padded_date_is_refused() {
        assert_eq!(data_version("2026-09-14").expect("a date"), "2026-09-14");
        for bad in [
            "2026-9-14",
            "2026-09-14T00:00:00Z",
            "20260914",
            "",
            "abcd-ef-gh",
        ] {
            assert_eq!(
                data_version(bad).expect_err("not a padded date").code(),
                "DataVersionUnreadable",
                "{bad:?} was accepted as a bundle version"
            );
        }
        /* And the ordering it protects, stated as the property the string compare relies on. */
        assert!("2026-09-14" < "2026-10-01");
    }

    /// DEFECT THIS PREVENTS: A VERSION FIELD NOBODY CAN PARSE BEING TREATED AS ZERO.
    ///
    /// WHAT MUTATION MAKES THIS RED: `Version::parse(..).unwrap_or_default()` anywhere in this
    /// file, which turns every unparsable version into 0.0.0 and offers an upgrade to it.
    #[test]
    fn a_version_field_that_is_not_a_version_names_itself() {
        let b = Bench::new("0.1.0");
        for (field, doc) in [
            ("version", {
                let mut d = probe::sane();
                d["version"] = json!("two point oh");
                d
            }),
            ("yanked", {
                let mut d = probe::sane();
                d["yanked"] = json!(["two point oh"]);
                d
            }),
            ("minimum_from", {
                let mut d = probe::sane();
                d["minimum_from"] = json!("old");
                d
            }),
            ("artifact version", {
                let mut d = probe::sane();
                d["artifacts"][0]["version"] = json!("latest");
                d
            }),
        ] {
            let no = b.ask(&doc, &b.local()).expect_err("an unparsable version");
            assert_eq!(no.code(), "VersionUnreadable", "{field} was defaulted");
            assert!(
                no.to_string().contains(field),
                "the refusal must name {field}, got {no}"
            );
        }
    }
}
