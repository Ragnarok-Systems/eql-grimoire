//! The player surface: a WebView2 child window bounded to a rectangle the Watch screen reserves in
//! the BODY of the root window. Decision D1's one webview exception, built rather than deferred.
//!
//! WHY THIS CAN EXIST NOW AND COULD NOT BEFORE. The round-one note in `screens/watch.rs` said a
//! surface was structurally impossible because the Watch screen drew inside a DEFERRED viewport,
//! which exposes no native window handle. That was true of the pop-out window and it is still true
//! of it. It was never true of the body: `App::ui` is the ROOT viewport's callback and eframe hands
//! it an `&mut eframe::Frame`, which implements `HasWindowHandle` for the root window
//! (eframe 0.36 epi.rs:701). `wry::WebViewBuilder::build_as_child` wants exactly that. So the
//! surface lives in the body, the pop-out window keeps the browser path, and
//! [`Player::sync`] is only ever handed a host by the root pass.
//!
//! THE ORIGIN PROBLEM. Twitch refuses to be framed unless `parent=` names the hosting page's
//! domain, and enforces it with a `frame-ancestors` CSP header. A `file://` page has no domain, a
//! `with_html` page has an opaque origin, and a port in `parent` is rejected outright
//! (`302 InvalidCharInParent`). The route that works with no TCP port and no local server is a wry
//! custom protocol with `with_https_scheme(true)`, which on Windows serves the host page from
//! `https://<scheme>.localhost`. So the page is served from [`origin`], `parent` is [`host`], and
//! the two cannot drift, because both are built from [`SCHEME`].
//!
//! WHAT WAS MEASURED, AND THE ONE THING THE BRIEF GOT WRONG.
//! `https://www.youtube.com/embed/live_stream?channel=<UC id>` DOES NOT WORK ANY MORE. It fails
//! with IFrame API error 150 and reports its own `videoId` as the literal string `"live_stream"`,
//! which is YouTube parsing that path segment as a video id rather than as the channel-redirect
//! endpoint it used to be. It fails identically for a channel that is live and one that is not,
//! with and without an `origin` parameter, while `https://www.youtube.com/embed/<video id>` from
//! the SAME origin in the SAME process plays at hd720. So a channel id is not enough to embed
//! YouTube: a live VIDEO ID is required, and [`crate::watcher::Channel::video_id`] is where it
//! comes from. Twitch needs no such thing, because a Twitch channel IS its player address.
//!
//! SOUND. The player starts muted, and nothing here claims otherwise. What the surface reports is
//! not the flag it asked for but `ICoreWebView2_8::IsDocumentPlayingAudio`, the browser's own
//! answer to "is this document emitting audio", read back from native code. That distinction is
//! the whole point: asking a player `isMuted()` returns the player's model of itself, which is the
//! value we set, and a spike that read it back concluded audio was playing when the app in front
//! of a person was silent.

use std::path::{Path, PathBuf};

/* -------------------------------------------------------------------- the names -- */

/// The custom protocol's scheme. Everything else about the origin is derived from it, so the host
/// page's domain and the `parent` Twitch is told to expect cannot disagree.
pub const SCHEME: &str = "grimoire";

/// The vendor folder under the per-user local data directory. See [`profile_dir`].
pub const PROFILE_VENDOR: &str = "EQLGrimoire";

/// The leaf under [`PROFILE_VENDOR`] that holds the WebView2 user data.
pub const PROFILE_LEAF: &str = "WebView2";

/// The domain the host page is served from: `<scheme>.localhost`, which is what wry's Windows
/// custom-protocol handler resolves `<scheme>://localhost/...` to. This is the value Twitch's
/// `parent` parameter must carry, and it has no port, because a port in `parent` is refused.
pub fn host() -> String {
    format!("{SCHEME}.localhost")
}

/// The host page's origin. `https`, because [`with_https_scheme`][1] is set: the `http` variant
/// of the same host was measured BLOCKED by Twitch's `frame-ancestors`, which names the https form
/// only.
///
/// [1]: wry::WebViewBuilderExtWindows::with_https_scheme
pub fn origin() -> String {
    format!("https://{}", host())
}

/// [`origin`] as a URL QUERY VALUE, which is where YouTube wants it. Only `:` and `/` need
/// escaping in `https://grimoire.localhost`, and spelling those two out is smaller and more
/// readable than pulling in an encoder for a string this module builds itself; the test walks the
/// whole value, so a host that ever needs more encoding than this fails rather than half encodes.
pub fn origin_as_query_value() -> String {
    origin().replace(':', "%3A").replace('/', "%2F")
}

/// The address the webview is pointed at. wry rewrites `<scheme>://localhost/<path>` into the
/// platform form, which on Windows with the https scheme is [`origin`]`/<path>`.
pub fn page_url() -> String {
    format!("{SCHEME}://localhost/index.html")
}

/// Twitch's sign-in page. One address for every channel, loaded top level: see
/// [`Feed::TwitchSignIn`].
/// Where to send somebody who has no device code yet. Rarely used: the Watch screen stages this
/// feed from `AuthView::Waiting`, which always carries a real `verification_uri`. It exists so a
/// caller cannot be stuck with no address at all, and because `activate` with no code still
/// prompts for the login, which is the half that signs the video in.
pub fn twitch_sign_in_url() -> String {
    "https://www.twitch.tv/activate".to_owned()
}

/// The channel's own page on YouTube: `https://www.youtube.com/@<handle>`.
///
/// A LEADING `@` IS TRIMMED rather than trusted. `settings::YOUTUBE_HANDLE` stores the bare form
/// and a test in that module holds it there, but this function takes a `&str` and the `@` is how
/// YouTube itself prints the handle, so a caller that pastes the displayed form gets the right URL
/// instead of `/@@broken_stoic`.
///
/// THE HANDLE FORM, AND `screens::watch` MAKES THE OPPOSITE ARGUMENT ABOUT LINKS, ON PURPOSE.
/// That module deleted its own `youtube_url` and routed both browser buttons through
/// `settings::Platform::url`, which uses `settings::YOUTUBE_CHANNEL_ID`, because a handle is the
/// half of a YouTube identity its owner can change and a stale one lands a reader on whoever
/// picked the name up. That argument is about a URL HANDED TO THE SYSTEM BROWSER, where a wrong
/// destination opens outside this app's chrome with this app's maker mark on the click, and where
/// nothing in the app would ever notice it was wrong.
///
/// It does not carry here, for a reason that is measured rather than aesthetic: THIS HANDLE IS
/// ALREADY EXERCISED EVERY POLL. `watcher::YoutubePage` fetches `/@{handle}/live` on every tick of
/// `watcher::POLL_EVERY`, and refuses the answer unless the page names that same handle as its
/// channel (`no canonicalBaseUrl, vanityChannelUrl or ownerProfileUrl for it`). So a handle that
/// changed under this app does not rot silently: it shows up as a YouTube poll failure, in words,
/// on the Watch screen and on this app's own rail square, long before anybody could be sent to a
/// stranger's channel. The id stays the checkable half in `settings.rs` and stays what the browser
/// buttons use.
///
/// AND THE OWNER NAMED THIS URL. `https://www.youtube.com/@broken_stoic` is the destination asked
/// for, which is a product decision about where a row goes rather than a plumbing detail, so it is
/// followed rather than improved on. The `/videos` tab of the same channel was considered and not
/// taken for the same reason: it was not what was asked for, and the channel page leads to it.
pub fn channel_page_url(handle: &str) -> String {
    format!(
        "https://www.youtube.com/@{}",
        handle.trim().trim_start_matches('@')
    )
}

/* --------------------------------------------------------------------- the feed -- */

/// What the surface has been asked to show. There is no "offline" variant on purpose: when the
/// channel is not live there is nothing to embed (on YouTube there is not even an id), so the
/// Watch screen draws words instead of asking for a surface at all.
///
/// TWO OF THESE ARE PLAYERS AND ONE IS A PAGE, and [`load`] is where that parts company. The
/// two players are FRAMED, because Twitch will not be framed by anything that cannot name a real
/// domain in `parent=` and our custom protocol is how it gets one. The page is loaded TOP LEVEL,
/// because it is an ordinary web document and because it could not be framed even if we wanted it
/// to be; see [`load`] for the header that was measured.
///
/// ONE ENUM AND NOT A SECOND "MODE" BESIDE IT. Everything after the address is identical: the same
/// [`Stage`], the same child window, the same physical bounds, the same WebView2 profile, the same
/// tracking prevention readback and the same failure and recovery rules, all of which took several
/// rounds to get right. A parallel mode would have to be threaded through every one of them and
/// would be a second place for each to be got wrong. What actually differs is one match, in one
/// function, and that is what [`load`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    /// A Twitch channel, by login. A channel IS its player address, so no lookup is needed.
    Twitch { login: String },
    /// One YouTube video, by id. A channel id will NOT do; see the module note.
    YouTube { video_id: String },
    /// The channel's own page on YouTube, by handle, as a whole web page rather than a player.
    ///
    /// THIS IS THE VIDEOS SCREEN AND IT IS A YOUTUBE SURFACE ON BOTH SETTINGS. Twitch VODs expire
    /// and YouTube is where the archive lives, so `settings::Platform` decides what Watch live
    /// plays and does not reach this at all. `screens::videos` is the only thing that builds one,
    /// and it builds it from `settings::YOUTUBE_HANDLE`.
    YouTubeChannel { handle: String },
    /// Twitch's own sign-in page, as a whole web page, in this player's own profile.
    ///
    /// THIS IS HOW A SIGNED-IN VIEW HAPPENS, AND THE APP TOUCHES NO CREDENTIAL TO MAKE IT. The
    /// player keeps its own WebView2 profile (`profile_dir`), which is why it never carries the
    /// session in the reader's browser. The same fact cuts the other way: a session made INSIDE
    /// that profile is carried from then on, by the embed as well, because the cookie is set on
    /// `.twitch.tv` and tracking prevention is off on the profile precisely so the framed player
    /// receives it (`surface::disable_tracking_prevention`). So the Watch screen swaps its demand
    /// to this feed, the surface rebuilds top level on `twitch.tv/login`, the reader types on
    /// Twitch's page, and the screen swaps back. Nothing of this app's draws a form, reads a
    /// field, or looks at the store; there is no code path here that could.
    ///
    /// TOP LEVEL, NOT FRAMED, for the same reason the YouTube channel page is: a sign-in page
    /// forbids being framed by anyone, and it would be wrong to want to. `load` answers
    /// `(url, None)` for it. It carries no login because the page is the same for every channel.
    ///
    /// IT NEVER GOES TO THE POP-OUT. `choose_stage` keeps it in the body: the pop-out is a
    /// picture with two chips and no room to read a form, and a sign-in page sitting in a small
    /// always-on-top window over the game is not a thing a reader asked for.
    ///
    /// YOUTUBE HAS NO COUNTERPART, and that is Google's decision rather than this app's: account
    /// sign-in inside an embedded browser is refused outright (measured: 403
    /// `disallowed_useragent`), and spoofing the agent to get past that is a line this app does
    /// not cross.
    /// ONE PAGE THAT DOES BOTH HALVES OF SIGNING IN, which is why it carries an address rather
    /// than being a constant.
    ///
    /// `at` IS THE DEVICE FLOW'S OWN `verification_uri`, which is `twitch.tv/activate` with the
    /// code already in the query. Loading THAT here, rather than `twitch.tv/login`, is what
    /// collapses two sign-ins into one, and the mechanism is worth stating because it looks like
    /// a coincidence and is not:
    ///
    ///   1. The page is behind Twitch's login, so a signed-out reader is shown Twitch's own login
    ///      form first. They type their password on Twitch's page, and Twitch sets its session
    ///      cookie IN THIS PLAYER'S WEBVIEW2 PROFILE, because that is the browser that asked. The
    ///      VIDEO is now signed in: subscriber ad-free, channel points, the view on their account.
    ///   2. The same page then shows the activation prompt for our code, they approve the two
    ///      scopes, and `twitch_auth`'s polling thread is handed a token. CHAT can now send.
    ///
    /// One button, one password typed on Twitch's own page, both halves. This app still never
    /// draws a login form, never sees the password, and never reads the cookie; the cookie simply
    /// belongs to the browser Twitch was talking to, which happens to be ours.
    ///
    /// WHY IT WAS TWO. `twitch.tv/login` was staged here and the token flow lived on the Chat
    /// screen behind its own button, so there were two controls with near-identical names doing
    /// unrelated things. The owner reached for the wrong one three times running, which is not a
    /// mistake three times, it is two doors to one room.
    TwitchSignIn { at: String },
}

/* THERE IS NO `Feed::platform()` HERE AND THAT IS DELIBERATE. It was written, it compiled, its
 * test passed, and a grep for a non-test caller found none: every surface that names a platform
 * already holds `settings::Platform` because it is what CHOSE the feed. A method that only its own
 * test reaches is this codebase's signature defect, and the rule is that it goes back out with the
 * test that was standing in for a caller, not that a caller is invented to keep it. */

/// WHICH WINDOW THE VIDEO IS PARENTED INTO, and everything that placing it there needs.
///
/// THE TWO ARMS CARRY DIFFERENT DATA BECAUSE THEY ARE DIFFERENT PROBLEMS, and that is the whole
/// reason this is an enum rather than a rect and a flag. The body's rectangle is in the ROOT
/// viewport's points and only means anything paired with the ROOT's scale; the pop-out's is its own
/// window's client area, read from the OS in physical pixels at placement time. Holding the rect
/// and the scale as two loose fields made "a root rect paired with a pop-out scale" a thing you
/// could write down, and that pairing is where every cross-monitor DPI bug in this feature would
/// live. Here it cannot be written down at all.
///
/// IT IS CALLED `Seat` AND NOT `Host` ON PURPOSE. `screens::watch::tests::no_other_window_draws_this_screen`
/// forbids the string `Host::` in `main.rs` and `windows.rs`, and that guard is about a deleted
/// argument that named which window a SCREEN was drawn in. This names which window the SURFACE is
/// parented into and never reaches `WatchScreen` at all, so the guard stays literally true and is
/// not weakened to let this through.
#[derive(Clone, Debug, PartialEq)]
pub enum Seat {
    /// The main window's folio. This is the docked state and the default.
    Body {
        /// Where the video goes, in egui points, in the ROOT viewport's coordinates.
        rect: egui::Rect,
        /// The ROOT viewport's scale, which is the only scale that rect means anything in.
        pixels_per_point: f32,
        /// Something egui is drawing this frame would cross `rect`. A native child window owns its
        /// rectangle and egui cannot paint over it, so the surface yields the region rather than
        /// fighting for it: the rule is that nothing overlaps the video, not that the video wins.
        occluded: bool,
    },
    /// The Watch pop-out's client area. This is the undocked state.
    ///
    /// THERE IS NO RECT HERE AND THAT IS DELIBERATE. The video fills the pop-out edge to edge, so
    /// the rectangle is whatever `GetClientRect` says at placement time. Carrying a rect computed
    /// one pass earlier, in another window's coordinate space, is how the video ends up the size
    /// the window used to be.
    PopOut {
        hwnd: isize,
        /// Shapes the video WITHDRAWS from, in the pop-out's own client pixels, so that the
        /// chrome underneath is both visible and clickable. Empty when the pointer is elsewhere.
        ///
        /// This is not app chrome drawn over the video. It is the video made absent from a few
        /// hundred pixels, which is the same posture [`occluded_by_overlays`] already takes with
        /// the whole region. Measured on this machine before it was built: both the pixels and
        /// the pointer fall through.
        carve_px: Vec<(i32, i32, i32, i32)>,
    },
}

/// A seat's identity, without the per-frame geometry. Comparing these is how the surface knows
/// whether it has to move, and the geometry is deliberately not part of that answer: a pop-out
/// being resized is not a reparent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatId {
    Body,
    PopOut(isize),
}

impl SeatId {
    pub fn of(seat: &Seat) -> SeatId {
        match seat {
            Seat::Body { .. } => SeatId::Body,
            Seat::PopOut { hwnd, .. } => SeatId::PopOut(*hwnd),
        }
    }
}

/// One frame's request of the surface. The Watch screen fills it in, the App acts on it, and it is
/// gone by the next frame: a screen never holds the surface, so it can never drive a stale rect.
///
/// THERE IS ONE OF THESE PER FRAME AND IT HAS ONE SEAT. That sentence is the whole concurrency
/// story of this feature: two windows cannot both hold the video, because there is no way to say
/// that they do.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    /// Which window it goes in.
    pub seat: Seat,
    /// What to play there.
    pub feed: Feed,
    /// Start with sound. False is the shipped default and the only value that starts a stream; see
    /// the module note on sound.
    pub sound: bool,
}

impl Stage {
    /// The body's rectangle, when it is the body's. `None` while the pop-out has the video, which
    /// is the honest answer rather than a rectangle in the wrong window's coordinates.
    pub fn body_rect(&self) -> Option<egui::Rect> {
        match &self.seat {
            Seat::Body { rect, .. } => Some(*rect),
            Seat::PopOut { .. } => None,
        }
    }

    pub fn seat_id(&self) -> SeatId {
        SeatId::of(&self.seat)
    }
}

/// The pop-out saying "I can take the video, and here is where it would go".
///
/// IT CARRIES NO FEED AND NO SOUND FLAG, AND THAT ABSENCE IS THE VOLUME REQUIREMENT EXPRESSED AS A
/// TYPE. The surface rebuilds itself whenever `(Feed, bool)` changes, and mute is baked into the
/// embed URL, so a rebuild is a fresh page at the default volume. Denying the pop-out any way to
/// name either value is what makes "the volume carries across the move" an invariant of the
/// design rather than a thing that has to be remembered at each call site.
#[derive(Clone, Debug, PartialEq)]
pub struct PipOffer {
    pub hwnd: isize,
    /// See [`Seat::PopOut::carve_px`].
    pub carve_px: Vec<(i32, i32, i32, i32)>,
}

/// Which seat wins this frame. Pure, total, and the only place the question is answered.
///
/// THE POP-OUT WINS WHENEVER IT IS OPEN AND THERE IS SOMETHING TO PLAY, which is the owner's own
/// description of what should happen: "we should be stoping the video in main window and playing
/// video in the popup". Nothing has to stop the body's video, because there is only ever one
/// surface and moving it IS stopping it here and starting it there. The body's `Stage` is
/// discarded for the frame, so the folio draws its own words instead.
///
/// `demand` HAS ONE PRODUCER, `WatchScreen::demand`, and both seats read it. That is what stops the
/// pop-out and the body disagreeing about the feed or the mute flag for even a single frame, which
/// would be a rebuild, which would be a reset volume.
pub fn choose_stage(
    body: Option<Stage>,
    pip: Option<PipOffer>,
    demand: Option<(Feed, bool)>,
) -> Option<Stage> {
    match (pip, demand) {
        /* A SIGN-IN PAGE STAYS IN THE BODY. See `Feed::TwitchSignIn`: the pop-out has no room
         * to read a form, and the body has just reserved a rectangle for exactly this. */
        (Some(_), Some((Feed::TwitchSignIn { .. }, _))) => body,
        (Some(p), Some((feed, sound))) => Some(Stage {
            seat: Seat::PopOut {
                hwnd: p.hwnd,
                carve_px: p.carve_px,
            },
            feed,
            sound,
        }),
        _ => body,
    }
}

/* ------------------------------------------------------------------ pure rules -- */

/// The per-user WebView2 profile folder, `<local data dir>/EQLGrimoire/WebView2`.
///
/// THIS IS NOT A DETAIL. wry's default is `<exe path>.WebView2`, a sibling of the binary. People
/// install this app where they like, including Program Files, where that path is not writable and
/// the webview fails to start with an error that names none of this. `dirs::data_local_dir` is
/// `%LOCALAPPDATA%` on Windows, which is per user and always writable.
pub fn profile_dir_under(local_data: &Path) -> PathBuf {
    local_data.join(PROFILE_VENDOR).join(PROFILE_LEAF)
}

/// [`profile_dir_under`] against this machine's local data directory, or `None` when the OS does
/// not name one. `None` is reported as a refusal to start the surface, never worked around by
/// falling back to the executable's folder.
pub fn profile_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| profile_dir_under(&d))
}

/// The embed address for one feed, muted or not, or `None` for a feed that is not framed at all.
///
/// YouTube takes `mute`, Twitch takes `muted`, and both are spelled here rather than at the call
/// site so a screen can never hand one platform the other's parameter name.
///
/// `None` IS NOT A FAILURE, IT IS A DIFFERENT KIND OF SURFACE. A channel page is a whole web
/// document loaded top level; there is no iframe for an embed address to go in, and there is no
/// mute flag either. [`load`] is the one function that decides which kind a feed is.
pub fn embed_url(feed: &Feed, sound: bool) -> Option<String> {
    Some(match feed {
        /* NOT FRAMED. See the variants' own notes and [`load`]. */
        Feed::YouTubeChannel { .. } | Feed::TwitchSignIn { .. } => return None,
        /* `parent` carries the bare host and never a port: Twitch answers a port with
         * `302 InvalidCharInParent`. `origin` is not a Twitch parameter and is not sent. */
        Feed::Twitch { login } => format!(
            "https://player.twitch.tv/?channel={}&parent={}&muted={}&autoplay=true",
            login,
            host(),
            if sound { "false" } else { "true" }
        ),
        /* `enablejsapi` is not for us to call: it is what makes the player accept its own
         * postMessage control channel, and without it YouTube logs an origin complaint on every
         * load. `origin` must be percent encoded, which is why it is written out rather than
         * interpolated raw. */
        Feed::YouTube { video_id } => format!(
            "https://www.youtube.com/embed/{}?autoplay=1&mute={}&enablejsapi=1&playsinline=1&origin={}",
            video_id,
            if sound { "0" } else { "1" },
            origin_as_query_value()
        ),
    })
}

/// The host page, in full, as a pure function of the feed and the sound flag, or `None` for a feed
/// that is not framed and therefore has no host page at all.
///
/// SERVED FROM MEMORY. Nothing is read from disk to answer the protocol handler, so there is no
/// file for an installer to miss and no path for a sandbox to refuse. The page is one iframe on
/// black with no chrome of its own, because the app draws none over the video and the page must
/// not either.
pub fn host_page(feed: &Feed, sound: bool) -> Option<String> {
    /* Exhaustive rather than a catch-all, so a fourth feed has to say which kind it is here rather
     * than silently inheriting one. The `?` on the framed arms is `Some` by construction; it is
     * written as a `?` rather than an unwrap because a panic on the UI thread over a page string
     * would take the whole app down for a defect that shows as a black rectangle. */
    let (src, title) = match feed {
        Feed::Twitch { login } => (embed_url(feed, sound)?, format!("{login} on Twitch")),
        Feed::YouTube { video_id } => (embed_url(feed, sound)?, format!("{video_id} on YouTube")),
        /* NOT FRAMED. Nothing of ours is served for a whole page; see [`load`]. */
        Feed::YouTubeChannel { .. } | Feed::TwitchSignIn { .. } => return None,
    };
    Some(format!(
        concat!(
            "<!doctype html>\n",
            "<meta charset=\"utf-8\">\n",
            "<title>{title}</title>\n",
            "<style>\n",
            "  html, body {{ margin:0; padding:0; height:100%; background:#000; overflow:hidden; }}\n",
            "  iframe {{ display:block; width:100%; height:100%; border:0; }}\n",
            "</style>\n",
            "<iframe src=\"{src}\" allow=\"autoplay; encrypted-media; fullscreen; picture-in-picture\" allowfullscreen></iframe>\n"
        ),
        title = title,
        src = src
    ))
}

/// WHAT THE WEBVIEW IS POINTED AT, AND WHAT OUR OWN PROTOCOL SERVES THERE. `(url, served)`.
///
/// `served` is `Some(page)` for a FRAMED feed: `url` is [`page_url`], ours, the custom protocol
/// answers it with `page`, and the platform's player sits in an iframe inside that. The whole
/// indirection exists for one reason, recorded in the module header: Twitch will not be framed
/// unless `parent=` names a real domain, and a wry custom protocol is how this app has one without
/// a TCP port.
///
/// `served` is `None` for a TOP LEVEL feed: `url` is the page itself, the webview fetches it the
/// way a browser would, no protocol is registered and there is no iframe anywhere.
///
/// TOP LEVEL IS A REQUIREMENT FOR THE CHANNEL PAGE AND NOT A SIMPLIFICATION, MEASURED 2026-09-03.
/// `https://www.youtube.com/@broken_stoic`, `/@broken_stoic/videos` and
/// `/channel/UCf4fNJTJt8F1MZAQ2iqIZ9A` each answer `200` carrying `X-Frame-Options: SAMEORIGIN`.
/// None of their three CSP headers carries `frame-ancestors` (they are `script-src`,
/// `require-trusted-types-for` and `base-uri`), so `X-Frame-Options` is the operative control and
/// it forbids the cross origin case outright. Served through the host page the folio would be a
/// blank rectangle, because `https://grimoire.localhost` is not `https://www.youtube.com`. Reusing
/// the host page here would have looked like sharing and would have been a defect.
///
/// ONE FUNCTION AND ONE MATCH, so the address and what is served at it cannot come apart. A tuple
/// rather than an enum because there are exactly two shapes and no third state for a caller to
/// handle: `served.is_some()` IS the question, asked once, in `surface::Player::build`.
pub fn load(feed: &Feed, sound: bool) -> (String, Option<String>) {
    match feed {
        Feed::Twitch { .. } | Feed::YouTube { .. } => (page_url(), host_page(feed, sound)),
        Feed::YouTubeChannel { handle } => (channel_page_url(handle), None),
        /* THE ADDRESS COMES FROM THE FEED, so the page carrying the code is the page loaded. */
        Feed::TwitchSignIn { at } => (at.clone(), None),
    }
}

/// An egui rectangle as WebView2 wants it: PHYSICAL pixels, which is egui points times
/// `pixels_per_point`. Returns `(x, y, width, height)`.
///
/// TWO GUARDS, AND THE PAIR OF CLAMPS THAT USED TO SIT BESIDE THEM IS GONE. A non-finite
/// coordinate becomes zero, and an unusable scale falls back to 1.0. `egui::Rect::NOTHING` is built
/// from infinities and reaches here on the first frame of any screen that reserves space before it
/// has laid anything out, so both are load bearing and both are mutation proved by
/// `a_degenerate_rect_still_yields_numbers`.
///
/// What was removed was `clamp(0.0, u32::MAX)` and `clamp(i32::MIN, i32::MAX)` around the casts.
/// A mutation that deleted them did not turn that test red, which is the honest answer: Rust's
/// float-to-integer casts have saturated since 1.45 (a negative width already casts to 0 and NaN
/// already casts to 0), so those two lines could not change any value this function returns. Code
/// that cannot change an output is the same defect as a control for a feature that does not exist,
/// whichever direction it errs in.
pub fn physical_bounds(rect: egui::Rect, pixels_per_point: f32) -> (i32, i32, u32, u32) {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let px = |v: f32| -> f32 {
        if v.is_finite() {
            (v * ppp).round()
        } else {
            0.0
        }
    };
    (
        px(rect.min.x) as i32,
        px(rect.min.y) as i32,
        px(rect.width()) as u32,
        px(rect.height()) as u32,
    )
}

/* ------------------------------------------------------------------- occlusion -- */

/// Does an egui layer in this order float ABOVE the body, where the video is?
///
/// NOTHING MAY BE DRAWN OVER THE VIDEO, and the reason is not taste. A native child window owns
/// its rectangle: egui paints into the GL surface underneath and the webview is composited over
/// it by the OS, so a tooltip that lands on the video is not drawn on top of it, it is drawn
/// UNDER it and simply disappears. The Twitch Developer Services Agreement is also read as
/// requiring the player to ship as-is, without app chrome over the video rect. Both point the
/// same way, so the surface yields its region for the frame instead of winning a fight it would
/// lose silently.
///
/// `Background` and `Middle` are the body and its windows, which is where the video itself lives;
/// the three above it are popups, menus, tooltips and the debug layer.
pub fn floats_above_body(order: egui::Order) -> bool {
    match order {
        egui::Order::Background | egui::Order::Middle => false,
        egui::Order::Foreground | egui::Order::Tooltip | egui::Order::Debug => true,
    }
}

/// Do these two rectangles share any area? Strictly: two rectangles that merely touch along an
/// edge do not overlap, so a dropdown that ends exactly where the video begins does not blank it.
/// A rectangle with no area never overlaps anything, which is what keeps `Rect::NOTHING` and the
/// zero-sized state of a freshly created `Area` from hiding the video on their first frame.
pub fn overlaps(video: egui::Rect, overlay: egui::Rect) -> bool {
    let has_area = |r: egui::Rect| r.width() > 0.0 && r.height() > 0.0;
    has_area(video)
        && has_area(overlay)
        && overlay.min.x < video.max.x
        && video.min.x < overlay.max.x
        && overlay.min.y < video.max.y
        && video.min.y < overlay.max.y
}

/// Is anything egui is drawing above the body crossing `video` this frame?
///
/// Asked of egui's own layer list rather than of a hand written list of overlays, because a hand
/// written list is a list somebody has to remember to add the next menu to. Every popup, dropdown,
/// tooltip and hint in this app is an `Area` in one of the orders [`floats_above_body`] names, so
/// they are all covered by construction, including the ones added after this was written.
pub fn occluded_by_overlays(ctx: &egui::Context, video: egui::Rect) -> bool {
    /* The ids are collected and the lock released BEFORE any rect is read. `AreaState::load` takes
     * the same memory lock itself, and taking it again from inside a closure that already holds it
     * is a hang, not a compile error. */
    let above: Vec<egui::Id> = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|layer| floats_above_body(layer.order))
            .map(|layer| layer.id)
            .collect()
    });
    above
        .into_iter()
        .filter_map(|id| egui::AreaState::load(ctx, id))
        .any(|state| overlaps(video, state.rect()))
}

/* ------------------------------------------------------------------ the surface -- */

/// What the surface is doing, in words a screen can print without adding a claim of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Sound {
    /// Muted, which is how every stream starts.
    #[default]
    Off,
    /// Sound was asked for and the browser reports the document IS emitting audio.
    On,
    /// Sound was asked for and the browser reports the document is NOT emitting audio. Said out
    /// loud rather than hidden, because the alternative is an app claiming sound that is not there.
    AskedButSilent,
}

impl Sound {
    /// The words for this state. One rule, so the Watch screen cannot invent a second.
    pub fn word(&self) -> &'static str {
        match self {
            Sound::Off => "sound off",
            Sound::On => "sound on",
            Sound::AskedButSilent => "sound on, but no audio is playing yet",
        }
    }
}

/* -------------------------------------------------------------- what went wrong -- */

/// Why there is no surface, and whether asking again could change that.
///
/// ONE `Option<String>` USED TO CARRY BOTH, AND THAT WAS A PLAYER THAT DIED FOR THE LIFE OF THE
/// PROCESS. A `set_bounds` that failed once wrote its sentence into the same field the Watch
/// screen read to decide whether anything could be embedded; the field was cleared only by a
/// successful BUILD, a build happened only when the feed or the sound flag changed, and the only
/// controls that could change either were the ones that field had just disabled. One hiccup and
/// the way back was to restart the app.
///
/// The two lifetimes are not the same and now they do not share a type. A [`Refused`](Self::Refused)
/// is decided once and no click changes it: there is no per-user data folder to keep the WebView2
/// profile in, or there is no WebView2 at all (see `surface_absent`). A [`Failed`](Self::Failed) is
/// one attempt's outcome, and there are two ways back from it: a placement call retries itself on
/// the next frame while the surface is alive, and a build waits for the reader to ask again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No surface can be built in this process, and nothing the reader does changes that.
    Refused(String),
    /// The last attempt failed. Asking again is a real thing to do.
    Failed(String),
}

impl Problem {
    /// The words, which the Watch screen prints verbatim. Both variants carry a sentence, because
    /// a reader is owed the reason whether or not there is anything they can do about it.
    pub fn words(&self) -> &str {
        match self {
            Problem::Refused(s) | Problem::Failed(s) => s,
        }
    }

    /// Could asking again change this? This is what keeps the Watch here button alive after a
    /// build that failed, and what keeps it dead when the profile folder never existed: an enabled
    /// control for an outcome that cannot change is the same defect as a control for a feature
    /// that does not exist.
    pub fn can_retry(&self) -> bool {
        matches!(self, Problem::Failed(_))
    }
}

/// Does this frame's `Player::sync` go on to touch the surface at all?
///
/// THE MIDDLE CASE IS THE RECOVERY PATH AND IT IS THE WHOLE POINT. A [`Problem::Failed`] with a
/// LIVE webview is a placement call that did not work on a surface that is still there, so the
/// frame carries on and pushes the bounds again; that is what lets a transient failure clear
/// itself with nobody clicking anything. A `Failed` with NO webview is a build that did not work,
/// and building costs an environment attach, so it is not retried on a timer: it waits for
/// [`after_retry`], which the reader's next ask reaches. A [`Problem::Refused`] never proceeds.
pub fn keeps_syncing(problem: Option<&Problem>, has_webview: bool) -> bool {
    match problem {
        None => true,
        Some(p) => p.can_retry() && has_webview,
    }
}

/// What one frame's placement calls (`set_bounds`, then `set_visible`) leave in `problem`.
///
/// IT DOES NOT LATCH, and that is the fix. A call that worked clears whatever the last one that
/// did not had written, so the player comes back on its own the moment the placement does.
///
/// THE `has_webview` GUARD IS NOT BELT AND BRACES. With no webview both calls are no-ops that
/// return `Ok` without touching anything, and letting a no-op clear the problem would erase the
/// BUILD failure the Watch screen is at that moment printing, leaving a reader with no player, no
/// reason and an enabled button that quietly does nothing.
pub fn placement_problem(
    outcome: Result<(), String>,
    has_webview: bool,
    previous: Option<Problem>,
) -> Option<Problem> {
    match outcome {
        Err(e) => Some(Problem::Failed(e)),
        Ok(()) if has_webview => None,
        Ok(()) => previous,
    }
}

/// `problem` after the reader has asked to watch again.
///
/// THE USER-DRIVEN HALF OF THE RECOVERY, and it is user-driven on purpose. The failure this
/// clears is a build that did not work, and the ordinary cause of that is a machine with no
/// WebView2 runtime, which no amount of retrying fixes; a frame-rate retry loop against it would
/// spend the reader's CPU forever and say nothing new. So the app tries exactly once more, each
/// time somebody asks for the player: the Watch here button and every pill in the window raise
/// `screens::Ask::WatchHere`, and `App::answer` is where this is called.
///
/// A [`Problem::Refused`] survives it. Clearing a reason that is still true would put the sentence
/// back on screen one frame later, which is a flicker that teaches a reader the app is guessing.
pub fn after_retry(problem: Option<Problem>) -> Option<Problem> {
    problem.filter(|p| !p.can_retry())
}

/// What a screen may know about the surface, in primitives, cloned once per frame.
///
/// WHY A SNAPSHOT AND NOT THE PLAYER ITSELF. `Player` owns an OS window on Windows and nothing at
/// all elsewhere, so a screen holding one would be a screen with two shapes. This is the same
/// arrangement `screens::watch::View` has with `watcher::Channel`: one line reads the other lane's
/// type and every formatter below works on primitives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerView {
    /// Why there is no surface, and whether that can change. `None` when nothing is wrong.
    pub problem: Option<Problem>,
    /// IS THERE A SURFACE ALIVE RIGHT NOW? The Stop control is enabled off this and nothing else.
    /// A surface stops being EMBEDDABLE the moment the channel goes offline or the preferred
    /// platform is switched, and it is still a surface: it is hidden, it still holds a WebView2
    /// child window and it is still somebody's bandwidth. Deciding Stop from embeddability is what
    /// stranded it alive with its only control greyed out.
    pub playing: bool,
    /// What the BROWSER says about audio. See [`Sound`].
    pub sound: Sound,
    /// The WebView2 profile folder in use. Shown because it answers "where would a login be kept",
    /// which is a question with only one honest answer and it is a path.
    pub profile: Option<PathBuf>,
    /// Edge tracking prevention on our own profile, read back after being set. `0` is off, which
    /// is what lets the player receive its own cookies; see the surface module.
    pub tracking_prevention: Option<i32>,
    /// IS THE VIDEO IN ANOTHER WINDOW RIGHT NOW? True while the Watch pop-out holds it.
    ///
    /// THE FOLIO NEEDS THIS AND NOTHING ELSE ABOUT THE SEAT. It reserves a rectangle and paints it
    /// black for the surface to be composited into, and with the video somewhere else that
    /// rectangle is a black hole in the middle of the main window with no explanation in it. A
    /// boolean is deliberately all that is exposed: which window, and its handle, are the
    /// registry's business and a screen that could name them could try to drive them.
    pub hosted_elsewhere: bool,
}

/* THE HANDLE RESOLVER IS COMPILED EVERYWHERE, unlike the surface below it, and that is on purpose.
 * Its decision half (`hwnd::pick`) is pure and is where every rule about which window the video may
 * be moved into lives, so it is tested on the Linux CI that can never run one Win32 call under it.
 * The calls themselves have honest no-op twins off Windows; `surface_absent` is what those
 * platforms actually run. */
pub mod hwnd;

#[cfg(windows)]
mod surface;
#[cfg(windows)]
pub use surface::Player;

#[cfg(not(windows))]
mod surface_absent;
#[cfg(not(windows))]
pub use surface_absent::Player;

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn twitch() -> Feed {
        Feed::Twitch {
            login: "broken_stoic".into(),
        }
    }

    fn body_stage(sound: bool) -> Stage {
        Stage {
            seat: Seat::Body {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 450.0)),
                pixels_per_point: 1.5,
                occluded: false,
            },
            feed: twitch(),
            sound,
        }
    }

    fn offer() -> PipOffer {
        PipOffer {
            hwnd: 0x1234,
            carve_px: vec![(100, 8, 122, 30)],
        }
    }

    /// THE POP-OUT TAKES THE VIDEO WHEN IT IS OPEN AND THERE IS SOMETHING TO PLAY, and that is the
    /// owner's own sentence: stop it in the main window, play it in the pop-out. There is no second
    /// step that stops the body, because there is only ever one surface and one seat on it.
    #[test]
    fn the_pop_out_takes_the_video_when_it_is_open_and_something_is_playing() {
        let out = choose_stage(
            Some(body_stage(true)),
            Some(offer()),
            Some((twitch(), true)),
        )
        .expect("an open pop-out with something to play is a stage");
        assert_eq!(
            out.seat_id(),
            SeatId::PopOut(0x1234),
            "the pop-out was open and the video stayed in the body"
        );
        assert_eq!(
            out.body_rect(),
            None,
            "a pop-out seat still answers with a rectangle in the MAIN window's coordinates"
        );
    }

    /// THE FEED AND THE MUTE FLAG CROSS THE MOVE UNCHANGED, AND THIS IS THE VOLUME TEST.
    ///
    /// `Player::sync` rebuilds the whole webview whenever `(Feed, bool)` changes, and a rebuild is a
    /// fresh navigation with `muted=` re-applied from the URL, which is a stream restarting at the
    /// default volume. So the ONE thing that must never differ between the seat the body asked for
    /// and the seat the pop-out gets is that pair. It is driven over both values of the flag,
    /// because a version of this that only checked `false` would pass on a `choose_stage` that
    /// hard-coded mute.
    #[test]
    fn moving_the_video_changes_nothing_that_would_rebuild_it() {
        for sound in [false, true] {
            let body = body_stage(sound);
            let out = choose_stage(Some(body.clone()), Some(offer()), Some((twitch(), sound)))
                .expect("there is a stage");
            assert_eq!(
                out.feed, body.feed,
                "the feed changed across the move, which is a rebuild, which is a reset volume"
            );
            assert_eq!(
                out.sound, body.sound,
                "the mute flag changed across the move, which is a rebuild, which is a reset volume"
            );
        }
    }

    /// WITH NOTHING TO PLAY, AN OPEN POP-OUT CHANGES NOTHING. An offer is not a request: it says
    /// only that the video MAY live there. A pop-out open on an offline channel must not drag a
    /// surface that does not exist into itself, and must not suppress the body's own staging on the
    /// frame the channel comes back.
    #[test]
    fn an_open_pop_out_with_nothing_to_play_leaves_the_body_alone() {
        assert_eq!(
            choose_stage(None, Some(offer()), None),
            None,
            "an offer alone conjured a stage out of nothing"
        );
        let body = body_stage(false);
        assert_eq!(
            choose_stage(Some(body.clone()), Some(offer()), None).map(|s| s.seat_id()),
            Some(SeatId::Body),
            "the video left the body with no demand to justify it"
        );
    }

    /// A SIGN-IN PAGE NEVER MOVES INTO THE POP-OUT, EVEN WHEN THE POP-OUT IS OPEN AND OFFERING.
    ///
    /// The offer says the video MAY live there; a form to type a password into is not the video,
    /// and a small always-on-top window over the game is not where a reader can read one. The
    /// body reserved a rectangle for the page (`WatchScreen::folio` stages it whether or not the
    /// video was in the pop-out a frame ago), and that is where it goes.
    #[test]
    fn a_sign_in_page_stays_in_the_body_while_the_pop_out_offers() {
        let body = Stage {
            feed: Feed::TwitchSignIn {
                at: "https://www.twitch.tv/activate?device-code=WMCLHMKG".to_owned(),
            },
            ..body_stage(false)
        };
        let out = choose_stage(
            Some(body.clone()),
            Some(offer()),
            Some((
                Feed::TwitchSignIn {
                    at: "https://www.twitch.tv/activate?device-code=WMCLHMKG".to_owned(),
                },
                false,
            )),
        )
        .expect("the body staged the page");
        assert_eq!(
            out.seat_id(),
            SeatId::Body,
            "a sign-in form was sent into the pop-out"
        );
        assert_eq!(out, body);
        /* And the page is a whole page: nothing of ours is served for it, and it has no embed. */
        let page = Feed::TwitchSignIn {
            at: "https://www.twitch.tv/activate?device-code=WMCLHMKG".to_owned(),
        };
        assert_eq!(host_page(&page, false), None);
        assert_eq!(embed_url(&page, true), None);
        let (url, served) = load(&page, false);
        /* THE CODE REACHES THE PAGE. A sign-in feed that dropped it would load a bare activation
         * prompt and the reader would be asked to type eight characters nobody showed them. */
        assert_eq!(url, "https://www.twitch.tv/activate?device-code=WMCLHMKG");
        assert_eq!(
            served, None,
            "a top level page has nothing of ours served under it"
        );
    }

    /// WITH THE POP-OUT SHUT, THE BODY KEEPS THE VIDEO AND KEEPS ITS RECTANGLE. This is the shipped
    /// path and by far the commonest one, so it is asserted rather than assumed to fall out.
    #[test]
    fn the_body_keeps_the_video_when_no_pop_out_is_open() {
        let body = body_stage(true);
        let out = choose_stage(Some(body.clone()), None, Some((twitch(), true)))
            .expect("the body staged a surface");
        assert_eq!(out, body, "the body's own stage came back changed");
        assert!(out.body_rect().is_some());
    }

    fn tw() -> Feed {
        Feed::Twitch {
            login: "broken_stoic".into(),
        }
    }
    /// The host page and the embed URL for a feed that IS framed, with the invariant spelled out
    /// once rather than at every assertion below.
    ///
    /// [`host_page`] and [`embed_url`] answer `None` for [`Feed::YouTubeChannel`], which is not
    /// framed at all: every form of that page carries `X-Frame-Options: SAMEORIGIN` (measured
    /// 2026-09-03), so it is loaded top level and nothing of ours is served for it. Every test
    /// below is about the two PLAYER feeds, and the `expect` is what says so.
    fn framed_page(feed: &Feed, sound: bool) -> String {
        host_page(feed, sound).expect("a player feed is framed and always has a host page")
    }
    fn framed_embed(feed: &Feed, sound: bool) -> String {
        embed_url(feed, sound).expect("a player feed is framed and always has an embed url")
    }

    fn yt() -> Feed {
        Feed::YouTube {
            video_id: "rFZHOHl-L8A".into(),
        }
    }

    /// The origin, the host and the page address are one derivation from [`SCHEME`], because the
    /// bug they replace is a `parent` that names one domain while the page is served from another,
    /// and Twitch answers that with a blocked frame and no message a user could act on.
    #[test]
    fn the_origin_is_one_derivation_from_the_scheme() {
        assert_eq!(host(), "grimoire.localhost");
        assert_eq!(origin(), "https://grimoire.localhost");
        assert_eq!(page_url(), "grimoire://localhost/index.html");
        assert!(
            !host().contains(':'),
            "a port in the host is a port in parent, which Twitch refuses with InvalidCharInParent"
        );
        assert!(
            origin().starts_with("https://"),
            "the http form of this same host was measured BLOCKED by frame-ancestors"
        );
    }

    /// The Twitch page names the channel and the host, and starts muted.
    #[test]
    fn the_twitch_page_carries_the_player_url_the_parent_and_muted() {
        let page = framed_page(&tw(), false);
        assert!(
            page.contains("https://player.twitch.tv/?channel=broken_stoic&parent=grimoire.localhost&muted=true&autoplay=true"),
            "the twitch page must carry the player url whole; got:\n{page}"
        );
        assert!(page.contains("<iframe src="), "the player is an iframe");
    }

    /// THE ASSERTION THAT WOULD HAVE CAUGHT THE MEASURED 302. Twitch answers a `parent` carrying a
    /// port with `InvalidCharInParent` and never renders. There is no port anywhere in this crate's
    /// origin, so the way this regresses is somebody deciding a local server is simpler than a
    /// custom protocol and pasting `localhost:3000` in here. This test is what stops that landing
    /// silently: it reads the `parent` value out of the generated page and refuses a colon.
    #[test]
    fn a_twitch_page_never_carries_a_bare_port_in_parent() {
        for sound in [false, true] {
            let page = framed_page(&tw(), sound);
            let at = page.find("parent=").expect("the twitch page sets parent");
            let value: String = page[at + "parent=".len()..]
                .chars()
                .take_while(|c| *c != '&' && *c != '"' && *c != '\'')
                .collect();
            assert_eq!(value, "grimoire.localhost");
            assert!(
                !value.contains(':'),
                "parent={value:?} carries a port; Twitch answers that with 302 InvalidCharInParent"
            );
        }
    }

    /// YouTube takes a VIDEO id and the muted spelling is its own.
    ///
    /// THE CHANNEL FORM IS NOT TESTED HERE BECAUSE IT DOES NOT WORK. `/embed/live_stream?channel=`
    /// was measured failing with IFrame API error 150 for a live channel and an offline one alike;
    /// see the module note. If it ever comes back, it comes back with a measurement, not with a
    /// hopeful edit here.
    #[test]
    fn the_youtube_page_carries_a_video_id_the_origin_and_mute() {
        let page = framed_page(&yt(), false);
        assert!(
            page.contains("https://www.youtube.com/embed/rFZHOHl-L8A?autoplay=1&mute=1&enablejsapi=1&playsinline=1&origin=https%3A%2F%2Fgrimoire.localhost"),
            "the youtube page must carry the embed url whole; got:\n{page}"
        );
        assert!(
            !page.contains("live_stream"),
            "the channel-redirect endpoint is dead and must not come back without a measurement"
        );
    }

    /// EVERY PAGE STARTS MUTED, in whichever spelling its platform uses, and the sound flag is the
    /// only thing that changes it. Both platforms are walked because the two use different
    /// parameter names and different truth values (`muted=true` against `mute=1`), so getting one
    /// right proves nothing about the other.
    #[test]
    fn sound_off_is_the_default_and_the_flag_is_what_changes_it() {
        assert!(framed_page(&tw(), false).contains("muted=true"));
        assert!(framed_page(&tw(), true).contains("muted=false"));
        assert!(framed_page(&yt(), false).contains("mute=1"));
        assert!(framed_page(&yt(), true).contains("mute=0"));
        assert!(
            framed_embed(&yt(), false).contains("&mute=1&"),
            "mute is the youtube spelling, muted is the twitch one"
        );
    }

    /// The arithmetic that puts the surface where the reader sees the hole. Bounds are PHYSICAL
    /// pixels; egui rectangles are points. A build that forgets `pixels_per_point` looks correct on
    /// a 100% display and is wrong by nearly half on the 175% display this was measured on.
    #[test]
    fn rects_become_physical_pixels_at_the_measured_scale() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 50.4), egui::vec2(1100.0, 649.6));
        assert_eq!(physical_bounds(r, 1.75), (0, 88, 1925, 1137));
        assert_eq!(
            physical_bounds(r, 1.0),
            (0, 50, 1100, 650),
            "at 100% the numbers are the points, which is why 100% cannot prove the scaling"
        );
        /* Rounds, does not truncate: 13.0 points at 175% is 22.75 physical pixels, and a
         * truncating cast puts the surface a pixel above the hole on every odd rect. */
        let odd = egui::Rect::from_min_size(egui::pos2(7.0, 13.0), egui::vec2(9.0, 5.0));
        assert_eq!(physical_bounds(odd, 1.75), (12, 23, 16, 9));
    }

    /// A degenerate rect is a number, not a panic and not a garbage cast. `Rect::NOTHING` is built
    /// from infinities and reaches this function on the first frame of any screen that reserves
    /// space before it has laid anything out.
    #[test]
    fn a_degenerate_rect_still_yields_numbers() {
        assert_eq!(physical_bounds(egui::Rect::NOTHING, 1.75), (0, 0, 0, 0));
        assert_eq!(physical_bounds(egui::Rect::ZERO, 1.75), (0, 0, 0, 0));
        let inverted = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(10.0, 10.0));
        let (_, _, w, h) = physical_bounds(inverted, 1.75);
        assert_eq!(
            (w, h),
            (0, 0),
            "a negative size clamps to zero, it does not wrap"
        );
        let (_, _, w2, h2) = physical_bounds(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            f32::NAN,
        );
        assert_eq!((w2, h2), (10, 10), "an unusable scale falls back to 1.0");
    }

    /* A LOCAL DATA FOLDER IN THIS PLATFORM'S OWN SPELLING, AND THE PROFILE THAT BELONGS UNDER IT.
     *
     * THE PAIR IS GATED, NOT THE TEST, and the difference matters: the property below holds on
     * every platform, so it is checked on every platform, and only the spelling changes.
     *
     * WHY IT HAD TO BE GATED AT ALL, MEASURED. `\` is a separator on Windows and an ordinary
     * character everywhere else, so the Windows literal that was hard coded here is ONE path
     * component under Linux. `join`ing two names onto it produced
     * `C:\Users\someone\AppData\Local/EQLGrimoire/WebView2` while the expected value stayed a
     * single component, and `assert_eq!` failed. Run on rustc 1.97.1 under Ubuntu, the same
     * `profile_dir_under` and the same two literals print `assert_eq(dir, want) holds = false`.
     * The gate on `player::surface` is `cfg(windows)` and this crate's CI runs ubuntu-latest, so
     * the machine that would have caught it is exactly the one no gate here ever runs on. */
    #[cfg(windows)]
    const LOCAL_DATA: &str = r"C:\Users\someone\AppData\Local";
    #[cfg(windows)]
    const PROFILE_THERE: &str = r"C:\Users\someone\AppData\Local\EQLGrimoire\WebView2";
    #[cfg(not(windows))]
    const LOCAL_DATA: &str = "/home/someone/.local/share";
    #[cfg(not(windows))]
    const PROFILE_THERE: &str = "/home/someone/.local/share/EQLGrimoire/WebView2";

    /// THE PROFILE IS UNDER THE USER'S DATA FOLDER AND NEVER BESIDE THE EXE. wry's default is
    /// `<exe path>.WebView2`, which in Program Files is not writable, and the failure that produces
    /// names neither the folder nor the reason.
    #[test]
    fn the_profile_lives_under_local_data_and_not_beside_the_binary() {
        let local = Path::new(LOCAL_DATA);
        let dir = profile_dir_under(local);
        assert_eq!(dir, Path::new(PROFILE_THERE));
        assert!(
            dir.starts_with(local),
            "the profile is under the data folder"
        );
        /* The two names, in order, and nothing else between the root and the leaf. This is the
         * half of the assertion that is a claim about the LAYOUT rather than about one platform's
         * punctuation, so it is what carries the rule on a machine whose separator is not `\`. */
        let under: Vec<String> = dir
            .strip_prefix(local)
            .expect("the profile is under the data folder")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(under, vec![PROFILE_VENDOR, PROFILE_LEAF]);
        let exe = std::env::current_exe().expect("this test has an executable");
        let beside = PathBuf::from(format!("{}.WebView2", exe.display()));
        assert_ne!(
            dir, beside,
            "never wry's default, which is beside the binary"
        );
        assert!(
            !dir.starts_with(exe.parent().expect("the exe has a parent")),
            "the profile is not in the install folder"
        );
    }

    /// The sound words are one rule. A screen that spelled its own would eventually print "sound
    /// on" beside a surface reporting silence, which is the exact claim this module refuses to make.
    #[test]
    fn every_sound_state_has_its_own_words_and_only_one_says_sound_is_on() {
        assert_eq!(Sound::default(), Sound::Off);
        assert_eq!(Sound::Off.word(), "sound off");
        assert_eq!(Sound::On.word(), "sound on");
        assert!(Sound::AskedButSilent.word().contains("no audio"));
        assert_ne!(Sound::On.word(), Sound::AskedButSilent.word());
    }

    /// EVERY ORDER IS DECIDED, and the match is exhaustive on purpose: an egui release that adds a
    /// sixth order fails to compile here rather than silently letting a new kind of popup paint
    /// itself into a hole where the video used to be.
    #[test]
    fn only_the_layers_above_the_body_can_occlude_it() {
        assert!(!floats_above_body(egui::Order::Background));
        assert!(
            !floats_above_body(egui::Order::Middle),
            "the body and its windows are where the video itself lives"
        );
        assert!(
            floats_above_body(egui::Order::Foreground),
            "popups and menus"
        );
        assert!(floats_above_body(egui::Order::Tooltip));
        assert!(floats_above_body(egui::Order::Debug));
    }

    /// The overlap rule, including the two degenerate cases that would otherwise blank the video
    /// for no reason: a dropdown that stops exactly where the video starts, and an `Area` that has
    /// not been laid out yet and so reports no size at all.
    #[test]
    fn only_a_real_overlap_makes_the_surface_yield() {
        let video = egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(800.0, 400.0));
        let dropdown = egui::Rect::from_min_size(egui::pos2(600.0, 60.0), egui::vec2(180.0, 120.0));
        assert!(
            overlaps(video, dropdown),
            "a find dropdown reaching into the body"
        );

        let above = egui::Rect::from_min_size(egui::pos2(600.0, 0.0), egui::vec2(180.0, 100.0));
        assert!(
            !overlaps(video, above),
            "touching the top edge is not overlapping it; a dropdown that stops at the video must \
             not blank the video"
        );
        let elsewhere = egui::Rect::from_min_size(egui::pos2(900.0, 200.0), egui::vec2(50.0, 50.0));
        assert!(!overlaps(video, elsewhere));

        assert!(
            !overlaps(video, egui::Rect::NOTHING),
            "an area with no rectangle yet occludes nothing"
        );
        let no_size = egui::Rect::from_min_size(egui::pos2(400.0, 300.0), egui::vec2(0.0, 0.0));
        assert!(
            !overlaps(video, no_size),
            "a freshly created Area reports zero size on its first frame, dead centre of the video"
        );
        assert!(
            !overlaps(egui::Rect::NOTHING, dropdown),
            "no video rect, nothing to occlude"
        );
    }

    /* ---- the failure that used to be permanent ---- */

    fn failed() -> Option<Problem> {
        Some(Problem::Failed("could not place the player: oh".to_owned()))
    }
    fn refused() -> Option<Problem> {
        Some(Problem::Refused("nowhere to put the profile".to_owned()))
    }

    /// TWO LIFETIMES, TWO VARIANTS. Both carry a sentence for the reader; only one of them is
    /// worth offering a button for. A single `Option<String>` could tell a reader why and could
    /// not tell the screen which, and that is what put an enabled control on an outcome no click
    /// could change and a disabled one on an outcome a click could.
    #[test]
    fn only_a_failure_is_worth_asking_again_about() {
        assert!(Problem::Failed("x".into()).can_retry());
        assert!(!Problem::Refused("x".into()).can_retry());
        assert_eq!(
            Problem::Failed("could not place it".into()).words(),
            "could not place it"
        );
        assert_eq!(
            Problem::Refused("no data folder".into()).words(),
            "no data folder"
        );
    }

    /// THE FRAME AFTER A PLACEMENT THAT FAILED STILL RUNS, WHICH IS HOW IT RECOVERS.
    ///
    /// This is the rule that was missing. `set_bounds` failing wrote a problem, the problem
    /// stopped the surface being embeddable, nothing asked for a stage, and so the call that would
    /// have worked was never made again. A failure with a LIVE surface has to keep syncing or
    /// there is no second attempt; a failure with no surface must not, because that one is a
    /// `build_as_child` and retrying it every frame is a spin against a machine that has no
    /// WebView2 runtime.
    #[test]
    fn a_live_surface_keeps_syncing_after_a_failure_and_a_refusal_never_does() {
        assert!(keeps_syncing(None, true), "nothing is wrong");
        assert!(
            keeps_syncing(None, false),
            "nothing is wrong and nothing is built yet"
        );
        assert!(
            keeps_syncing(failed().as_ref(), true),
            "the surface is alive and its placement is what failed; the next frame is the retry"
        );
        assert!(
            !keeps_syncing(failed().as_ref(), false),
            "a build that failed waits for the reader, it does not spin"
        );
        assert!(!keeps_syncing(refused().as_ref(), true));
        assert!(!keeps_syncing(refused().as_ref(), false));
    }

    /// A PLACEMENT FAILURE DOES NOT LATCH: the next call that works clears it, with nobody
    /// clicking anything.
    ///
    /// AND A NO-OP CLEARS NOTHING. With no webview, `set_bounds` and `set_visible` are skipped and
    /// report `Ok` without having placed anything. Letting that erase the problem would wipe the
    /// BUILD failure the Watch screen is printing at that moment, so the reader would be left with
    /// no player and no reason. That case is the third assertion and it is the one a naive
    /// "clear on success" gets wrong.
    #[test]
    fn a_placement_that_works_clears_the_one_that_did_not_and_a_no_op_clears_nothing() {
        assert_eq!(
            placement_problem(Ok(()), true, failed()),
            None,
            "the surface is placed again, so the stale sentence goes"
        );
        assert_eq!(
            placement_problem(Err("nope".to_owned()), true, None),
            Some(Problem::Failed("nope".to_owned()))
        );
        assert_eq!(
            placement_problem(Ok(()), false, failed()),
            failed(),
            "no webview means nothing was placed; a no-op must not clear a build failure"
        );
        assert_eq!(placement_problem(Ok(()), false, None), None);
        assert_eq!(
            placement_problem(Ok(()), true, refused()),
            None,
            "a refusal cannot coexist with a live webview, and if it ever did the webview wins"
        );
    }

    /// ASKING AGAIN CLEARS A FAILURE AND LEAVES A REFUSAL STANDING. A refusal that vanished on a
    /// click would come straight back on the next frame, and a sentence that blinks is a sentence
    /// a reader stops believing.
    #[test]
    fn asking_again_clears_a_failure_and_never_a_refusal() {
        assert_eq!(after_retry(failed()), None);
        assert_eq!(after_retry(refused()), refused());
        assert_eq!(after_retry(None), None);
    }

    /// The `origin` YouTube is handed is THE PAGE'S OWN ORIGIN, encoded, and not a second spelling
    /// of it. The value used to be a literal `https%3A%2F%2F` beside `host()`, which is two places
    /// that have to agree about the scheme; the http form of this host was measured blocked, so a
    /// drift there is a player that does not load and a reason nobody can see.
    #[test]
    fn the_youtube_origin_parameter_is_the_encoded_page_origin() {
        assert_eq!(origin_as_query_value(), "https%3A%2F%2Fgrimoire.localhost");
        assert!(
            framed_embed(&yt(), false).ends_with(&format!("&origin={}", origin_as_query_value())),
            "the embed carries the encoded origin, not a hand written copy of it"
        );
        assert!(
            !origin_as_query_value().contains(':') && !origin_as_query_value().contains('/'),
            "nothing a query value may not carry survives the encoding"
        );
    }
}
