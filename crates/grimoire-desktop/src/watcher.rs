//! Broken Stoic live status, polled off the UI thread. Decision D2.
//!
//! WHERE THE ANSWER COMES FROM, AND WHY IT IS A LADDER.
//! There are no Twitch or YouTube credentials, and the channel page is a JavaScript shell with no
//! ld+json (measured 2026-09-02: 195 KB, zero `isLiveBroadcast`), so the page is not a source. What
//! does answer, unauthenticated, is the GraphQL endpoint Twitch's own web client calls, with the web
//! client's public Client-ID. That is the first rung. decapi.me is the second, a plain text service
//! that says `<login> is offline` or an uptime. The third rung is Helix, the official API, and it is
//! EMPTY on purpose: it refuses with "no credentials configured" so that real keys can be dropped in
//! later without touching the ladder. Rungs are tried in order every 90 seconds and the first one
//! that answers wins; the pill's tooltip names which one it was.
//!
//! THE ONE RULE THAT MATTERS: A FAILED POLL NEVER SAYS "OFFLINE".
//! "Offline" is a fact about the channel. "The request failed" is a fact about this machine's
//! network. Conflating them puts a grey `offline` on screen while Broken Stoic is live and the wifi
//! is flapping, which is the exact moment the app is supposed to be sending people to the stream.
//! So a failed poll keeps the LAST KNOWN channel, sets `error`, leaves `checked_at` alone, and the
//! UI shows the age of what it knows. `live: Some(false)` is written only when a source actually
//! said so. That rule is `merge()` and it has a test that fails without it.
//!
//! YouTube: the `@handle/live` page is fetched and read for the live markers YouTube embeds in its
//! player JSON. That is best effort and is labelled as such by its source name, `youtube-page`.
//!
//! WHICH CHANNEL, AND WHY THIS FILE NO LONGER ASKS. Both handles are the constants
//! [`crate::settings::TWITCH_HANDLE`] and [`crate::settings::YOUTUBE_HANDLE`], read directly. They
//! were `Settings` fields carried in through a `WatchConfig`, and that struct existed only to move
//! them off the UI thread's copy; with two `&'static str` there is nothing to move, so it is gone.
//! With it went the "no YouTube handle" state: `Status::youtube` was an `Option` that meant
//! UNCONFIGURED rather than offline, and unconfigured is not a state this build can be in.

use chrono::{DateTime, Utc};
use regex::Regex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/* ------------------------------------------------------------- the constants -- */

/// D2: poll every 90 seconds.
pub const POLL_EVERY: Duration = Duration::from_secs(90);
/// Every request, connect plus read. A live pill that hangs the poll thread for a minute is a pill
/// that lies about its age for a minute.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const USER_AGENT: &str = "EQL-Grimoire/0.1";

/// The endpoint Twitch's web client uses. D2, measured.
pub const TWITCH_GQL_URL: &str = "https://gql.twitch.tv/gql";
/// The web client's public Client-ID. It is in every page load of twitch.tv, it is not a secret,
/// and it is the reason the GQL rung works without an account.
pub const TWITCH_WEB_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
/// decapi's uptime endpoint, plain text. The login goes on the end.
pub const DECAPI_UPTIME_URL: &str = "https://decapi.me/twitch/uptime/";

/// Source names, as they appear in `Channel::source`. One place, so the UI and the tests agree.
pub const SRC_UNCHECKED: &str = "unchecked";
pub const SRC_TWITCH_GQL: &str = "twitch-gql";
pub const SRC_DECAPI: &str = "decapi";
pub const SRC_TWITCH_HELIX: &str = "twitch-helix";
pub const SRC_YOUTUBE_PAGE: &str = "youtube-page";

/* ---------------------------------------------------------------- the shapes -- */

/// One channel's last known state. `live == None` means never checked, or nothing has ever been
/// known. `checked_at` is stamped ONLY by a successful check, which is what makes its age honest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub handle: String,
    pub live: Option<bool>,
    pub title: Option<String>,
    pub viewers: Option<u64>,
    pub game: Option<String>,
    /// The id of the video that is live RIGHT NOW, when one is and the poller found it.
    ///
    /// WHY A WATCHER FIELD AND NOT A LOOKUP INSIDE THE PLAYER. YouTube cannot be embedded from a
    /// channel id any more: `/embed/live_stream?channel=<UC id>` was measured failing with IFrame
    /// API error 150 for a live channel and an offline one alike, reporting its own `videoId` as
    /// the literal string "live_stream", while `/embed/<video id>` from the same origin in the
    /// same process plays. So a video id is required, and this poller already downloads and
    /// parses the very bytes that carry it: the title regex below has always matched
    /// `"videoDetails":{"videoId":"...","title":"..."}` and thrown the id away. A second fetch
    /// somewhere else would be a second poll rate and a second thing to go stale.
    ///
    /// `None` on Twitch always, because a Twitch channel IS its player address and there is
    /// nothing to look up. `None` on YouTube whenever the channel is not live, because then there
    /// is no video, which is the honest reason the Watch screen has nothing to embed.
    pub video_id: Option<String>,
    pub checked_at: Option<DateTime<Utc>>,
    /// Which implementation produced the state above. `SRC_UNCHECKED` until one has.
    pub source: &'static str,
    /// The last poll's failure, if the last poll failed. Cleared by the next success. The state
    /// fields above are NOT touched by a failure, so `error` plus `live: Some(true)` reads as "was
    /// live as of `checked_at`, and I cannot currently confirm it", which is the truth.
    pub error: Option<String>,
}

impl Channel {
    /// The state before anything has been asked.
    pub fn unchecked(handle: &str) -> Channel {
        Channel {
            handle: handle.to_owned(),
            live: None,
            title: None,
            viewers: None,
            game: None,
            video_id: None,
            checked_at: None,
            source: SRC_UNCHECKED,
            error: None,
        }
    }
}

/* The age of a check is printed by exactly one rule, `titlebar::age_of` over `checked_at`, on
 * the pill, the Watch window and Settings alike. Two helpers that worded the same seconds a
 * second way ("1h 12m ago" against the pill's "1h") were cut from here so two surfaces cannot
 * call one check two different ages. */

/// What `Watcher::status()` hands back. Cheap to clone: two small structs.
///
/// BOTH CHANNELS ARE ALWAYS PRESENT. `youtube` used to be an `Option` whose `None` meant "no
/// handle configured", which is a different thing from offline and had to be spelled differently
/// on every surface that read it. The handle is a constant now, so that state cannot occur;
/// "nothing is known yet" is `Channel::unchecked`, which is what `live: None` has always meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub twitch: Channel,
    pub youtube: Channel,
}

impl Status {
    fn initial() -> Status {
        Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        }
    }

    /// The channel for one platform.
    ///
    /// BOTH ARE ALWAYS POLLED; THIS ONLY CHOOSES WHICH ONE IS BEING ASKED ABOUT. The preference
    /// (`settings::Settings::watch_on`) is a reading preference, not a polling one: the poll rate
    /// is fixed against two unauthenticated endpoints and skipping one to honour a preference
    /// would mean the OTHER platform's state was stale the moment the preference changed.
    pub fn on(&self, on: crate::settings::Platform) -> &Channel {
        match on {
            crate::settings::Platform::Twitch => &self.twitch,
            crate::settings::Platform::YouTube => &self.youtube,
        }
    }
}

/* ---------------------------------------------------------------- the ladder -- */

/// One way of asking whether a channel is live. `check` runs on the poll thread and may block for
/// up to `HTTP_TIMEOUT`; it must never be called from the UI.
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, handle: &str) -> Result<Channel, String>;
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .user_agent(USER_AGENT)
        /* 4xx and 5xx become `Err`, which is what the ladder wants: a 503 from decapi is a reason
         * to fall through, not a body to parse. */
        .http_status_as_error(true)
        .build()
        .new_agent()
}

/// Twitch logins are 4 to 25 characters of `[A-Za-z0-9_]`. Anything else is refused before it can
/// be interpolated into a query or a URL.
///
/// THE ONLY HANDLE PRODUCTION PASSES IS [`crate::settings::TWITCH_HANDLE`], and a test holds that
/// constant to this function. The guard stays because its job is the interpolation, not the
/// settings screen it used to name in this message: a `Result` here is what stops a mistyped
/// constant reaching `gql.twitch.tv` as a malformed query, and it says so in one clear line
/// instead of a confusing upstream 400.
fn twitch_login(handle: &str) -> Result<String, String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let rx = RX.get_or_init(|| Regex::new(r"^[A-Za-z0-9_]{1,25}$").expect("static regex"));
    let h = handle.trim();
    if h.is_empty() {
        return Err("twitch handle is empty".to_owned());
    }
    if !rx.is_match(h) {
        return Err(format!(
            "twitch handle {h:?} is not a valid login (letters, digits, underscore, up to 25)"
        ));
    }
    Ok(h.to_ascii_lowercase())
}

/// YouTube handles are 3 to 30 characters of letters, digits, `_`, `-`, `.`, with or without the
/// leading `@` the site displays, and this returns the bare form the `/@{login}/live` URL wants.
///
/// `None` means the string is not a shape YouTube accepts, so no URL is built from it. It used to
/// mean "not configured" as well, and that reading is gone with the settings field: production
/// passes [`crate::settings::YOUTUBE_HANDLE`] and a test holds that constant to this function.
fn youtube_login(handle: &str) -> Option<String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let rx = RX.get_or_init(|| Regex::new(r"^[A-Za-z0-9._-]{3,30}$").expect("static regex"));
    let h = handle.trim().trim_start_matches('@');
    if h.is_empty() || !rx.is_match(h) {
        return None;
    }
    Some(h.to_owned())
}

/// Rung 1: Twitch's public GQL endpoint. D2, measured.
pub struct TwitchGql {
    agent: ureq::Agent,
}

impl Default for TwitchGql {
    fn default() -> Self {
        Self { agent: agent() }
    }
}

impl TwitchGql {
    /// The exact query from D2, with the login JSON-escaped by serde rather than pasted, so a handle
    /// can never break out of the string literal.
    fn query_for(login: &str) -> String {
        let quoted = serde_json::to_string(login).unwrap_or_else(|_| "\"\"".to_owned());
        format!("{{ user(login:{quoted}) {{ id displayName stream {{ id title viewersCount game {{ name }} }} }} }}")
    }
}

impl Source for TwitchGql {
    fn name(&self) -> &'static str {
        SRC_TWITCH_GQL
    }
    fn check(&self, handle: &str) -> Result<Channel, String> {
        let login = twitch_login(handle)?;
        let body = serde_json::json!({ "query": Self::query_for(&login) });
        let mut resp = self
            .agent
            .post(TWITCH_GQL_URL)
            .header("Client-ID", TWITCH_WEB_CLIENT_ID)
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("POST {TWITCH_GQL_URL}: {e}"))?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("reading {TWITCH_GQL_URL} body: {e}"))?;
        parse_gql(handle, &text)
    }
}

/// Parse a GQL response body into a `Channel`. Public so the tests can feed it the measured
/// response from D2 without a network.
///
/// `stream: null` is offline. `stream: {..}` is live. `user: null` is "Twitch has no such login",
/// which is an ERROR and not offline: a typo in settings must not paint a grey pill that looks
/// like a fact about the channel.
pub fn parse_gql(handle: &str, body: &str) -> Result<Channel, String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("gql answered something that is not JSON: {e}"))?;
    if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
        let msgs: Vec<String> = errs
            .iter()
            .map(|e| {
                e.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unnamed error")
                    .to_owned()
            })
            .collect();
        return Err(format!("gql refused: {}", msgs.join("; ")));
    }
    let user = match v.get("data").and_then(|d| d.get("user")) {
        Some(serde_json::Value::Null) => {
            return Err(format!("twitch has no user with login {:?}", handle.trim()))
        }
        Some(u) => u,
        None => return Err("gql answered without a data.user field".to_owned()),
    };
    let display = user
        .get("displayName")
        .and_then(|s| s.as_str())
        .unwrap_or(handle.trim())
        .to_owned();
    let mut ch = Channel::unchecked(&display);
    ch.source = SRC_TWITCH_GQL;
    match user.get("stream") {
        None => return Err("gql user object carries no stream field".to_owned()),
        Some(serde_json::Value::Null) => {
            ch.live = Some(false);
        }
        Some(s) => {
            ch.live = Some(true);
            ch.title = s
                .get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.to_owned());
            ch.viewers = s.get("viewersCount").and_then(|n| n.as_u64());
            ch.game = s
                .get("game")
                .and_then(|g| g.get("name"))
                .and_then(|n| n.as_str())
                .map(|n| n.to_owned());
        }
    }
    Ok(ch)
}

/// Rung 2: decapi.me, plain text.
pub struct Decapi {
    agent: ureq::Agent,
}

impl Default for Decapi {
    fn default() -> Self {
        Self { agent: agent() }
    }
}

impl Source for Decapi {
    fn name(&self) -> &'static str {
        SRC_DECAPI
    }
    fn check(&self, handle: &str) -> Result<Channel, String> {
        let login = twitch_login(handle)?;
        let url = format!("{DECAPI_UPTIME_URL}{login}");
        let mut resp = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .map_err(|e| format!("reading {url} body: {e}"))?;
        parse_decapi(handle, &text)
    }
}

/// Parse decapi's uptime text. Two shapes are facts about the channel; everything else is an error.
///
///   `<login> is offline`                     live false
///   `2 hours, 15 minutes, 3 seconds`          live true (an uptime; decapi carries no title)
///   `[Error from Twitch API] ...`, `User not found`, an HTML page, an empty body: `Err`
pub fn parse_decapi(handle: &str, body: &str) -> Result<Channel, String> {
    static UPTIME: OnceLock<Regex> = OnceLock::new();
    let uptime = UPTIME.get_or_init(|| {
        Regex::new(r"^\d+\s+(second|minute|hour|day|week|month|year)s?\b").expect("static regex")
    });
    let t = body.trim();
    if t.is_empty() {
        return Err("decapi answered with an empty body".to_owned());
    }
    let mut ch = Channel::unchecked(handle.trim());
    ch.source = SRC_DECAPI;
    let lower = t.to_ascii_lowercase();
    if lower.ends_with(" is offline") {
        ch.live = Some(false);
        return Ok(ch);
    }
    if uptime.is_match(t) {
        ch.live = Some(true);
        return Ok(ch);
    }
    /* Keep the first line only: if decapi hands back an HTML error page, the whole thing in a
     * tooltip is noise. */
    let first = t.lines().next().unwrap_or(t);
    let shown: String = first.chars().take(160).collect();
    Err(format!("decapi answered something unrecognised: {shown:?}"))
}

/// Rung 3: Twitch Helix, the official API. The slot for keys.
///
/// EMPTY ON PURPOSE. No credentials exist tonight (D2), and an implementation that pretended to work
/// would either fail identically to this or, worse, silently hit a rate limit. It carries the two
/// fields a real implementation needs and refuses until both are present, so wiring keys later is
/// filling in `check` and nothing else.
#[derive(Default)]
pub struct TwitchHelix {
    pub client_id: Option<String>,
    pub app_token: Option<String>,
}

impl Source for TwitchHelix {
    fn name(&self) -> &'static str {
        SRC_TWITCH_HELIX
    }
    fn check(&self, _handle: &str) -> Result<Channel, String> {
        match (&self.client_id, &self.app_token) {
            (Some(_), Some(_)) => {
                /* Keys present but the request path is not written. Say that rather than "no
                 * credentials", or the person who just pasted keys goes looking for a typo. */
                Err("helix keys are set but the helix request is not implemented yet".to_owned())
            }
            _ => Err("no credentials configured".to_owned()),
        }
    }
}

/// YouTube, best effort, from the `@handle/live` page. Not a rung on the Twitch ladder: it answers
/// for `Status::youtube` alone.
pub struct YoutubePage {
    agent: ureq::Agent,
}

impl Default for YoutubePage {
    fn default() -> Self {
        Self { agent: agent() }
    }
}

impl Source for YoutubePage {
    fn name(&self) -> &'static str {
        SRC_YOUTUBE_PAGE
    }
    fn check(&self, handle: &str) -> Result<Channel, String> {
        let login = youtube_login(handle)
            .ok_or_else(|| format!("youtube handle {handle:?} is not a valid handle"))?;
        let url = format!("https://www.youtube.com/@{login}/live");
        let mut resp = self
            .agent
            .get(&url)
            .header("Accept-Language", "en-US,en;q=0.8")
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let text = resp
            .body_mut()
            .with_config()
            /* Channel pages run past a megabyte. Four is headroom, not a limit anyone should hit. */
            .limit(4 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("reading {url} body: {e}"))?;
        parse_youtube_page(&login, &text)
    }
}

/// Read a YouTube page body for the live markers the player JSON carries.
///
/// `"isLive":true` or `"isLiveNow":true` means a broadcast is up. A page that is recognisably
/// THIS CHANNEL'S page with neither marker is not live: `/live` redirects to the channel home
/// when nothing is on. Two things have to hold before "offline" is said, because the module rule
/// is that a failed poll never says offline: the body carries `ytInitialData` (a YouTube page at
/// all, not a consent wall or a block page), AND it names the channel that was asked for
/// (`youtube_page_is_channel`). A bot check, a sign-in wall or a regional block can embed
/// `ytInitialData` too, and without the second test every one of them would be recorded as a
/// confirmed offline. Both failures are errors, and the error says which test failed.
pub fn parse_youtube_page(handle: &str, body: &str) -> Result<Channel, String> {
    static TITLE: OnceLock<Regex> = OnceLock::new();
    /* Group 1 is the video id, group 2 is the title. The id used to be matched and discarded; it is
     * kept now because `Channel::video_id` is the only way YouTube can be embedded at all, and
     * these are the bytes that carry it. See that field for the measurement. */
    let details_rx = TITLE.get_or_init(|| {
        Regex::new(r#""videoDetails":\{"videoId":"([^"]*)","title":"((?:[^"\\]|\\.)*)""#)
            .expect("static regex")
    });
    if !body.contains("ytInitialData") {
        return Err("youtube page did not look like a channel page (no ytInitialData); consent wall or block?".to_owned());
    }
    if !youtube_page_is_channel(handle, body) {
        return Err(format!("youtube page did not name @{handle} as its channel (no canonicalBaseUrl, vanityChannelUrl or ownerProfileUrl for it); a bot check, sign-in wall or regional block?"));
    }
    let mut ch = Channel::unchecked(handle);
    ch.source = SRC_YOUTUBE_PAGE;
    let live = body.contains(r#""isLive":true"#) || body.contains(r#""isLiveNow":true"#);
    ch.live = Some(live);
    if live {
        let caps = details_rx.captures(body);
        ch.title = caps.as_ref().and_then(|c| c.get(2)).and_then(|m| {
            /* The capture is the raw JSON string contents; wrap it back in quotes and let serde
             * undo the escapes rather than hand-rolling &. */
            serde_json::from_str::<String>(&format!("\"{}\"", m.as_str())).ok()
        });
        /* An empty id is no id. A live page with a blank `videoId` is a page whose player JSON is
         * not what this regex expects, and handing "" to an embed URL builds a link to nothing. */
        ch.video_id = caps
            .as_ref()
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
            .filter(|id| !id.is_empty());
    }
    Ok(ch)
}

/// Does this body identify itself as `@handle`'s page? YouTube's channel and player JSON carry
/// the handle in one of three places: `"canonicalBaseUrl":"/@handle"` (the channel header),
/// `"vanityChannelUrl":"http://www.youtube.com/@handle"` and `"ownerProfileUrl":".../@handle"`
/// (the player's microformat on a live page). Compared case-insensitively, because YouTube
/// prints the handle in the channel's own casing whatever was typed into Settings. An
/// interstitial that merely echoes the requested URL (a sign-in wall's `continue=` link) does not
/// carry any of these keys, which is why the URL alone is not enough.
pub fn youtube_page_is_channel(handle: &str, body: &str) -> bool {
    let want = format!("/@{}", handle.trim().trim_start_matches('@')).to_ascii_lowercase();
    let lower = body.to_ascii_lowercase();
    [
        r#""canonicalbaseurl":""#,
        r#""vanitychannelurl":""#,
        r#""ownerprofileurl":""#,
    ]
    .iter()
    .any(|key| {
        let mut from = 0usize;
        while let Some(pos) = lower[from..].find(key) {
            let at = from + pos + key.len();
            let rest = &lower[at..];
            /* the value is a URL or a path; the handle path must appear at its end, before the
             * closing quote, so `/@handle2` does not pass for `/@handle` */
            let end = rest.find('"').unwrap_or(rest.len());
            let value = rest[..end].trim_end_matches('/');
            if value.ends_with(&want) {
                return true;
            }
            from = at;
        }
        false
    })
}

/* ------------------------------------------------------------------ the rule -- */

/// Fold one poll's outcome into the last known channel. THE rule of this module.
///
/// Success: the new channel replaces the old one, `checked_at` is stamped now, `error` is cleared.
/// Failure: every state field is kept as it was, `checked_at` is kept as it was, and only `error`
/// changes. In particular `live` is never set to `Some(false)` by a failure. An unchecked channel
/// that fails stays `live: None`, which the UI renders as "unknown", not "offline".
pub fn merge(prev: &Channel, outcome: Result<Channel, String>, now: DateTime<Utc>) -> Channel {
    match outcome {
        Ok(mut fresh) => {
            fresh.checked_at = Some(now);
            fresh.error = None;
            fresh
        }
        Err(e) => {
            let mut kept = prev.clone();
            kept.error = Some(e);
            kept
        }
    }
}

/// Try each source in order; the first `Ok` wins. On total failure the error names every rung and
/// what it said, so the tooltip can show "twitch-gql: timed out; decapi: 503; twitch-helix: no
/// credentials configured" rather than a bare "failed".
pub fn try_sources(sources: &[Box<dyn Source>], handle: &str) -> Result<Channel, String> {
    let mut reasons: Vec<String> = Vec::with_capacity(sources.len());
    for s in sources {
        match s.check(handle) {
            Ok(ch) => return Ok(ch),
            Err(e) => {
                log::warn!("live status: {} failed for {handle}: {e}", s.name());
                reasons.push(format!("{}: {e}", s.name()));
            }
        }
    }
    if reasons.is_empty() {
        return Err("no live status sources configured".to_owned());
    }
    Err(reasons.join("; "))
}

/* ---------------------------------------------------------------- the thread -- */

/// The poller. `start` spawns one background thread; `status` reads what it last stored. Dropping
/// the watcher asks the thread to stop at its next wake and does not wait for it: a request can be
/// mid-flight for up to `HTTP_TIMEOUT` and the UI thread must not stall on that.
pub struct Watcher {
    shared: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    nudge: Arc<AtomicBool>,
}

impl Watcher {
    /// Spawn the poll thread with the production ladder: GQL, decapi, Helix, and the YouTube page.
    /// Never blocks; the first result lands within one request's time.
    ///
    /// IT TAKES NOTHING because there is nothing left to pass: both handles are constants and the
    /// interval is `POLL_EVERY`, which is deliberately not a setting (a user-tunable poll rate
    /// against an unauthenticated endpoint is a rate-limit incident waiting to happen, and the
    /// pill's age already tells the reader how fresh the answer is).
    pub fn start() -> Watcher {
        let ladder: Vec<Box<dyn Source>> = vec![
            Box::new(TwitchGql::default()),
            Box::new(Decapi::default()),
            Box::new(TwitchHelix::default()),
        ];
        Self::spawn(POLL_EVERY, ladder, Box::new(YoutubePage::default()))
    }

    /// The same thread with caller-supplied sources and a caller-chosen interval. What the tests
    /// use to prove the thread's behaviour without a network and without a 90 second wait; what a
    /// future key-bearing Helix gets wired through.
    pub fn spawn(
        interval: Duration,
        ladder: Vec<Box<dyn Source>>,
        youtube: Box<dyn Source>,
    ) -> Watcher {
        let shared = Arc::new(Mutex::new(Status::initial()));
        let stop = Arc::new(AtomicBool::new(false));
        let nudge = Arc::new(AtomicBool::new(false));
        let w = Watcher {
            shared: shared.clone(),
            stop: stop.clone(),
            nudge: nudge.clone(),
        };

        let twitch_handle = crate::settings::TWITCH_HANDLE;
        let yt_handle = crate::settings::YOUTUBE_HANDLE;

        let spawned = thread::Builder::new()
            .name("live-status".to_owned())
            .spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let now = Utc::now();
                    let twitch = try_sources(&ladder, twitch_handle);
                    {
                        let mut st = lock(&shared);
                        let before = st.twitch.live;
                        st.twitch = merge(&st.twitch, twitch, now);
                        if before != st.twitch.live {
                            log::info!(
                                "live status: {} live={:?} via {}",
                                st.twitch.handle,
                                st.twitch.live,
                                st.twitch.source
                            );
                        }
                    }
                    {
                        let out = youtube.check(yt_handle);
                        let mut st = lock(&shared);
                        let prev = st.youtube.clone();
                        st.youtube = merge(&prev, out, Utc::now());
                    }
                    /* Sleep in slices so a stop or a nudge is honoured within a quarter second rather
                     * than at the end of a 90 second nap. */
                    let deadline = Instant::now() + interval;
                    while Instant::now() < deadline {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if nudge.swap(false, Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            });
        if let Err(e) = spawned {
            /* No thread means no polls, ever. Say so in the status rather than leaving "unchecked"
             * to look like a poll that has not landed yet. */
            let mut st = lock(&w.shared);
            st.twitch.error = Some(format!("could not start the live status thread: {e}"));
            st.youtube.error = Some(format!("could not start the live status thread: {e}"));
        }
        w
    }

    /// The last known state. A clone of two small structs; safe to call every frame.
    pub fn status(&self) -> Status {
        lock(&self.shared).clone()
    }

    /// Ask for a poll now rather than at the next tick. Returns immediately. The Watch screen's
    /// "Check now" button reaches this through `Ask::CheckLive`, which the App answers.
    pub fn refresh(&self) {
        self.nudge.store(true, Ordering::Relaxed);
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A poisoned mutex holds a perfectly good `Status`; a panic on the poll thread must not take the
/// UI thread with it.
fn lock(m: &Mutex<Status>) -> std::sync::MutexGuard<'_, Status> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/* ------------------------------------------------------------------ the tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::atomic::AtomicUsize;

    /// The response D2 measured on 2026-09-02, byte for byte.
    const GQL_MEASURED_OFFLINE: &str =
        r#"{"data":{"user":{"id":"29737511","displayName":"Broken_Stoic","stream":null}}}"#;

    /// The same shape with `stream` populated, as Twitch returns it while a broadcast is up.
    const GQL_LIVE_SHAPED: &str = r#"{"data":{"user":{"id":"29737511","displayName":"Broken_Stoic","stream":{"id":"41234567890","title":"poSky keys with the guild","viewersCount":37,"game":{"name":"EverQuest"}}}}}"#;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_756_800_000 + secs, 0)
            .single()
            .expect("fixed timestamp")
    }

    #[test]
    fn gql_measured_response_is_offline_not_error() {
        let ch = parse_gql("Broken_Stoic", GQL_MEASURED_OFFLINE).expect("the measured body parses");
        assert_eq!(ch.live, Some(false));
        assert_eq!(ch.handle, "Broken_Stoic");
        assert_eq!(ch.source, SRC_TWITCH_GQL);
        assert_eq!(ch.title, None);
        assert_eq!(ch.viewers, None);
        assert_eq!(ch.game, None);
        assert_eq!(ch.checked_at, None, "parse does not stamp time; merge does");
    }

    #[test]
    fn gql_live_shaped_response_carries_title_viewers_game() {
        let ch = parse_gql("broken_stoic", GQL_LIVE_SHAPED).expect("live body parses");
        assert_eq!(ch.live, Some(true));
        assert_eq!(
            ch.handle, "Broken_Stoic",
            "displayName wins over the typed casing"
        );
        assert_eq!(ch.title.as_deref(), Some("poSky keys with the guild"));
        assert_eq!(ch.viewers, Some(37));
        assert_eq!(ch.game.as_deref(), Some("EverQuest"));
        assert_eq!(ch.source, SRC_TWITCH_GQL);
    }

    #[test]
    fn gql_live_with_null_game_still_live() {
        let body = r#"{"data":{"user":{"id":"1","displayName":"x","stream":{"id":"2","title":"t","viewersCount":0,"game":null}}}}"#;
        let ch = parse_gql("x", body).unwrap();
        assert_eq!(ch.live, Some(true));
        assert_eq!(ch.game, None);
        assert_eq!(ch.viewers, Some(0));
    }

    #[test]
    fn gql_unknown_login_is_an_error_not_offline() {
        let body = r#"{"data":{"user":null}}"#;
        let e = parse_gql("brokn_stoic", body).expect_err("a typo must not read as offline");
        assert!(e.contains("brokn_stoic"), "the error names the login: {e}");
    }

    #[test]
    fn gql_errors_array_is_an_error() {
        let body = r#"{"errors":[{"message":"service unavailable"}],"data":null}"#;
        let e = parse_gql("x", body).expect_err("errors array is a refusal");
        assert!(e.contains("service unavailable"), "{e}");
    }

    #[test]
    fn gql_non_json_is_an_error() {
        assert!(parse_gql("x", "<html>rate limited</html>").is_err());
        assert!(parse_gql("x", "").is_err());
    }

    #[test]
    fn gql_query_escapes_the_login() {
        let q = TwitchGql::query_for("broken_stoic");
        assert!(q.contains(r#"user(login:"broken_stoic")"#), "{q}");
        let q = TwitchGql::query_for("a\"b");
        assert!(
            q.contains(r#"login:"a\"b""#),
            "a quote in the login is escaped, not a break-out: {q}"
        );
    }

    #[test]
    fn twitch_login_is_validated_and_folded() {
        assert_eq!(twitch_login("Broken_Stoic").unwrap(), "broken_stoic");
        assert_eq!(twitch_login("  Broken_Stoic ").unwrap(), "broken_stoic");
        assert!(twitch_login("").is_err());
        assert!(twitch_login("has space").is_err());
        assert!(twitch_login("semi;colon").is_err());
        assert!(
            twitch_login("abcdefghijklmnopqrstuvwxyz").is_err(),
            "26 chars is over Twitch's 25"
        );
    }

    #[test]
    fn decapi_offline_text_is_live_false() {
        let ch = parse_decapi("Broken_Stoic", "broken_stoic is offline").unwrap();
        assert_eq!(ch.live, Some(false));
        assert_eq!(ch.source, SRC_DECAPI);
        assert_eq!(ch.handle, "Broken_Stoic");
        /* trailing newline, as a plain text endpoint tends to send */
        assert_eq!(
            parse_decapi("x", "x is offline\n").unwrap().live,
            Some(false)
        );
    }

    #[test]
    fn decapi_uptime_text_is_live_true() {
        for s in [
            "2 hours, 15 minutes, 3 seconds",
            "1 minute",
            "45 seconds",
            "1 day, 2 hours\n",
        ] {
            let ch = parse_decapi("Broken_Stoic", s).unwrap_or_else(|e| panic!("{s:?}: {e}"));
            assert_eq!(ch.live, Some(true), "{s:?}");
            assert_eq!(ch.title, None, "decapi carries no title; none is invented");
            assert_eq!(ch.viewers, None);
        }
    }

    #[test]
    fn decapi_unknown_text_is_an_error() {
        assert!(parse_decapi("x", "[Error from Twitch API] User not found").is_err());
        assert!(parse_decapi("x", "").is_err());
        assert!(parse_decapi("x", "<html><body>503</body></html>").is_err());
        assert!(
            parse_decapi("x", "offline").is_err(),
            "a bare word is not the documented shape"
        );
    }

    #[test]
    fn helix_without_keys_refuses() {
        let h = TwitchHelix::default();
        assert_eq!(h.name(), SRC_TWITCH_HELIX);
        assert_eq!(
            h.check("broken_stoic").unwrap_err(),
            "no credentials configured"
        );
        /* one key is not credentials */
        let half = TwitchHelix {
            client_id: Some("id".into()),
            app_token: None,
        };
        assert_eq!(
            half.check("broken_stoic").unwrap_err(),
            "no credentials configured"
        );
        /* both keys: still refuses, but says why, and never returns a Channel it did not fetch */
        let both = TwitchHelix {
            client_id: Some("id".into()),
            app_token: Some("tok".into()),
        };
        let e = both.check("broken_stoic").unwrap_err();
        assert!(e.contains("not implemented"), "{e}");
    }

    #[test]
    fn failed_poll_keeps_previous_and_never_flips_live() {
        /* Known live at t=0. */
        let mut prev = Channel::unchecked("Broken_Stoic");
        prev.live = Some(true);
        prev.title = Some("raid night".to_owned());
        prev.viewers = Some(50);
        prev.game = Some("EverQuest".to_owned());
        prev.checked_at = Some(at(0));
        prev.source = SRC_TWITCH_GQL;

        let after = merge(
            &prev,
            Err("twitch-gql: timed out; decapi: 503".to_owned()),
            at(90),
        );
        assert_eq!(after.live, Some(true), "a failed request is not an offline");
        assert_eq!(after.title, prev.title);
        assert_eq!(after.viewers, prev.viewers);
        assert_eq!(after.game, prev.game);
        assert_eq!(
            after.checked_at,
            Some(at(0)),
            "checked_at is only stamped by a success"
        );
        assert_eq!(
            after.source, SRC_TWITCH_GQL,
            "the source that last answered stays named"
        );
        assert_eq!(
            after.error.as_deref(),
            Some("twitch-gql: timed out; decapi: 503")
        );
        assert_eq!(
            crate::titlebar::age_of(after.checked_at, at(90)).as_deref(),
            Some("1m"),
            "and the age reflects the last SUCCESS"
        );
    }

    #[test]
    fn failed_first_poll_stays_unknown_not_offline() {
        let prev = Channel::unchecked("Broken_Stoic");
        let after = merge(&prev, Err("no route to host".to_owned()), at(0));
        assert_eq!(after.live, None, "never Some(false) from a failure");
        assert_eq!(after.checked_at, None);
        assert_eq!(after.source, SRC_UNCHECKED);
        assert!(after.error.is_some());
        assert_eq!(
            crate::titlebar::age_of(after.checked_at, at(0)),
            None,
            "no check, no age"
        );
    }

    #[test]
    fn successful_poll_replaces_state_stamps_time_clears_error() {
        let mut prev = Channel::unchecked("Broken_Stoic");
        prev.live = Some(true);
        prev.error = Some("earlier failure".to_owned());
        prev.checked_at = Some(at(0));
        let fresh = parse_gql("Broken_Stoic", GQL_MEASURED_OFFLINE).unwrap();
        let after = merge(&prev, Ok(fresh), at(90));
        assert_eq!(
            after.live,
            Some(false),
            "a source SAID offline, so offline it is"
        );
        assert_eq!(after.checked_at, Some(at(90)));
        assert_eq!(after.error, None);
        assert_eq!(after.source, SRC_TWITCH_GQL);
    }

    /* ---- the ladder, with fakes ---- */

    struct Fake {
        name: &'static str,
        answer: Result<Option<bool>, &'static str>,
        calls: Arc<AtomicUsize>,
    }

    impl Fake {
        fn boxed(
            name: &'static str,
            answer: Result<Option<bool>, &'static str>,
        ) -> (Box<dyn Source>, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Box::new(Fake {
                    name,
                    answer,
                    calls: calls.clone(),
                }),
                calls,
            )
        }
    }

    impl Source for Fake {
        fn name(&self) -> &'static str {
            self.name
        }
        fn check(&self, handle: &str) -> Result<Channel, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.answer {
                Ok(live) => {
                    let mut ch = Channel::unchecked(handle);
                    ch.live = live;
                    ch.source = self.name;
                    Ok(ch)
                }
                Err(e) => Err(e.to_owned()),
            }
        }
    }

    #[test]
    fn ladder_tries_in_order_and_first_ok_wins() {
        let (a, a_calls) = Fake::boxed("first", Err("down"));
        let (b, b_calls) = Fake::boxed("second", Ok(Some(true)));
        let (c, c_calls) = Fake::boxed("third", Ok(Some(false)));
        let ladder = vec![a, b, c];
        let ch = try_sources(&ladder, "broken_stoic").unwrap();
        assert_eq!(ch.source, "second");
        assert_eq!(ch.live, Some(true));
        assert_eq!(a_calls.load(Ordering::SeqCst), 1);
        assert_eq!(b_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            c_calls.load(Ordering::SeqCst),
            0,
            "a rung after the winner is not asked"
        );
    }

    #[test]
    fn ladder_total_failure_names_every_rung() {
        let (a, _) = Fake::boxed("twitch-gql", Err("timed out"));
        let (b, _) = Fake::boxed("decapi", Err("http status: 503"));
        let (c, _) = Fake::boxed("twitch-helix", Err("no credentials configured"));
        let e = try_sources(&[a, b, c], "broken_stoic").unwrap_err();
        assert_eq!(e, "twitch-gql: timed out; decapi: http status: 503; twitch-helix: no credentials configured");
    }

    fn wait_for<F: Fn(&Status) -> bool>(w: &Watcher, ok: F) -> Status {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let st = w.status();
            if ok(&st) {
                return st;
            }
            assert!(
                Instant::now() < deadline,
                "poll thread never stored a result: {st:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn thread_stores_result_and_status_is_readable_without_blocking() {
        let (src, calls) = Fake::boxed("fake-gql", Ok(Some(true)));
        let (yt, _) = Fake::boxed("fake-youtube", Ok(Some(false)));
        let w = Watcher::spawn(Duration::from_secs(3600), vec![src], yt);
        /* start() returns before the first poll lands; the initial state is unchecked, not offline */
        let first = w.status();
        assert!(first.twitch.live.is_none() || first.twitch.live == Some(true));
        let st = wait_for(&w, |s| s.twitch.checked_at.is_some());
        assert_eq!(st.twitch.live, Some(true));
        assert_eq!(st.twitch.source, "fake-gql");
        assert_eq!(st.twitch.error, None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        /* a nudge triggers another poll well before the hour is up */
        w.refresh();
        let deadline = Instant::now() + Duration::from_secs(5);
        while calls.load(Ordering::SeqCst) < 2 {
            assert!(
                Instant::now() < deadline,
                "refresh() did not wake the poll thread"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// THE WATCHER POLLS BROKEN STOIC ON BOTH SERVICES, AND NOTHING TELLS IT TO.
    ///
    /// This is the test the unit turns on. `Watcher::spawn` is handed sources and an interval and
    /// no handles at all, so the only place the thread can get a channel from is
    /// `settings::TWITCH_HANDLE` and `settings::YOUTUBE_HANDLE`. Each `Fake` echoes back the
    /// handle it was asked for as `Channel::handle`, which is what makes the assertion a
    /// measurement of the argument the thread actually passed rather than of a label.
    ///
    /// THE PREDECESSORS THIS REPLACES both drove a handle in through `WatchConfig`:
    /// `empty_youtube_handle_means_none_and_no_request` proved an empty field switched YouTube
    /// off, a state a constant cannot be in, and `configured_youtube_handle_is_polled_and_labelled`
    /// proved a filled field was polled, using `@SomeHandle`, a channel that does not exist. Both
    /// are gone with the field.
    #[test]
    fn the_watcher_polls_broken_stoic_on_both_services() {
        let (tw, tw_calls) = Fake::boxed("fake-gql", Ok(Some(false)));
        let (yt, yt_calls) = Fake::boxed("fake-youtube", Ok(Some(true)));
        let w = Watcher::spawn(Duration::from_secs(3600), vec![tw], yt);
        let st = wait_for(&w, |s| {
            s.twitch.checked_at.is_some() && s.youtube.checked_at.is_some()
        });
        assert_eq!(st.twitch.handle, crate::settings::TWITCH_HANDLE);
        assert_eq!(st.twitch.handle, "Broken_Stoic");
        assert_eq!(st.twitch.live, Some(false));
        assert_eq!(st.youtube.handle, crate::settings::YOUTUBE_HANDLE);
        assert_eq!(st.youtube.handle, "broken_stoic");
        assert_eq!(st.youtube.live, Some(true));
        assert_eq!(st.youtube.source, "fake-youtube");
        assert_eq!(tw_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            yt_calls.load(Ordering::SeqCst),
            1,
            "YouTube is polled every cycle now, not only once a handle is typed"
        );
    }

    /// The constants are the ONLY handles production passes, so the two guards that stand between
    /// them and a URL have to accept them. `youtube_login` also has to return the constant
    /// unchanged, because `settings::YOUTUBE_HANDLE` is documented as already being the bare form
    /// the `/@{login}/live` URL wants and `Status::initial` uses it without normalising.
    #[test]
    fn the_channel_constants_pass_the_guards_that_build_the_urls() {
        assert_eq!(
            twitch_login(crate::settings::TWITCH_HANDLE).as_deref(),
            Ok("broken_stoic")
        );
        assert_eq!(
            youtube_login(crate::settings::YOUTUBE_HANDLE).as_deref(),
            Some(crate::settings::YOUTUBE_HANDLE),
            "the constant is stored in the bare form, so normalising it is a no-op"
        );
    }

    #[test]
    fn thread_failure_leaves_live_unknown_with_error() {
        let (src, _) = Fake::boxed("fake-gql", Err("no route to host"));
        let (yt, _) = Fake::boxed("fake-youtube", Err("no route to host"));
        let w = Watcher::spawn(Duration::from_secs(3600), vec![src], yt);
        let st = wait_for(&w, |s| s.twitch.error.is_some());
        assert_eq!(st.twitch.live, None, "never Some(false) from a failure");
        assert_eq!(st.twitch.checked_at, None);
        assert_eq!(
            st.twitch.error.as_deref(),
            Some("fake-gql: no route to host")
        );
    }

    /* ---- youtube page reading ---- */

    #[test]
    fn youtube_page_live_marker_reads_live_with_title() {
        let body = r#"<html><script>var ytInitialData = {};</script><script>var ytInitialPlayerResponse = {"videoDetails":{"videoId":"abc123","title":"Sky keys & \"motes\"","isLive":true},"microformat":{"playerMicroformatRenderer":{"isLiveNow":true,"ownerProfileUrl":"http://www.youtube.com/@SomeHandle"}}};</script></html>"#;
        let ch = parse_youtube_page("SomeHandle", body).unwrap();
        assert_eq!(ch.live, Some(true));
        assert_eq!(ch.source, SRC_YOUTUBE_PAGE);
        assert_eq!(
            ch.title.as_deref(),
            Some("Sky keys & \"motes\""),
            "JSON escapes undone"
        );
    }

    /// THE VIDEO ID IS THE ONLY THING THAT CAN BE EMBEDDED, so the poll that already reads these
    /// bytes has to keep it. `/embed/live_stream?channel=<UC id>` was measured failing with IFrame
    /// API error 150 for a live channel and an offline one alike, while `/embed/<video id>` from
    /// the same origin plays; see `Channel::video_id`. Without this the Watch screen can know that
    /// Broken Stoic is live on YouTube and still have nothing to put on the screen.
    ///
    /// Live and offline are both walked, because the id is only read on the live arm and an id
    /// left over from a previous poll would point the player at a broadcast that has ended.
    #[test]
    fn a_live_youtube_page_yields_the_video_id_and_an_offline_one_yields_none() {
        let live = r#"<html><script>var ytInitialData = {};</script><script>var ytInitialPlayerResponse = {"videoDetails":{"videoId":"rFZHOHl-L8A","title":"Sky keys","isLive":true},"microformat":{"playerMicroformatRenderer":{"isLiveNow":true,"ownerProfileUrl":"http://www.youtube.com/@SomeHandle"}}};</script></html>"#;
        let ch = parse_youtube_page("SomeHandle", live).unwrap();
        assert_eq!(ch.live, Some(true));
        assert_eq!(
            ch.video_id.as_deref(),
            Some("rFZHOHl-L8A"),
            "the id beside the title, not the channel id and not the title"
        );

        let off = r#"var ytInitialData = {"header":{"canonicalBaseUrl":"/@SomeHandle"}}; var ytInitialPlayerResponse = {"videoDetails":{"videoId":"staleABC123","title":"yesterday","isLive":false}};"#;
        let ch = parse_youtube_page("SomeHandle", off).unwrap();
        assert_eq!(ch.live, Some(false));
        assert_eq!(
            ch.video_id, None,
            "an offline page carries an id for something that is NOT live; embedding it would put \
             a finished broadcast on screen under a LIVE heading"
        );

        /* A live page whose player JSON is not the shape this regex expects has no id, and no id
         * is `None`, not `Some("")`, which would build an embed URL pointing at nothing. */
        let blank = r#"<html>ytInitialData {"header":{"canonicalBaseUrl":"/@SomeHandle"}} {"videoDetails":{"videoId":"","title":"x","isLive":true}}</html>"#;
        let ch = parse_youtube_page("SomeHandle", blank).unwrap();
        assert_eq!(ch.live, Some(true));
        assert_eq!(ch.video_id, None, "an empty id is no id");
    }

    #[test]
    fn youtube_page_without_live_marker_is_offline() {
        let body = r#"<html><script>var ytInitialData = {"contents":{},"metadata":{"channelMetadataRenderer":{"vanityChannelUrl":"http://www.youtube.com/@somehandle"}}};</script></html>"#;
        let ch = parse_youtube_page("SomeHandle", body).unwrap();
        assert_eq!(ch.live, Some(false));
        assert_eq!(ch.title, None);
        /* a false marker is offline too */
        let body = r#"var ytInitialData = {"header":{"canonicalBaseUrl":"/@SomeHandle"}}; var ytInitialPlayerResponse = {"videoDetails":{"isLive":false}};"#;
        assert_eq!(
            parse_youtube_page("SomeHandle", body).unwrap().live,
            Some(false)
        );
    }

    #[test]
    fn youtube_consent_wall_is_an_error_not_offline() {
        let body = "<html><body>Before you continue to YouTube</body></html>";
        assert!(parse_youtube_page("SomeHandle", body).is_err());
    }

    /// The trap the first cut had: a page that carries `ytInitialData` but is not the channel's
    /// page (a bot check, a sign-in wall, a regional block that still embeds the shell) was
    /// recorded as a confirmed offline. It is a failed poll and says which test failed.
    #[test]
    fn youtube_interstitial_with_ytinitialdata_is_an_error_not_offline() {
        let body = r#"<html><script>var ytInitialData = {"contents":{"errorMessage":"Sorry for the interruption"}};</script><a href="https://accounts.google.com/ServiceLogin?continue=https://www.youtube.com/@SomeHandle/live">Sign in</a></html>"#;
        let e = parse_youtube_page("SomeHandle", body).unwrap_err();
        assert!(e.contains("did not name @SomeHandle"), "{e}");
        /* a page that names ANOTHER channel is not this one's either */
        let other = r#"var ytInitialData = {"header":{"canonicalBaseUrl":"/@SomeHandle2"}};"#;
        assert!(parse_youtube_page("SomeHandle", other).is_err());
        assert!(!youtube_page_is_channel("SomeHandle", other));
        assert!(
            youtube_page_is_channel("somehandle", r#"{"canonicalBaseUrl":"/@SomeHandle"}"#),
            "case folded"
        );
        assert!(
            youtube_page_is_channel(
                "@SomeHandle",
                r#"{"ownerProfileUrl":"http://www.youtube.com/@somehandle/"}"#
            ),
            "a leading @ and a trailing slash are tolerated"
        );
    }

    #[test]
    fn youtube_login_is_validated() {
        assert_eq!(
            youtube_login("@BrokenStoic").as_deref(),
            Some("BrokenStoic")
        );
        assert_eq!(
            youtube_login("Broken.Stoic-1").as_deref(),
            Some("Broken.Stoic-1")
        );
        assert_eq!(youtube_login(""), None);
        assert_eq!(youtube_login("@"), None);
        assert_eq!(youtube_login("ab"), None, "under three chars");
        assert_eq!(youtube_login("has space"), None);
        assert_eq!(youtube_login("slash/../x"), None);
    }

    #[test]
    fn the_state_before_the_first_poll_is_unchecked_on_both_services() {
        assert_eq!(POLL_EVERY, Duration::from_secs(90), "D2's rate");
        let st = Status::initial();
        assert_eq!(st.twitch.handle, crate::settings::TWITCH_HANDLE);
        assert_eq!(st.twitch.live, None);
        assert_eq!(st.twitch.source, SRC_UNCHECKED);
        /* Not an absent channel any more: "nothing known yet" is what unchecked means, and the
         * screen that reads it must not print offline for it. */
        assert_eq!(st.youtube.handle, crate::settings::YOUTUBE_HANDLE);
        assert_eq!(st.youtube.live, None);
        assert_eq!(st.youtube.source, SRC_UNCHECKED);
    }

    /// The FULL body the endpoint returned when probed on 2026-09-02 during this build, byte for
    /// byte. D2 quotes only `data`; the real reply carries `extensions` with a request id and a
    /// timing beside it, and the parser must not care about anything outside `data.user`.
    const GQL_PROBED_FULL: &str = r#"{"data":{"user":{"id":"29737511","displayName":"Broken_Stoic","stream":null}},"extensions":{"durationMilliseconds":42,"requestID":"01M1H2FECKSAHJ4KS3F4JPE29E"}}"#;

    #[test]
    fn gql_probed_full_body_with_extensions_parses_offline() {
        let ch = parse_gql("Broken_Stoic", GQL_PROBED_FULL).expect("the probed body parses");
        assert_eq!(ch.live, Some(false));
        assert_eq!(ch.handle, "Broken_Stoic");
        assert_eq!(ch.source, SRC_TWITCH_GQL);
        assert_eq!(ch.error, None);
    }

    #[test]
    fn request_constants_are_what_d2_measured() {
        assert_eq!(USER_AGENT, "EQL-Grimoire/0.1");
        assert_eq!(HTTP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(POLL_EVERY, Duration::from_secs(90));
        assert_eq!(TWITCH_GQL_URL, "https://gql.twitch.tv/gql");
        assert_eq!(TWITCH_WEB_CLIENT_ID, "kimne78kx3ncx6brgo4mv6wki5h1ko");
        assert_eq!(DECAPI_UPTIME_URL, "https://decapi.me/twitch/uptime/");
    }
}
