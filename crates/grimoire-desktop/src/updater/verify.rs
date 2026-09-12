//! The trust root: the compiled-in public keys, and every check that turns bytes from a stranger
//! into bytes a publisher signed.
//!
//! Nothing in this file reaches the network or the clock. It is handed bytes and it answers.

use std::io::{Read, Write};

use minisign_verify::{Error as MsError, PublicKey, Signature};
use sha2::{Digest, Sha256};

use super::manifest::Manifest;
use super::Refusal;

/// EVERY PUBLIC KEY THIS BUILD WILL ACCEPT A SIGNATURE FROM, NEWEST FIRST.
///
/// # A LIST, WITH ONE ENTRY, ON PURPOSE
///
/// A shipped binary trusts what it was compiled with, and there is no way to teach it another
/// key, because teaching it one would itself be an update and an update is the thing the key
/// authorises. So rotation can only work forwards: release N+1 carries key N+1 and is SIGNED by
/// key N, and only once the field is on N+1 does signing switch. That sequence is impossible
/// unless this was a list from the first release, which is why it is one now. It costs nothing
/// today and cannot be added later.
///
/// # A PLAIN `const`, NOT `option_env!`, NOT `env!`, NOT A `build.rs`
///
/// `option_env!` would let a build with no key configured produce a binary with nothing to verify
/// against, and the only two things such a binary can do are refuse every update forever (a
/// silent dead feature) or skip verification (the catastrophe). Neither may be reachable. `env!`
/// would make the key a build-environment fact rather than a committed one, so no reader of the
/// source could say which key a given release trusts. A `build.rs` would be the first in this
/// workspace and buys nothing a `const` does not.
///
/// The key is also not a setting, not a file beside the exe, and not fetched. There is NO
/// configuration that can change which keys are trusted, because a trust root a local attacker
/// can edit is not a trust root.
///
/// # THESE ARE PUBLIC KEYS
///
/// They are in the source in the clear, they are meant to be, and the same note applies here as
/// to `secret::ENTROPY`: calling a public key a secret would be security theatre.
///
/// # THE ENTRY BELOW IS A DEVELOPMENT ANCHOR AND MUST BE REPLACED BEFORE ONE BINARY SHIPS
///
/// It is a real minisign key, generated unencrypted (the `minisign -G -W` shape) on 2026-09-11,
/// and its secret half was written OUTSIDE this repository and has never been in it. It exists so
/// that this module's tests verify against the same kind of thing production does and so that a
/// human can sign a manifest by hand today. It is NOT the release key: section 0 of the decision
/// spec says a human generates that one, pastes it here, and only then does a release go out. A
/// binary shipped on this key would be auto-updatable by whoever holds the development secret.
///
/// THAT RULE IS NO LONGER ONLY THIS COMMENT. [`DEV_ANCHOR`] names the string, a release-profile
/// build that still carries it fails to COMPILE (see the `const _` below), the release workflow
/// refuses to run on it by name, and [`trust_root_is_development`] puts it on the Settings screen
/// so the fact is visible while it is true. A comment saying so is a comment; four mechanical
/// guards are not.
/// THE RELEASE KEY. Generated 2026-09-12 with `minisign -G -W`, which is the no-password shape;
/// its secret half was never in this repository and never will be.
///
/// FIRST IN THE LIST AND ALONE IN IT. The development anchor is gone from here on purpose: a
/// binary that trusts it is auto-updatable by whoever holds the development secret, and the
/// `const _` guard below refuses a release-profile build that still carries it. Adding a second
/// entry here is how a key is ROTATED: ship a build trusting both, wait for it to reach people,
/// then drop the old one. Dropping one before that strands every copy still trusting only it.
pub const RELEASE_KEY: &str = "RWQBQa0Zx48DlWupSZ95woah8ZkvQg1zOSTZbXJMXqOo4X1t+XPB0TXU";

pub const KEYS: &[&str] = &[RELEASE_KEY];

/// THE DEVELOPMENT ANCHOR, NAMED SO IT CAN BE REFUSED BY MACHINE.
///
/// Spelled out as its own constant rather than left as an anonymous entry in [`KEYS`] because
/// every guard below has to be able to ask "is THIS still the trust root", and a guard that
/// compares against a copy of the string pasted somewhere else is a guard that goes quiet the day
/// the two copies drift. The release workflow reads this file by shape and matches this same
/// literal, which is the one place the duplication is unavoidable and is also the place a test
/// checks (`the_release_workflow_refuses_to_ship_the_development_anchor`).
pub const DEV_ANCHOR: &str = "RWQZwerBZWgbQmgNL9JnxWBBgsbah9tphw9hkZfUD/Mprnz+X+/7qzjh";

/// Is the development anchor one of the keys this build will accept signatures from?
///
/// A `const fn` BECAUSE THE ANSWER HAS TO BE AVAILABLE TO THE COMPILER. The guard that matters is
/// the one that stops a release binary existing at all, and that can only be a compile-time
/// assertion. `str` has no `const` equality on stable, so the bytes are walked by hand; it is a
/// dozen lines once, against a rule that cannot otherwise be enforced until after the binary is
/// already built and signed.
pub const fn trust_root_is_development() -> bool {
    let mut i = 0;
    while i < KEYS.len() {
        if const_str_eq(KEYS[i], DEV_ANCHOR) {
            return true;
        }
        i += 1;
    }
    false
}

/// `a == b`, in a `const` context.
const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/* A RELEASE-PROFILE BUILD MAY NOT CARRY THE DEVELOPMENT ANCHOR, AND THIS IS WHERE THAT STOPS
 * BEING A SENTENCE IN A DOC COMMENT.
 *
 * WHY A COMPILE ERROR AND NOT A TEST. The failure being prevented is one-way: once a binary built
 * on this key is in somebody's hands, whoever holds the development secret can update it for ever
 * and there is no way to take the key back out of the copies already downloaded. A test can be
 * skipped, filtered, or simply not run by the person who tags the release; a build that does not
 * compile cannot be shipped by accident. `cargo test` and `cargo clippy` are debug builds, so the
 * gate stays green while the anchor is still the right key to develop against.
 *
 * WHY THERE IS AN ESCAPE HATCH. `cargo run --release` is how the owner runs this app fast on his
 * own machine today, and a guard that made that impossible would be removed rather than obeyed.
 * `GRIMOIRE_DEV_RELEASE=1` opens it, and closing that door for the pipeline is the release
 * workflow's own guard, which reads this file's `KEYS` declaration for the anchor literal and
 * refuses whatever any environment variable says. Two independent guards, and the one that cannot
 * be talked round is the one in CI. */
#[cfg(not(debug_assertions))]
const _: () = assert!(
    !trust_root_is_development() || option_env!("GRIMOIRE_DEV_RELEASE").is_some(),
    "updater::verify::KEYS still holds the development anchor, and this is a release-profile \
     build. A binary shipped on that key is auto-updatable by whoever holds the development \
     secret, for ever, with no way to take the key back out of the copies already downloaded. \
     Generate the release keypair and paste its public half into KEYS (RELEASING.md, section 0) \
     before building anything anybody else will run. To run a release-profile build locally on the \
     development key anyway, set GRIMOIRE_DEV_RELEASE=1."
);

/// The envelope version this build knows. See [`open`] for why an unknown one is refused rather
/// than guessed at.
pub const ENVELOPE_VERSION: u64 = 1;

/// The one JSON object served at the manifest URL, before anything about it is believed.
///
/// EVERY FIELD IS OPTIONAL HERE AND NONE OF THEM IS OPTIONAL IN THE PROTOCOL. The difference
/// matters for exactly one body: the placeholder currently sitting at
/// `grimoire/stable/latest.json` is a bare smoke-test document with a `version` and an empty
/// `artifacts` array. Deserialised into a struct with required fields it would come back as
/// "missing field `envelope`", a serde sentence about a Rust type. Deserialised into this it
/// comes back as [`Refusal::ManifestUnsigned`], which is the true statement: that object is not a
/// signed manifest at all.
///
/// NO `extra` HERE, UNLIKE [`Manifest`]. Unknown keys on the MANIFEST are kept so the screen can
/// show what was served; unknown keys on the envelope are dropped by serde's default, and that is
/// right: the envelope is three fields and a version number, and anything a future envelope adds
/// comes with a version bump this build refuses outright.
#[derive(Debug, serde::Deserialize)]
struct RawEnvelope {
    envelope: Option<u64>,
    manifest: Option<String>,
    signature: Option<String>,
}

/// A manifest whose signature checked out against a compiled-in key.
///
/// # THIS TYPE IS THE ORDERING RULE, ENFORCED BY THE COMPILER
///
/// The spec says nothing downstream of a failed signature check runs, including display, and that
/// nothing parses a single field of an unverified manifest for any purpose. A comment saying so
/// is a comment. [`super::manifest::judge`] takes one of these and [`open`] is the only thing
/// that makes one, so the rule is a type error to break rather than a review note to miss.
#[derive(Clone, Debug)]
pub struct Verified {
    doc: Manifest,
    text: String,
    trusted_comment: String,
}

impl Verified {
    /// The manifest document. Safe to read: a publisher wrote these bytes.
    pub fn doc(&self) -> &Manifest {
        &self.doc
    }

    /// The exact signed bytes. The caller persists these, not a re-serialization: a manifest
    /// re-encoded by serde is a different byte sequence and would no longer verify, which is the
    /// canonicalisation trap the envelope's string-in-a-string shape exists to avoid.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The minisign trusted comment, which is covered by the signature and carries the channel,
    /// kind and version in words. A signature file found on its own still says what it is for.
    pub fn trusted_comment(&self) -> &str {
        &self.trusted_comment
    }
}

/// Open the body served at the manifest URL, using the keys this binary was compiled with.
///
/// THE PRODUCTION ENTRY POINT IS ONE LINE ON TOP OF [`open_with`] AND THAT IS DELIBERATE. This
/// crate has already shipped a defect where a test seam and the production wire disagreed:
/// `twitch_auth.rs:142-155` records a `Fake` that answered a 400 WITH a body while the real
/// `ureq` agent discarded 4xx bodies, and sign-in was refused on its first poll. A seam that
/// differs from production by one argument cannot do that, and a test holds [`KEYS`] itself to
/// being a usable key so the one argument is known good.
pub fn open(body: &str) -> Result<Verified, Refusal> {
    open_with(body, KEYS)
}

/// [`open`], against a named key list, so a test can sign with a key it generated a moment ago
/// rather than against a frozen fixture nobody can regenerate.
pub fn open_with(body: &str, keys: &[&str]) -> Result<Verified, Refusal> {
    let raw: RawEnvelope =
        serde_json::from_str(body).map_err(|e| Refusal::EnvelopeUnreadable(e.to_string()))?;

    /* THE ENVELOPE VERSION IS THE FIRST QUESTION AND AN UNKNOWN ONE IS A REFUSAL, NOT A SKIP.
     * The envelope is what says WHERE the signature is and what it covers. A client that guessed
     * at an envelope shape it does not know would be verifying the wrong bytes, which is worse
     * than verifying none. */
    let envelope = raw.envelope.ok_or(Refusal::ManifestUnsigned {
        missing: "envelope",
    })?;
    if envelope != ENVELOPE_VERSION {
        return Err(Refusal::EnvelopeVersion {
            saw: envelope,
            knows: ENVELOPE_VERSION,
        });
    }
    let text = raw.manifest.ok_or(Refusal::ManifestUnsigned {
        missing: "manifest",
    })?;
    let sig_text = raw.signature.ok_or(Refusal::ManifestUnsigned {
        missing: "signature",
    })?;

    let signature =
        Signature::decode(&sig_text).map_err(|e| Refusal::ManifestBadSignature(e.to_string()))?;
    let trusted_comment = signature.trusted_comment().to_owned();

    /* THE SIGNED BYTES ARE EXACTLY `text.as_bytes()`. Not a re-serialization of the parsed
     * document, not the whole envelope, not a canonical form: a JSON string has exactly one byte
     * sequence, which is the entire reason the manifest travels as a string inside the envelope
     * rather than as a nested object. */
    verify_bytes(keys, text.as_bytes(), &signature)
        .map_err(|e| Refusal::ManifestBadSignature(e.to_string()))?;

    /* AND ONLY NOW IS ANY FIELD OF IT LOOKED AT. */
    let doc: Manifest =
        serde_json::from_str(&text).map_err(|e| Refusal::ManifestUnreadable(e.to_string()))?;
    Ok(Verified {
        doc,
        text,
        trusted_comment,
    })
}

/// Parse the key list, refusing a build that cannot verify anything at all.
fn anchors(keys: &[&str]) -> Result<Vec<PublicKey>, Refusal> {
    if keys.is_empty() {
        return Err(Refusal::NoTrustAnchor {
            why: "the trusted key list is empty".to_owned(),
        });
    }
    keys.iter()
        .map(|k| {
            PublicKey::from_base64(k).map_err(|e| Refusal::NoTrustAnchor {
                why: format!("{k:?} is not a minisign public key: {e}"),
            })
        })
        .collect()
}

/// The most useful of several failures.
///
/// `UnexpectedKeyId` means "a key we do not carry made this", which is the boring and common
/// case. `InvalidSignature` means "a key we DO carry is claimed and the maths says no", which is
/// the interesting one. Reporting the first error in key order would hide the second behind the
/// first whenever the list grows past one entry, so the interesting one wins.
fn best_failure(errors: Vec<MsError>) -> MsError {
    let mut fallback = None;
    for e in errors {
        match e {
            MsError::UnexpectedKeyId => fallback = fallback.or(Some(MsError::UnexpectedKeyId)),
            other => return other,
        }
    }
    fallback.unwrap_or(MsError::InvalidSignature)
}

/// Verify a whole in-memory message against any of the trusted keys.
///
/// `allow_legacy` IS ALWAYS FALSE, WHICH MEANS PREHASHED SIGNATURES ONLY, AND THAT IS A
/// CONSTRAINT ON THE PUBLISHER. `minisign-verify` refuses a legacy (non-prehashed) signature
/// outright when the flag is false: see its `verify`, which returns `UnexpectedAlgorithm` on that
/// path. So the release pipeline must sign the manifest with `-H` as well as the artifacts; the
/// distinction that matters here is whole-buffer versus streamed, not hashed versus not.
fn verify_bytes(keys: &[&str], message: &[u8], signature: &Signature) -> Result<(), Refusal> {
    let mut errors = Vec::new();
    for key in anchors(keys)? {
        match key.verify(message, signature, false) {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(e),
        }
    }
    Err(Refusal::ManifestBadSignature(
        best_failure(errors).to_string(),
    ))
}

/// What a signed manifest promises about one file: how long it is, what it hashes to, and who
/// signed it.
///
/// All three travel together because all three are checked in one pass over the bytes, and
/// splitting them into three calls would mean three passes over ten megabytes.
#[derive(Clone, Copy, Debug)]
pub struct Seal<'a> {
    /// Enforced EXACTLY. Not a ceiling: the honest cap is published and signed, so a stream that
    /// ends short or has more to give is a refusal rather than a truncation nobody notices.
    pub size: u64,
    /// 64 lowercase hex.
    pub sha256: &'a str,
    /// The full text of the detached minisign signature for the file.
    pub signature: &'a str,
}

/// THE READ BUFFER. A POLICY, NOT A MEASUREMENT, AND IT SAYS SO.
///
/// Nothing about 64 KiB was measured on this machine, so it is not dressed as a number that was.
/// It is large enough that the syscall count is not the cost of a ten megabyte download and small
/// enough that a progress callback fires often enough to be worth coalescing at the other half's
/// 100 ms valve. If it ever needs a real number, measure the download against the read and put
/// the measurement here.
const CHUNK: usize = 64 * 1024;

/// Copy `src` to `dst`, enforcing everything the signed manifest said about it, in ONE pass.
///
/// # WHY ONE FUNCTION AND NOT A STRUCT THE CALLER FEEDS
///
/// The obvious shape is a verifier the download loop pushes chunks into. It cannot be written
/// here without a self-referential struct: `minisign_verify::StreamVerifier` borrows both the
/// public key and the signature, so a struct that owned the key, the signature and the verifier
/// would have to hold references into itself. Inverting it costs nothing, because every caller
/// wants exactly this loop: read a chunk, write it, hash it, feed it to the stream verifier.
///
/// # THE ARTIFACT IS NEVER HELD IN MEMORY
///
/// `dst` is where the bytes go, which for a download is the `.part` file and for a re-verification
/// from disk is `None`. Ten megabytes never becomes a `Vec`.
///
/// # THE ORDER OF THE THREE ANSWERS IS THE POINT
///
/// Size, then sha256, then signature. A truncated download and a tampered download must not be
/// the same event: the first is the CDN having a bad day and the second is somebody serving bytes
/// the key never signed, and a reader who is told the wrong one goes looking in the wrong place.
/// The sha256 is trustworthy because the manifest carrying it is signed, and its job is to give
/// corruption a cause of its own and to be the string a human pastes into a bug report.
pub fn copy_sealed<R: Read, W: Write>(
    mut src: R,
    mut dst: Option<W>,
    seal: Seal<'_>,
    keys: &[&str],
    progress: &mut dyn FnMut(u64),
) -> Result<(), Refusal> {
    let said = normal_sha256(seal.sha256)?;
    let signature =
        Signature::decode(seal.signature).map_err(|e| Refusal::ArtifactCheckUnbuildable {
            field: "signature",
            why: e.to_string(),
        })?;
    let trusted = anchors(keys)?;

    let mut hasher = Sha256::new();

    /* A KEY THAT CANNOT EVEN BE SET UP AGAINST THIS SIGNATURE IS A FAILURE HELD BACK, NOT RAISED
     * HERE. `verify_stream` refuses on two counts: the signature was made by a key this build does
     * not carry, and the signature is not prehashed. Both are real answers and NEITHER may be
     * reported before the size and the hash have had their turn, because the whole reason those
     * two checks exist is to give a truncated or corrupt download a cause of its own. Raising a
     * key-id mismatch first would report a bad CDN as an attack. */
    let mut streams = Vec::new();
    let mut setup: Vec<MsError> = Vec::new();
    for k in &trusted {
        match k.verify_stream(&signature) {
            Ok(s) => streams.push(s),
            Err(e) => setup.push(e),
        }
    }

    let mut buf = vec![0u8; CHUNK];
    let mut seen: u64 = 0;
    loop {
        /* READ AT MOST ONE BYTE PAST THE PROMISED LENGTH. That one byte is what turns "the server
         * has more to give" into an answer, without reading an unbounded body to find out how
         * much more. The promised length is itself the cap, which is why there is no second
         * invented ceiling in this loop. */
        let room = seal.size.saturating_sub(seen).saturating_add(1);
        let want = usize::try_from(room).unwrap_or(CHUNK).min(CHUNK);
        let n = match src.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                return Err(Refusal::Io {
                    doing: "read the download",
                    path: std::path::PathBuf::from("<stream>"),
                    why: e.to_string(),
                })
            }
        };
        seen += n as u64;
        if seen > seal.size {
            return Err(Refusal::ArtifactSizeMismatch {
                said: seal.size,
                got: seen,
            });
        }
        let chunk = &buf[..n];
        if let Some(w) = dst.as_mut() {
            w.write_all(chunk).map_err(|e| Refusal::Io {
                doing: "write the download to",
                path: std::path::PathBuf::from("<the staged file>"),
                why: e.to_string(),
            })?;
        }
        hasher.update(chunk);
        for s in &mut streams {
            s.update(chunk);
        }
        progress(seen);
    }
    if seen != seal.size {
        return Err(Refusal::ArtifactSizeMismatch {
            said: seal.size,
            got: seen,
        });
    }
    let got = hex(&hasher.finalize());
    if got != said {
        return Err(Refusal::ArtifactHashMismatch { said, got });
    }
    let mut errors = setup;
    for s in &mut streams {
        match s.finalize() {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(e),
        }
    }
    /* ONE CAUSE THAT IS NOT A REFUSAL OF THE FILE. A signature made without minisign's `-H` is
     * not prehashed, and streaming verification is only defined for the prehashed form, so this
     * build cannot check it at all. That is a broken release pipeline and must not be reported as
     * "these bytes were tampered with", which would send somebody hunting an attacker. */
    if errors
        .iter()
        .any(|e| matches!(e, MsError::UnsupportedLegacyMode))
    {
        return Err(Refusal::ArtifactCheckUnbuildable {
            field: "signature",
            why: "it was not made with minisign -H, so it cannot be checked while the file \
                  streams past"
                .to_owned(),
        });
    }
    Err(Refusal::ArtifactBadSignature(
        best_failure(errors).to_string(),
    ))
}

/// [`copy_sealed`] against a file already on disk, writing nothing.
///
/// This is step 8 of the file dance: a staged artifact sits on disk between the download and the
/// install, and at that moment the only things present are the file and its `.minisig`. Nothing
/// here touches the network or the manifest.
pub fn check_file(path: &std::path::Path, seal: Seal<'_>, keys: &[&str]) -> Result<(), Refusal> {
    let f = std::fs::File::open(path).map_err(|e| super::io("open", path, &e))?;
    copy_sealed(
        std::io::BufReader::new(f),
        None::<std::io::Sink>,
        seal,
        keys,
        &mut |_| {},
    )
    .map_err(|why| Refusal::StagedFileChangedOnDisk {
        path: path.to_path_buf(),
        why: Box::new(why),
    })
}

/// 64 lowercase hex, or a refusal.
///
/// STRICT ON CASE ON PURPOSE. The publisher controls this field, so there is no compatibility to
/// buy by accepting both. What accepting both costs is real: the comparison then has a
/// `to_lowercase()` on one side, and the day somebody writes the other side without it the check
/// passes on every hash and fails on none.
fn normal_sha256(said: &str) -> Result<String, Refusal> {
    let bad = |why: &str| Refusal::ArtifactCheckUnbuildable {
        field: "sha256",
        why: why.to_owned(),
    };
    if said.len() != 64 {
        return Err(bad(&format!(
            "a sha256 is 64 hex characters and this one is {}",
            said.len()
        )));
    }
    if !said
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad("a sha256 here is 64 LOWERCASE hex characters"));
    }
    Ok(said.to_owned())
}

/// Lowercase hex, written here rather than pulled in, because one loop is not a dependency.
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        /* A `write!` into a String cannot fail; the result is discarded rather than unwrapped so
         * that no path in the verifier can panic. */
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
pub(crate) mod probe {
    //! Real minisign keys and real signatures, made fresh in the test process.
    //!
    //! WHY THIS AND NOT FROZEN FIXTURES. A signature pasted into the source can only prove that
    //! one byte sequence verifies. It cannot produce a manifest with a different channel, a
    //! different `published`, a different version or an unknown field, so every rule downstream
    //! of the signature gate would have to be tested at a layer production does not use, which is
    //! this tree's signature defect wearing a different hat. `minisign` is a DEV dependency only:
    //! `cargo tree -p grimoire-desktop -e normal` does not contain it, and the shipping binary
    //! carries `minisign-verify`, which has zero dependencies of its own.

    use minisign::{KeyPair, SecretKey};

    /// A keypair, and the base64 public key in the form [`super::KEYS`] holds.
    pub(crate) struct Signer {
        pub(crate) secret: SecretKey,
        pub(crate) public_b64: String,
    }

    impl Signer {
        pub(crate) fn new() -> Signer {
            let kp = KeyPair::generate_unencrypted_keypair().expect("a keypair in a test");
            Signer {
                public_b64: kp.pk.to_base64(),
                secret: kp.sk,
            }
        }

        /// The full text of a detached minisign signature over `message`.
        pub(crate) fn sign(&self, message: &[u8]) -> String {
            minisign::sign(None, &self.secret, message, Some("grimoire test"), None)
                .expect("signing in a test")
                .into_string()
        }

        /// The one JSON object the manifest URL serves, around a manifest document.
        pub(crate) fn envelope(&self, manifest_text: &str) -> String {
            let signature = self.sign(manifest_text.as_bytes());
            serde_json::to_string(&serde_json::json!({
                "envelope": super::ENVELOPE_VERSION,
                "manifest": manifest_text,
                "signature": signature,
            }))
            .expect("a JSON object built from strings in a test")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::probe::Signer;
    use super::*;
    use crate::updater::manifest::probe as mprobe;

    /// DEFECT THIS PREVENTS: SHIPPING A BUILD THAT CAN VERIFY NOTHING.
    ///
    /// [`KEYS`] is a hand-pasted base64 string. Paste it with a missing character, or paste the
    /// secret key's line by mistake, and every check in this module answers
    /// [`Refusal::NoTrustAnchor`] forever: the app would silently never update and the only
    /// symptom would be its absence. Nothing else in the crate would go red, because a `const` of
    /// the wrong string still compiles.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the last character of the [`KEYS`] entry, or set it to
    /// `&[]`.
    #[test]
    fn the_compiled_in_key_is_a_key() {
        let parsed = anchors(KEYS).expect("the shipped key list must parse");
        assert_eq!(parsed.len(), KEYS.len());
        assert!(
            anchors(&[]).is_err(),
            "an empty key list must be a named refusal and not an accepted state"
        );
        assert!(
            anchors(&["not a key"]).is_err(),
            "a key list this build cannot parse must be a named refusal"
        );
    }

    /// DEFECT THIS PREVENTS: A RELEASE BEING CUT ON THE DEVELOPMENT KEY, WHICH CANNOT BE UNDONE.
    ///
    /// # THE FAILURE IS A CLICK, NOT AN ATTACK
    ///
    /// Whoever cuts the first release generates a real keypair, puts the secret half in
    /// `MINISIGN_SECRET_KEY`, and forgets section 0 of RELEASING.md, which is the step that pastes
    /// the public half into [`KEYS`]. Nothing in the pipeline noticed before this: the workflow's
    /// key-agreement guard only asks whether the secret matches SOME key in the source, and the
    /// development anchor is a real minisign key that matches the shape it scans for. The rule
    /// lived entirely in a doc comment, in a tree whose whole doctrine is that a comment saying so
    /// is a comment.
    ///
    /// # WHY THE CHECK IS NOT SIMPLY `assert_ne!(KEYS[0], DEV_ANCHOR)`
    ///
    /// That assertion is red today and has to be, because the development anchor IS the right
    /// trust root for a tree that has never published a release and whose tests sign with a key
    /// they generate. A gate that is red until an unrelated future event is a gate people turn
    /// off. The rule that actually needs enforcing is narrower and is enforceable now: the anchor
    /// may not reach a RELEASE. That is what the `const _` guard in this file does (a
    /// release-profile build carrying it does not compile), what the release workflow does (it
    /// reads this file and refuses by name), and what [`trust_root_is_development`] lets the
    /// Settings screen say out loud while it is still true. This test stands over the machinery
    /// all three are built on.
    ///
    /// WHAT MUTATION MAKES THIS RED: make `const_str_eq` answer `true` for everything, or `false`
    /// for everything; make `trust_root_is_development` answer a constant; or drop [`DEV_ANCHOR`]
    /// out of [`KEYS`] without also removing it from the `KEYS` declaration, which is the
    /// half-finished rotation this is here to catch.
    #[test]
    fn the_build_can_tell_whether_its_trust_root_is_the_development_anchor() {
        assert!(
            const_str_eq(DEV_ANCHOR, DEV_ANCHOR),
            "the comparison the compile-time guard is built on does not recognise the anchor"
        );
        assert!(
            !const_str_eq(DEV_ANCHOR, &DEV_ANCHOR[..DEV_ANCHOR.len() - 1]),
            "a truncated anchor compares equal, so the guard would fire on keys that are not it"
        );
        assert!(!const_str_eq(DEV_ANCHOR, "RW"));
        assert_eq!(
            trust_root_is_development(),
            KEYS.contains(&DEV_ANCHOR),
            "this build cannot tell whether it is carrying the development anchor, so neither the \
             compile-time guard nor the line the Settings screen draws means anything"
        );
        anchors(&[DEV_ANCHOR]).expect(
            "the development anchor must be a real minisign key, or the guards above are firing \
             on a placeholder rather than on a usable trust root",
        );
    }

    /// DEFECT THIS PREVENTS: THE TWO MECHANICAL GUARDS OVER THE TRUST ROOT BEING DELETED QUIETLY.
    ///
    /// A SOURCE-TEXT TEST, WHICH IS A FLOOR AND SAYS SO. It is the same instrument
    /// `the_preflight_uses_the_environment_variable_main_actually_reads` (`install.rs`) uses, for
    /// the same reason: neither a `#[cfg(not(debug_assertions))]` item nor a workflow step can be
    /// reached from a debug-profile test any other way, and an unenforced rule over the trust root
    /// is the single most consequential thing in this feature.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `const _: () = assert!(..)` guard from this file;
    /// delete the anchor guard step from `.github/workflows/release.yml`; or drop the pointer to
    /// section 0 of RELEASING.md out of its message, which is the sentence that tells the person
    /// who hits it what to do instead.
    #[test]
    fn the_release_workflow_refuses_to_ship_the_development_anchor() {
        let here = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("updater")
                .join("verify.rs"),
        )
        .expect("this module's own source");
        let guard = here
            .split("#[cfg(not(debug_assertions))]")
            .nth(1)
            .unwrap_or_default();
        assert!(
            guard.contains("const _: () = assert!(") && guard.contains("trust_root_is_development"),
            "the release-profile build guard is gone, so a release binary can be built on the \
             development key and nothing would say so"
        );

        let workflow = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".github")
            .join("workflows")
            .join("release.yml");
        let yml = std::fs::read_to_string(&workflow).unwrap_or_else(|e| {
            panic!(
                "{} could not be read ({e}); the release pipeline's own guard over the trust root \
                 is unverifiable",
                workflow.display()
            )
        });
        assert!(
            yml.contains(DEV_ANCHOR),
            "release.yml does not name the development anchor, so a release cut on it would pass \
             every guard in the pipeline: the key-agreement step only asks whether the secret \
             matches SOME key in the source, and this one does"
        );
        assert!(
            yml.contains("RELEASING.md") && yml.contains("section 0"),
            "the release guard does not tell whoever hits it which step they skipped"
        );
    }

    /// DEFECT THIS PREVENTS: A MANIFEST NOBODY SIGNED BEING READ ANYWAY.
    ///
    /// The whole ordering rule rests on this call: if `open` returned a `Verified` for an
    /// unsigned body, every rule after it would be running on attacker-chosen input. The
    /// placeholder object that is in the live bucket RIGHT NOW is the exact shape this must
    /// refuse, so it is the fixture.
    ///
    /// WHAT MUTATION MAKES THIS RED: make `open_with` skip `verify_bytes` and parse the manifest
    /// string directly, or let a missing `signature` default to an empty string.
    #[test]
    fn an_unsigned_body_is_refused_before_a_single_field_is_read() {
        /* The object sitting at grimoire/stable/latest.json today, verbatim in shape: a version
         * and an empty artifacts array, with no envelope, no manifest string and no signature. */
        let placeholder = r#"{"version":"0.0.0-smoketest","artifacts":[]}"#;
        let no = open_with(placeholder, KEYS).expect_err("a smoke-test object is not a manifest");
        assert_eq!(no.code(), "ManifestUnsigned");
        assert!(
            no.to_string().contains("envelope"),
            "the refusal must name what was missing, got {no}"
        );

        let signer = Signer::new();
        let text = mprobe::manifest_text(&mprobe::sane());
        let signed = signer.envelope(&text);
        let keys: &[&str] = &[&signer.public_b64];

        /* A well-formed envelope with the signature removed. */
        let mut v: serde_json::Value = serde_json::from_str(&signed).expect("test envelope");
        v.as_object_mut().expect("an object").remove("signature");
        let stripped = v.to_string();
        assert_eq!(
            open_with(&stripped, keys)
                .expect_err("an envelope with no signature is not signed")
                .code(),
            "ManifestUnsigned"
        );

        /* A SIGNATURE THAT IS PRESENT, WELL FORMED, AND OVER SOMETHING ELSE. The two cases above
         * are both an ABSENT signature, and absence is caught by the `Option` whether or not
         * anything ever verifies anything. This is the case that goes through `verify_bytes`, and
         * without it a build that ignored the verification result entirely would pass this test.
         * A mutation run found exactly that gap. */
        let elsewhere = signer.sign(b"some other document");
        let swapped = serde_json::json!({"envelope": 1, "manifest": text, "signature": elsewhere})
            .to_string();
        assert_eq!(
            open_with(&swapped, keys)
                .expect_err("a signature over other bytes")
                .code(),
            "ManifestBadSignature"
        );

        /* And the same body, whole, verifies, so the refusals above are about the signature and
         * not about the fixture being broken. */
        open_with(&signed, keys).expect("the intact envelope verifies");
    }

    /// DEFECT THIS PREVENTS: A MANIFEST EDITED IN FLIGHT STILL BEING BELIEVED.
    ///
    /// One byte of the manifest string is the whole attack: change `"version":"0.2.0"` to a
    /// version that points at an artifact URL of the attacker's choosing. The signature covers
    /// `manifest.as_bytes()` exactly, so any edit at all must fail.
    ///
    /// EVERY BYTE, NOT ONE. A check written against the first N bytes, or against a re-serialized
    /// form, would pass for edits later in the document; this walks the whole string.
    ///
    /// WHAT MUTATION MAKES THIS RED: sign or verify a re-serialization
    /// (`serde_json::to_string(&doc)`) instead of `text.as_bytes()`; or truncate the message
    /// passed to `verify_bytes`.
    #[test]
    fn one_changed_byte_of_the_manifest_fails_verification() {
        let signer = Signer::new();
        let text = mprobe::manifest_text(&mprobe::sane());
        let keys: &[&str] = &[&signer.public_b64];
        let signature = signer.sign(text.as_bytes());

        let envelope = |m: &str| {
            serde_json::json!({"envelope": 1, "manifest": m, "signature": signature}).to_string()
        };
        open_with(&envelope(&text), keys).expect("the untouched manifest verifies");

        let bytes = text.as_bytes();
        for i in (0..bytes.len()).step_by(7) {
            let mut edited = bytes.to_vec();
            /* Flip one bit, so the edit is a byte the document could plausibly have held rather
             * than something a JSON parser would reject before the signature was ever asked. */
            edited[i] ^= 0b0000_0001;
            let Ok(edited) = String::from_utf8(edited) else {
                continue;
            };
            if edited == text {
                continue;
            }
            let no = open_with(&envelope(&edited), keys)
                .expect_err("an edited manifest must not verify");
            assert_eq!(
                no.code(),
                "ManifestBadSignature",
                "byte {i} was changed and the refusal was {no}"
            );
        }
    }

    /// DEFECT THIS PREVENTS: TRUSTING A SIGNATURE THAT IS PERFECTLY VALID, FOR SOMEBODY ELSE'S
    /// KEY.
    ///
    /// This is the one a corruption check cannot catch and the one that matters: the bytes are
    /// intact, the signature is real, the maths checks out against the key that made it. The only
    /// thing wrong is that the key is not ours. A verifier that decoded the signature and did not
    /// compare key ids, or that verified against the key carried IN the signature, would pass
    /// this and fail nothing else in this file.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `verify_bytes` return `Ok(())` when the error is
    /// `UnexpectedKeyId`; or pass the attacker's key list through to `open`.
    #[test]
    fn a_signature_from_another_key_is_refused() {
        let ours = Signer::new();
        let theirs = Signer::new();
        let text = mprobe::manifest_text(&mprobe::sane());

        let attacker = theirs.envelope(&text);
        let no = open_with(&attacker, &[&ours.public_b64])
            .expect_err("a stranger's key must not be trusted");
        assert_eq!(no.code(), "ManifestBadSignature");

        /* The same bytes verify for the key that did sign them, which is what makes the refusal
         * above about the KEY and not about the fixture. */
        open_with(&attacker, &[&theirs.public_b64]).expect("their own key verifies their own work");

        /* And a build that carries both keys accepts either, which is the rotation the slice
         * exists for: release N+1 carries N and N+1. */
        open_with(&attacker, &[&ours.public_b64, &theirs.public_b64])
            .expect("a build carrying both keys accepts either");
    }

    /// DEFECT THIS PREVENTS: AN ENVELOPE SHAPE THIS BUILD DOES NOT KNOW BEING GUESSED AT.
    ///
    /// The envelope says where the signature is. A client that read envelope 2 with envelope 1's
    /// rules would be verifying the wrong bytes, which is worse than verifying none.
    ///
    /// WHAT MUTATION MAKES THIS RED: replace the `envelope != ENVELOPE_VERSION` check with
    /// `envelope > ENVELOPE_VERSION`, or delete it.
    #[test]
    fn an_envelope_version_this_build_does_not_know_is_refused() {
        let signer = Signer::new();
        let text = mprobe::manifest_text(&mprobe::sane());
        let signature = signer.sign(text.as_bytes());
        let keys: &[&str] = &[&signer.public_b64];
        for saw in [0u64, 2, 99] {
            let body =
                serde_json::json!({"envelope": saw, "manifest": text, "signature": signature})
                    .to_string();
            let no = open_with(&body, keys).expect_err("an envelope version nobody knows");
            assert_eq!(no.code(), "EnvelopeVersion", "envelope {saw} was accepted");
        }
    }

    /* ------------------------------------------------------- the artifact seal -- */

    fn sha256_of(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// DEFECT THIS PREVENTS: A TRUNCATED, PADDED OR CORRUPT DOWNLOAD BEING INSTALLED, AND THE
    /// THREE OF THEM BEING THE SAME EVENT.
    ///
    /// A short body is a connection that dropped. A long body is a server that lied about its
    /// length. A wrong hash is corruption in transit or at the CDN. A wrong signature is somebody
    /// serving bytes the key never signed. The whole reason the manifest carries both a size and
    /// a hash when it already carries a signature is so those four are four different sentences.
    ///
    /// THE PARTIAL FILE IS THE CALLER'S TO DELETE AND THE TEST FOR THAT IS IN `install.rs`; what
    /// is proved here is that each failure is refused, with its own cause, and that nothing is
    /// held in memory to do it.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `seen != seal.size` check after the loop (the short
    /// body passes); drop the `seen > seal.size` check inside it (the long body passes); compare
    /// the hash with `starts_with` (the flipped byte passes); or move the hash check after the
    /// signature check, which makes corruption report as tampering.
    #[test]
    fn a_download_is_held_to_the_size_the_hash_and_the_signature() {
        let signer = Signer::new();
        let keys: &[&str] = &[&signer.public_b64];
        let body = b"the artifact, such as it is, in a test".to_vec();
        let signature = signer.sign(&body);
        let sha = sha256_of(&body);
        /* ONE SEAL FOR ALL FOUR CASES, WHICH IS THE POINT. The promise never changes; only the
         * bytes the server sends do. A test that adjusted the seal to match each body would be
         * testing nothing. */
        let seal = Seal {
            size: body.len() as u64,
            sha256: &sha,
            signature: &signature,
        };

        let run = |bytes: &[u8], s: Seal<'_>| {
            let mut out = Vec::new();
            let answer = copy_sealed(bytes, Some(&mut out), s, keys, &mut |_| {});
            answer.map(|()| out)
        };

        let out = run(&body, seal).expect("the honest download passes");
        assert_eq!(out, body, "the bytes must reach the sink unchanged");

        let short = &body[..body.len() - 1];
        let no = run(short, seal).expect_err("a short body");
        assert_eq!(no.code(), "ArtifactSizeMismatch");

        let mut long = body.clone();
        long.push(b'!');
        let no = run(&long, seal).expect_err("a long body");
        assert_eq!(no.code(), "ArtifactSizeMismatch");

        /* AND A BODY THAT NEVER ENDS IS STOPPED ONE BYTE PAST THE PROMISE, which is a separate
         * rule from the one above and was covered by nothing until a mutation run said so. A
         * server that simply keeps sending has no `size` to disagree with and no EOF to reach; the
         * only thing standing between it and the disk is that the read length is capped at what
         * is left to read plus one. Without that cap this assertion never returns at all. */
        struct Endless(u64);
        impl Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0 += buf.len() as u64;
                buf.fill(b'x');
                Ok(buf.len())
            }
        }
        let mut endless = Endless(0);
        let no = copy_sealed(&mut endless, None::<std::io::Sink>, seal, keys, &mut |_| {})
            .expect_err("a body that never ends");
        assert_eq!(no.code(), "ArtifactSizeMismatch");
        assert_eq!(
            endless.0,
            body.len() as u64 + 1,
            "the check read past the promised size instead of stopping one byte over it"
        );

        let mut flipped = body.clone();
        flipped[3] ^= 0b0000_0001;
        let no = run(&flipped, seal).expect_err("corrupt bytes");
        assert_eq!(
            no.code(),
            "ArtifactHashMismatch",
            "corruption must not be reported as tampering, got {no}"
        );

        /* The same length and the same hash, signed by somebody else. */
        let theirs = Signer::new();
        let their_sig = theirs.sign(&body);
        let no = copy_sealed(
            &body[..],
            None::<std::io::Sink>,
            Seal {
                size: body.len() as u64,
                sha256: &sha,
                signature: &their_sig,
            },
            keys,
            &mut |_| {},
        )
        .expect_err("a stranger's signature over the right bytes");
        assert_eq!(no.code(), "ArtifactBadSignature");
    }

    /// DEFECT THIS PREVENTS: A MALFORMED `sha256` SILENTLY DISABLING THE HASH CHECK.
    ///
    /// A comparison against a 63-character string, or against uppercase hex, is a comparison that
    /// can never be true, and a check that can never be true fails closed here only because it is
    /// written to. Strict on case as well as length, because a field accepted in two spellings
    /// gets a `to_lowercase()` on one side of the comparison and, eventually, not the other.
    ///
    /// WHAT MUTATION MAKES THIS RED: accept any length; or accept uppercase by lowercasing the
    /// input here.
    #[test]
    fn a_hash_the_check_cannot_use_is_refused_rather_than_ignored() {
        let ok = "0".repeat(64);
        assert_eq!(normal_sha256(&ok).expect("64 lowercase hex"), ok);
        for bad in [
            "".to_owned(),
            "0".repeat(63),
            "0".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            let no = normal_sha256(&bad).expect_err("not a sha256");
            assert_eq!(
                no.code(),
                "ArtifactCheckUnbuildable",
                "{bad:?} was accepted"
            );
        }
    }

    /// DEFECT THIS PREVENTS: PROGRESS THAT LIES, AND A DOWNLOAD HELD IN MEMORY.
    ///
    /// The other half draws a progress bar from this callback and coalesces it at 100 ms. A
    /// callback that reported the buffer size rather than the running total, or that fired once at
    /// the end, would paint a bar that jumps from nothing to done.
    ///
    /// WHAT MUTATION MAKES THIS RED: call `progress(n)` with the chunk length instead of `seen`;
    /// or move the call out of the loop.
    #[test]
    fn progress_counts_up_to_the_promised_size() {
        let signer = Signer::new();
        let body = vec![7u8; CHUNK * 2 + 11];
        let signature = signer.sign(&body);
        let sha = sha256_of(&body);
        let mut seen = Vec::new();
        copy_sealed(
            &body[..],
            None::<std::io::Sink>,
            Seal {
                size: body.len() as u64,
                sha256: &sha,
                signature: &signature,
            },
            &[&signer.public_b64],
            &mut |n| seen.push(n),
        )
        .expect("the download passes");
        assert!(seen.len() > 2, "a multi-chunk body reports more than once");
        assert!(
            seen.windows(2).all(|w| w[0] < w[1]),
            "progress must only ever count up: {seen:?}"
        );
        assert_eq!(
            seen.last().copied(),
            Some(body.len() as u64),
            "the last report is the whole file"
        );
    }

    /// DEFECT THIS PREVENTS: A FILE THAT VERIFIED WHEN IT WAS DOWNLOADED BEING INSTALLED AFTER
    /// SOMETHING CHANGED IT ON DISK.
    ///
    /// A staged artifact sits on disk between the download and the install, and the install is a
    /// copy. Step 8 re-verifies from disk, with no manifest and no network, which is the entire
    /// reason the `.minisig` travels beside the binary. The cause is its own so that a person
    /// reading the failure knows the download was fine and something local was not.
    ///
    /// WHAT MUTATION MAKES THIS RED: have `check_file` return the inner refusal rather than
    /// wrapping it, or have it return `Ok(())` when the file is missing.
    #[test]
    fn a_staged_file_is_re_verified_from_disk() {
        let signer = Signer::new();
        let keys: &[&str] = &[&signer.public_b64];
        let body = b"a staged payload".to_vec();
        let signature = signer.sign(&body);
        let sha = sha256_of(&body);
        let seal = Seal {
            size: body.len() as u64,
            sha256: &sha,
            signature: &signature,
        };

        let dir = std::env::temp_dir().join(format!(
            "grimoire-updater-verify-{}-staged",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let p = dir.join("payload.bin");
        std::fs::write(&p, &body).expect("plant the staged file");
        check_file(&p, seal, keys).expect("an untouched staged file re-verifies");

        std::fs::write(&p, b"a staged payload!").expect("something edits it on disk");
        let no = check_file(&p, seal, keys).expect_err("an edited staged file must not install");
        assert_eq!(no.code(), "StagedFileChangedOnDisk");
        assert!(
            no.to_string().contains("payload.bin"),
            "the refusal must name the file, got {no}"
        );

        std::fs::remove_file(&p).expect("remove it");
        assert!(
            check_file(&p, seal, keys).is_err(),
            "a missing staged file is not a pass"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
