//! THE DEVICE CODE SIGN-IN, WHICH IS HOW THIS APP GETS PERMISSION TO SPEAK WITHOUT EVER SEEING A
//! PASSWORD.
//!
//! Reading Twitch chat needs no account at all (`crate::chat` logs in as `justinfan<random>` with
//! no password and is answered). SENDING needs a user token, and there are three ways to get one.
//! Two of them are refused here on principle and one is built:
//!
//!   1. A login form in this app. Never. The app would see the password, which is the one thing it
//!      must never do, and no amount of care makes that safe.
//!   2. Reading the session out of the webview's cookie jar after the Watch screen's sign-in. The
//!      cookies are right there and this would work. It is still scraping somebody's auth out of a
//!      store that was not offered to us, and a token taken that way carries whatever scopes the
//!      web client has rather than the two this app asked for.
//!   3. THE DEVICE CODE GRANT FLOW, which Twitch documents for exactly this shape of program. The
//!      app asks for a code, the owner types it on `twitch.tv/activate`, Twitch shows them the two
//!      permissions being requested, and the app is handed a token afterwards. The password is
//!      typed on Twitch's own page in the owner's own browser and this process never learns it.
//!
//! WHAT THIS FILE HOLDS AND WHAT IT REFUSES TO HOLD. It holds the app's public
//! [`settings::TWITCH_CLIENT_ID`], which Twitch's own guide says may be embedded in a web page's
//! source. It never holds, and this crate never registers, a client SECRET: the app is a Public
//! client, and for a Public client Twitch's refresh endpoint says `client_secret` "is not required".
//! A secret compiled into a desktop binary is not secret from anybody holding the binary, so the
//! honest thing is not to have one. The cost is in [`Tokens::refresh`]'s note.
//!
//! THE TOKEN IS NEVER PRINTED. [`Tokens`] has a hand written `Debug` that redacts both strings, so
//! a `{:?}` in a log line or a panic message cannot leak the thing that speaks as the owner.
//! `a_token_cannot_be_printed_by_accident` is what holds that.
//!
//! NO NETWORK IN THE TESTS. Every call goes through [`Wire`], the same seam `chat::Wire` is, so the
//! whole protocol is driven in tests by the exact JSON Twitch's documentation prints, and
//! `cargo test` dials nothing. The one live check lives in `tests/live_twitch.rs`, is `#[ignore]`,
//! and stops at the device request, which needs no human.

use std::fmt;
use std::time::Duration;

/// Where the sealed refresh token lives: `%APPDATA%\\eql-grimoire\\twitch-token.bin`.
///
/// BESIDE `settings.json` AND DELIBERATELY NOT IN IT. Settings is plain JSON that a person is
/// invited to open and edit, and everything in it is inert. A bearer token is not inert, so it
/// gets its own file, sealed, and nothing that reads settings ever touches it.
pub fn token_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join(crate::settings::APP_DIR).join("twitch-token.bin"))
}

/// ONLY THE REFRESH TOKEN IS KEPT, AND THE ACCESS TOKEN NEVER IS.
///
/// An access token lasts about four hours and this file has no idea how long ago it was written,
/// because what Twitch returns is a DURATION and not an expiry instant. Reloading one would mean
/// either trusting a value that has probably lapsed, or writing a wall clock time and trusting
/// the machine's clock not to have moved. Both are worse than the third option: keep the durable
/// half, and spend one request at startup turning it into a fresh pair. `restore` does that, so
/// the app is never holding an access token whose age it cannot account for.
fn save(path: Option<&std::path::Path>, t: &Tokens) {
    let Some(path) = path else { return };
    let Some(sealed) = crate::secret::seal(t.refresh.as_bytes()) else {
        /* No DPAPI here, so nothing is written. The screen says the sign-in lasts until the app
         * closes; see `secret`'s note on why this refuses rather than falling back to plain. */
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, &sealed) {
        log::warn!("the sign-in could not be saved, so it will not survive a restart: {e}");
    }
}

/// The saved refresh token, or `None` for every reason there is: no file, no DPAPI, a blob from
/// another user or machine, or a torn write. All of them mean the same thing to the caller.
fn load(path: Option<&std::path::Path>) -> Option<String> {
    let sealed = std::fs::read(path?).ok()?;
    let plain = crate::secret::unseal(&sealed)?;
    let s = String::from_utf8(plain).ok()?;
    (!s.is_empty()).then_some(s)
}

/// Remove the saved sign-in. Called when Twitch refuses it, so a token known to be dead is not
/// left on disk to be retried on every launch forever.
fn forget(path: Option<&std::path::Path>) {
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}

/// Where a device authorisation starts.
pub const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
/// Where a device code, or a refresh token, is exchanged for an access token.
pub const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
/// Where a token is asked who it belongs to.
pub const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";

/// How long before an access token lapses to go and renew it.
pub const RENEW_MARGIN: Duration = Duration::from_secs(120);

/// The shortest wait between renewals, whatever the token says.
///
/// A FLOOR AND NOT A NICETY. `expires_in` is a number off the wire; a token that arrived saying
/// it lasted thirty seconds, or zero, would give a negative wait, which saturates to nothing, and
/// the renew loop would become a tight loop hammering Twitch's token endpoint until it rate
/// limited the app. `a_short_lived_token_does_not_become_a_tight_loop` is the guard.
pub const RENEW_FLOOR: Duration = Duration::from_secs(60);

/// How long to wait before renewing a token that lasts `expires_in`.
pub fn renew_after(expires_in: Duration) -> Duration {
    expires_in.saturating_sub(RENEW_MARGIN).max(RENEW_FLOOR)
}

/// The RFC 8628 grant type, spelled exactly as Twitch requires.
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// How long to wait between polls when Twitch does not say. Their own example answers `5`.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// SOMETHING THAT CAN POST A FORM AND HAND BACK THE STATUS AND THE BODY.
///
/// The seam exists so the flow above can be driven by the documented responses rather than by a
/// live service. It carries the status separately from the body because this protocol's "not yet"
/// is a 400 with a message in it, not a transport failure, and a wire that collapsed the two would
/// make `authorization_pending` indistinguishable from a dropped connection.
pub trait Wire: Send {
    fn post_form(&mut self, url: &str, form: &[(&str, &str)]) -> Result<(u16, String), String>;
    /// A GET carrying `Authorization: OAuth <token>`, which is the ONE shape Twitch's validate
    /// endpoint takes. Note `OAuth` and not `Bearer`: the id service is older than the API and
    /// answers 401 to a Bearer prefix.
    fn get_authed(&mut self, url: &str, token: &str) -> Result<(u16, String), String>;
}

/// The production wire. `ureq`, which this crate already carries for the live poller.
pub struct Https {
    agent: ureq::Agent,
}

impl Default for Https {
    fn default() -> Self {
        Https {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(15)))
                /* THE 4xx BODY IS THE PROTOCOL AND MUST NOT BE THROWN AWAY.
                 *
                 * THIS ONE LINE IS THE WHOLE FLOW. `ureq` defaults `http_status_as_error` to
                 * true, which turns a 400 into `Err(Error::StatusCode)` AND DISCARDS THE BODY.
                 * This protocol says "the owner has not finished typing" with a 400 whose body
                 * is `{"status":400,"message":"authorization_pending"}`, so without this the
                 * reader sees a bodyless 400, fails to match the message, and refuses the sign-in
                 * on its FIRST poll, five seconds in, before the code has even been read off the
                 * screen. The owner saw exactly that: "Sign-in failed: Twitch answered 400" with
                 * no message, because there was none.
                 *
                 * THE UNIT TESTS COULD NOT CATCH IT. `Fake` answers (400, PENDING) WITH a body,
                 * which is what a wire is supposed to do, so `poll` was proved against a response
                 * the real wire never produced. A seam is only as good as the production side
                 * agreeing with it. `the_wire_keeps_the_body_of_a_refusal` checks the config, and
                 * `the_production_wire_reads_a_pending_authorisation` in tests/live_twitch.rs
                 * checks the real thing against the real service. */
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }
}

impl Wire for Https {
    fn post_form(&mut self, url: &str, form: &[(&str, &str)]) -> Result<(u16, String), String> {
        /* THE ERROR BODY IS WANTED, NOT JUST THE ERROR. `authorization_pending` arrives as a 400
         * and is the NORMAL state of this flow for as long as the owner is typing, so a wire that
         * threw away 4xx bodies would turn every poll into an unexplained failure. */
        match self.agent.post(url).send_form(form.to_vec()) {
            Ok(mut r) => {
                let code = r.status().as_u16();
                let body = r.body_mut().read_to_string().unwrap_or_default();
                Ok((code, body))
            }
            Err(ureq::Error::StatusCode(code)) => Ok((code, String::new())),
            Err(e) => Err(format!("{e}")),
        }
    }

    fn get_authed(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
        match self
            .agent
            .get(url)
            .header("Authorization", &format!("OAuth {token}"))
            .call()
        {
            Ok(mut r) => {
                let code = r.status().as_u16();
                let body = r.body_mut().read_to_string().unwrap_or_default();
                Ok((code, body))
            }
            Err(ureq::Error::StatusCode(code)) => Ok((code, String::new())),
            Err(e) => Err(format!("{e}")),
        }
    }
}

/// What the owner has to do, and how long they have to do it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The eight characters to type. Shown on screen in full: it authorises nothing on its own and
    /// is useless to anybody who is not already signed in to this account.
    pub user_code: String,
    /// Twitch's page to type it on, with the code already in the query.
    pub verification_uri: String,
    /// The handle the app polls with. NOT drawn: it is the half that becomes a token.
    pub device_code: String,
    /// How long to wait between polls. Twitch's own answer, not a number this app chose.
    pub interval: Duration,
    /// How long the code is good for. Their example says 1800 seconds.
    pub expires_in: Duration,
}

/// The token pair. See the module note on why this cannot be printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    /// Speaks as the owner, for `expires_in`. This is the value that must never be logged.
    pub access: String,
    /// Buys a new pair without the owner typing anything again.
    ///
    /// IT EXPIRES, AND THAT IS THE PRICE OF HAVING NO SECRET. Twitch: "refresh tokens generated by
    /// a Public client type will expire 30 days after they are generated". A Confidential client's
    /// never expires, but a Confidential client must send a `client_secret` to refresh, and this
    /// app has none by design. So at worst the owner types eight characters once a month, and the
    /// screen has to say so plainly rather than silently stopping being able to send.
    pub refresh: String,
    /// How long the access token lasts. Refresh before it lapses, not after.
    pub expires_in: Duration,
}

/// REDACTED, DELIBERATELY, AND THIS IS THE POINT OF WRITING IT BY HAND.
///
/// `#[derive(Debug)]` on this struct would put a token that can speak as the owner into any log
/// line, panic message or `dbg!` that ever touched it. Rust makes that a one word mistake, so the
/// derive is refused and this prints what is safe: that a token exists and how long it lasts.
impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"<redacted>")
            .field("refresh", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Where a poll got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The owner has not finished typing. Keep polling; this is the normal state.
    Pending,
    /// Signed in.
    Ready(Box<Tokens>),
    /// The code ran out, or was used already. Start again.
    Expired(String),
    /// Anything else, in words, with nothing secret in it.
    Refused(String),
}

/// Ask Twitch to start a device authorisation.
pub fn begin(w: &mut dyn Wire, client_id: &str, scopes: &str) -> Result<Device, String> {
    let (code, body) = w.post_form(DEVICE_URL, &[("client_id", client_id), ("scopes", scopes)])?;
    if code != 200 {
        return Err(refusal(code, &body));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("the device reply was not JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let user_code = s("user_code");
    let device_code = s("device_code");
    if user_code.is_empty() || device_code.is_empty() {
        /* THE BODY IS NOT QUOTED BACK. It carries `device_code`, which is the half that becomes a
         * token, and an error message is exactly the sort of string that ends up in a log. */
        return Err("the device reply carried no code".to_owned());
    }
    Ok(Device {
        user_code,
        verification_uri: s("verification_uri"),
        device_code,
        interval: v
            .get("interval")
            .and_then(|x| x.as_u64())
            .map_or(DEFAULT_INTERVAL, Duration::from_secs),
        expires_in: v
            .get("expires_in")
            .and_then(|x| x.as_u64())
            .map_or(Duration::from_secs(1800), Duration::from_secs),
    })
}

/// Ask once whether the owner has finished. See [`Step`].
pub fn poll(w: &mut dyn Wire, client_id: &str, scopes: &str, device_code: &str) -> Step {
    let form = [
        ("client_id", client_id),
        ("scopes", scopes),
        ("device_code", device_code),
        ("grant_type", DEVICE_GRANT),
    ];
    let (code, body) = match w.post_form(TOKEN_URL, &form) {
        Ok(v) => v,
        /* A DROPPED CONNECTION IS NOT A DENIAL. Wi-fi dropping while somebody types must leave the
         * flow alive, so a transport failure is Pending and the caller keeps trying until the code
         * itself expires. */
        Err(_) => return Step::Pending,
    };
    if code == 200 {
        return match tokens_from(&body) {
            Some(t) => Step::Ready(Box::new(t)),
            None => Step::Refused("the token reply carried no token".to_owned()),
        };
    }
    let msg = message_in(&body);
    let low = msg.to_lowercase();
    if low.contains("authorization_pending") || low.contains("authorization pending") {
        Step::Pending
    } else if low.contains("invalid device code") || low.contains("expired") {
        Step::Expired(msg)
    } else {
        Step::Refused(refusal(code, &body))
    }
}

/// WHO DOES THIS TOKEN BELONG TO? Returns the login.
///
/// NEEDED BECAUSE IRC DEMANDS IT. Twitch's chat server authenticates on `PASS oauth:<token>` and
/// then requires the `NICK` to be the login that token belongs to; a mismatch is refused at the
/// handshake with no useful message. The token is opaque to this app, so the only honest way to
/// learn the name is to ask, which is what this endpoint is for. One call per sign-in.
///
/// IT ALSO PROVES THE TOKEN IS ALIVE, which is why `restore` gets a free liveness check out of
/// the same request it needs anyway.
pub fn validate(w: &mut dyn Wire, access: &str) -> Result<String, String> {
    let (code, body) = w.get_authed(VALIDATE_URL, access)?;
    if code != 200 {
        return Err(refusal(code, &body));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("the validate reply was not JSON: {e}"))?;
    match v.get("login").and_then(|x| x.as_str()) {
        Some(l) if !l.is_empty() => Ok(l.to_owned()),
        /* THE BODY IS NOT QUOTED BACK: a validate reply carries the account's user id and the
         * granted scopes, which is more about the owner than an error message needs to say. */
        _ => Err("the validate reply carried no login".to_owned()),
    }
}

/// Trade a refresh token for a new pair. No `client_secret`: see the module note.
pub fn refresh(w: &mut dyn Wire, client_id: &str, refresh_token: &str) -> Result<Tokens, String> {
    let form = [
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    let (code, body) = w.post_form(TOKEN_URL, &form)?;
    if code != 200 {
        return Err(refusal(code, &body));
    }
    tokens_from(&body).ok_or_else(|| "the refresh reply carried no token".to_owned())
}

fn tokens_from(body: &str) -> Option<Tokens> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let access = s("access_token");
    if access.is_empty() {
        return None;
    }
    Some(Tokens {
        access,
        refresh: s("refresh_token"),
        expires_in: v
            .get("expires_in")
            .and_then(|x| x.as_u64())
            .map_or(Duration::from_secs(3600), Duration::from_secs),
    })
}

/// The `message` field Twitch puts its refusals in, or empty.
fn message_in(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("error_description"))
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// A refusal in words. The STATUS is always named, because a message Twitch did not send is worse
/// than a bare number when somebody is trying to work out why they cannot talk.
fn refusal(code: u16, body: &str) -> String {
    let msg = message_in(body);
    if msg.is_empty() {
        format!("Twitch answered {code}")
    } else {
        format!("Twitch answered {code}: {msg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A wire that answers with what it was handed, and records what it was asked.
    struct Fake {
        replies: VecDeque<(u16, String)>,
        asked: Vec<(String, Vec<(String, String)>)>,
    }

    impl Fake {
        fn new(replies: &[(u16, &str)]) -> Fake {
            Fake {
                replies: replies.iter().map(|(c, b)| (*c, (*b).to_owned())).collect(),
                asked: Vec::new(),
            }
        }
    }

    impl Wire for Fake {
        /* Every validate goes to the same place and answers the same login, because no test here
         * is about WHO signed in; they are about the flow around it. A test that cared would
         * script it like any other reply. */
        fn get_authed(&mut self, url: &str, _token: &str) -> Result<(u16, String), String> {
            self.asked
                .push((url.to_owned(), vec![("GET".to_owned(), String::new())]));
            Ok((
                200,
                r#"{"login":"reviird","user_id":"1","scopes":["chat:read","chat:edit"]}"#
                    .to_owned(),
            ))
        }

        fn post_form(&mut self, url: &str, form: &[(&str, &str)]) -> Result<(u16, String), String> {
            self.asked.push((
                url.to_owned(),
                form.iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ));
            self.replies
                .pop_front()
                .ok_or_else(|| "the script ran out".to_owned())
        }
    }

    /// Twitch's own documented device reply, verbatim from their guide.
    const DEVICE_OK: &str = r#"{"device_code":"ike3GM8QIdYZs43KdrWPIO36LofILoCyFEzjlQ91","expires_in":1800,"interval":5,"user_code":"ABCDEFGH","verification_uri":"https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH"}"#;
    /// Their documented "not yet".
    const PENDING: &str = r#"{"status":400,"message":"authorization_pending"}"#;
    /// Their documented "that code is spent".
    const SPENT: &str = r#"{"status":400,"message":"invalid device code"}"#;
    const TOKEN_OK: &str = r#"{"access_token":"AAAA","expires_in":14124,"refresh_token":"RRRR","scope":["chat:read","chat:edit"],"token_type":"bearer"}"#;

    /// THE REQUEST IS THE ONE TWITCH DOCUMENTS, FIELD FOR FIELD.
    ///
    /// `scopes` AND NOT `scope` IS THE WHOLE REASON THIS ASSERTS THE FORM. Twitch spells this field
    /// plural where nearly every other OAuth service spells it singular, and the endpoint answers a
    /// missing `scopes` with a token carrying no permissions rather than an error, so the mistake
    /// would show up as "sending silently does nothing" long after this code was written.
    #[test]
    fn the_device_request_is_the_documented_one() {
        let mut w = Fake::new(&[(200, DEVICE_OK)]);
        let d = begin(&mut w, "CID", "chat:read chat:edit").expect("a documented reply parses");
        assert_eq!(d.user_code, "ABCDEFGH");
        assert_eq!(d.device_code, "ike3GM8QIdYZs43KdrWPIO36LofILoCyFEzjlQ91");
        assert_eq!(d.interval, Duration::from_secs(5));
        assert_eq!(d.expires_in, Duration::from_secs(1800));
        assert!(d.verification_uri.contains("twitch.tv/activate"));

        let (url, form) = &w.asked[0];
        assert_eq!(url, DEVICE_URL);
        assert_eq!(
            form,
            &vec![
                ("client_id".to_owned(), "CID".to_owned()),
                ("scopes".to_owned(), "chat:read chat:edit".to_owned()),
            ],
            "the device request must be exactly client_id and scopes, and `scopes` is plural"
        );
        /* AND NO SECRET WENT ANYWHERE. A Public client sends none, and this is the assertion that
         * fails the day somebody adds one to make an error go away. */
        assert!(
            !form.iter().any(|(k, _)| k.contains("secret")),
            "a client secret was sent; this app is a Public client and has none"
        );
    }

    /// PENDING IS NOT A FAILURE, AND THIS IS THE STATE THE FLOW SPENDS ALL ITS TIME IN.
    ///
    /// Twitch answers `400` with `authorization_pending` for as long as the owner is typing. A
    /// reader that treated 400 as an error would abandon every sign-in that took longer than one
    /// poll, which is all of them.
    #[test]
    fn a_pending_authorisation_keeps_waiting_and_a_finished_one_yields_a_token() {
        let mut w = Fake::new(&[(400, PENDING), (400, PENDING), (200, TOKEN_OK)]);
        assert_eq!(poll(&mut w, "CID", "S", "DC"), Step::Pending);
        assert_eq!(poll(&mut w, "CID", "S", "DC"), Step::Pending);
        match poll(&mut w, "CID", "S", "DC") {
            Step::Ready(t) => {
                assert_eq!(t.access, "AAAA");
                assert_eq!(t.refresh, "RRRR");
                assert_eq!(t.expires_in, Duration::from_secs(14124));
            }
            other => panic!("a documented token reply did not read as Ready: {other:?}"),
        }
        /* The exchange carries the grant type Twitch requires, spelled exactly. */
        let (url, form) = &w.asked[2];
        assert_eq!(url, TOKEN_URL);
        assert!(form.contains(&("grant_type".to_owned(), DEVICE_GRANT.to_owned())));
        assert!(!form.iter().any(|(k, _)| k.contains("secret")));
    }

    /// A SPENT OR EXPIRED CODE IS ITS OWN ANSWER, because the only cure is starting again and the
    /// screen has to say so rather than spinning.
    #[test]
    fn a_spent_code_is_told_apart_from_a_pending_one() {
        let mut w = Fake::new(&[(400, SPENT)]);
        match poll(&mut w, "CID", "S", "DC") {
            Step::Expired(why) => assert!(why.contains("invalid device code"), "{why}"),
            other => panic!("a spent code read as {other:?}, so the screen would poll forever"),
        }
    }

    /// A DROPPED CONNECTION MUST NOT CANCEL A SIGN-IN SOMEBODY IS HALFWAY THROUGH.
    #[test]
    fn a_transport_failure_is_patience_and_not_a_refusal() {
        struct Dead;
        impl Wire for Dead {
            fn post_form(&mut self, _: &str, _: &[(&str, &str)]) -> Result<(u16, String), String> {
                Err("the network went away".to_owned())
            }
            fn get_authed(&mut self, _: &str, _: &str) -> Result<(u16, String), String> {
                Err("the network went away".to_owned())
            }
        }
        assert_eq!(poll(&mut Dead, "CID", "S", "DC"), Step::Pending);
    }

    /// THE TOKEN CANNOT BE PRINTED BY ACCIDENT.
    ///
    /// One `{:?}` in a log line is all it takes to write the thing that speaks as the owner into a
    /// file. `Tokens` therefore has no `derive(Debug)`, and this fails the day one is added.
    #[test]
    fn a_token_cannot_be_printed_by_accident() {
        let t = Tokens {
            access: "SUPERSECRETACCESS".to_owned(),
            refresh: "SUPERSECRETREFRESH".to_owned(),
            expires_in: Duration::from_secs(60),
        };
        let printed = format!("{t:?}");
        assert!(
            !printed.contains("SUPERSECRETACCESS") && !printed.contains("SUPERSECRETREFRESH"),
            "a token printed itself: {printed}"
        );
        assert!(printed.contains("<redacted>"), "{printed}");
        /* And inside the one type that carries it around, too. */
        let step = Step::Ready(Box::new(t));
        assert!(!format!("{step:?}").contains("SUPERSECRETACCESS"));
    }

    /// A REFUSAL NAMES THE STATUS EVEN WHEN TWITCH SENDS NO WORDS, and never quotes the body, which
    /// is where the device code lives.
    #[test]
    fn a_refusal_is_readable_and_carries_nothing_secret() {
        let mut w = Fake::new(&[(401, r#"{"message":"invalid client"}"#)]);
        let e = begin(&mut w, "CID", "S").expect_err("401 is not a device");
        assert!(e.contains("401") && e.contains("invalid client"), "{e}");

        let mut w = Fake::new(&[(503, "")]);
        let e = begin(&mut w, "CID", "S").expect_err("503 is not a device");
        assert!(e.contains("503"), "{e}");

        /* A 200 whose body is missing the codes must not report the body back. */
        let mut w = Fake::new(&[(200, r#"{"device_code":"SECRETHALF"}"#)]);
        let e = begin(&mut w, "CID", "S").expect_err("no user_code is not a device");
        assert!(
            !e.contains("SECRETHALF"),
            "the refusal quoted the device code: {e}"
        );
    }

    /// A file this test owns, under the OS temp dir and NEVER under %APPDATA%.
    ///
    /// THE OWNER`S REAL FILES ARE OFF LIMITS TO TESTS, and this project has been bitten: a unit
    /// once overwrote the real settings.json with its fixture. `token_path()` points at
    /// %APPDATA%\eql-grimoire and nothing here may call it; every test hands its own path in,
    /// which is the whole reason `save`, `load` and `restore_with` take one.
    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("eqlg-auth-{}-{tag}.bin", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn tokens(refresh: &str) -> Tokens {
        Tokens {
            access: "ACCESS".to_owned(),
            refresh: refresh.to_owned(),
            expires_in: Duration::from_secs(14124),
        }
    }

    /// A SIGN-IN SURVIVES A RESTART, AND WHAT IS ON DISK IS THE REFRESH TOKEN AND NOTHING ELSE.
    ///
    /// The second half is the assertion that matters. An access token is good for about four
    /// hours and this file cannot know how long ago one was written, because Twitch returns a
    /// DURATION and not an instant; saving one would mean reloading a value that has probably
    /// lapsed. So the file must contain the refresh token and must NOT contain the access token,
    /// and a `save` that wrote the whole struct would pass a round trip test while doing the
    /// wrong thing. That is what the second assertion catches.
    #[cfg(windows)]
    #[test]
    fn a_saved_sign_in_keeps_only_the_refresh_token_and_comes_back_after_a_restart() {
        let p = tmp("round");
        save(Some(&p), &tokens("REFRESH-ME"));
        let raw = std::fs::read(&p).expect("the file was written");
        assert!(
            !raw.windows(6).any(|w| w == b"ACCESS"),
            "the access token was written to disk; only the refresh token may be kept"
        );
        assert!(
            !raw.windows(10).any(|w| w == b"REFRESH-ME"),
            "the refresh token is on disk in the clear"
        );
        assert_eq!(load(Some(&p)).as_deref(), Some("REFRESH-ME"));

        /* AND THE RESTART. A fresh `Auth`, pointed at that file, spends the saved token for a new
         * pair and ends up signed in, with no device code and nobody typing anything. */
        let ctx = egui::Context::default();
        let a = Auth::default();
        assert_eq!(a.view(), AuthView::Out, "a fresh Auth starts signed out");
        a.restore_with(&ctx, "CID", Some(p.clone()), || {
            Box::new(Fake::new(&[(200, TOKEN_OK)]))
        });
        assert!(
            settles(
                &a,
                AuthView::In {
                    login: "reviird".to_owned()
                }
            ),
            "a saved sign-in did not come back: {:?}",
            a.view()
        );
        assert!(a.signed_in());
        /* The renewal replaced the pair, so what is on disk now is the NEW refresh token. A
         * refresh token is one time use; leaving the spent one would break the next launch. */
        assert_eq!(load(Some(&p)).as_deref(), Some("RRRR"));
        let _ = std::fs::remove_file(&p);
    }

    /// A LAPSED SIGN-IN CLEARS ITSELF AND IS NOT AN ERROR ON SCREEN.
    ///
    /// Twitch expires a Public client`s refresh token 30 days after it is issued, so this is the
    /// NORMAL end of a sign-in and not a fault. It must land on `Out`, which is the screen that
    /// offers a Sign in button, and the dead token must be deleted rather than retried on every
    /// launch forever.
    #[cfg(windows)]
    #[test]
    fn a_lapsed_saved_sign_in_is_cleared_rather_than_retried_forever() {
        let p = tmp("lapsed");
        save(Some(&p), &tokens("TOO-OLD"));
        let ctx = egui::Context::default();
        let a = Auth::default();
        a.restore_with(&ctx, "CID", Some(p.clone()), || {
            Box::new(Fake::new(&[(
                400,
                r#"{"status":400,"message":"Invalid refresh token"}"#,
            )]))
        });
        /* WAIT ON THE FILE, NOT ON THE STATE. `AuthView::Out` is what a fresh `Auth` ALREADY is,
         * so `settles(&a, Out)` returns true instantly and proves nothing about whether the thread
         * ever ran. That is a test that cannot fail, and it is exactly how the first version of
         * this passed while the file it was about sat untouched on disk. The deletion is the one
         * observable that only happens if the thread got there. */
        assert!(
            gone(&p),
            "a dead token was left on disk to be retried on every launch forever"
        );
        assert_eq!(
            a.view(),
            AuthView::Out,
            "a lapsed sign-in is the normal end of one, not a red error line"
        );
        assert!(!a.signed_in(), "a refused token was kept anyway");
    }

    /// Wait briefly for the auth thread to remove a file.
    fn gone(p: &std::path::Path) -> bool {
        for _ in 0..300 {
            if !p.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// NOTHING SAVED MEANS NO THREAD AND NO REQUEST, which is the first launch and every launch
    /// after a sign-out.
    #[test]
    fn with_nothing_saved_restore_does_nothing_at_all() {
        let ctx = egui::Context::default();
        let a = Auth::default();
        a.restore_with(&ctx, "CID", Some(tmp("absent")), || {
            panic!("restore built a wire with nothing saved, so it would have dialled")
        });
        assert_eq!(a.view(), AuthView::Out);
        assert!(!a.signed_in());
    }

    /// Wait briefly for the auth thread to reach a state. Threads, so a poll and not a sleep.
    /// Wait for the auth thread to reach `want`, or give up.
    ///
    /// THE CEILING IS GENEROUS ON PURPOSE AND IT USED TO BE TWO SECONDS.
    ///
    /// This returns the instant the condition holds, so a bigger ceiling costs a healthy machine
    /// nothing at all: it is only ever paid in full by a test that was going to fail anyway. Two
    /// seconds was not a budget, it was a bet that a freshly spawned thread would be scheduled and
    /// finish inside it, and that bet loses on a loaded machine.
    ///
    /// MEASURED. `a_saved_sign_in_keeps_only_the_refresh_token_and_comes_back_after_a_restart`
    /// passed alone every time and failed roughly two runs in three inside the full suite, while
    /// the machine was also building and running other work. The view it reported was `Out`, the
    /// INITIAL state, not `Failed` and not `Refused`: the worker had not reported yet, rather than
    /// having reported something wrong. Nothing was broken except this number.
    ///
    /// A wall clock in a test is a flake generator. This one cannot be removed (the thing under
    /// test IS a background thread) so it is made too big to lose instead of tuned to a machine.
    fn settles(a: &Auth, want: AuthView) -> bool {
        for _ in 0..1_000 {
            if a.view() == want {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// THE PRODUCTION WIRE KEEPS THE BODY OF A REFUSAL, WHICH IS THE WHOLE PROTOCOL.
    ///
    /// THIS IS THE GUARD THE SEAM COULD NOT PROVIDE. Every other test here drives `Fake`, which
    /// answers 400 WITH a body because that is what a wire is supposed to do. The real wire did
    /// not: `ureq` defaults to turning 4xx into an error and dropping the body, so the shipped
    /// build refused every sign-in on its first poll with "Twitch answered 400" and no message.
    ///
    /// It asserts the CONFIG rather than making a request, because a test that dialled Twitch to
    /// prove this would fail whenever Twitch was down. Deleting the `http_status_as_error(false)`
    /// line makes this red. The end to end version lives in tests/live_twitch.rs and is run by
    /// hand.
    #[test]
    fn the_wire_keeps_the_body_of_a_refusal() {
        let w = Https::default();
        assert!(
            !w.agent.config().http_status_as_error(),
            "the wire turns a 4xx into an error and discards its body. This protocol says \
             `authorization_pending` in the body of a 400, so every sign-in would be refused on \
             its first poll"
        );
    }

    /// A FAILED SIGN-IN CAN BE RETRIED, which sounds obvious and was not true.
    ///
    /// `begin_with` claims `running` so two clicks cannot start two flows. Nothing released it
    /// when the thread ENDED, so the first failure latched it forever and `Try signing in again`
    /// became a button that visibly did nothing. WHAT MUTATION MAKES THIS RED: removing the
    /// `Drop` on `Held`.
    #[test]
    fn a_failed_sign_in_can_be_started_again() {
        let ctx = egui::Context::default();
        let a = Auth::default();
        /* A device request that refuses outright, so the thread ends almost at once. */
        a.begin_with(
            &ctx,
            "CID",
            "S",
            Box::new(Fake::new(&[(401, r#"{"message":"nope"}"#)])),
        );
        assert!(
            settled(&a, |v| matches!(v, AuthView::Failed(_))),
            "the first attempt did not finish: {:?}",
            a.view()
        );
        /* AND AGAIN. If `running` is still claimed this second call returns at the guard, the
         * state never moves off `Failed`, and the wire below is never asked for anything. */
        a.begin_with(
            &ctx,
            "CID",
            "S",
            Box::new(Fake::new(&[(200, DEVICE_OK), (400, PENDING)])),
        );
        assert!(
            settled(&a, |v| matches!(v, AuthView::Waiting { .. })),
            "a second sign-in never started, so `Try again` is a dead button: {:?}",
            a.view()
        );
    }

    /// Wait briefly for the auth thread to reach a state matching a predicate.
    fn settled(a: &Auth, ok: impl Fn(&AuthView) -> bool) -> bool {
        for _ in 0..300 {
            if ok(&a.view()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// A SHORT LIVED TOKEN DOES NOT BECOME A TIGHT LOOP.
    ///
    /// `renew_after` is `expires_in` minus a margin, and `Duration` subtraction saturates at zero.
    /// A token that arrives claiming to last less than the margin, which is what a clock skew, a
    /// proxy, or a service having a bad day produces, would therefore say "renew immediately", and
    /// the renew loop would hammer the token endpoint as fast as the network allowed until Twitch
    /// rate limited the app. The floor is what stops that, and this is the assertion that fails
    /// if somebody simplifies it away.
    #[test]
    fn a_short_lived_token_does_not_become_a_tight_loop() {
        for absurd in [0u64, 1, 30, 119, 120] {
            let d = renew_after(Duration::from_secs(absurd));
            assert!(
                d >= RENEW_FLOOR,
                "a token claiming {absurd}s renews after {d:?}, which is a tight loop against \
                 Twitch's token endpoint"
            );
        }
        /* And a normal token renews BEFORE it lapses, not after, or every renewal races a token
         * that has already stopped working. */
        let normal = Duration::from_secs(14124);
        let after = renew_after(normal);
        assert!(
            after < normal,
            "renewal must happen before the token lapses"
        );
        assert_eq!(after, normal - RENEW_MARGIN);
    }

    /// REFRESHING SENDS NO SECRET, which is what being a Public client buys and what this app
    /// depends on.
    #[test]
    fn refreshing_needs_no_client_secret() {
        let mut w = Fake::new(&[(200, TOKEN_OK)]);
        let t = refresh(&mut w, "CID", "OLDREFRESH").expect("a documented reply parses");
        assert_eq!(t.access, "AAAA");
        let (url, form) = &w.asked[0];
        assert_eq!(url, TOKEN_URL);
        assert!(form.contains(&("grant_type".to_owned(), "refresh_token".to_owned())));
        assert!(
            !form.iter().any(|(k, _)| k.contains("secret")),
            "refresh sent a client secret; a Public client has none and Twitch does not want one"
        );
    }
}

/* ------------------------------------------------------------------- the driver -- */

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// What the screen draws. A snapshot, cloned out from under the lock once a frame, exactly as
/// `player::PlayerView` is and for the same reason: a screen asks, it never drives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AuthView {
    /// Nobody has signed in. Reading works; sending does not.
    #[default]
    Out,
    /// Twitch is waiting for the owner to type `user_code` on `verification_uri`.
    Waiting {
        user_code: String,
        verification_uri: String,
    },
    /// Signed in, as this login. The TOKEN is not here: a view is drawn, and a drawn token is a
    /// leaked token. The login is not secret, is on screen anyway, and is what the chat reader
    /// needs for its `NICK`.
    In { login: String },
    /// It did not work, in words.
    Failed(String),
}

/// The sign-in, driven on its own thread.
///
/// THE TOKEN NEVER LEAVES THIS TYPE. `view()` is what the UI gets and it carries no secret; the
/// only way out is [`Auth::with_token`], which lends it under the lock to the one caller that has
/// to put it on a socket. There is no getter that returns it, so it cannot be copied into a
/// struct, a log line, or a `Cx`.
///
/// IN MEMORY ONLY, THIS ROUND, AND THE SCREEN SAYS SO. Persisting it means encrypting it at rest
/// (DPAPI on Windows, which ties the ciphertext to this user account) and that is its own change
/// with its own tests. Until then a launch starts signed out, which is honest and costs the owner
/// eight characters.
pub struct Auth {
    state: Arc<Mutex<AuthView>>,
    token: Arc<Mutex<Option<Tokens>>>,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Default for Auth {
    fn default() -> Self {
        Auth {
            state: Arc::new(Mutex::new(AuthView::Out)),
            token: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Auth {
    /// What the screen draws.
    pub fn view(&self) -> AuthView {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// STOP SHOWING THE SIGN-IN PAGE WITHOUT ABANDONING THE SIGN-IN.
    ///
    /// The Watch folio stages the activation page while the state is `Waiting`, so the way to put
    /// the video back is to stop being in that state. THE POLLING THREAD IS UNTOUCHED: it keeps
    /// asking every five seconds until the code expires, so somebody who steps away to fetch a
    /// phone and comes back finds the sign-in still live and the token still arrives. Tearing the
    /// flow down because the reader looked away would throw away a code they are mid-way through.
    pub fn unwatch(&self) {
        let mut g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if matches!(*g, AuthView::Waiting { .. }) {
            *g = AuthView::Out;
        }
    }

    /// Whether a token exists. The question every caller actually has.
    pub fn signed_in(&self) -> bool {
        self.token
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Lend the access token to the one thing that puts it on a socket.
    ///
    /// A CLOSURE AND NOT A GETTER, deliberately. A `-> Option<String>` would let any caller copy
    /// the token into a struct that derives `Debug`, and the redaction on `Tokens` would be worth
    /// nothing. This hands out a `&str` that cannot outlive the call.
    pub fn with_token<R>(&self, f: impl FnOnce(Option<&str>) -> R) -> R {
        let g = self.token.lock().unwrap_or_else(|p| p.into_inner());
        f(g.as_ref().map(|t| t.access.as_str()))
    }

    /// PICK UP A SAVED SIGN-IN, IF THERE IS ONE. Call once at startup.
    ///
    /// It spends the saved refresh token for a fresh pair rather than trusting anything it read,
    /// because the only durable half is the refresh token; see `save`. That costs one request per
    /// launch and buys never holding an access token whose age this app cannot account for.
    ///
    /// A FAILURE HERE IS NOT AN ERROR ON SCREEN. Twitch expires a Public client`s refresh token 30
    /// days after it is issued, so a lapsed one is the NORMAL end of a sign-in, not a fault. The
    /// state goes to `Out`, which is the screen that offers a Sign in button, and the dead token
    /// is deleted so it is not retried on every launch forever. A red failure line for an expected
    /// monthly expiry would teach the owner to ignore red lines.
    ///
    /// SILENT WHEN THERE IS NOTHING SAVED, which is the first launch, every launch off Windows,
    /// and every launch after a sign-out. No thread, no request.
    pub fn restore(&self, ctx: &egui::Context) {
        self.restore_with(ctx, crate::settings::TWITCH_CLIENT_ID, token_path(), || {
            Box::new(Https::default())
        });
    }

    /// The same, over a caller supplied path and wire. What a test drives with no disk of the
    /// owner`s and no network.
    pub fn restore_with(
        &self,
        ctx: &egui::Context,
        client_id: &str,
        path: Option<std::path::PathBuf>,
        wire: impl FnOnce() -> Box<dyn Wire>,
    ) {
        let Some(saved) = load(path.as_deref()) else {
            return;
        };
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let held = Held {
            running: self.running.clone(),
            state: self.state.clone(),
            token: self.token.clone(),
            stop: self.stop.clone(),
            ctx: ctx.clone(),
            path,
        };
        let client_id = client_id.to_owned();
        let mut wire = wire();
        let spawned = std::thread::Builder::new()
            .name("twitch-auth-restore".to_owned())
            .spawn(move || match refresh(wire.as_mut(), &client_id, &saved) {
                Ok(t) => keep_fresh(&held, wire.as_mut(), &client_id, t),
                Err(e) => {
                    log::info!("the saved Twitch sign-in was not accepted, so it was cleared: {e}");
                    held.drop_token();
                    held.set(AuthView::Out);
                }
            });
        if spawned.is_err() {
            self.running.store(false, Ordering::SeqCst);
        }
    }

    /// Start a sign-in. Idempotent while one is already running.
    pub fn begin(&self, ctx: &egui::Context, client_id: &str, scopes: &str) {
        self.begin_with(ctx, client_id, scopes, Box::new(Https::default()));
    }

    /// The same, over a caller supplied wire. What a test drives the real thread with.
    pub fn begin_with(
        &self,
        ctx: &egui::Context,
        client_id: &str,
        scopes: &str,
        mut wire: Box<dyn Wire>,
    ) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let held = Held {
            running: self.running.clone(),
            state: self.state.clone(),
            token: self.token.clone(),
            stop: self.stop.clone(),
            ctx: ctx.clone(),
            path: token_path(),
        };
        let (client_id, scopes) = (client_id.to_owned(), scopes.to_owned());
        let spawned = std::thread::Builder::new()
            .name("twitch-auth".to_owned())
            .spawn(move || {
                let set = |v: AuthView| held.set(v);
                let device = match begin(wire.as_mut(), &client_id, &scopes) {
                    Ok(d) => d,
                    Err(e) => return set(AuthView::Failed(e)),
                };
                set(AuthView::Waiting {
                    user_code: device.user_code.clone(),
                    verification_uri: device.verification_uri.clone(),
                });
                let deadline = std::time::Instant::now() + device.expires_in;
                while std::time::Instant::now() < deadline {
                    if held.stop.load(Ordering::Relaxed) {
                        return set(AuthView::Out);
                    }
                    std::thread::sleep(device.interval);
                    match poll(wire.as_mut(), &client_id, &scopes, &device.device_code) {
                        Step::Pending => {}
                        Step::Ready(t) => {
                            return keep_fresh(&held, wire.as_mut(), &client_id, *t);
                        }
                        Step::Expired(why) | Step::Refused(why) => {
                            return set(AuthView::Failed(why));
                        }
                    }
                }
                set(AuthView::Failed(
                    "the code ran out before it was entered".to_owned(),
                ));
            });
        if let Err(e) = spawned {
            self.running.store(false, Ordering::SeqCst);
            *self.state.lock().unwrap_or_else(|p| p.into_inner()) =
                AuthView::Failed(format!("the sign-in thread could not start: {e}"));
        }
    }
}

/// What a live sign-in thread holds: the state the screen reads, the token, the stop flag, the
/// context to wake, and where to save. Bundled because two entry points (`begin_with` and
/// `restore`) hand the same five things to the same loop, and five loose parameters threaded
/// through two call sites is how one of them ends up with a stale copy.
struct Held {
    /// Released when the thread finishes, whatever way it finishes.
    ///
    /// THE LATCH THAT MADE `TRY AGAIN` A DEAD BUTTON. `begin_with` claims `running` with a swap
    /// so two clicks in one instant cannot start two flows, and nothing ever cleared it: the
    /// first failure left it claimed forever, so every later click returned at the guard and did
    /// nothing at all. On screen that is a button that does not respond, which is what the owner
    /// reported. Clearing it in `Drop` covers every exit, including the early returns and a
    /// panic, which an explicit store at each `return` would not.
    running: Arc<AtomicBool>,
    state: Arc<Mutex<AuthView>>,
    token: Arc<Mutex<Option<Tokens>>>,
    stop: Arc<AtomicBool>,
    ctx: egui::Context,
    path: Option<std::path::PathBuf>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Held {
    fn set(&self, v: AuthView) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = v;
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    fn keep(&self, t: &Tokens) {
        *self.token.lock().unwrap_or_else(|p| p.into_inner()) = Some(t.clone());
        save(self.path.as_deref(), t);
    }
    fn drop_token(&self) {
        *self.token.lock().unwrap_or_else(|p| p.into_inner()) = None;
        forget(self.path.as_deref());
    }
}

/// SIGNED IN, AND THEN KEPT SIGNED IN, for as long as the app runs.
///
/// An access token lasts about four hours. Without this the app would stop being able to speak
/// partway through a stream with nothing on screen to say why, which is the failure mode this
/// whole file is arranged to prevent. The refresh token is ONE TIME USE, so every renewal
/// replaces the pair and the new refresh token is what the next renewal spends; that is also why
/// each renewal re-saves, or a crash would leave a spent token on disk.
fn keep_fresh(held: &Held, wire: &mut dyn Wire, client_id: &str, first: Tokens) {
    let mut have = first;
    /* ASKED ONCE, NOT ONCE PER RENEWAL. A refresh returns a new token for the SAME account, so
     * the login cannot change under a live sign-in; asking again every few hours would be a
     * request that can only ever return the same answer. */
    let login = match validate(wire, &have.access) {
        Ok(l) => l,
        Err(e) => {
            held.drop_token();
            return held.set(AuthView::Failed(format!(
                "signed in, but Twitch would not say which account this is, so chat cannot \
                 log in: {e}"
            )));
        }
    };
    held.keep(&have);
    held.set(AuthView::In {
        login: login.clone(),
    });
    loop {
        if !nap(&held.stop, renew_after(have.expires_in)) {
            return held.set(AuthView::Out);
        }
        match refresh(wire, client_id, &have.refresh) {
            Ok(next) => {
                have = next;
                held.keep(&have);
                /* The screen already says this login; re-setting it is what wakes the frame that
                 * hands the NEW token to the chat reader. */
                held.set(AuthView::In {
                    login: login.clone(),
                });
            }
            /* THE TOKEN GOES WITH THE FAILURE, ON DISK AS WELL AS IN MEMORY. Leaving a lapsed
             * one in place would leave `signed_in()` answering true while every message silently
             * failed to send, which is the worst of the three states this can be in, and leaving
             * it on disk would retry a dead token on every launch forever. Twitch expires a
             * Public client's refresh token 30 days after it is issued, so this arm is reached in
             * normal use and has to say so plainly. */
            Err(e) => {
                held.drop_token();
                return held.set(AuthView::Failed(format!(
                    "the sign-in lapsed and could not be renewed, so sign in again: {e}"
                )));
            }
        }
    }
}

/// Sleep, in slices, so a shutdown is not waited out. `false` when it was told to stop.
///
/// SLICED BECAUSE THE WAIT IS HOURS LONG. `Drop` sets the flag and does not join, exactly as the
/// chat reader does, but a thread parked in one four hour `sleep` would keep the process alive
/// after the window closed. Half a second is short enough that nobody sees the delay.
fn nap(stop: &AtomicBool, how_long: Duration) -> bool {
    let until = std::time::Instant::now() + how_long;
    while std::time::Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    !stop.load(Ordering::Relaxed)
}

impl Drop for Auth {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
