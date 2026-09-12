//! THE ONE PLACE THIS FEATURE TOUCHES THE NETWORK, and the seam a test drives instead.
//!
//! # NO SECOND HTTP STACK, WHICH IS THE HEADLINE CONSTRAINT
//!
//! `ureq` 3 is already the only HTTP client in this binary and `rustls` with the `ring` provider
//! is already the only TLS stack (`Cargo.toml:53-58`, whose comment states that a second provider
//! makes `CryptoProvider::get_default` ambiguous). The crate an updater checklist would reach for,
//! `self_update`, pulls `reqwest` and usually `native-tls`: both of those. So the agent here is
//! built the way `watcher.rs:162` and `channel_art.rs:336` already build theirs, out of the same
//! `config_builder`, reusing their constants rather than declaring a second set that can drift.
//!
//! # `http_status_as_error(true)`, AND THAT IS THE OPPOSITE OF `twitch_auth`
//!
//! `twitch_auth::Https` sets it to FALSE, and its long comment (`twitch_auth.rs:142-155`) records
//! why: a 400 body is that protocol's way of saying "the owner is still typing", so discarding
//! 4xx bodies refused every sign-in on its first poll. Nothing of the sort is true here. A 404 at
//! the manifest URL means there is no manifest, not that the manifest is in the 404's body, and a
//! client that parsed an error page would be parsing an unsigned document. The two settings are
//! opposite because the two protocols are opposite, and both are checked by a test.
//!
//! # THE SEAM, AND WHY THE PRODUCTION SIDE IS CHECKED AND NOT JUST FAKED
//!
//! [`Wire`] exists so the worker in [`super::run`] can be driven from bytes in a test. That alone
//! is not enough and this crate has the scar to prove it: the `Fake` wire in `twitch_auth`
//! answered a 400 WITH a body, the real agent discarded it, and the unit tests could not see the
//! difference. So [`the_production_wire_refuses_a_missing_manifest_instead_of_parsing_it`] asserts
//! the shipped agent's own configuration, exactly as `the_wire_keeps_the_body_of_a_refusal` does
//! for the other one, and `tests/live_update_channel.rs` dials the real host by hand.

use std::io::Read;
use std::time::Duration;

/// The host the update channel is served from.
///
/// A CONSTANT AND NOT A SETTING. The URL a client fetches its manifest from is half of the trust
/// root: the other half is [`super::verify::KEYS`], and a signature check against a compiled-in
/// key is worth a great deal less if a local attacker can repoint the client at their own bucket
/// and wait for a key rotation. Neither half is configurable, for the same reason.
pub const HOST: &str = "https://updates.ragnarok.systems";

/// The most the manifest object may be.
///
/// A CEILING, NOT A PREDICTION, AND IT SAYS SO. The envelope is a few kilobytes of JSON with two
/// base64-ish strings in it; nothing about 256 KiB was measured, and its only job is that a server
/// which answers the manifest URL with a video does not become a `String` in this process.
/// `channel_art.rs:378` caps its GQL body the same way and at the same figure.
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// The most any artifact may be, before the manifest has said anything.
///
/// THE REAL CAP IS THE SIGNED `size` ON THE ARTIFACT and [`super::install::stage`] enforces it
/// exactly. This is the floor under that: it is what stops a correctly signed manifest with a
/// wrong `size` from filling a disk. The two artifacts it has to clear were measured on this
/// machine on 2026-09-11 (the release binary at 10,874,880 bytes and the gzipped data bundle at
/// 7,110,362 bytes); 128 MiB is a ceiling above both and is not a prediction of anything.
///
/// # IT IS ENFORCED IN TWO PLACES AND THE FIRST ONE IS THE ONE THAT MATTERS
///
/// [`super::manifest::judge`] refuses an artifact whose signed `size` is larger than this, with
/// [`super::Refusal::ArtifactTooLarge`], BEFORE a byte is requested. That is the check this
/// comment used to claim and did not have: the constant was only ever applied as `ureq`'s body
/// limit at [`Https::artifact`], which is enforced while bytes stream, so a signed manifest
/// carrying a `size` of ten terabytes wrote the whole 128 MiB to the reader's disk on every check
/// before failing. The body limit stays as the second lock, for the case the manifest is honest
/// about `size` and the server is not.
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

/// How long a single artifact download may take in total.
///
/// A POLICY, NOT A MEASUREMENT, AND IT IS NOT DRESSED AS ONE. [`crate::watcher::HTTP_TIMEOUT`] is
/// ten seconds and is right for a manifest; applying it to a ten megabyte download would fail
/// every real transfer at ten seconds, which is the one mistake this constant exists to stop.
/// Nothing about thirty minutes was measured, because nothing here can measure the reader's
/// connection. Its only job is that a connection which stops giving bytes does not leave the
/// UPDATES section saying WORKING until the app is closed: the download is abandoned, the partial
/// file is deleted by [`super::install::stage`], and the next check starts over. If it ever needs
/// a real number, measure the shipped artifact over the slowest connection a reader actually has
/// and put that measurement here.
pub const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// Is this channel name safe to put in a URL?
///
/// THE CHANNEL IS THE ONE PART OF THE MANIFEST URL A PERSON CAN EDIT, and they edit it by hand in
/// `settings.json` as well as through the two buttons on the Settings screen. Lowercase letters,
/// digits and a hyphen is every channel this pipeline will ever publish, and refusing the rest is
/// what stops `../../` or a whole second URL being interpolated into [`manifest_url`]. The same
/// guard, for the same reason, as `watcher::valid_handle` (`watcher.rs:174`).
pub fn channel_is_safe(channel: &str) -> bool {
    !channel.is_empty()
        && channel.len() <= 32
        && channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Where the signed envelope for a channel lives.
pub fn manifest_url(channel: &str) -> String {
    format!("{HOST}/grimoire/{channel}/latest.json")
}

/// Getting bytes from a URL, as a trait so the worker can be driven without a network.
///
/// TWO METHODS AND NOT ONE, because the two calls want opposite things. A manifest is small, is
/// wanted whole, and has a short deadline. An artifact is ten megabytes, must never become a
/// `Vec` (see [`super::verify::copy_sealed`]), and needs a deadline long enough to actually
/// transfer it.
pub trait Wire: Send {
    /// The manifest object, whole, capped at [`MAX_MANIFEST_BYTES`].
    fn manifest(&self, url: &str) -> Result<String, String>;
    /// A reader over the artifact body, capped at [`MAX_ARTIFACT_BYTES`]. The caller streams it
    /// straight to disk and never holds it.
    fn artifact(&self, url: &str) -> Result<Box<dyn Read>, String>;
}

/// The shipped agent.
///
/// ONE AGENT FOR BOTH CALLS, with the download overriding the timeout per request. Two agents
/// would be two `user_agent` strings and two `http_status_as_error` settings to keep in step, and
/// keeping two copies of a decision in step is how the one that matters drifts.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(crate::watcher::HTTP_TIMEOUT))
        .user_agent(crate::watcher::USER_AGENT)
        /* A 404 IS THE ABSENCE OF A MANIFEST AND NOT A MANIFEST. See the module note: this is the
         * opposite of `twitch_auth::Https` and deliberately so. Without it a 404 body, or a
         * proxy's error page, would be handed to `verify::open` as if it were a document, and
         * while the signature gate would refuse it, the refusal a person reads would be
         * "not a signed manifest" rather than "the server has nothing there". */
        .http_status_as_error(true)
        /* A REDIRECT IS SOMEBODY ELSE'S ADDRESS, AND THIS CLIENT ONLY HAS ONE.
         *
         * `ureq` follows redirects by default. [`super::manifest::judge`] refuses an artifact url
         * that is not https under [`HOST`], and that refusal would be worth very little if the
         * first hop were then allowed to hand the transfer to anywhere at all: the manifest url
         * would pass the check and a 302 would move the download to another host, which is the
         * same outcome by a different route. Zero redirects turns a 3xx into a status error, which
         * `http_status_as_error` above makes a refusal rather than a document.
         *
         * The cost is that the bucket may never be moved by redirect. That is the intended cost:
         * the host is a compiled-in constant for exactly the same reason, and moving it is a
         * release. */
        .max_redirects(0)
        .build()
        .new_agent()
}

/// The production wire.
pub struct Https {
    agent: ureq::Agent,
}

impl Default for Https {
    fn default() -> Self {
        Https { agent: agent() }
    }
}

impl Wire for Https {
    fn manifest(&self, url: &str) -> Result<String, String> {
        let mut resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(MAX_MANIFEST_BYTES)
            .read_to_string()
            .map_err(|e| format!("reading {url} body: {e}"))
    }

    fn artifact(&self, url: &str) -> Result<Box<dyn Read>, String> {
        let resp = self
            .agent
            .get(url)
            /* THE TEN SECOND GLOBAL TIMEOUT IS RIGHT FOR A MANIFEST AND WRONG FOR TEN MEGABYTES.
             * Overridden per request rather than by a second agent, so there is still exactly one
             * place that decides the user agent and the 4xx behaviour. The connect phase keeps
             * the short deadline, because a server that will not answer at all should be given up
             * on in seconds whatever is being fetched. */
            .config()
            .timeout_global(Some(DOWNLOAD_DEADLINE))
            .timeout_connect(Some(crate::watcher::HTTP_TIMEOUT))
            .build()
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        Ok(Box::new(
            resp.into_body()
                .into_with_config()
                .limit(MAX_ARTIFACT_BYTES)
                .reader(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEFECT THIS PREVENTS: THE SEAM AND THE PRODUCTION WIRE DISAGREEING, WHICH HAS SHIPPED HERE.
    ///
    /// `twitch_auth.rs:142-155` records the whole of it: a `Fake` answered a 400 with a body, the
    /// real `ureq` agent discarded 4xx bodies, every unit test was green, and the sign-in was
    /// refused on its first poll five seconds in. The defence that was added there was a test
    /// against the PRODUCTION agent's configuration, and this is the same test facing the other
    /// way, because this wire needs the opposite answer.
    ///
    /// The user agent is asserted too, and not as decoration: it is what the update host's logs
    /// identify this client by, and a request that arrives with `ureq/3.4.0` on it is a request
    /// nobody can attribute.
    ///
    /// AND IT DOES NOT FOLLOW A REDIRECT OFF THE UPDATE HOST. `judge` refuses an artifact url that
    /// is not https under [`HOST`], and a 302 is that refusal being walked around: the url in the
    /// document passes the check and the bytes come from somewhere else. `ureq`'s default is to
    /// follow, so this is a setting that has to be made rather than one that has to be kept.
    ///
    /// WHAT MUTATION MAKES THIS RED: `.http_status_as_error(false)` in [`agent`], which would hand
    /// a 404 error page to the signature gate as though it were a document; dropping the
    /// `.user_agent(..)` line; or dropping `.max_redirects(0)`.
    #[test]
    fn the_production_wire_refuses_a_missing_manifest_instead_of_parsing_it() {
        let w = Https::default();
        assert_eq!(
            w.agent.config().max_redirects(),
            0,
            "the wire follows redirects, so a signed manifest naming an address on the update \
             host could still hand the download to any server that answers with a 302"
        );
        assert!(
            w.agent.config().http_status_as_error(),
            "the wire hands a 404 body back as a success, so an error page would reach the \
             signature gate and be reported as 'not a signed manifest' rather than as 'there is \
             nothing published there'"
        );
        let ua = format!("{:?}", w.agent.config().user_agent());
        assert!(
            ua.contains(crate::watcher::USER_AGENT),
            "the wire does not identify itself as {}; it sent {ua} instead, which the update \
             host's logs cannot attribute to this app",
            crate::watcher::USER_AGENT
        );
    }

    /// DEFECT THIS PREVENTS: A CHANNEL NAME OUT OF A HAND-EDITED SETTINGS FILE REACHING A URL.
    ///
    /// `settings.json` is a file the owner edits, and `updater.channel` is a string in it. Without
    /// this guard a value of `../../../evil.example.com/x` is pasted straight into
    /// [`manifest_url`], and the client fetches its trust decisions from somewhere nobody chose.
    /// The signature gate would still refuse whatever came back, which is exactly why this is the
    /// second lock and not the only one.
    ///
    /// WHAT MUTATION MAKES THIS RED: let `channel_is_safe` return `true` for everything, or drop
    /// the `b == b'-'` clause (which refuses the real `pre-release` style name and would turn a
    /// legitimate channel into a permanent refusal).
    #[test]
    fn a_channel_name_may_not_carry_a_url_into_the_manifest_address() {
        for good in ["stable", "beta", "pre-release", "x1"] {
            assert!(
                channel_is_safe(good),
                "{good} is a channel this may publish"
            );
        }
        for bad in [
            "",
            "..",
            "../../etc",
            "stable/../beta",
            "Stable",
            "sta ble",
            "https://evil.example",
            "stable?x=1",
        ] {
            assert!(
                !channel_is_safe(bad),
                "{bad:?} was accepted as a channel name and would be interpolated into the \
                 manifest URL"
            );
        }
        assert_eq!(
            manifest_url("stable"),
            "https://updates.ragnarok.systems/grimoire/stable/latest.json",
            "the manifest address is what the release pipeline uploads to; if these two ever \
             disagree the client checks an address nothing publishes"
        );
    }
}
