//! The channel's own artwork, for the rectangle the player would have taken.
//!
//! WHY THIS EXISTS. With the channel offline the Watch screen had no picture anywhere: a pill, a
//! heading, a last checked line, a disabled button, a sentence, two large buttons and another
//! sentence. The owner, looking at it: "this is the live page shouldnt we show the twitch /
//! youtube offline image instead of this". He is right, and the picture he means is a real thing
//! that exists per channel, not a placeholder.
//!
//! WHERE THE PICTURE COMES FROM, AND WHY IT IS A LADDER LIKE THE WATCHER'S.
//! The streamer's own offline screen is Helix `offline_image_url`, which needs an app token this
//! binary cannot hold. It is ALSO on the unauthenticated GraphQL endpoint the watcher already
//! calls for live status, as `User.offlineImageURL`, and that was measured on 2026-09-03 with the
//! web client's public Client-ID and no account:
//!
//! ```text
//! POST https://gql.twitch.tv/gql   Client-ID: kimne78kx3ncx6brgo4mv6wki5h1ko
//! { user(login:"broken_stoic") { offlineImageURL profileImageURL(width:300) } }
//! HTTP 200
//! {"data":{"user":{"id":"29737511","login":"broken_stoic","displayName":"Broken_Stoic",
//!  "offlineImageURL":"https://static-cdn.jtvnw.net/jtv_user_pictures/ce3293f0-5e9c-4fe0-bb85
//!   -905fd4fcd647-channel_offline_image-1920x1080.png",
//!  "profileImageURL":"https://static-cdn.jtvnw.net/jtv_user_pictures/9d3a94dc-2569-4508-8fa3
//!   -c451d3c0486c-profile_image-300x300.png","stream":null}}}
//! ```
//!
//! So the real artwork IS reachable and it is the first rung. The rungs, in order, and every one
//! of them is a thing the channel's owner uploaded rather than a graphic somebody else drew:
//!
//!   Twitch    1. `offlineImageURL`, the streamer's own offline screen, 1920x1080.
//!             2. `profileImageURL`, the 300x300 avatar, when no offline screen is set.
//!             3. nothing. The screen keeps the words it already had.
//!   YouTube   1. the channel banner from the channel page (`imageBannerViewModel`).
//!             2. the channel page's `og:image`, which is the avatar.
//!             3. nothing.
//!
//! WHAT IS DELIBERATELY NOT A RUNG. Twitch's live preview thumbnail
//! (`.../previews-ttv/live_user_<login>-<w>x<h>.jpg`) answers 200 with about 6.8 KB while the
//! channel is offline, and that body is Twitch's GENERIC offline graphic: the same bytes for every
//! offline channel on the service. Painting it under this channel's name would be presenting
//! somebody else's picture as Broken Stoic's, which is the one thing this screen may not do. It is
//! not fetched and it is not a fallback.
//!
//! THE URL SAYS PNG AND THE BYTES ARE JPEG, MEASURED. The offline screen above is served from a
//! `.png` address with `Content-Type: image/png` and its first bytes are `ff d8 ff`, a JPEG. The
//! format is therefore guessed from the CONTENT and never from the address or the header; a
//! decoder trusting either would fail on the one image this module exists to show.
//!
//! WHAT IT COSTS AND WHERE THAT IS SPENT. A decoded image is uncompressed pixels: the 1920x1080
//! offline screen is 8.3 MB of RGBA at full size, which is not a thing to hand a texture uploader
//! in an app whose whole working set is under 200 MB. Everything is downscaled to
//! [`MAX_ART_W`] wide before it becomes a `ColorImage`, and the `ColorImage` is dropped as soon as
//! egui has uploaded it.
//!
//! THE RULES THAT MATTER MORE THAN THE PICTURE.
//!   LAZY. Nothing here runs until a screen asks for a platform's artwork, which is the first
//!   frame the Watch screen draws with no player in it. Not at startup, not per navigation, and
//!   never per frame: one attempt per platform per run of the app, and [`Slot::started`] is what
//!   makes that true.
//!   OFF THE UI THREAD. The fetch is a worker thread and the frame loop never waits on it. The
//!   agent carries [`crate::watcher::HTTP_TIMEOUT`] and this app's user agent, so a host that
//!   hangs costs a thread ten seconds and costs the reader nothing.
//!   CACHED ON DISK, UNDER THE PER USER FOLDER. `%LOCALAPPDATA%/EQLGrimoire/art`, beside the
//!   WebView2 profile and for the same reason (`crate::player::profile_dir`): people install where
//!   they like, including Program Files, where the folder beside the executable is not writable.
//!   The cache key is the URL, and these URLs carry the image's own id, so a banner the streamer
//!   changes is a different key and a stale picture cannot be served.
//!   SILENT AND HONEST WHEN IT FAILS. No network, no cache, a 404, a body that is not an image the
//!   build can decode: the answer is `None`, the screen keeps its words, and the reason goes to
//!   the log. There is no broken image box and no empty rectangle.
//!
//! WHAT THIS MODULE DOES NOT DECIDE. Whether there is room for a picture, and where it goes, is
//! the screen's business: see `crate::screens::watch::art_band`. This module answers only "is
//! there artwork for this platform, what is it, and what may the screen honestly call it".

use crate::settings::{Platform, DISPLAY_NAME};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, OnceLock};

/* -------------------------------------------------------------- the io switch -- */

/// Whether this module may touch a host at all. OFF until [`allow_fetching`] is called, which
/// `main` does once and nothing else does.
///
/// WHY A SWITCH AND NOT A `cfg(test)`. The Watch screen is drawn by twenty odd tests against a
/// real `egui::Context`, and every one of them draws the offline case, which is the case this
/// module answers. Without a switch `cargo test` would post to `gql.twitch.tv` and pull a channel
/// page off YouTube, which is impolite to two hosts that owe this project nothing and makes the
/// suite depend on a network it should not need. A `cfg(test)` would do that job while changing
/// what the compiled-for-test code DOES, which is the shape of defect that hides in exactly the
/// path nobody runs twice. This is one production flag, on one line of `main`, that a test can
/// read and assert on.
static FETCHING: AtomicBool = AtomicBool::new(false);

/// Let this module make requests. Called by `main` at startup, and by nothing else.
pub fn allow_fetching() {
    FETCHING.store(true, Ordering::Relaxed);
}

/// Whether requests are allowed. `false` in every test binary, because none of them call
/// [`allow_fetching`].
pub fn fetching_allowed() -> bool {
    FETCHING.load(Ordering::Relaxed)
}

/// How many fetches this run has STARTED. Two at the very most, one per platform.
///
/// IT IS HERE BECAUSE A SWITCH NOBODY CAN OBSERVE IS A SWITCH NOBODY CAN TEST. Asserting that
/// `artwork` returns `None` proves nothing about the switch: the first call returns `None` either
/// way, because starting a fetch and having its answer are a frame apart. This is the count the
/// Watch screen's test reads to prove that drawing the offline screen asked no host for anything,
/// and it is the number `spawn` prints when it does ask.
static STARTS: AtomicUsize = AtomicUsize::new(0);

/// The count above. Zero for the whole of a test binary.
pub fn fetches_started() -> usize {
    STARTS.load(Ordering::Relaxed)
}

/* ------------------------------------------------------------------ the limits -- */

/// The widest an image is kept at. A texture is uncompressed: at 960 the 16 by 9 offline screen is
/// 960 by 540, which is 2.07 MB of RGBA, against 8.29 MB at its native 1920 by 1080. The Watch
/// body is 1280 points wide at the default window size and the art band never takes all of it, so
/// 960 is still over one device pixel per point on an ordinary display.
pub const MAX_ART_W: u32 = 960;

/// The most an image body may be. The measured offline screen is 243,386 bytes; a megabyte and a
/// half is generous for anything a channel page serves and small enough that a host answering with
/// something enormous is refused rather than decoded.
pub const MAX_BYTES: u64 = 1_536 * 1024;

/// The channel page, for the YouTube rung. Roughly 1.4 MB measured, so the limit is four.
pub const MAX_PAGE_BYTES: u64 = 4 * 1024 * 1024;

/// The folder under [`crate::player::PROFILE_VENDOR`] that holds cached image bodies.
pub const CACHE_LEAF: &str = "art";

/* ------------------------------------------------------------------- the kinds -- */

/// Which rung answered, which is the only thing the caption is allowed to say.
///
/// THIS IS NOT DECORATION. A 300 by 300 avatar stretched across the player's rectangle and a
/// 1920 by 1080 offline screen look different enough that a reader would ask what they were
/// looking at, and the honest answer differs per rung. `caption` is the one place that answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Twitch `User.offlineImageURL`: the screen the streamer uploaded for exactly this.
    TwitchOffline,
    /// Twitch `User.profileImageURL`: the avatar, when no offline screen is set.
    TwitchProfile,
    /// The YouTube channel banner from the channel page.
    YouTubeBanner,
    /// The YouTube channel page's `og:image`, which is the avatar.
    YouTubeAvatar,
}

impl Kind {
    /// What the screen may call this picture, in one line, naming the channel and the platform.
    ///
    /// THE PROFILE RUNGS SAY WHY THEY ARE THE ONES SHOWING. An avatar under a player rectangle
    /// with no explanation reads as a mistake; "no offline screen is set" is a fact about the
    /// channel that the empty `offlineImageURL` field is exactly the evidence for.
    pub fn caption(self) -> String {
        match self {
            Kind::TwitchOffline => format!("{DISPLAY_NAME}'s own Twitch offline screen"),
            Kind::TwitchProfile => {
                format!(
                    "{DISPLAY_NAME}'s Twitch profile picture. The channel sets no offline screen."
                )
            }
            Kind::YouTubeBanner => format!("{DISPLAY_NAME}'s YouTube channel banner"),
            Kind::YouTubeAvatar => {
                format!(
                    "{DISPLAY_NAME}'s YouTube profile picture. The channel page shows no banner."
                )
            }
        }
    }
}

/// One picture, ready for the screen: the texture and the pixel size it was uploaded at.
///
/// `px` is the DECODED size, after [`MAX_ART_W`], and it is here because the screen letterboxes by
/// aspect ratio and the aspect of an avatar (square) and of a banner (very wide) are not the same.
#[derive(Clone)]
pub struct Art {
    pub kind: Kind,
    pub texture: egui::TextureHandle,
    pub px: egui::Vec2,
}

/* ------------------------------------------------------------------- the store -- */

/// What the worker hands back: the decoded pixels and which rung they came from.
struct Loaded {
    kind: Kind,
    image: egui::ColorImage,
}

/// One platform's state on the UI thread.
#[derive(Default)]
struct Slot {
    /// Has the one attempt been made? This is what stops a fetch per frame and per navigation.
    started: bool,
    /// The worker's channel, until it has answered.
    rx: Option<Receiver<Result<Loaded, String>>>,
    /// The uploaded texture, once there is one. Cloning a `TextureHandle` is an `Arc` bump.
    art: Option<Art>,
}

#[derive(Default)]
struct Store {
    twitch: Slot,
    youtube: Slot,
}

impl Store {
    fn slot(&mut self, on: Platform) -> &mut Slot {
        match on {
            Platform::Twitch => &mut self.twitch,
            Platform::YouTube => &mut self.youtube,
        }
    }
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Store::default()))
}

/// The artwork for one platform, or `None` while there is none.
///
/// CALL IT EVERY FRAME. It is a mutex, a match and a `try_recv`; the fetch behind it happens once.
/// `None` means one of: the worker has not answered yet, or it answered that there is nothing to
/// show. Both are the same thing to the screen, which keeps its words either way.
///
/// THE LOCK IS NEVER HELD ACROSS `load_texture`. The worker sends on a channel and then asks the
/// context to repaint; it never takes this lock, so there is no cycle between this mutex and
/// egui's. Taking the value out and uploading it outside the guard keeps that true even if the
/// worker ever changes.
pub fn artwork(ctx: &egui::Context, on: Platform) -> Option<Art> {
    if !fetching_allowed() {
        return None;
    }
    let ready = {
        let mut st = store().lock().ok()?;
        let slot = st.slot(on);
        if let Some(art) = &slot.art {
            return Some(art.clone());
        }
        if !slot.started {
            slot.started = true;
            slot.rx = Some(spawn(ctx.clone(), on));
            return None;
        }
        match slot.rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(answer)) => {
                slot.rx = None;
                answer
            }
            Some(Err(TryRecvError::Disconnected)) => {
                slot.rx = None;
                Err("the artwork thread stopped without answering".to_owned())
            }
            Some(Err(TryRecvError::Empty)) | None => return None,
        }
    };
    let loaded = match ready {
        Ok(l) => l,
        Err(why) => {
            /* DEGRADE SILENTLY AND HONESTLY: nothing on screen changes, and the reason is in the
             * log for whoever is looking for it. */
            log::warn!("channel art for {}: {why}", on.key());
            return None;
        }
    };
    let px = egui::vec2(loaded.image.size[0] as f32, loaded.image.size[1] as f32);
    /* The one thing that CHANGED, said once per platform per run: which rung answered and how big
     * the texture is. `RUST_LOG=info` is how the pipeline was proved end to end without a screen,
     * and it is what names the rung if the picture on screen ever looks like the wrong one. */
    log::info!(
        "channel art for {}: {:?}, {} by {} texture",
        on.key(),
        loaded.kind,
        loaded.image.size[0],
        loaded.image.size[1]
    );
    let texture = ctx.load_texture(
        format!("channel-art-{}", on.key()),
        loaded.image,
        egui::TextureOptions::LINEAR,
    );
    let art = Art {
        kind: loaded.kind,
        texture,
        px,
    };
    if let Ok(mut st) = store().lock() {
        st.slot(on).art = Some(art.clone());
    }
    Some(art)
}

fn spawn(ctx: egui::Context, on: Platform) -> Receiver<Result<Loaded, String>> {
    STARTS.fetch_add(1, Ordering::Relaxed);
    log::info!(
        "channel art for {}: asking now ({} fetch(es) started this run)",
        on.key(),
        fetches_started()
    );
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(format!("grimoire-art-{}", on.key()))
        .spawn(move || {
            let answer = fetch(on);
            let _ = tx.send(answer);
            /* The frame loop is idle between polls; without this the picture would appear on
             * whatever frame the reader's mouse happened to cause. */
            ctx.request_repaint();
        });
    if let Err(e) = spawned {
        /* A thread that could not start is a failure with a reason, and the receiver is already
         * disconnected, which `artwork` reads as exactly that. */
        log::warn!(
            "channel art for {}: could not start the thread: {e}",
            on.key()
        );
    }
    rx
}

/* ------------------------------------------------------------------ the fetches -- */

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(crate::watcher::HTTP_TIMEOUT))
        .user_agent(crate::watcher::USER_AGENT)
        .http_status_as_error(true)
        .build()
        .new_agent()
}

/// The whole worker: resolve the address, get the bytes, decode them.
fn fetch(on: Platform) -> Result<Loaded, String> {
    let agent = agent();
    let (kind, url) = match on {
        Platform::Twitch => {
            let body = twitch_gql(&agent)?;
            twitch_art_url(&body)?
        }
        Platform::YouTube => {
            let page = youtube_page(&agent)?;
            youtube_art_url(&page)?
        }
    };
    let bytes = bytes_for(&agent, &url)?;
    let image = decode(&bytes)?;
    Ok(Loaded { kind, image })
}

/// The one GQL request, with the same endpoint, Client-ID and login the watcher polls.
fn twitch_gql(agent: &ureq::Agent) -> Result<String, String> {
    let login = serde_json::to_string(&crate::settings::TWITCH_HANDLE.to_ascii_lowercase())
        .map_err(|e| format!("could not quote the twitch login: {e}"))?;
    let query =
        format!("{{ user(login:{login}) {{ offlineImageURL profileImageURL(width:300) }} }}");
    let body = serde_json::json!({ "query": query });
    let url = crate::watcher::TWITCH_GQL_URL;
    let mut resp = agent
        .post(url)
        .header("Client-ID", crate::watcher::TWITCH_WEB_CLIENT_ID)
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("POST {url}: {e}"))?;
    resp.body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()
        .map_err(|e| format!("reading {url} body: {e}"))
}

fn youtube_page(agent: &ureq::Agent) -> Result<String, String> {
    let url = Platform::YouTube.url();
    let mut resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    resp.body_mut()
        .with_config()
        .limit(MAX_PAGE_BYTES)
        .read_to_string()
        .map_err(|e| format!("reading {url} body: {e}"))
}

/// The disk cache first, then the network, then the disk cache is written.
///
/// A CACHE WRITE THAT FAILS IS NOT A FETCH THAT FAILED. A read only profile folder costs a
/// download on the next launch and nothing else, so it is logged and swallowed.
fn bytes_for(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let file = cache_file(url);
    if let Some(path) = &file {
        match std::fs::read(path) {
            Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
            Ok(_) => log::warn!(
                "channel art: cached {} is empty, refetching",
                path.display()
            ),
            Err(_) => { /* not cached yet, which is the ordinary case */ }
        }
    }
    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("reading {url} body: {e}"))?;
    if let Some(path) = &file {
        if let Err(e) = write_cache(path, &bytes) {
            log::warn!("channel art: could not cache {}: {e}", path.display());
        }
    }
    Ok(bytes)
}

fn write_cache(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    /* Written beside and renamed, so a run that dies mid write cannot leave a truncated body that
     * the next run would decode as a corrupt image. */
    let part = path.with_extension("part");
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path)
}

/* -------------------------------------------------------------- the cache path -- */

/// FNV-1a, 64 bit, over the URL's bytes. A file name, not a security claim: it has to be stable
/// across runs and across builds, which is exactly what `std`'s `DefaultHasher` does not promise.
pub fn url_key(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// The cache folder under a given local data directory. Beside the WebView2 profile, under the
/// same vendor folder, for the reason `crate::player::profile_dir_under` gives.
pub fn cache_dir_under(local_data: &Path) -> PathBuf {
    local_data
        .join(crate::player::PROFILE_VENDOR)
        .join(CACHE_LEAF)
}

/// Where one URL's body is cached, or `None` on a system that names no per user local data folder.
/// `None` is a machine that fetches every launch, never a fall back to the executable's folder.
pub fn cache_file(url: &str) -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| cache_dir_under(&d).join(url_key(url)))
}

/* ------------------------------------------------------------------ the parsers -- */

/// Pick the artwork address out of a Twitch GQL response: the offline screen, else the avatar.
///
/// AN ABSENT FIELD, A NULL AND AN EMPTY STRING ARE THE SAME FACT: the channel has not uploaded
/// one. All three fall to the next rung rather than producing an empty URL to fetch.
pub fn twitch_art_url(body: &str) -> Result<(Kind, String), String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("gql answered something that is not JSON: {e}"))?;
    if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
        let msgs: Vec<&str> = errs
            .iter()
            .map(|e| {
                e.get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unnamed error")
            })
            .collect();
        return Err(format!("gql refused: {}", msgs.join("; ")));
    }
    let user = v
        .get("data")
        .and_then(|d| d.get("user"))
        .filter(|u| !u.is_null())
        .ok_or_else(|| "gql answered without a data.user object".to_owned())?;
    let pick = |field: &str| {
        user.get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if let Some(url) = pick("offlineImageURL") {
        return Ok((Kind::TwitchOffline, url));
    }
    if let Some(url) = pick("profileImageURL") {
        return Ok((Kind::TwitchProfile, url));
    }
    Err("this twitch channel carries neither an offline screen nor a profile picture".to_owned())
}

/// Pick the artwork address out of a YouTube channel page: the banner, else the `og:image` avatar.
///
/// THE BANNER IS A LIST OF SIZES AND THE BIGGEST WINS, because the page's own first entry is the
/// 1060 wide one it happens to be laying out and the same object carries the 2560. Downscaling a
/// large source is [`MAX_ART_W`]'s job; upscaling a small one is nobody's.
pub fn youtube_art_url(page: &str) -> Result<(Kind, String), String> {
    if let Some(url) = youtube_banner(page) {
        return Ok((Kind::YouTubeBanner, url));
    }
    if let Some(url) = youtube_og_image(page) {
        return Ok((Kind::YouTubeAvatar, url));
    }
    Err("this youtube channel page carries neither a banner nor an og:image".to_owned())
}

/// The widest `imageBannerViewModel` source on the page, if there is one.
fn youtube_banner(page: &str) -> Option<String> {
    static RX: OnceLock<regex::Regex> = OnceLock::new();
    let rx = RX.get_or_init(|| {
        regex::Regex::new(r#""url":"(https://[^"\\]+)","width":(\d+)"#).expect("static regex")
    });
    let at = page.find("\"imageBannerViewModel\"")?;
    /* A bounded window, so a match cannot be picked up from some unrelated object further down a
     * 1.4 MB page. The banner object measured at just over 2 KB with four sources in it. */
    let end = (at + 8 * 1024).min(page.len());
    let window = page.get(at..end)?;
    rx.captures_iter(window)
        .filter_map(|c| {
            let w: u32 = c.get(2)?.as_str().parse().ok()?;
            Some((w, c.get(1)?.as_str().to_owned()))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, url)| url)
}

/// The page's `og:image`, which on a YouTube channel page is the avatar.
fn youtube_og_image(page: &str) -> Option<String> {
    static RX: OnceLock<regex::Regex> = OnceLock::new();
    let rx = RX.get_or_init(|| {
        regex::Regex::new(r#"og:image"\s+content="(https://[^"]+)""#).expect("static regex")
    });
    rx.captures(page)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/* ------------------------------------------------------------------- the pixels -- */

/// The size an image is kept at: unchanged under [`MAX_ART_W`], and scaled by width over it.
///
/// The height is computed in `u64` and floored at one, so a very wide, very short banner (the
/// measured YouTube one is 2560 by 424) cannot round to a zero height that the decoder would
/// refuse and the resizer would panic on.
pub fn scaled_size(w: u32, h: u32) -> (u32, u32) {
    if w <= MAX_ART_W || w == 0 || h == 0 {
        return (w, h);
    }
    let nh = (u64::from(h) * u64::from(MAX_ART_W) / u64::from(w)).max(1);
    (MAX_ART_W, nh as u32)
}

/// Bytes to pixels, with the format read out of the BYTES.
///
/// NOT OUT OF THE ADDRESS AND NOT OUT OF THE HEADER, and this is measured rather than defensive:
/// Twitch serves the offline screen from a `.png` URL with `Content-Type: image/png` and the body
/// starts `ff d8 ff`, which is a JPEG. This build decodes JPEG and PNG, which is what both hosts
/// were measured serving; anything else is a named refusal and the screen keeps its words.
pub fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let format = image::guess_format(bytes)
        .map_err(|e| format!("the body is not an image this build recognises: {e}"))?;
    let img = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| format!("could not decode the {format:?} body: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(format!("the image has no pixels ({w} by {h})"));
    }
    let (nw, nh) = scaled_size(w, h);
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured response, verbatim from the probe recorded in this module's header.
    const MEASURED: &str = r#"{"data":{"user":{"id":"29737511","login":"broken_stoic","displayName":"Broken_Stoic","offlineImageURL":"https://static-cdn.jtvnw.net/jtv_user_pictures/ce3293f0-5e9c-4fe0-bb85-905fd4fcd647-channel_offline_image-1920x1080.png","profileImageURL":"https://static-cdn.jtvnw.net/jtv_user_pictures/9d3a94dc-2569-4508-8fa3-c451d3c0486c-profile_image-300x300.png","stream":null}},"extensions":{"durationMilliseconds":42}}"#;

    #[test]
    fn the_measured_gql_response_yields_the_offline_screen() {
        let (kind, url) = twitch_art_url(MEASURED).expect("the measured response parses");
        assert_eq!(kind, Kind::TwitchOffline);
        assert_eq!(
            url,
            "https://static-cdn.jtvnw.net/jtv_user_pictures/ce3293f0-5e9c-4fe0-bb85-905fd4fcd647-channel_offline_image-1920x1080.png"
        );
    }

    /// The rung that matters: no offline screen is the AVATAR, and it is labelled as the avatar.
    #[test]
    fn an_empty_offline_screen_falls_to_the_profile_picture() {
        for empty in ["null", "\"\"", "\"   \""] {
            let body = format!(
                r#"{{"data":{{"user":{{"offlineImageURL":{empty},"profileImageURL":"https://x/p.png"}}}}}}"#
            );
            let (kind, url) = twitch_art_url(&body).expect("the profile rung answers");
            assert_eq!(
                kind,
                Kind::TwitchProfile,
                "{empty} means no offline screen was uploaded"
            );
            assert_eq!(url, "https://x/p.png");
        }
        /* And the field being ABSENT is the same fact as it being empty. */
        let (kind, _) =
            twitch_art_url(r#"{"data":{"user":{"profileImageURL":"https://x/p.png"}}}"#)
                .expect("the profile rung answers");
        assert_eq!(kind, Kind::TwitchProfile);
    }

    #[test]
    fn neither_field_is_a_refusal_and_not_an_empty_url() {
        let err = twitch_art_url(r#"{"data":{"user":{"offlineImageURL":""}}}"#)
            .expect_err("nothing to show is an error, never an empty address to fetch");
        assert!(err.contains("neither"), "{err}");
        let err = twitch_art_url(r#"{"data":{"user":null}}"#).expect_err("a null user is refused");
        assert!(err.contains("data.user"), "{err}");
        let err = twitch_art_url(r#"{"errors":[{"message":"service error"}]}"#)
            .expect_err("gql errors are refused");
        assert!(err.contains("service error"), "{err}");
        assert!(twitch_art_url("not json").is_err());
    }

    #[test]
    fn the_youtube_banner_is_the_widest_source_and_beats_the_og_image() {
        let page = r#"<meta property="og:image" content="https://yt3.googleusercontent.com/AVATAR=s900-c-k-c0x00ffffff-no-rj">
          "banner":{"imageBannerViewModel":{"image":{"sources":[
            {"url":"https://yt3.googleusercontent.com/B=w1060-k","width":1060,"height":175},
            {"url":"https://yt3.googleusercontent.com/B=w2560-k","width":2560,"height":424},
            {"url":"https://yt3.googleusercontent.com/B=w1707-k","width":1707,"height":283}]}}}"#;
        let (kind, url) = youtube_art_url(page).expect("the banner rung answers");
        assert_eq!(kind, Kind::YouTubeBanner);
        assert_eq!(url, "https://yt3.googleusercontent.com/B=w2560-k");
    }

    #[test]
    fn a_page_with_no_banner_falls_to_the_og_image_avatar() {
        let page =
            r#"<meta property="og:image" content="https://yt3.googleusercontent.com/AVATAR=s900">"#;
        let (kind, url) = youtube_art_url(page).expect("the avatar rung answers");
        assert_eq!(kind, Kind::YouTubeAvatar);
        assert_eq!(url, "https://yt3.googleusercontent.com/AVATAR=s900");
        assert!(youtube_art_url("<html>nothing here</html>").is_err());
    }

    /// A banner marker with its sources a long way off must not drag in an unrelated `url`/`width`
    /// pair from elsewhere on a 1.4 MB page.
    #[test]
    fn the_banner_window_is_bounded() {
        let page = format!(
            "\"imageBannerViewModel\"{}{}",
            " ".repeat(9 * 1024),
            r#"{"url":"https://elsewhere/x.png","width":4000}"#
        );
        assert!(
            youtube_banner(&page).is_none(),
            "a match past the window is not this banner"
        );
    }

    #[test]
    fn scaled_size_caps_the_width_and_never_returns_a_zero_height() {
        assert_eq!(scaled_size(1920, 1080), (960, 540));
        assert_eq!(
            scaled_size(300, 300),
            (300, 300),
            "small images are untouched"
        );
        assert_eq!(
            scaled_size(960, 540),
            (960, 540),
            "exactly the cap is untouched"
        );
        assert_eq!(scaled_size(2560, 424), (960, 159));
        assert_eq!(
            scaled_size(100_000, 1),
            (960, 1),
            "a hairline banner keeps one row rather than rounding to none"
        );
        assert_eq!(scaled_size(0, 0), (0, 0));
    }

    /// THE ONE THE MEASUREMENT PAID FOR: the offline screen is served from a `.png` address with
    /// `Content-Type: image/png` and its body is a JPEG. A decoder that trusted either would fail
    /// on the only image this module exists to draw.
    #[test]
    fn the_format_comes_from_the_bytes_and_not_from_the_name() {
        /* THE FIRST SIXTEEN BYTES OF THE MEASURED OFFLINE SCREEN, verbatim. Its address ends
         * `.png` and its `Content-Type` is `image/png`; these are the bytes on the wire. */
        let measured_head: &[u8] = &[
            0xff, 0xd8, 0xff, 0xdb, 0x00, 0x84, 0x00, 0x05, 0x03, 0x04, 0x04, 0x04, 0x03, 0x05,
            0x04, 0x04,
        ];
        assert_eq!(
            image::guess_format(measured_head).expect("the measured head is guessable"),
            image::ImageFormat::Jpeg,
            "the body Twitch serves from a .png address with Content-Type image/png is a JPEG, \n             so the format has to come from the bytes"
        );
        /* And a real JPEG body decodes end to end. Encoded here rather than committed as a
         * fixture so the test carries no binary blob and no path. */
        let mut jpeg = Vec::new();
        let pixels = vec![0x40u8; 8 * 8 * 3];
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&pixels, 8, 8, image::ExtendedColorType::Rgb8)
            .expect("the encoder writes a baseline jpeg");
        assert_eq!(&jpeg[..2], &[0xff, 0xd8], "a JPEG starts with SOI");
        let img = decode(&jpeg).expect("a jpeg body decodes with no name to go on");
        assert_eq!(img.size, [8, 8]);
    }

    #[test]
    fn a_body_that_is_not_an_image_is_a_named_refusal() {
        let err = decode(b"<!doctype html><html>404</html>").expect_err("html is not an image");
        assert!(
            err.contains("not an image"),
            "the reason must name what happened: {err}"
        );
    }

    #[test]
    fn url_key_is_stable_and_differs_per_url() {
        let a = url_key("https://static-cdn.jtvnw.net/a-channel_offline_image-1920x1080.png");
        assert_eq!(
            a,
            url_key("https://static-cdn.jtvnw.net/a-channel_offline_image-1920x1080.png")
        );
        assert_ne!(
            a,
            url_key("https://static-cdn.jtvnw.net/b-channel_offline_image-1920x1080.png")
        );
        assert_eq!(
            a.len(),
            16,
            "a fixed length hex name, always a legal file name"
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// The cache goes under the per user local data folder, beside the WebView2 profile, and never
    /// beside the executable. Same argument as `player::profile_dir`, same test shape.
    #[test]
    fn the_cache_lives_under_the_per_user_vendor_folder() {
        let root = Path::new("C:\\Users\\somebody\\AppData\\Local");
        let dir = cache_dir_under(root);
        let under: Vec<String> = dir
            .strip_prefix(root)
            .expect("the cache is under the data folder")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(under, vec![crate::player::PROFILE_VENDOR, CACHE_LEAF]);
    }

    #[test]
    fn every_caption_names_the_channel_and_uses_no_dashes() {
        for kind in [
            Kind::TwitchOffline,
            Kind::TwitchProfile,
            Kind::YouTubeBanner,
            Kind::YouTubeAvatar,
        ] {
            let c = kind.caption();
            assert!(c.contains(DISPLAY_NAME), "{c}");
            assert!(
                !c.contains('\u{2014}') && !c.contains('\u{2013}'),
                "no em or en dashes: {c}"
            );
        }
        assert!(
            Kind::TwitchProfile.caption().contains("no offline screen"),
            "the avatar rung must say why it is the one showing"
        );
    }
}
