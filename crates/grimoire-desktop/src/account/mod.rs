//! THE RAGNAROK ACCOUNT SIGN-IN, WHICH IS HOW THIS APP LEARNS WHO IS USING IT WITHOUT HOLDING A
//! SINGLE PROVIDER CREDENTIAL.
//!
//! The roadmap board needs to know who is voting, and an account is signed in with Twitch, YouTube,
//! GitHub or Discord. This app's source is public, so it cannot carry a client secret for any of
//! them, and it must never see a password. The goal doc (`docs/RAGNAROK-ACCOUNTS-SIGN-IN-GOAL.md`,
//! "The contract between the Worker and the app") is the API this module builds against, word for
//! word; nothing here invents an endpoint or a field.
//!
//! THE SHAPE IS RFC 8252, A NATIVE APP SIGNING IN THROUGH THE SYSTEM BROWSER:
//!
//!   1. A random PKCE verifier (RFC 7636) and a random state. The challenge is the verifier's
//!      SHA-256 in base64url, which is the only method the website accepts.
//!   2. A listener on `127.0.0.1:0`, never `0.0.0.0`: see [`loopback`].
//!   3. The system browser opens `BASE/app?challenge&port&state`. The website signs the owner in
//!      with whichever provider they pick and then ASKS, every time, whether to sign Grimoire in.
//!      It never approves on its own, because any local program can open that URL.
//!   4. On Continue it redirects to `http://127.0.0.1:P/callback?code&state`, and the listener
//!      takes the code only if the state matches exactly.
//!   5. The code and the verifier are exchanged at `BASE/api/app/token` for an app token. A code
//!      stolen off the loopback is worthless without the verifier, which never left this process.
//!   6. The token is sealed with DPAPI beside `settings.json`, checked with `/api/me` at startup,
//!      and revoked with `/api/app/sign-out`.
//!
//! WHAT IS NEVER PRINTED. [`AppToken`] and [`Hidden`] (the verifier, the state and the code) have
//! hand written `Debug` impls that redact, exactly as `twitch_auth::Tokens` does, and a refusal
//! quotes the service's one `error` word and never its body.
//!
//! NO NETWORK IN THE TESTS, AND A REAL SOCKET ANYWAY. Every request goes through [`Wire`], so the
//! protocol is driven by the contract's own JSON and `cargo test` dials nothing outside this
//! machine. The loopback listener in the tests IS real, on 127.0.0.1, with a test client playing
//! the browser: the listener is the part with the most ways to be wrong and a fake of it would
//! prove nothing about it. The one live check is `tests/live_account.rs`, `#[ignore]`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use sha2::{Digest, Sha256};

pub mod loopback;
pub mod session;

pub use loopback::{Loopback, Waited, WAIT_FOR_BROWSER};
pub use session::{sign_in, Account, AccountView, Ask, Browser, Config, Ended, WireMaker};

/// Where the account service lives. Every path in the contract hangs off this.
pub const BASE_URL: &str = "https://ragnarok.systems/account";

/// The environment variable that points the app at another account service, for `wrangler dev`
/// and the end to end check. See [`base_url`] for what it may say.
pub const BASE_ENV: &str = "GRIMOIRE_ACCOUNT_BASE";

/// How long one request to the account service may take, end to end. The same budget
/// `twitch_auth::Https` gives Twitch.
pub const WIRE_TIMEOUT: Duration = Duration::from_secs(15);

/* ------------------------------------------------------------------ the base -- */

/// WHICH ACCOUNT SERVICE TO TALK TO, given what `GRIMOIRE_ACCOUNT_BASE` says (or `None`).
///
/// AN OVERRIDE IS A PLACE THE TOKEN GOES, SO IT IS NARROW. Whatever this returns receives the
/// sign-in code, the verifier, and on every launch the app token itself. So an override is taken
/// only when it is `https` (any host: a staging Worker is a real deployment), or plain `http` on
/// `localhost` or `127.0.0.1` (the developer's own `wrangler dev`). Everything else is refused,
/// including the lookalikes: `localhost.example.com`, `127.0.0.1.nip.io`, `localhost@example.com`,
/// and anything with a query, a fragment or credentials in it.
///
/// Empty counts as unset. A trailing slash is dropped so `BASE/app` never becomes `BASE//app`.
pub fn base_url(env: Option<&str>) -> Result<String, String> {
    let raw = match env {
        None | Some("") => return Ok(BASE_URL.to_owned()),
        Some(v) => v,
    };
    let refuse = |why: &str| -> Result<String, String> {
        Err(format!(
            "{BASE_ENV} was not used ({why}). Only https, or http on localhost or 127.0.0.1, may \
             stand in for the account service"
        ))
    };
    if raw
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '?' | '#' | '@' | '\\'))
    {
        return refuse("it carries a character a base address cannot");
    }
    let Some((scheme, rest)) = raw.split_once("://") else {
        return refuse("it has no scheme");
    };
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return refuse("its host is not a plain name");
    }
    if let Some(p) = port {
        if !p.bytes().all(|b| b.is_ascii_digit()) || !matches!(p.parse::<u16>(), Ok(n) if n > 0) {
            return refuse("its port is not a port");
        }
    }
    let local = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    match scheme.to_ascii_lowercase().as_str() {
        "https" => {}
        "http" if local => {}
        _ => return refuse("it is not https, and plain http is only allowed on this computer"),
    }
    Ok(raw.trim_end_matches('/').to_owned())
}

/// The account service this process uses: the override if it passes [`base_url`], otherwise the
/// real one. A refused override is logged and IGNORED rather than obeyed, because obeying it would
/// hand a token to whatever it named.
pub fn base() -> String {
    match std::env::var(BASE_ENV) {
        Ok(v) => base_url(Some(&v)).unwrap_or_else(|e| {
            log::warn!("{e}");
            BASE_URL.to_owned()
        }),
        Err(_) => BASE_URL.to_owned(),
    }
}

/// The desktop sign-in page, with the three values the contract asks for. All three are already
/// URL safe (base64url and digits), so nothing needs escaping.
pub fn app_url(base: &str, challenge: &str, port: u16, state: &Hidden) -> String {
    format!(
        "{base}/app?challenge={challenge}&port={port}&state={}",
        state.expose()
    )
}

/* ----------------------------------------------------------------- the secrets -- */

/// A STRING THAT MUST NEVER BE PRINTED: the PKCE verifier, the state, and the sign-in code.
///
/// `Debug` redacts, there is no `Display`, and the only way to the bytes is `expose`, which is
/// crate private so the value cannot be copied out by a caller that does not already own it.
#[derive(Clone)]
pub struct Hidden(String);

impl Hidden {
    pub(crate) fn new(s: String) -> Hidden {
        Hidden(s)
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// THE APP TOKEN: `rsa_` and 32 random bytes in base64url. It signs this app in as the owner's
/// account for 90 days, sliding, until it is revoked.
///
/// REDACTED BY HAND, FOR THE REASON `twitch_auth::Tokens` GIVES. A `#[derive(Debug)]` here would
/// put a bearer credential into any log line, panic message or `dbg!` that touched it.
/// `an_app_token_cannot_be_printed_by_accident` fails the day a derive comes back.
#[derive(Clone)]
pub struct AppToken(String);

impl AppToken {
    /// A token as the contract mints it, or `None`.
    ///
    /// CHECKED AND NOT MERELY NON-EMPTY, because this string goes into an `Authorization` header
    /// and onto disk. A server that answered with a CR or LF in it would otherwise be writing
    /// headers of its own into every later request.
    pub(crate) fn parse(s: &str) -> Option<AppToken> {
        let rest = s.strip_prefix("rsa_")?;
        let ok = !rest.is_empty()
            && s.len() <= 512
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        ok.then(|| AppToken(s.to_owned()))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AppToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AppToken").field(&"<redacted>").finish()
    }
}

/// Thirty two bytes from the operating system's generator, in base64url: 43 characters.
///
/// THE GENERATOR IS THE ONE RUSTLS ALREADY USES. `ring` is compiled into this binary for TLS and
/// its `SecureRandom` is the OS source (`ProcessPrng` on Windows, `getrandom` on Linux), so the
/// sign-in names no randomness crate of its own. See the `base64` note in `Cargo.toml`.
fn random_b64() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| {
            "this computer would not supply random bytes, so a sign-in cannot be started \
             safely"
                .to_owned()
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The S256 challenge for a verifier: base64url, no padding, of its SHA-256 (RFC 7636 4.2).
pub fn challenge_for(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// One sign-in's PKCE pair and state. Fresh for every attempt, never reused, never saved.
#[derive(Debug)]
pub struct Pkce {
    /// Sent once, to the token endpoint, and nowhere else. Never in a URL.
    pub verifier: Hidden,
    /// Goes to the browser. Public by design: it proves nothing without the verifier.
    pub challenge: String,
    /// Goes to the browser and must come back byte for byte.
    pub state: Hidden,
}

impl Pkce {
    pub fn new() -> Result<Pkce, String> {
        let verifier = random_b64()?;
        Ok(Pkce {
            challenge: challenge_for(&verifier),
            verifier: Hidden::new(verifier),
            state: Hidden::new(random_b64()?),
        })
    }
}

/// Equal, in time that depends on the length and not on where the first difference is.
///
/// FOR THE STATE. The length of a state is not a secret (the contract says 16 to 128 characters and
/// this app always makes 43), so an early return on a length mismatch leaks nothing.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/* ----------------------------------------------------------------- the store -- */

/// Where the sealed app token lives: `%APPDATA%\\eql-grimoire\\account-token.bin`.
///
/// BESIDE `twitch-token.bin` AND NOT INSIDE IT, and sealed with a different entropy
/// (`secret::Purpose::AccountToken`), so one sign-out never touches the other sign-in and neither
/// file can be swapped for the other.
pub fn token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(crate::settings::APP_DIR).join("account-token.bin"))
}

/// Seal and write the token. Off Windows `seal_for` refuses and NOTHING is written: the Settings
/// screen says the sign-in lasts until the app closes. See `secret`'s note on why this refuses
/// rather than falling back to plaintext.
pub fn save_token(path: Option<&Path>, t: &AppToken) {
    let Some(path) = path else { return };
    let Some(sealed) =
        crate::secret::seal_for(crate::secret::Purpose::AccountToken, t.expose().as_bytes())
    else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, &sealed) {
        log::warn!("the account sign-in could not be saved, so it will not survive a restart: {e}");
    }
}

/// The saved token, or `None` for every reason there is: no file, no DPAPI, a blob for another
/// user, machine or purpose, a torn write, or bytes that are not a token.
pub fn load_token(path: Option<&Path>) -> Option<AppToken> {
    let sealed = std::fs::read(path?).ok()?;
    let plain = crate::secret::unseal_for(crate::secret::Purpose::AccountToken, &sealed)?;
    AppToken::parse(std::str::from_utf8(&plain).ok()?)
}

/// Remove the saved sign-in.
pub fn forget_token(path: Option<&Path>) {
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}

/* ------------------------------------------------------------------ the wire -- */

/// SOMETHING THAT CAN MAKE THE CONTRACT'S THREE KINDS OF REQUEST AND HAND BACK STATUS AND BODY.
///
/// The status is separate from the body for the reason `twitch_auth::Wire` gives: `invalid_grant`
/// and `unauthenticated` are 4xx answers with meaning, not transport failures.
pub trait Wire: Send {
    /// `POST` a JSON body, unauthenticated. The token exchange.
    fn post_json(&mut self, url: &str, json: &str) -> Result<(u16, String), String>;
    /// `GET` with `Authorization: Bearer <token>`. `/api/me`.
    fn get_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String>;
    /// `POST` with no body and `Authorization: Bearer <token>`. `/api/app/sign-out`.
    fn post_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String>;
}

/// The production wire, on the `ureq` agent this crate already carries.
pub struct Https {
    agent: ureq::Agent,
}

impl Default for Https {
    fn default() -> Self {
        Https {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(WIRE_TIMEOUT))
                /* THE 4xx BODY IS THE PROTOCOL. `invalid_grant` is a 400 with a JSON body, and
                 * `ureq`'s default turns a 400 into an error and throws the body away, which is
                 * the defect `twitch_auth::Https` shipped and wrote up at length. */
                .http_status_as_error(false)
                /* NO REDIRECTS. The contract's JSON API never redirects, and a bearer token
                 * following a 3xx to wherever it points is a token going somewhere nobody chose.
                 * A 3xx comes back as its status and is refused like any other surprise. */
                .max_redirects(0)
                .build()
                .new_agent(),
        }
    }
}

fn answer(
    r: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, String), String> {
    match r {
        Ok(mut r) => {
            let code = r.status().as_u16();
            let body = r.body_mut().read_to_string().unwrap_or_default();
            Ok((code, body))
        }
        Err(ureq::Error::StatusCode(code)) => Ok((code, String::new())),
        Err(e) => Err(format!("{e}")),
    }
}

impl Wire for Https {
    fn post_json(&mut self, url: &str, json: &str) -> Result<(u16, String), String> {
        answer(
            self.agent
                .post(url)
                .content_type("application/json")
                .send(json),
        )
    }

    fn get_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
        answer(
            self.agent
                .get(url)
                .header("Authorization", &format!("Bearer {token}"))
                .call(),
        )
    }

    fn post_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
        answer(
            self.agent
                .post(url)
                .header("Authorization", &format!("Bearer {token}"))
                .send_empty(),
        )
    }
}

/* --------------------------------------------------------------- the contract -- */

/// `ACCOUNT` from the contract: who is signed in, and with what.
///
/// ONLY THE FIELDS THIS APP DRAWS. The contract also sends `id` and each identity's `linked_at`;
/// nothing in this build reads either, and a field parsed for nobody is the defect `reach.rs`
/// exists to catch. `serde` ignores the rest, so the website can add fields without breaking this.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Profile {
    pub display_name: String,
    #[serde(default)]
    pub identities: Vec<Identity>,
}

/// One linked sign-in method on the account.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Identity {
    /// `twitch`, `google`, `github` or `discord` (or `fake` on a development Worker).
    pub provider: String,
    pub username: String,
}

/// A provider as a person knows it. `google` IS SHOWN AS YOUTUBE, which is the goal doc's decision:
/// the account is the YouTube channel's Google account, and "Google" would read as something else.
pub fn provider_label(provider: &str) -> &str {
    match provider {
        "twitch" => "Twitch",
        "google" => "YouTube",
        "github" => "GitHub",
        "discord" => "Discord",
        other => other,
    }
}

/// Why a request to the account service did not give the answer wanted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `400 {"error": "invalid_grant"}` from the token endpoint. The code is spent either way.
    InvalidGrant,
    /// `401` from `/api/me`: the token is revoked or expired. The saved one should go.
    Unauthenticated,
    /// No answer at all. NOT a refusal of the token: a laptop offline is not signed out.
    Unreachable(String),
    /// Any other answer, in words, with nothing from the body but its `error` word.
    Answered(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::InvalidGrant => f.write_str("the sign-in code was not accepted"),
            Refused::Unauthenticated => {
                f.write_str("the account service no longer accepts this sign-in")
            }
            Refused::Unreachable(e) => write!(f, "the account service could not be reached: {e}"),
            Refused::Answered(s) => f.write_str(s),
        }
    }
}

/// Trade a sign-in code and its verifier for an app token and the account it signs in.
pub fn exchange(
    w: &mut dyn Wire,
    base: &str,
    code: &Hidden,
    verifier: &Hidden,
) -> Result<(AppToken, Profile), Refused> {
    let body = serde_json::json!({ "code": code.expose(), "verifier": verifier.expose() });
    let (status, reply) = w
        .post_json(&format!("{base}/api/app/token"), &body.to_string())
        .map_err(Refused::Unreachable)?;
    if status == 400 && error_word(&reply) == "invalid_grant" {
        return Err(Refused::InvalidGrant);
    }
    if status != 200 {
        return Err(Refused::Answered(refusal(status, &reply)));
    }
    /* NO PART OF THIS BODY IS EVER QUOTED IN AN ERROR: it carries the token. */
    let v: serde_json::Value = serde_json::from_str(&reply)
        .map_err(|_| Refused::Answered("the token reply was not JSON".to_owned()))?;
    let token = v
        .get("token")
        .and_then(|t| t.as_str())
        .and_then(AppToken::parse)
        .ok_or_else(|| Refused::Answered("the token reply carried no usable token".to_owned()))?;
    let profile = v
        .get("account")
        .cloned()
        .and_then(|a| serde_json::from_value::<Profile>(a).ok())
        .ok_or_else(|| Refused::Answered("the token reply carried no account".to_owned()))?;
    Ok((token, profile))
}

/// Who this token signs in, which is also whether it still works.
pub fn me(w: &mut dyn Wire, base: &str, token: &AppToken) -> Result<Profile, Refused> {
    let (status, reply) = w
        .get_bearer(&format!("{base}/api/me"), token.expose())
        .map_err(Refused::Unreachable)?;
    match status {
        200 => serde_json::from_str::<Profile>(&reply)
            .map_err(|_| Refused::Answered("the account reply was not an account".to_owned())),
        401 => Err(Refused::Unauthenticated),
        s => Err(Refused::Answered(refusal(s, &reply))),
    }
}

/// Revoke the token on the website.
///
/// A 401 IS SUCCESS HERE. It means the token is already revoked or expired, which is the state
/// sign-out exists to reach, and calling it a failure would log a problem that is not one.
pub fn sign_out(w: &mut dyn Wire, base: &str, token: &AppToken) -> Result<(), Refused> {
    let (status, reply) = w
        .post_bearer(&format!("{base}/api/app/sign-out"), token.expose())
        .map_err(Refused::Unreachable)?;
    match status {
        200..=299 | 401 => Ok(()),
        s => Err(Refused::Answered(refusal(s, &reply))),
    }
}

/// The `error` field of a JSON body, reduced to a safe word, or empty.
fn error_word(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(safe_word))
        .unwrap_or_default()
}

/// Letters, digits and underscores, at most 64 of them. What an error code from the network is
/// allowed to put on screen or in a log.
pub(crate) fn safe_word(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(64)
        .collect()
}

/// A refusal in words: the status always, the `error` word when there is one, the body never.
fn refusal(status: u16, body: &str) -> String {
    let word = error_word(body);
    if word.is_empty() {
        format!("the account service answered {status}")
    } else {
        format!("the account service answered {status} ({word})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A wire that answers from a script and records what it was asked, bearer included.
    struct Fake {
        replies: VecDeque<(u16, String)>,
        asked: Vec<(String, String, String)>,
    }

    impl Fake {
        fn new(replies: &[(u16, &str)]) -> Fake {
            Fake {
                replies: replies.iter().map(|(c, b)| (*c, (*b).to_owned())).collect(),
                asked: Vec::new(),
            }
        }
        fn next(&mut self, url: &str, what: String) -> Result<(u16, String), String> {
            self.asked.push((url.to_owned(), what, String::new()));
            self.replies
                .pop_front()
                .ok_or_else(|| "the script ran out".to_owned())
        }
    }

    impl Wire for Fake {
        fn post_json(&mut self, url: &str, json: &str) -> Result<(u16, String), String> {
            self.next(url, format!("POST {json}"))
        }
        fn get_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
            self.next(url, format!("GET Bearer {token}"))
        }
        fn post_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
            self.next(url, format!("POST Bearer {token}"))
        }
    }

    const BASE: &str = "https://accounts.test/account";
    const TOKEN: &str = "rsa_Zm9vYmFyYmF6cXV4cXV1eHF1dXhxdXV4cXV1eHF1dXg";
    const ACCOUNT: &str = r#"{"id":"acc_01","display_name":"Broken Stoic","identities":[{"provider":"twitch","username":"broken_stoic","linked_at":"2026-09-12T10:00:00Z"}]}"#;

    /// RFC 7636 APPENDIX B, VERBATIM. If this is wrong every challenge the website receives is
    /// wrong, and every sign-in fails at the exchange with a code that "does not match".
    #[test]
    fn the_challenge_is_the_rfc_7636_appendix_b_vector() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    /// A FRESH PAIR IS WHAT THE CONTRACT ACCEPTS, AND IS NEVER THE SAME TWICE.
    ///
    /// The contract: the challenge is 43 characters, the state 16 to 128 of `[A-Za-z0-9_-]`, and
    /// RFC 7636 wants a verifier of 43 to 128 unreserved characters. A generator that returned a
    /// constant would pass every length check, so two draws are compared as well.
    #[test]
    fn a_fresh_pkce_has_the_contract_shape_and_never_repeats() {
        let url_safe = |s: &str| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        let a = Pkce::new().expect("the OS supplies random bytes");
        let b = Pkce::new().expect("the OS supplies random bytes");
        for p in [&a, &b] {
            assert_eq!(p.verifier.expose().len(), 43);
            assert_eq!(p.challenge.len(), 43);
            assert_eq!(p.state.expose().len(), 43);
            assert!(url_safe(p.verifier.expose()) && url_safe(&p.challenge));
            assert!(url_safe(p.state.expose()));
            assert_eq!(p.challenge, challenge_for(p.verifier.expose()));
        }
        assert_ne!(a.verifier.expose(), b.verifier.expose());
        assert_ne!(a.state.expose(), b.state.expose());
        assert_ne!(
            a.verifier.expose(),
            a.state.expose(),
            "the state and the verifier must be separate draws"
        );
    }

    #[test]
    fn constant_time_equality_is_still_equality() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(ct_eq(b"", b""));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"xbc"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"abc", b"ABC"));
    }

    /// THE OVERRIDE TAKES HTTPS AND THIS COMPUTER'S OWN HTTP, AND NOTHING THAT MERELY LOOKS LIKE IT.
    ///
    /// Whatever this accepts receives the verifier and, on every launch, the app token. Each
    /// refused value below is a way an earlier draft of a check like this has been fooled
    /// somewhere: a host that starts with `localhost`, credentials before an `@`, a fragment or a
    /// query that moves the real host, an IPv6 literal the host check did not expect.
    #[test]
    fn the_base_override_is_https_or_local_http_and_nothing_else() {
        assert_eq!(base_url(None).as_deref(), Ok(BASE_URL));
        assert_eq!(base_url(Some("")).as_deref(), Ok(BASE_URL));
        for (given, want) in [
            (
                "https://ragnarok.systems/account",
                "https://ragnarok.systems/account",
            ),
            (
                "https://staging.ragnarok.systems/account/",
                "https://staging.ragnarok.systems/account",
            ),
            (
                "http://localhost:8787/account",
                "http://localhost:8787/account",
            ),
            (
                "http://127.0.0.1:8787/account",
                "http://127.0.0.1:8787/account",
            ),
            ("http://localhost/account", "http://localhost/account"),
            (
                "HTTP://LOCALHOST:8787/account",
                "HTTP://LOCALHOST:8787/account",
            ),
        ] {
            assert_eq!(base_url(Some(given)).as_deref(), Ok(want), "{given}");
        }
        for refused in [
            "http://ragnarok.systems/account",
            "http://localhost.evil.example/account",
            "http://127.0.0.1.nip.io/account",
            "http://127.0.0.2:8787/account",
            "http://0.0.0.0:8787/account",
            "http://localhost@evil.example/account",
            "https://user:pw@ragnarok.systems/account",
            "http://evil.example#@localhost/account",
            "http://evil.example/?next=http://localhost",
            "http://[::1]:8787/account",
            "http://localhost:99999/account",
            "http://localhost:/account",
            "http://localhost:0/account",
            "http://localhost:87a/account",
            "ftp://localhost/account",
            "javascript://localhost/account",
            "localhost:8787/account",
            "https://",
            "https:///account",
            " https://ragnarok.systems/account",
            "https://ragnarok.systems/account\n",
            "http://localhost\\@evil.example/account",
        ] {
            assert!(
                base_url(Some(refused)).is_err(),
                "{refused:?} was accepted as the account service, so the token would go there"
            );
        }
    }

    /// THE TOKEN CANNOT BE PRINTED BY ACCIDENT, AND NEITHER CAN THE VALUES THAT BUY ONE.
    ///
    /// WHAT MUTATION MAKES THIS RED: `#[derive(Debug)]` on `AppToken` or `Hidden` in place of the
    /// hand written impls.
    #[test]
    fn an_app_token_cannot_be_printed_by_accident() {
        let t = AppToken::parse(TOKEN).expect("a contract shaped token");
        let printed = format!("{t:?}");
        assert!(
            !printed.contains(TOKEN) && !printed.contains("Zm9v"),
            "{printed}"
        );
        assert!(printed.contains("<redacted>"), "{printed}");
        assert!(!format!("{:?}", Some(t.clone())).contains("Zm9v"));

        let p = Pkce::new().expect("random");
        let printed = format!("{p:?}");
        assert!(
            !printed.contains(p.verifier.expose()) && !printed.contains(p.state.expose()),
            "the verifier or the state printed itself: {printed}"
        );
        let code = Hidden::new("THE-CODE-ITSELF".to_owned());
        assert!(!format!("{code:?}").contains("THE-CODE"));
        assert!(!format!("{:?}", Waited::Code(code)).contains("THE-CODE"));
    }

    /// A TOKEN IS WHAT THE CONTRACT MINTS AND NOTHING THAT COULD WRITE A HEADER.
    #[test]
    fn only_a_contract_shaped_token_is_accepted() {
        assert!(AppToken::parse(TOKEN).is_some());
        assert!(AppToken::parse("rsa_a-b_C9").is_some());
        for bad in [
            "",
            "rsa_",
            "abc",
            "Bearer rsa_abc",
            "rsa_abc def",
            "rsa_abc\r\nX-Evil: 1",
            "rsa_abc=",
        ] {
            assert!(
                AppToken::parse(bad).is_none(),
                "{bad:?} was taken as a token"
            );
        }
        assert!(AppToken::parse(&format!("rsa_{}", "a".repeat(600))).is_none());
    }

    /// THE EXCHANGE IS THE CONTRACT'S, FIELD FOR FIELD, AND READS THE CONTRACT'S REPLY.
    #[test]
    fn the_exchange_sends_code_and_verifier_and_reads_the_documented_reply() {
        let reply = format!(r#"{{"token":"{TOKEN}","account":{ACCOUNT}}}"#);
        let mut w = Fake::new(&[(200, &reply)]);
        let (token, profile) = exchange(
            &mut w,
            BASE,
            &Hidden::new("CODE".to_owned()),
            &Hidden::new("VERIFIER".to_owned()),
        )
        .expect("the documented reply parses");
        assert_eq!(token.expose(), TOKEN);
        assert_eq!(profile.display_name, "Broken Stoic");
        assert_eq!(
            profile.identities,
            vec![Identity {
                provider: "twitch".to_owned(),
                username: "broken_stoic".to_owned()
            }]
        );
        let (url, what, _) = &w.asked[0];
        assert_eq!(url, "https://accounts.test/account/api/app/token");
        let body: serde_json::Value =
            serde_json::from_str(what.strip_prefix("POST ").expect("a POST")).expect("JSON");
        assert_eq!(
            body,
            serde_json::json!({"code": "CODE", "verifier": "VERIFIER"})
        );
    }

    /// `invalid_grant` IS ITS OWN ANSWER, AND NO REFUSAL QUOTES MORE THAN THE ERROR WORD.
    #[test]
    fn invalid_grant_is_named_and_a_refusal_quotes_nothing_but_the_error_word() {
        let code = Hidden::new("C".to_owned());
        let verifier = Hidden::new("V".to_owned());
        let mut w = Fake::new(&[(400, r#"{"error":"invalid_grant"}"#)]);
        assert_eq!(
            exchange(&mut w, BASE, &code, &verifier).map(|_| ()),
            Err(Refused::InvalidGrant)
        );

        /* A 500 whose body carries a token must not repeat it. */
        let leaky = format!(r#"{{"error":"server<b>_error","token":"{TOKEN}"}}"#);
        let mut w = Fake::new(&[(500, &leaky)]);
        match exchange(&mut w, BASE, &code, &verifier) {
            Err(Refused::Answered(s)) => {
                assert!(s.contains("500") && s.contains("serverb_error"), "{s}");
                assert!(!s.contains("Zm9v") && !s.contains('<'), "{s}");
            }
            other => panic!("a 500 read as {:?}", other.map(|_| ())),
        }

        /* A 200 with a token that could write a header is not a sign-in. */
        let crlf = format!(r#"{{"token":"rsa_x\r\nX: y","account":{ACCOUNT}}}"#);
        let mut w = Fake::new(&[(200, &crlf)]);
        assert!(matches!(
            exchange(&mut w, BASE, &code, &verifier),
            Err(Refused::Answered(_))
        ));
    }

    /// `/me` SAYS 401 AS ITS OWN ANSWER, AND BOTH AUTHENTICATED CALLS CARRY THE BEARER.
    #[test]
    fn me_names_a_revoked_token_and_sign_out_carries_the_bearer() {
        let t = AppToken::parse(TOKEN).expect("token");
        let mut w = Fake::new(&[
            (200, ACCOUNT),
            (401, r#"{"error":"unauthenticated"}"#),
            (204, ""),
            (401, ""),
            (503, ""),
        ]);
        assert_eq!(
            me(&mut w, BASE, &t).map(|p| p.display_name),
            Ok("Broken Stoic".to_owned())
        );
        assert_eq!(me(&mut w, BASE, &t), Err(Refused::Unauthenticated));
        assert_eq!(sign_out(&mut w, BASE, &t), Ok(()));
        assert_eq!(
            sign_out(&mut w, BASE, &t),
            Ok(()),
            "already revoked is signed out"
        );
        assert!(matches!(
            sign_out(&mut w, BASE, &t),
            Err(Refused::Answered(_))
        ));
        assert_eq!(w.asked[0].0, "https://accounts.test/account/api/me");
        assert_eq!(w.asked[0].1, format!("GET Bearer {TOKEN}"));
        assert_eq!(
            w.asked[2].0,
            "https://accounts.test/account/api/app/sign-out"
        );
        assert_eq!(w.asked[2].1, format!("POST Bearer {TOKEN}"));

        struct Dead;
        impl Wire for Dead {
            fn post_json(&mut self, _: &str, _: &str) -> Result<(u16, String), String> {
                Err("offline".to_owned())
            }
            fn get_bearer(&mut self, _: &str, _: &str) -> Result<(u16, String), String> {
                Err("offline".to_owned())
            }
            fn post_bearer(&mut self, _: &str, _: &str) -> Result<(u16, String), String> {
                Err("offline".to_owned())
            }
        }
        assert!(
            matches!(me(&mut Dead, BASE, &t), Err(Refused::Unreachable(_))),
            "offline must never read as a revoked token, or a laptop on a train signs itself out"
        );
    }

    /// THE PRODUCTION WIRE KEEPS REFUSAL BODIES AND FOLLOWS NO REDIRECT.
    ///
    /// The seam cannot catch either: `Fake` always has a body and never redirects. Deleting
    /// `http_status_as_error(false)` or `max_redirects(0)` makes this red.
    #[test]
    fn the_wire_keeps_refusal_bodies_and_follows_no_redirects() {
        let w = Https::default();
        assert!(!w.agent.config().http_status_as_error());
        assert_eq!(w.agent.config().max_redirects(), 0);
    }

    /// THE ACCOUNT TOKEN HAS ITS OWN FILE, BESIDE THE TWITCH ONE AND NOT IN IT.
    #[test]
    fn the_account_token_file_is_not_the_twitch_token_file() {
        let (Some(mine), Some(twitch)) = (token_path(), crate::twitch_auth::token_path()) else {
            return; /* no config directory on this machine: nothing to compare */
        };
        assert_eq!(
            mine.file_name().and_then(|n| n.to_str()),
            Some("account-token.bin")
        );
        assert_ne!(mine, twitch);
        assert_eq!(mine.parent(), twitch.parent());
    }

    /// A saved token is sealed, comes back, and a blob sealed for Twitch does not load as one.
    #[cfg(windows)]
    #[test]
    fn a_saved_token_is_sealed_comes_back_and_a_twitch_blob_is_not_one() {
        let p = std::env::temp_dir().join(format!("eqlg-account-store-{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&p);
        save_token(Some(&p), &AppToken::parse(TOKEN).expect("token"));
        let raw = std::fs::read(&p).expect("written");
        assert!(
            !raw.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes()),
            "the token is on disk in the clear"
        );
        assert_eq!(load_token(Some(&p)).map(|t| t.0), Some(TOKEN.to_owned()));
        std::fs::write(&p, crate::secret::seal(TOKEN.as_bytes()).expect("DPAPI")).expect("write");
        assert!(
            load_token(Some(&p)).is_none(),
            "a blob sealed for the Twitch token loaded as an account token"
        );
        forget_token(Some(&p));
        assert!(!p.exists());
    }
}
