//! The YouTube half of the merged feed: a second, hidden WebView2 child window that runs
//! `youtube.com/live_chat`, and the plumbing that gets what it says into a log the UI reads.
//!
//! WHAT THIS FILE IS. One webview, one page, one log. [`YtChat::start`] records a video id.
//! [`YtChat::sync`], called once per frame from the root pass, builds the page on first need,
//! navigates it when the stream changes, watches it for silence, reloads it when the silence is
//! not the room's, and drops it the moment nobody is asking. Every message the page posts arrives
//! on the UI thread, is parsed by [`super::model::parse_batch`], is deduplicated by its YouTube
//! id, and lands in a `VecDeque` bounded by count. Nothing here touches disk except the WebView2
//! profile folder, and nothing here opens anything until somebody asks.
//!
//! THE SHAPE IS `chat.rs`, DELIBERATELY. The Twitch reader already solved "a source the UI must
//! read cheaply every frame, that must be promptly stoppable, whose failures must be words on a
//! screen rather than an empty list". Taken from it, name for name: the `Arc<Mutex<..>>` the UI
//! borrows under a closure rather than clones (`with_log`, chat.rs:668-671, and its note at
//! chat.rs:21-26 about why cloning two thousand `String`s at 60 Hz is not free), the poison
//! tolerant `lock` that hands back a panicked holder's data rather than taking the UI thread with
//! it (chat.rs:862-864), the ring buffer with a COUNTED eviction rather than a silent one
//! (chat.rs:423-432), the `refused`/`last_refusal` tripwire that makes "the page changed shape"
//! visible instead of looking like a quiet room (chat.rs:395-403), the `Tuning` struct so a test
//! can prove eviction with three lines instead of two thousand (chat.rs:599-611), and the rule
//! that [`YtChat`] does NOT implement `Default`, because a `Default` that opens a browser opens
//! one for every user on every launch forever (chat.rs:686-692).
//!
//! ONE THING IS DELIBERATELY NOT COPIED: `chat::Coalescer`. That exists because a socket delivers
//! one line at a time and a raid is 38 lines in a second. This page coalesces in the PAGE: the
//! injected script batches a `MutationObserver` burst behind a 250 ms timer and posts it as one
//! string, so a burst crosses the IPC boundary once and costs one repaint. The measured worst
//! case was a scroll-pause flush of six rows in one batch. A second coalescer here would be a
//! second answer to a question that already has one.
//!
//! WHY SILENCE IS NOT A STALL, AND WHAT IS. Probe lane 1 measured this room at 3.1 messages a
//! minute, watched a 93 second gap between two appends on a feed that was demonstrably healthy,
//! and read YouTube's own rate estimate (`chatRateMs_`) at 7.7 to 23.3 seconds. So a watchdog
//! that reloaded on message silence would reload a working page several times an hour and drop
//! its backlog each time. The honest liveness signal is that the injected script is still running
//! and still talking to us: it posts a `kind:"ping"` item every 20 seconds whether or not anybody
//! said anything, so the ARRIVAL OF ANY PAYLOAD, message or ping, is the heartbeat this file
//! watches. See `Tuning::silent_limit` for the number and for what this signal cannot see.
//!
//! WHY THE WEBVIEW HALF IS `cfg(windows)`. Same reason `player::surface` is: WebView2 is a Windows
//! component and `wry` on Linux links webkit2gtk through pkg-config at BUILD time, which this
//! tree's bare ubuntu CI does not have, so `wry` is declared under
//! `[target.'cfg(windows)'.dependencies]`. `AbsentPane` is what the other platforms get and it
//! says so in words, exactly as `player/surface_absent.rs` does, rather than being a silent no-op
//! that leaves a reader looking at an empty chat with no reason in it.
//!
//! WHAT IS BEHIND THE SEAM AND WHY THERE IS ONE. [`Pane`] is "the page", and it has exactly five
//! methods. The production implementor drives a real WebView2 child; `AbsentPane` refuses; and
//! the tests' one records what it was asked to do. Everything that can be got wrong without a
//! browser is therefore provable without one: the dedup, the cap, the drop count, the refusal
//! count, the origin check, and the whole watchdog ladder, which is decided by a pure function
//! (`plan`) and only then carried out. This is the same arrangement `chat::Wire` has with the
//! socket and `twitch_auth::Wire` has with the token endpoint.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use egui::ViewportId;

use crate::player::{after_retry, Problem};

use super::extract::live_chat_url;
use super::model::{self, Batch, YtMessage};

/* ------------------------------------------------------------------ the numbers -- */

/// How many YouTube messages the log keeps.
///
/// IT MATCHES `chat::LOG_CAP` ON PURPOSE, and that is the whole argument. These lines are
/// interleaved with the Twitch ones into one merged feed, and two different horizons would mean
/// the merged view silently ran out of one platform's history before the other's, so scrolling
/// back would show a conversation that was on Twitch only, which is a lie about the room. At the
/// 3.1 messages a minute this channel's YouTube side was measured at, two thousand lines is about
/// ten hours, which is longer than any stream: the cap is a memory floor, not a policy about how
/// much scrollback a reader gets.
pub const LOG_CAP: usize = 2000;

/// How many YouTube ids the dedup set remembers.
///
/// IT MUST EXCEED [`LOG_CAP`] AND THAT IS AN INVARIANT, NOT A MARGIN. If an id could fall out of
/// this set while its message was still in `lines`, a reload, which reposts the whole visible
/// backlog, would draw that message a second time under the first. `a_forgotten_id_could_never_be
/// _still_on_screen` holds the relation. The surplus above the cap absorbs the replay itself: a
/// fresh load was measured returning 75 to 77 already-seen ids.
pub const SEEN_CAP: usize = 4000;

/// How long the page may say NOTHING AT ALL before it is treated as gone.
///
/// THE SIGNAL IS THE PAYLOAD, NOT THE MESSAGE, and the difference is the whole watchdog. The
/// injected script posts a ping item every 20 seconds regardless of whether anybody typed, so on
/// a healthy page a payload arrives roughly three times a minute even in a dead-silent room. 90
/// seconds is four and a half missed pings, which is past any plausible jitter: a nominal one
/// second timer on a hidden page was measured returning in 1101, 2002 and 1991 ms, and one poll
/// gap of 34.7 s was seen against a nominal 10 s.
///
/// WHAT THIS SIGNAL CANNOT SEE, SAID PLAINLY. It proves the SCRIPT is alive. It does not prove
/// YouTube's own continuation is: the script's ping carries a `polls` counter that would prove
/// that, and it is the stronger rule Probe lane 1 recommends, but `model::YtKind` has no ping arm,
/// so the counter cannot cross the model seam and this file cannot read it. A page whose script
/// runs while YouTube's fetch loop has stopped would therefore not be caught here. That gap is
/// recorded rather than papered over; closing it needs a `polls` field on the model side, not a
/// second parser in this file.
pub const SILENT_LIMIT: Duration = Duration::from_secs(90);

/// The floor between reloads.
///
/// A reload drops the page's own backlog and costs a fresh fetch of a 249 KB document, and the
/// failure a reload cannot fix (a throttled or suspended renderer) is exactly the one that would
/// keep the silence going, so an ungated watchdog would hammer a dead channel forever. Two
/// minutes lets a reload be tried, watched for a whole `SILENT_LIMIT`, and tried again.
pub const RELOAD_EVERY: Duration = Duration::from_secs(120);

/// Every number the surface consults, in one struct, so a test can prove eviction with three lines
/// and the watchdog ladder in milliseconds. This is `chat::Tuning` and the reason is the same one:
/// the production path takes `Tuning::default()` and there is nothing else to get wrong.
#[derive(Clone, Debug)]
pub struct Tuning {
    pub log_cap: usize,
    pub seen_cap: usize,
    pub silent_limit: Duration,
    pub reload_every: Duration,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning {
            log_cap: LOG_CAP,
            seen_cap: SEEN_CAP,
            silent_limit: SILENT_LIMIT,
            reload_every: RELOAD_EVERY,
        }
    }
}

/* ------------------------------------------------------------------- the shapes -- */

/// Where the page is. The UI draws this, and it is the whole of the difference between a quiet
/// room and a page that has stopped talking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YtConn {
    /// [`YtChat::start`] has never been called. Nothing has been opened.
    Idle,
    /// A page has been asked for, or has been pointed at a new stream, and has not spoken yet.
    Loading,
    /// The page is talking to us. Messages, or at least pings, are arriving.
    Reading,
    /// Nothing has arrived for longer than `Tuning::silent_limit`, or the page could not be
    /// built at all. The sentence says which, and what was done about it.
    ///
    /// IT IS THE ONLY STATE THAT CARRIES WORDS, which is why a platform with no WebView2 lands
    /// here too. That is a compromise and it is named: a `Refused` arm would be the honest home
    /// for "no browser on this machine, and no click changes that", but the interface contract
    /// fixes this enum's arms and inventing one would break the two lanes that read it. The
    /// sentence is still the truth, and `Problem::can_retry` behind it still decides whether the
    /// surface will ever try again, so nothing retries a machine that cannot host a webview.
    Stalled(String),
    /// Nobody is asking for the feed any more and the page has been torn down.
    Stopped,
}

/// What the UI reads. Lives behind the surface's mutex; borrow it with [`YtChat::with_log`].
///
/// IT DERIVES `Clone` AND `Debug` AND NOTHING ELSE, WHICH IS A REACHABILITY DECISION, and the
/// argument is `chat::Event`'s (chat.rs:235-241): the five masking derives expand to an impl that
/// reads every field, so rustc's dead code lint goes blind to a field the screen never draws.
/// Keeping them off keeps its eyes open.
///
/// A CROSS-LANE REQUIREMENT FALLS OUT OF THIS, and it is written here rather than assumed:
/// `model::YtMessage` must derive `Clone` and `Debug` and, for the same reachability reason, none
/// of `PartialEq`, `Eq`, `Hash`, `PartialOrd` or `Ord`. That is exactly what `chat::Event` derives
/// and why.
#[derive(Clone, Debug)]
pub struct YtLog {
    pub state: YtConn,
    /// Oldest first, ARRIVAL order, which is not send order.
    ///
    /// THE SORT BELONGS TO THE MERGE AND NOT TO THIS DEQUE, and that is a measurement: DOM insert
    /// lag was 2.1 to 5.8 seconds normally and 44 to 66 seconds after a scroll-pause flush, and
    /// two rows were appended in the same millisecond with `ts_usec` twenty seconds apart. So
    /// `model::interleave` sorts on `ts_usec`, and this deque holds what arrived in the order it
    /// arrived, which is the only order this file can honestly claim to know.
    pub lines: VecDeque<YtMessage>,
    /// How many payload items this surface could not turn into anything.
    ///
    /// COUNTED, NEVER SILENTLY DROPPED, for chat.rs:395-403's reason and for one more that is
    /// specific to a scraped page: YouTube can change the shape of a chat row on any deploy, and
    /// the failure mode of a scraper that quietly discards what it does not recognise is a chat
    /// that looks calm and empty. A screen showing `refused: 4210` is a tripwire.
    pub refused: u64,
    pub last_refusal: Option<String>,
    /// How many lines the cap has pushed off the front. A count and not a flag, so a screen can
    /// say the log is incomplete rather than imply it is whole.
    pub dropped: u64,
}

impl YtLog {
    fn idle() -> YtLog {
        YtLog {
            state: YtConn::Idle,
            lines: VecDeque::new(),
            refused: 0,
            last_refusal: None,
            dropped: 0,
        }
    }

    /// Append, and drop the oldest when the cap is passed. The cap is a parameter rather than the
    /// constant so a test can prove eviction without pushing two thousand real messages through
    /// it. chat.rs:423-432.
    fn push(&mut self, m: YtMessage, cap: usize) {
        self.lines.push_back(m);
        while self.lines.len() > cap {
            if self.lines.pop_front().is_some() {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
    }

    /// A new stream is a new room: the lines from the old one are not part of it.
    fn clear_room(&mut self) {
        self.lines.clear();
    }
}

/// The ids already admitted, oldest first, so a reload's replay is absorbed in silence.
///
/// A `VecDeque` AND A LINEAR SCAN, NOT A `HashSet` PLUS A QUEUE, and the reason is not laziness.
/// Eviction has to be by age, so a hash set alone cannot do it and the two-structure version has
/// an invariant a future edit can break in one direction and not the other, which is a duplicate
/// message or an unbounded set and no test in between. The cost of the scan is bounded by
/// measurement: this room produces 3.1 messages a minute, and the worst burst that has to be
/// scanned against a full set is a reload replaying 77 ids, which is under a third of a million
/// short string comparisons, once, on a frame that was already loading a 249 KB page.
#[derive(Debug)]
struct Seen {
    ids: VecDeque<String>,
    cap: usize,
}

impl Seen {
    fn new(cap: usize) -> Seen {
        Seen {
            ids: VecDeque::new(),
            cap,
        }
    }

    /// True when this id is new and has been recorded, false when it is a repeat.
    fn admit(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|s| s == id) {
            return false;
        }
        self.ids.push_back(id.to_owned());
        while self.ids.len() > self.cap {
            self.ids.pop_front();
        }
        true
    }

    fn clear(&mut self) {
        self.ids.clear();
    }
}

/// Everything the surface can change from a `&self` call or from the page's own callback.
#[derive(Debug)]
struct Inner {
    log: YtLog,
    seen: Seen,
    /// The video id the reader has asked for. `None` means "nothing", which is a teardown.
    want: Option<String>,
    /// The video id the LIVE page is pointed at. `None` when there is no page.
    shown: Option<String>,
    /// Kept so the page's callback can wake the UI. Set by the first `start`.
    ctx: Option<egui::Context>,
    /// When the page last said ANYTHING, ping included. Set when a page is shown, so a page that
    /// loads and never runs its script still has a clock running against it.
    last_post: Option<Instant>,
    last_reload: Option<Instant>,
    /// How many times the watchdog has reloaded. It has no field of its own on [`YtLog`], so it is
    /// spelled into the [`YtConn::Stalled`] sentence: a reader owed a fresh-looking log deserves
    /// to be told it is fresh because it was reloaded four times, not to guess.
    reloads: u32,
    /// Why there is no page, and whether asking again could change it. `crate::player::Problem`
    /// rather than a second vocabulary, because the Refused-versus-Failed argument is already
    /// written down there (player.rs:575-608) and it is the same argument.
    problem: Option<Problem>,
}

impl Inner {
    fn new(tune: &Tuning) -> Inner {
        Inner {
            log: YtLog::idle(),
            seen: Seen::new(tune.seen_cap),
            want: None,
            shown: None,
            ctx: None,
            last_post: None,
            last_reload: None,
            reloads: 0,
            problem: None,
        }
    }
}

/* --------------------------------------------------------------------- the sink -- */

/// WHERE THE PAGE POSTS, AND THE ONLY THING THE PAGE'S CALLBACK IS EVER GIVEN.
///
/// THE CALLBACK MUST NOT BE ABLE TO REACH BACK INTO [`YtChat`], and this type is what enforces it.
/// `webview2_com::wait_with_pump` pumps the Win32 message loop while an environment and a
/// controller are being created (wry 0.56.1 webview2/mod.rs:365 and 426), so while `Pane::show` is
/// blocked building a page, an IPC callback belonging to a page that ALREADY exists can fire
/// re-entrantly, on this same thread, inside the build. A callback that held a `&mut YtChat` would
/// be a `RefCell` panic or a deadlock at that moment. A callback that holds only this takes the
/// mutex, appends, and drops it, and calls nothing that can pump. Same capture discipline as
/// `chat::ChatReader::start_with` handing its thread an `Arc`, a `Context` and a `String`
/// (chat.rs:763-770).
pub struct Sink {
    inner: Mutex<Inner>,
    tune: Tuning,
}

impl Sink {
    fn new(tune: Tuning) -> Sink {
        Sink {
            inner: Mutex::new(Inner::new(&tune)),
            tune,
        }
    }

    /// A poisoned mutex still holds a perfectly good log; a panic on one side must not take the UI
    /// thread with it. chat.rs:860-864.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// THE ONE DOOR EVERY MESSAGE COMES IN BY. `host` is the host of the source frame's URL, which
    /// wry reads off `WebMessageReceived` and puts in the request's URI (wry 0.56.1
    /// webview2/mod.rs:954-985); `json` is the exact string the page passed to
    /// `window.ipc.postMessage`.
    ///
    /// THE ORIGIN CHECK IS NOT CEREMONY. wry defines `window.ipc` on the page
    /// (webview2/mod.rs:946-951) and every script on it can call it, so this webview, navigated
    /// top level to a site nobody here controls, is a channel from that site's scripts to this
    /// app's log. Nothing here is a credential and the profile is signed out, but a page that can
    /// post arbitrary strings can put arbitrary strings on the reader's screen. A payload from
    /// anywhere but the live chat page is COUNTED as a refusal rather than dropped, so an
    /// unexpected sender shows up on the tripwire instead of nowhere.
    ///
    /// THE PER-BUILD NONCE PROBE LANE 2 ASKED FOR IS NOT HERE, and that is a fact about the
    /// interface contract rather than a decision: `extract::EXTRACT_JS` is a `&'static str`
    /// constant with no hole to bake a nonce into, so there is nothing for a payload to prove it
    /// knows. Closing that needs a `fn extract_js(nonce: &str) -> String` on the extract side.
    pub fn deliver(&self, host: &str, json: &str) {
        if host != YT_HOST {
            let mut it = self.lock();
            it.log.refused = it.log.refused.saturating_add(1);
            it.log.last_refusal = Some(format!(
                "a message arrived from {host}, which is not the live chat page"
            ));
            return;
        }
        self.absorb(model::parse_batch(json));
    }

    /// Fold one parsed batch into the log: carry the model's own refusals through, drop repeats in
    /// silence, count rows that cannot be deduplicated, and wake the UI once.
    ///
    /// A REPEAT IS NOT A REFUSAL AND THE DISTINCTION IS LOAD BEARING. Repeats are NORMAL and
    /// frequent here: a reload replays the whole visible backlog (75 to 77 rows measured), and the
    /// Top-chat to Live-chat switch replaces the list element so the script re-sweeps every child.
    /// Counting those as refusals would peg the tripwire at thousands within an hour and make the
    /// one number that is supposed to say "YouTube changed the page" say nothing at all.
    fn absorb(&self, batch: Batch) {
        let cap = self.tune.log_cap;
        let ctx = {
            let mut it = self.lock();
            it.last_post = Some(Instant::now());
            if !matches!(it.log.state, YtConn::Stopped) {
                it.log.state = YtConn::Reading;
            }
            it.log.refused = it.log.refused.saturating_add(u64::from(batch.refused));
            if let Some(why) = batch.last_refusal {
                it.log.last_refusal = Some(why);
            }
            for m in batch.kept {
                if m.id.is_empty() {
                    /* A row with no id cannot be deduplicated, so admitting it would draw it again
                     * on the next reload, forever. Counted rather than skipped, because an id
                     * disappearing from YouTube's rows is exactly the deploy this tripwire exists
                     * to announce. */
                    it.log.refused = it.log.refused.saturating_add(1);
                    it.log.last_refusal =
                        Some("a chat row arrived with no id, so it cannot be deduplicated".into());
                    continue;
                }
                if !it.seen.admit(&m.id) {
                    continue;
                }
                it.log.push(m, cap);
            }
            it.ctx.clone()
        };
        /* THE LOCK IS DROPPED BEFORE THE WAKE, on purpose. `request_repaint_of` reaches eframe's
         * repaint callback, and the shortest path to a deadlock in a UI thread callback is holding
         * a lock across a call into the framework that owns the thread. Nothing is lost by
         * dropping first: the log is already whole. */
        if let Some(ctx) = ctx {
            wake(&ctx);
        }
    }

    /// Read the log. A borrow under the lock rather than a clone: see the module note.
    fn with_log<R>(&self, f: impl FnOnce(&YtLog) -> R) -> R {
        f(&self.lock().log)
    }
}

/// The only host this surface accepts a message from. The live chat page is served from the bare
/// `www` host and nothing else was observed.
const YT_HOST: &str = "www.youtube.com";

/// Wake the UI. `request_repaint_of(ViewportId::ROOT)` and not `request_repaint()`, because this
/// runs inside no viewport's callback and the plain call would target whatever viewport egui last
/// had in hand. chat.rs:934-939 names the root the same way for the same reason.
fn wake(ctx: &egui::Context) {
    ctx.request_repaint_of(ViewportId::ROOT);
}

/* ---------------------------------------------------------------------- the seam -- */

/// THE PAGE, behind an interface, so everything above and below it is provable with no browser.
///
/// Five methods, each with exactly one production caller in `YtChat::tick`. `root` is the parent
/// window's handle: an `isize`, which is this tree's existing vocabulary for a window (see
/// `player::hwnd`, which is compiled on every platform and speaks `isize` throughout), and `None`
/// on a platform or a frame that does not name one, which the implementor turns into its own
/// honest sentence rather than this file guessing at one.
pub trait Pane {
    /// Point the page at `url`, building it if there is none and NAVIGATING if there is.
    ///
    /// NAVIGATING RATHER THAN REBUILDING IS THE ONE PLACE THIS DESIGN DIVERGES FROM
    /// `player::surface::Player::sync`, which tears its webview down on a feed change
    /// (surface.rs:205-212). The player must: the mute flag is baked into the embed URL and a
    /// rebuild is the only thing that re-applies it. Here nothing but the address differs, the
    /// injected script survives navigation by construction (it is a document-created script, not
    /// an `evaluate_script`), and a rebuild would cost a fresh environment attach and another
    /// `WM_USER + 0x65` sent at the root window by wry's `Drop` (webview2/mod.rs:70-77).
    fn show(&mut self, root: Option<isize>, url: &str) -> Result<(), Problem>;
    /// Reload the live page. The watchdog's only remedy, and it is not a remedy for a throttled
    /// renderer, which is why `Pane::suspended` exists beside it.
    fn reload(&mut self) -> Result<(), Problem>;
    /// Tear the page down. No hidden browser is left fetching a live chat after the reader stopped
    /// asking for it; the same rule `Player::stop` states about bandwidth (surface.rs:144-149).
    fn close(&mut self);
    /// Is there a live page right now?
    fn showing(&self) -> bool;
    /// The BROWSER's own answer to "is this document suspended", or `None` when it cannot be
    /// asked. See the implementation for why the answer is worth printing.
    fn suspended(&self) -> Option<bool>;
}

/* ---------------------------------------------------------------------- the plan -- */

/// What one call of `YtChat::tick` has decided to do to the page.
///
/// THE DECIDING IS SEPARATED FROM THE DOING FOR TWO REASONS AND BOTH ARE LOAD BEARING. The first
/// is that the whole watchdog ladder becomes a pure function over a struct and a clock, so a test
/// proves "silence reloads once, then waits" in microseconds with no browser. The second is the
/// re-entrancy note on [`Sink`]: `show` and `reload` can pump the Win32 message loop, and the
/// mutex must not be held across either, so the decision is taken under the lock and the lock is
/// gone before anything acts on it.
/// (No derives. A `Plan` is built by `plan`, matched once by `YtChat::tick` and dropped; it is
/// never cloned, compared or printed, and a derive nothing calls is the same unused thing as a
/// helper nothing calls.)
enum Plan {
    /// Nothing to do this frame, which is almost every frame.
    Nothing,
    /// Nobody is asking any more. Tear the page down.
    Close,
    /// Point the page here. `fresh` when the room changed, which clears the log.
    Show { url: String, fresh: bool },
    /// The page has gone quiet past the limit and the last reload is old enough to try again.
    Reload,
    /// Quiet past the limit, but a reload happened too recently to be worth another.
    Waiting,
}

/// The whole rule, as a function of state and a clock. See `Plan` for why it is a function.
fn plan(it: &Inner, tune: &Tuning, showing: bool, now: Instant) -> Plan {
    let Some(want) = it.want.as_deref() else {
        /* `shown` is checked as well as `showing` so a teardown still runs when a build failed
         * half way and left the surface with a recorded room and no page. */
        return if showing || it.shown.is_some() {
            Plan::Close
        } else {
            Plan::Nothing
        };
    };
    let same_room = it.shown.as_deref() == Some(want);
    if !showing || !same_room {
        /* A PROBLEM STOPS HERE AND IS NOT RETRIED ON A TIMER. `player::keeps_syncing` states the
         * argument (player.rs:610-623) and it holds unchanged: the ordinary reason a build fails
         * is a machine with no WebView2 runtime, and a frame-rate loop against that spends the
         * reader's CPU to learn nothing. The door back in is `YtChat::start`, which the screen
         * calls when the reader asks again, and which runs `after_retry`. */
        if it.problem.is_some() {
            return Plan::Nothing;
        }
        return Plan::Show {
            url: live_chat_url(want),
            fresh: !same_room,
        };
    }
    /* A live page on the right room. The only question left is whether it is still talking. */
    let quiet = now.saturating_duration_since(it.last_post.unwrap_or(now));
    if quiet < tune.silent_limit {
        return Plan::Nothing;
    }
    match it.last_reload {
        Some(t) if now.saturating_duration_since(t) < tune.reload_every => Plan::Waiting,
        _ => Plan::Reload,
    }
}

/* -------------------------------------------------------------------- the surface -- */

/// The YouTube chat surface. One per process, held beside `player::Player` in the `App`.
///
/// IT DOES NOT IMPLEMENT `Default` AND THAT IS THE POINT, and the argument is
/// `chat::ChatReader`'s, word for word (chat.rs:686-692): `Screens` derives `Default` and is built
/// whole at startup for every user whether or not the feed is ever looked at, so anything that
/// opens a browser from a `Default` opens one on every launch forever, including for the people
/// who never read chat. [`YtChat::idle`] is the constructor, it opens nothing, and
/// [`YtChat::start`] is the only door.
/// A read only view of a [`YtChat`]'s log. See [`YtChat::handle`].
#[derive(Clone)]
pub struct YtHandle {
    sink: Arc<Sink>,
}

impl Default for YtHandle {
    /// A handle onto a log nothing writes to: what every context that must NAME one without
    /// having a feed behind it uses, which is every test context in this crate.
    fn default() -> YtHandle {
        YtHandle {
            sink: Arc::new(Sink::new(Tuning::default())),
        }
    }
}

impl YtHandle {
    pub fn with_log<R>(&self, f: impl FnOnce(&YtLog) -> R) -> R {
        self.sink.with_log(f)
    }
}

pub struct YtChat {
    sink: Arc<Sink>,
    pane: Box<dyn Pane>,
}

impl YtChat {
    /// A surface that has done nothing. No window, no browser process, no folder on disk.
    ///
    /// WHAT IS AND IS NOT CREATED HERE. On Windows the `wry::WebContext` is constructed, because
    /// its profile folder is read inside the build and cannot be repointed afterwards, so it has
    /// to exist before any page does. That construction is free: on Windows a `WebContext` is a
    /// `PathBuf` in a box and `WebContextImpl::new` is an empty function (wry 0.56.1
    /// web_context.rs:101-112). The FOLDER is not created until the first `show`, so a launch that
    /// never opens the feed leaves nothing behind.
    pub fn idle() -> YtChat {
        YtChat::with_tuning(Tuning::default())
    }

    /// [`YtChat::idle`] on caller chosen numbers. `Tuning::default()` is what production uses.
    pub fn with_tuning(tune: Tuning) -> YtChat {
        let sink = Arc::new(Sink::new(tune));
        let pane = new_pane(&sink);
        YtChat { sink, pane }
    }

    /// Ask for `video_id`'s live chat. Records the want; [`YtChat::sync`] is what acts on it.
    ///
    /// A DIFFERENT VIDEO ID IS A REAL SWITCH HERE, AND IN `chat::ChatReader::start` IT IS A
    /// REFUSAL. That difference is deliberate and it is about where the value comes from. The
    /// Twitch reader is handed a channel name that a settings field is being TYPED into, so one
    /// frame carrying a half-edited name would tear down a live socket sixty times a second. This
    /// is handed `watcher::Channel::video_id`, which is polled, which already refuses an answer
    /// that does not name the right channel (player.rs:103-110), and which changes exactly when
    /// the stream does. Refusing the switch here would leave the app reading a dead stream's chat
    /// after he went live again, which is the failure this argument is choosing between.
    ///
    /// IT IS ALSO THE RETRY DOOR. `after_retry` clears a failed build so the next `sync` tries
    /// once more, and keeps a refusal, so a machine that cannot host a webview is not asked again
    /// and again. player.rs:655-659.
    pub fn start(&self, ctx: &egui::Context, video_id: &str) {
        let want = video_id.trim();
        let mut it = self.sink.lock();
        if it.ctx.is_none() {
            it.ctx = Some(ctx.clone());
        }
        if want.is_empty() {
            /* An empty id would build `.../live_chat?is_popout=1&v=` and load a page with no chat
             * on it, which is a blank feed with no reason attached. Said out loud instead. */
            it.log.state = YtConn::Stalled(
                "no live video id is known for this channel yet, so there is no chat page to open"
                    .to_owned(),
            );
            return;
        }
        it.problem = after_retry(it.problem.take());
        if it.want.as_deref() == Some(want) {
            return;
        }
        log::info!("youtube chat: asked for {want}");
        it.want = Some(want.to_owned());
        if matches!(it.log.state, YtConn::Idle | YtConn::Stopped) {
            it.log.state = YtConn::Loading;
        }
    }

    /// Read the log. See [`Sink::deliver`] for what put things in it.
    pub fn with_log<R>(&self, f: impl FnOnce(&YtLog) -> R) -> R {
        self.sink.with_log(f)
    }

    /// A READ ONLY VIEW OF THIS FEED'S LOG, CLONEABLE, THAT CANNOT OPEN A PAGE OR CLOSE ONE.
    ///
    /// The same split `chat::ChatHandle` makes and for the same reason: `YtChat` owns a browser
    /// pane and must live in exactly one place, the `App`, but the Chat screen is drawn in two
    /// windows and a tool window's context is a CLONE of what the root holds. So the LOG travels
    /// and the pane does not. One `Arc` clone per frame.
    ///
    /// NO `start`, NO `stop`, NO `sync`. A screen states that it wants the feed (`Cx::yt_wanted`)
    /// and `App::ui` acts, which is the rule every other thread owner in this crate follows.
    pub fn handle(&self) -> YtHandle {
        YtHandle {
            sink: self.sink.clone(),
        }
    }

    /// Stop asking. The page goes at the next [`YtChat::sync`], which is the same frame, because
    /// the screen that stops the feed is a screen that is being drawn.
    ///
    /// IT CANNOT TEAR THE PAGE DOWN ITSELF, and that is the interface contract rather than a
    /// choice: `stop` takes `&self` and the webview is owned behind a `&mut`. If `sync` were ever
    /// to stop being called, `Drop` is the backstop and it is not a hypothetical one, since
    /// dropping the `App` drops this, which drops the pane, which drops the webview.
    pub fn stop(&self) {
        let mut it = self.sink.lock();
        if it.want.is_some() {
            log::info!("youtube chat: asked to stop");
        }
        it.want = None;
    }

    /// One frame of the surface. Called from `App::ui`, from the ROOT viewport's pass only.
    ///
    /// `host` MUST BE THE ROOT WINDOW, for the same reason `Player::sync` says so
    /// (player/surface.rs:181-183): `eframe::Frame` carries the root handle even inside a deferred
    /// viewport's callback, so a page built from a pop-out's pass would be a child of the wrong
    /// window. It is only ever read for its handle; nothing here draws.
    ///
    /// THE FIRST BUILD IS A VISIBLE FRAME HITCH of a few hundred milliseconds, because
    /// `webview2_com::wait_with_pump` blocks while the environment and the controller are created.
    /// The player has the same hitch on its first build and it is accepted there; it is named here
    /// so nobody has to rediscover it, and it is one of the reasons the page is built on first
    /// need rather than at startup.
    pub fn sync(&mut self, host: &eframe::Frame, ctx: &egui::Context) {
        {
            let mut it = self.sink.lock();
            if it.ctx.is_none() {
                it.ctx = Some(ctx.clone());
            }
        }
        self.tick(root_handle(host), Instant::now());
    }

    /// [`YtChat::sync`] with the window handle and the clock handed in. This is the seam's other
    /// half: everything `sync` does that is not "ask eframe for a handle" happens here, so a test
    /// drives a whole watchdog ladder with a fake pane, a fake clock and no window at all.
    fn tick(&mut self, root: Option<isize>, now: Instant) {
        let (plan, want) = {
            let it = self.sink.lock();
            (
                plan(&it, &self.sink.tune, self.pane.showing(), now),
                it.want.clone(),
            )
        };
        /* THE LOCK IS GONE FROM HERE DOWN. See [`Sink`]: `show` and `reload` pump the message
         * loop, so an IPC callback can run re-entrantly, on this thread, inside either call. */
        match plan {
            Plan::Nothing => {}
            Plan::Close => {
                self.pane.close();
                let mut it = self.sink.lock();
                it.shown = None;
                it.last_post = None;
                it.log.state = YtConn::Stopped;
                log::info!("youtube chat: page closed");
            }
            Plan::Show { url, fresh } => {
                if fresh {
                    /* A NEW STREAM IS A NEW ROOM. Carrying the old lines across would put a
                     * finished stream's conversation above a new one's with no seam a reader could
                     * see, and carrying the old ids would suppress a genuinely new message that
                     * happened to reuse one. Both go. */
                    let mut it = self.sink.lock();
                    it.log.clear_room();
                    it.seen.clear();
                    it.log.state = YtConn::Loading;
                }
                let outcome = self.pane.show(root, &url);
                let mut it = self.sink.lock();
                match outcome {
                    Ok(()) => {
                        /* The clock starts at the LOAD and not at the first message, so a page
                         * that comes up and never runs its script is caught by the watchdog rather
                         * than sitting on "Loading" forever. */
                        it.last_post = Some(now);
                        it.shown = want;
                        it.problem = None;
                        it.log.state = YtConn::Loading;
                        log::info!("youtube chat: page up at {url}");
                    }
                    Err(p) => {
                        it.shown = None;
                        it.log.state = YtConn::Stalled(p.words().to_owned());
                        log::warn!("youtube chat: no page: {}", p.words());
                        it.problem = Some(p);
                    }
                }
            }
            Plan::Reload => {
                /* ASKED BEFORE THE RELOAD, because a reload replaces the document and the question
                 * is about the one that went quiet. WebView2 only suspends when the app calls
                 * `TrySuspend`, which nothing here ever does, so a `true` is a fact worth putting
                 * in front of a person: it separates "YouTube's feed stopped" from "this machine
                 * throttled the renderer", and a reload is a remedy for the first only. */
                let suspended = self.pane.suspended();
                let outcome = self.pane.reload();
                let mut it = self.sink.lock();
                it.reloads = it.reloads.saturating_add(1);
                it.last_reload = Some(now);
                /* The silence clock restarts with the page. Without this the next tick would see
                 * the same old `last_post`, decide `Reload` again, and be held off only by the
                 * reload floor, which would make every stall a permanent `Waiting`. */
                it.last_post = Some(now);
                let words = stall_words(it.reloads, suspended);
                match outcome {
                    Ok(()) => {
                        log::warn!("youtube chat: {words}");
                        it.log.state = YtConn::Stalled(words);
                    }
                    Err(p) => {
                        log::warn!("youtube chat: reload failed: {}", p.words());
                        it.log.state = YtConn::Stalled(p.words().to_owned());
                        it.problem = Some(p);
                    }
                }
            }
            Plan::Waiting => {
                let mut it = self.sink.lock();
                let n = it.reloads;
                it.log.state = YtConn::Stalled(format!(
                    "the chat page is still quiet after being reloaded {n} time(s); waiting before \
                     reloading it again"
                ));
            }
        }
    }

    /// One tick against a made up window handle and a made up clock. Test only: this is what lets
    /// the whole watchdog ladder be asserted in microseconds with no browser and no event loop.
    /// The handle is never dereferenced, because the pane behind it is the fake one.
    #[cfg(test)]
    fn step(&mut self, now: Instant) {
        self.tick(Some(1), now);
    }
}

/// The sentence a stall puts on the screen. Every clause in it is a fact, including the count,
/// because a log that quietly reloaded itself four times looks exactly like a healthy one.
fn stall_words(reloads: u32, suspended: Option<bool>) -> String {
    let tail = match suspended {
        Some(true) => {
            ", and the browser reports this document SUSPENDED, which a reload does not fix and \
             which nothing in this app ever asks for"
        }
        Some(false) => ", and the browser reports the document is not suspended",
        None => "",
    };
    format!(
        "nothing has arrived from the chat page for longer than {}s, which is far past its own \
         20s heartbeat, so it was reloaded ({reloads} time(s) so far){tail}",
        SILENT_LIMIT.as_secs()
    )
}

/* ------------------------------------------------------------------- the platform -- */

/// The ROOT window's handle, as eframe reports it, or `None` on a platform that does not name one.
///
/// READ FRESH EVERY FRAME AND NEVER MEMOISED. The player learned this the other way round and
/// caches its handle deliberately (player/surface.rs:69-74), but it caches it to find its way HOME
/// after a move. Nothing here ever moves, so the only use of the handle is "who is the parent of
/// the page I am about to build", and the honest answer to that is whatever eframe is holding on
/// the frame that is asking.
#[cfg(windows)]
fn root_handle(host: &eframe::Frame) -> Option<isize> {
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match host.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn root_handle(_host: &eframe::Frame) -> Option<isize> {
    None
}

#[cfg(windows)]
fn new_pane(sink: &Arc<Sink>) -> Box<dyn Pane> {
    Box::new(win::WebPane::new(sink.clone()))
}

#[cfg(not(windows))]
fn new_pane(_sink: &Arc<Sink>) -> Box<dyn Pane> {
    Box::new(AbsentPane)
}

/// The page on a platform that has no WebView2. Everything about it is `cfg(not(windows))`.
///
/// A PLATFORM GAP STATED OUT LOUD, NOT A STUB THAT PRETENDS, which is `player/surface_absent.rs`'s
/// posture and its words are borrowed. The refusal is a [`Problem::Refused`] and not a
/// `Problem::Failed`, so `after_retry` keeps it and `plan` never asks again: no click puts
/// WebView2 on a machine that is not Windows, and an enabled retry for an outcome that cannot
/// change is the same defect as a control for a feature that does not exist.
#[cfg(not(windows))]
pub struct AbsentPane;

#[cfg(not(windows))]
impl Pane for AbsentPane {
    fn show(&mut self, _root: Option<isize>, _url: &str) -> Result<(), Problem> {
        Err(Problem::Refused(
            "reading YouTube chat needs Microsoft WebView2, which exists on Windows only; the \
             Twitch side of the feed still works"
                .to_owned(),
        ))
    }

    fn reload(&mut self) -> Result<(), Problem> {
        Err(Problem::Refused(
            "there is no chat page on this platform to reload".to_owned(),
        ))
    }

    fn close(&mut self) {}

    fn showing(&self) -> bool {
        false
    }

    fn suspended(&self) -> Option<bool> {
        None
    }
}

/* ------------------------------------------------------------------- the real page -- */

#[cfg(windows)]
mod win {
    use std::path::PathBuf;

    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
    use windows_core::Interface;
    use wry::raw_window_handle::{
        HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
    };
    use wry::{WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

    use super::super::extract::EXTRACT_JS;
    use super::{Pane, Problem, Sink, YT_HOST};
    use crate::player::PROFILE_VENDOR;

    /// The leaf under `PROFILE_VENDOR` that holds THIS surface's WebView2 user data.
    ///
    /// A SECOND FOLDER, NOT THE PLAYER'S, AND IT IS A REQUIREMENT RATHER THAN TIDINESS. wry's own
    /// doc says it, with Microsoft's page linked beside it: "Webview instances with different
    /// `CoreWebView2EnvironmentOptions` must have different `data_directory`s" (wry 0.56.1
    /// web_context.rs:41-44), and a mismatch fails the build with
    /// `HRESULT_FROM_WIN32(ERROR_INVALID_STATE)`. This surface passes `ARGS` and the player does
    /// not, so the options differ by construction and the folders must too. The cost is a second
    /// browser process tree rather than a second renderer, which is the trade recorded in the
    /// design note; it is paid only while somebody is reading the feed, because the page is built
    /// on first need and dropped on the first frame nobody wants it.
    const PROFILE_LEAF: &str = "WebView2YtChat";

    /// The Chromium switches, spelled once.
    ///
    /// THE FIRST CLAUSE IS NOT DECORATION. `with_additional_browser_args` REPLACES wry's defaults
    /// rather than appending to them (wry 0.56.1 webview2/mod.rs:300 takes the caller's string
    /// through `unwrap_or_else`), so dropping that clause would silently re-enable the Edge mini
    /// menu, the PDF viewer UI and SmartScreen for this webview. The three that follow are the
    /// reason this surface needs a data directory of its own.
    ///
    /// AND THEY ARE A REQUEST, NOT A GUARANTEE. Microsoft's `AdditionalBrowserArguments` page says
    /// an unparseable or unwelcome switch is IGNORED with no error, so a successful build proves
    /// nothing about whether any of these took effect. That is what `WebPane::suspended` and the
    /// silence watchdog are for: the app finds out on the reader's machine, in a log line, rather
    /// than in an argument.
    ///
    /// `--autoplay-policy` IS ABSENT DELIBERATELY. There is no media on a chat page and nothing
    /// asked for it, and the player's copy of that flag is exactly what makes the two argument
    /// strings differ.
    const ARGS: &str = concat!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
        " --disable-background-timer-throttling",
        " --disable-renderer-backgrounding",
        " --disable-backgrounding-occluded-windows",
    );

    /// Where the page is put: a plausible chat viewport, parked far outside the parent's client
    /// rectangle so the parent clips it out of existence.
    ///
    /// VISIBLE AND OFF SCREEN, NOT `set_visible(false)`, AND THIS IS THE ONE DECISION THE WHOLE
    /// SURFACE RESTS ON. Microsoft documents what hiding costs, on `CoreWebView2Controller
    /// .IsVisible`: "There are CPU and memory benefits when the page is hidden. For instance
    /// Chromium has code that throttles activities on the page like animations and some tasks are
    /// run less frequently." A throttled page is a dead bridge, so the page is visible and simply
    /// has nowhere to appear.
    ///
    /// 420x900 AND NOT 1x1. The DOM shape that was measured (72 to 92 rows present in `#items`) is
    /// what a plausible chat viewport produces. A one pixel viewport is a different rendering
    /// problem for no benefit, and it is the one that breaks first if YouTube ever virtualises the
    /// list by height. The rectangle is pushed once at build and never touched again, so there is
    /// no `last_bounds` memo here to get wrong the way `Player::place` had to.
    const OFF_SCREEN: (i32, i32, u32, u32) = (-8192, -8192, 420, 900);

    /// A parent window, by handle, in the shape `wry::WebViewBuilder::build_as_child` wants.
    ///
    /// WHY THIS EXISTS AT ALL, since `eframe::Frame` already implements `HasWindowHandle` and the
    /// player passes it straight through. Because passing the frame down would put an
    /// `&eframe::Frame` in `Pane::show`'s signature, and nothing in a test can construct one, so
    /// the whole watchdog would be unprovable without a running app. An `isize` crosses the seam,
    /// a test passes any number, and this turns it back into what wry wants at the one place that
    /// actually builds a window.
    struct Root(std::num::NonZeroIsize);

    impl HasWindowHandle for Root {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
            /* SAFETY: the handle is the root window's, read off `eframe::Frame` on this same
             * frame, on the thread that owns it, and it is consumed immediately by
             * `build_as_child`. The borrow cannot outlive this call: `WindowHandle<'_>` is tied to
             * `&self` and `Root` is a stack local of `show`. */
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    /// The real page: a WebView2 child of the root window, on its own profile, running the
    /// extraction script and posting back through [`Sink::deliver`].
    pub struct WebPane {
        /// Held for the surface's lifetime. Its folder is read inside the build and cannot be
        /// repointed afterwards, so it has to exist before any page does; constructing it costs
        /// nothing on Windows (wry 0.56.1 web_context.rs:101-112).
        web_context: wry::WebContext,
        /// Kept so a failure can name the folder it could not use.
        profile: Option<PathBuf>,
        webview: Option<wry::WebView>,
        sink: std::sync::Arc<Sink>,
    }

    impl WebPane {
        pub fn new(sink: std::sync::Arc<Sink>) -> WebPane {
            let profile = dirs::data_local_dir().map(|d| d.join(PROFILE_VENDOR).join(PROFILE_LEAF));
            WebPane {
                web_context: wry::WebContext::new(profile.clone()),
                profile,
                webview: None,
                sink,
            }
        }
    }

    impl Pane for WebPane {
        fn show(&mut self, root: Option<isize>, url: &str) -> Result<(), Problem> {
            if let Some(wv) = &self.webview {
                return wv
                    .load_url(url)
                    .map_err(|e| Problem::Failed(format!("could not open {url}: {e}")));
            }
            let Some(root) = root.and_then(std::num::NonZeroIsize::new) else {
                return Err(Problem::Failed(
                    "the main window does not name a Win32 handle, so there is nowhere to put the \
                     chat page"
                        .to_owned(),
                ));
            };
            /* THE FOLDER IS CREATED HERE AND NOT AT STARTUP, so a launch that never reads chat
             * leaves nothing on disk. A machine where it cannot be created is a REFUSAL with the
             * path in the message, decided the same way `Player::new` decides it
             * (player/surface.rs:89-109): falling back to wry's default beside the binary would
             * put a browser profile in Program Files. */
            let Some(dir) = self.profile.clone() else {
                return Err(Problem::Refused(
                    "this system does not name a per-user local data folder, so there is nowhere \
                     to put the chat page's WebView2 profile"
                        .to_owned(),
                ));
            };
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return Err(Problem::Refused(format!(
                    "could not create {}: {e}",
                    dir.display()
                )));
            }

            let sink = self.sink.clone();
            let (x, y, w, h) = OFF_SCREEN;
            /* BOUND TO A LOCAL RATHER THAN PASSED AS A TEMPORARY, because
             * `build_as_child<W>(self, window: &'a W)` ties the window reference to the SAME
             * lifetime as the `&mut WebContext` the builder was made from (wry 0.56.1
             * lib.rs:921 and 1571). A temporary in the call would make those two lifetimes fight
             * over one statement for no reason. */
            let parent = Root(root);
            let webview = {
                let ctx = &mut self.web_context;
                WebViewBuilder::new_with_web_context(ctx)
                    .with_additional_browser_args(ARGS)
                    /* AN INITIALISATION SCRIPT AND NOT AN `evaluate_script`, and it is not a close
                     * call. This becomes WebView2's `AddScriptToExecuteOnDocumentCreated` (wry
                     * 0.56.1 webview2/mod.rs:1370), which runs before any page script on EVERY
                     * document. This surface reloads on purpose (the watchdog) and navigates on
                     * purpose (a new stream), and both wipe an `evaluate_script`. The script's own
                     * `if (window.__ytChat) return` guard is what makes running it again free. */
                    .with_initialization_script(EXTRACT_JS)
                    .with_ipc_handler(move |req: wry::http::Request<String>| {
                        /* THE UI THREAD, USUALLY BETWEEN FRAMES AND SOMETIMES NOT. See the note on
                         * `Sink`: this can fire re-entrantly inside another pane's build, so it
                         * captures nothing but the sink, does one parse and one append, and calls
                         * nothing that pumps a message loop. */
                        let host = req.uri().host().unwrap_or("").to_owned();
                        sink.deliver(&host, req.body());
                    })
                    .with_visible(true)
                    .with_bounds(wry::Rect {
                        position: wry::dpi::PhysicalPosition::new(x, y).into(),
                        size: wry::dpi::PhysicalSize::new(w, h).into(),
                    })
                    .with_url(url)
                    .build_as_child(&parent)
                    .map_err(|e| Problem::Failed(format!("could not create the chat page: {e}")))?
            };
            log::info!(
                "youtube chat: surface up, profile {}, args {ARGS}, accepting messages from \
                 {YT_HOST} only",
                dir.display()
            );
            self.webview = Some(webview);
            Ok(())
        }

        fn reload(&mut self) -> Result<(), Problem> {
            let Some(wv) = &self.webview else {
                return Err(Problem::Failed(
                    "there is no chat page to reload".to_owned(),
                ));
            };
            wv.reload()
                .map_err(|e| Problem::Failed(format!("could not reload the chat page: {e}")))
        }

        fn close(&mut self) {
            self.webview = None;
        }

        fn showing(&self) -> bool {
            self.webview.is_some()
        }

        /// `ICoreWebView2_3::IsSuspended`, the browser's own account of the document's state.
        ///
        /// THE READBACK POSTURE, WHICH THIS CODEBASE ALREADY TAKES ONCE. `Player::poll_audio` does
        /// not trust the mute flag it set, it asks `IsDocumentPlayingAudio`
        /// (player/surface.rs:560-593), because a player's own `isMuted()` returns the value we
        /// gave it. Same here: `ARGS` asks Chromium not to background this renderer and
        /// Microsoft documents that an unwelcome switch is ignored silently, so the only honest
        /// answer about the document's state comes from the document. Nothing in this app calls
        /// `TrySuspend`, so a `true` means something suspended it that we did not, which is
        /// exactly the diagnosis a reload cannot fix and the reader should be told about.
        fn suspended(&self) -> Option<bool> {
            let w3 = self
                .webview
                .as_ref()?
                .webview()
                .cast::<ICoreWebView2_3>()
                .ok()?;
            let mut out = windows_core::BOOL(0);
            /* SAFETY: `w3` is a live WebView2 interface obtained by QueryInterface from the
             * webview this pane owns, and `out` is a stack local of the out-parameter's type. */
            unsafe { w3.IsSuspended(&mut out) }.ok()?;
            Some(out.as_bool())
        }
    }
}

/* THERE IS NO `pub use win::WebPane`, AND THAT IS DELIBERATE. Nothing outside this file has any
 * business naming the real pane: the surface is driven through `YtChat::sync` and read through
 * `YtChat::with_log`, exactly as `PlayerView` keeps a screen from holding the player's webview
 * (player.rs:661-693). Re-exporting it would be one more `pub` item with no caller, which is this
 * repository's signature defect. */

/* ----------------------------------------------------------------------- the tests -- */

#[cfg(test)]
mod tests {
    /* `Batch`, `YtMessage`, `model`, `live_chat_url`, `Problem` and the clock types all arrive
     * through this one glob, which reaches the parent module's own imports as well as its items.
     * `model::YtKind` and `model::YtPiece` are written out at their use sites rather than imported
     * again, so nothing here has to guess how a uniform path resolves. */
    use super::*;

    /// What the fake pane was asked to do. Shared with the test through an `Arc` rather than read
    /// back off the trait object, because a downcast would need `Any` on [`Pane`] and that is a
    /// production cost paid entirely for a test.
    #[derive(Default)]
    struct Calls {
        shown: Vec<String>,
        reloads: u32,
        closes: u32,
        live: bool,
        /// What the next `show` answers with. `None` is success.
        refuse: Option<Problem>,
        suspended: Option<bool>,
    }

    /// A pane that records and hosts nothing. This is the browserless half of the seam, and it is
    /// the reason every assertion below can be made at all.
    struct FakePane {
        calls: Arc<Mutex<Calls>>,
    }

    impl Pane for FakePane {
        fn show(&mut self, _root: Option<isize>, url: &str) -> Result<(), Problem> {
            let mut c = self.calls.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(p) = c.refuse.clone() {
                return Err(p);
            }
            c.shown.push(url.to_owned());
            c.live = true;
            Ok(())
        }
        fn reload(&mut self) -> Result<(), Problem> {
            let mut c = self.calls.lock().unwrap_or_else(|p| p.into_inner());
            c.reloads += 1;
            Ok(())
        }
        fn close(&mut self) {
            let mut c = self.calls.lock().unwrap_or_else(|p| p.into_inner());
            c.closes += 1;
            c.live = false;
        }
        fn showing(&self) -> bool {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).live
        }
        fn suspended(&self) -> Option<bool> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .suspended
        }
    }

    /// Everything a test needs to drive a surface: the surface, the log behind it, and the record
    /// of what the page was asked to do.
    struct Rig {
        chat: YtChat,
        sink: Arc<Sink>,
        calls: Arc<Mutex<Calls>>,
        ctx: egui::Context,
    }

    impl Rig {
        /// A surface over the fake pane. Nothing in here can open anything.
        fn new(tune: Tuning) -> Rig {
            let sink = Arc::new(Sink::new(tune));
            let calls = Arc::new(Mutex::new(Calls::default()));
            let chat = YtChat {
                sink: sink.clone(),
                pane: Box::new(FakePane {
                    calls: calls.clone(),
                }),
            };
            Rig {
                chat,
                sink,
                calls,
                ctx: egui::Context::default(),
            }
        }

        fn calls<R>(&self, f: impl FnOnce(&mut Calls) -> R) -> R {
            f(&mut self.calls.lock().unwrap_or_else(|p| p.into_inner()))
        }

        fn log<R>(&self, f: impl FnOnce(&YtLog) -> R) -> R {
            self.sink.with_log(f)
        }
    }

    /// Numbers small enough that a whole watchdog ladder runs inside one test. This is the reason
    /// [`Tuning`] is a struct rather than four constants; chat.rs:599-611 says the same.
    fn fast() -> Tuning {
        Tuning {
            log_cap: 3,
            seen_cap: 8,
            silent_limit: Duration::from_millis(100),
            reload_every: Duration::from_millis(500),
        }
    }

    fn msg(id: &str) -> YtMessage {
        YtMessage {
            /* Test fixtures build unpaid rows; the paid path has its own test. */
            amount: None,
            id: id.to_owned(),
            author: "@somebody".to_owned(),
            author_type: String::new(),
            body: "hello".to_owned(),
            kind: model::YtKind::Chat,
            ts_usec: Some(1_788_572_284_975_162),
            pieces: Vec::<model::YtPiece>::new(),
            /* THE MODEL STAMPS THIS AND THIS FILE NEVER READS IT, which is why it is a constant
             * here rather than a clock reading. `model::YtMessage::seen_ms` is Unix milliseconds
             * frozen at parse time so that `model::interleave` has one clock both platforms speak;
             * everything this file decides (dedup, the cap, the drop count, the watchdog) is
             * decided off `Instant`s it takes itself, so a fabricated arrival stamp cannot make
             * any assertion below pass or fail. It is the millisecond half of the `ts_usec` above,
             * so the two do not contradict each other for anyone reading the fixture. */
            seen_ms: 1_788_572_284_975,
        }
    }

    /// A batch as the model would hand one over. Built here rather than parsed from a JSON string
    /// on purpose: what these tests are about is what THIS file does with a batch, and routing
    /// them through `model::parse_batch` would make every one of them go red for a defect in
    /// another lane's parser. The agreement between `extract::EXTRACT_JS`'s payload and
    /// `model::parse_batch` is that lane's test to write and the Verify phase's to run.
    fn batch(ids: &[&str]) -> Batch {
        Batch {
            kept: ids.iter().copied().map(msg).collect(),
            refused: 0,
            last_refusal: None,
        }
    }

    /// THE SAME MESSAGE TWICE IS ONE LINE.
    ///
    /// THE DEFECT: no dedup at all. It is invisible in a live-coded spike and unmissable in use,
    /// because a reload replays 75 to 77 already-seen rows and the Top-chat to Live-chat switch
    /// re-sweeps the whole list, so the log would double every time the watchdog fired.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `if !it.seen.admit(&m.id) { continue; }` guard
    /// in `Sink::absorb`.
    #[test]
    fn a_repeated_id_is_not_a_second_line() {
        let rig = Rig::new(fast());
        rig.sink.absorb(batch(&["a", "b"]));
        rig.sink.absorb(batch(&["b", "c"]));
        rig.log(|l| {
            assert_eq!(l.lines.len(), 3, "a, b and c, with b admitted once");
            assert_eq!(l.refused, 0, "a repeat is not something to be refused");
            assert_eq!(l.dropped, 0, "nothing was evicted at a cap of three");
        });
    }

    /// THE CAP EVICTS THE OLDEST AND SAYS SO.
    ///
    /// THE DEFECT: an unbounded deque, or a bounded one that evicts in silence. The first is a
    /// leak on a stream that runs for eight hours; the second tells a reader the log is complete
    /// when it is not.
    ///
    /// THE MUTATION THAT MAKES THIS RED: drop the `self.dropped = self.dropped.saturating_add(1)`
    /// line in `YtLog::push` (the count assertion), or the `while` around the pop (the length and
    /// the front-of-queue ones).
    #[test]
    fn the_cap_evicts_the_oldest_and_counts_it() {
        let rig = Rig::new(fast());
        rig.sink.absorb(batch(&["a", "b", "c", "d", "e"]));
        rig.log(|l| {
            assert_eq!(l.lines.len(), 3, "the cap is three");
            assert_eq!(l.dropped, 2, "and the two that went are counted");
            assert_eq!(
                l.lines.front().map(|m| m.id.as_str()),
                Some("c"),
                "the OLDEST goes, not the newest"
            );
        });
    }

    /// AN ID THAT FALLS OUT OF THE DEDUP SET COULD NEVER STILL BE ON SCREEN.
    ///
    /// THE DEFECT: `SEEN_CAP` at or below `LOG_CAP`. The failure needs a reload to show itself and
    /// then draws a visible message twice, under itself, which reads as YouTube sending it twice.
    ///
    /// THE MUTATION THAT MAKES THIS RED: set `SEEN_CAP` to `LOG_CAP`, or below it.
    /// A COMPILE TIME ASSERTION AND NOT A RUNTIME ONE, because both sides are constants and a
    /// test that can only ever pass or only ever fail is decided when the crate is built. clippy
    /// says so (`assertions_on_constants`) and it is right: this way the relationship is checked
    /// even by a build that never runs the tests.
    const _: () = assert!(SEEN_CAP > LOG_CAP);

    /// AND BY A COMFORTABLE MARGIN, not by one. Also compile time, same reason.
    const _: () = assert!(SEEN_CAP >= LOG_CAP * 2);

    /// THE DEDUP SET IS BOUNDED, AND IT FORGETS THE OLDEST FIRST.
    ///
    /// THE DEFECT: a set that only ever grows. An eight hour stream at the busiest measured
    /// YouTube rate is tens of thousands of ids nothing will ever ask about again.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `while self.ids.len() > self.cap` loop in
    /// `Seen::admit`.
    #[test]
    fn the_dedup_set_is_bounded_and_forgets_the_oldest_first() {
        let mut seen = Seen::new(2);
        assert!(seen.admit("a"));
        assert!(seen.admit("b"));
        assert!(!seen.admit("a"), "still remembered");
        assert!(seen.admit("c"), "and this pushes the oldest out");
        assert_eq!(seen.ids.len(), 2);
        assert!(seen.admit("a"), "the oldest has been forgotten");
    }

    /// A ROW WITH NO ID IS COUNTED, NEVER SILENTLY DROPPED AND NEVER DRAWN.
    ///
    /// THE DEFECT: two of them, and they are opposite. Drawing it puts a row in the log that every
    /// later reload draws again, because nothing can recognise it as a repeat. Skipping it quietly
    /// means the deploy where YouTube renames `.id` looks like a chat that went calm.
    ///
    /// THE MUTATION THAT MAKES THIS RED: change the `m.id.is_empty()` arm in `Sink::absorb` from
    /// counting a refusal to a bare `continue`.
    #[test]
    fn a_row_with_no_id_is_counted_not_drawn() {
        let rig = Rig::new(fast());
        let mut b = batch(&["good"]);
        b.kept.push(msg(""));
        rig.sink.absorb(b);
        rig.log(|l| {
            assert_eq!(l.lines.len(), 1, "only the row that can be deduplicated");
            assert_eq!(l.refused, 1);
            assert!(
                l.last_refusal
                    .as_deref()
                    .is_some_and(|w| w.contains("no id")),
                "the refusal says what was wrong: {:?}",
                l.last_refusal
            );
        });
    }

    /// THE MODEL'S OWN REFUSALS REACH THE TRIPWIRE.
    ///
    /// THE DEFECT: reading `Batch::kept` and throwing `Batch::refused` away. The parser would then
    /// be free to discard a shape it did not recognise, and the screen would show a calm, empty,
    /// wrong chat, which is the exact conflation this whole surface is arranged to prevent.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `saturating_add(u64::from(batch.refused))`
    /// line, or the `last_refusal` assignment beside it.
    #[test]
    fn what_the_model_refused_is_carried_through_to_the_log() {
        let rig = Rig::new(fast());
        rig.sink.absorb(Batch {
            kept: Vec::new(),
            refused: 7,
            last_refusal: Some("a row shape nobody recognises".to_owned()),
        });
        rig.log(|l| {
            assert_eq!(l.refused, 7);
            assert_eq!(
                l.last_refusal.as_deref(),
                Some("a row shape nobody recognises")
            );
            assert!(l.lines.is_empty());
        });
    }

    /// A MESSAGE FROM ANYWHERE BUT THE CHAT PAGE IS REFUSED AND COUNTED.
    ///
    /// THE DEFECT: trusting `window.ipc`. wry defines it on the page (wry 0.56.1
    /// webview2/mod.rs:946-951) and every script the site serves can call it, so without this
    /// check any of them can put a line of its own choosing in front of the reader.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `host != YT_HOST` branch in `Sink::deliver`.
    #[test]
    fn a_message_from_another_origin_is_refused() {
        let rig = Rig::new(fast());
        rig.sink.deliver("ads.example", "[]");
        rig.log(|l| {
            assert_eq!(l.refused, 1);
            assert!(l.lines.is_empty());
            assert!(
                l.last_refusal
                    .as_deref()
                    .is_some_and(|w| w.contains("ads.example")),
                "the refusal names the sender: {:?}",
                l.last_refusal
            );
        });
    }

    /// IDLE OPENS NOTHING, AND NEITHER DOES A FRAME WITH NOBODY ASKING.
    ///
    /// THE DEFECT: a surface that builds its page from a constructor, or from the first `sync`,
    /// which puts a second browser process tree on the machine of every reader who never opens
    /// the feed. This is `ChatReader`'s no-`Default` rule (chat.rs:686-692) at the frame level.
    ///
    /// THE MUTATION THAT MAKES THIS RED: make `plan` return a `Show` when `want` is `None`.
    #[test]
    fn nothing_is_opened_until_somebody_asks() {
        let mut rig = Rig::new(fast());
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.chat.step(t0 + Duration::from_secs(60));
        rig.log(|l| assert_eq!(l.state, YtConn::Idle));
        rig.calls(|c| {
            assert!(!c.live, "no page was built");
            assert!(c.shown.is_empty());
        });
    }

    /// SILENCE PAST THE LIMIT RELOADS THE PAGE ONCE, AND THEN WAITS.
    ///
    /// THE DEFECT: an ungated watchdog. A reload drops the page's backlog and costs a fresh 249 KB
    /// fetch, and the failure it cannot fix, a throttled or suspended renderer, is precisely the
    /// one that keeps the silence going, so without the floor a dead channel is hammered on every
    /// tick forever.
    ///
    /// THE MUTATION THAT MAKES THIS RED: remove the `last_reload` arm from `plan`, so `Waiting`
    /// can never be returned. The third assertion then sees two reloads.
    #[test]
    fn silence_reloads_the_page_once_and_then_waits() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.calls(|c| assert!(c.live, "the ask built a page"));

        /* Inside the limit, a quiet room is just a quiet room. */
        rig.chat.step(t0 + Duration::from_millis(50));
        rig.calls(|c| assert_eq!(c.reloads, 0, "50ms of quiet is not a stall"));

        /* Past it, exactly one reload, and the reader is told. */
        rig.chat.step(t0 + Duration::from_millis(200));
        rig.calls(|c| assert_eq!(c.reloads, 1));
        rig.log(|l| match &l.state {
            YtConn::Stalled(w) => assert!(w.contains("reloaded"), "{w}"),
            other => panic!("a stall should be visible, got {other:?}"),
        });

        /* Still quiet, but the floor has not passed: waiting, not reloading again. */
        rig.chat.step(t0 + Duration::from_millis(400));
        rig.calls(|c| assert_eq!(c.reloads, 1, "the reload floor holds it off"));
        rig.log(|l| match &l.state {
            YtConn::Stalled(w) => assert!(w.contains("waiting"), "{w}"),
            other => panic!("still stalled, got {other:?}"),
        });

        /* Past the floor and still silent: a second reload is allowed. */
        rig.chat.step(t0 + Duration::from_millis(900));
        rig.calls(|c| assert_eq!(c.reloads, 2));
    }

    /// A PAYLOAD, EVEN ONE WITH NO MESSAGES IN IT, IS THE HEARTBEAT.
    ///
    /// THE DEFECT: timing the watchdog off the last MESSAGE. This room was measured at 3.1
    /// messages a minute with a 93 second gap between two appends on a demonstrably healthy feed,
    /// so that watchdog would reload a working page several times an hour and drop its backlog
    /// each time. The script's 20 second ping is what makes an empty payload meaningful.
    ///
    /// THE MUTATION THAT MAKES THIS RED: move the `it.last_post = Some(..)` line in `Sink::absorb`
    /// inside the loop over `batch.kept`, so an empty batch stops counting as life.
    #[test]
    fn an_empty_batch_still_counts_as_the_page_being_alive() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.chat.step(Instant::now());
        /* What a ping looks like once the model has had it: nothing kept, nothing refused. */
        rig.sink.absorb(Batch {
            kept: Vec::new(),
            refused: 0,
            last_refusal: None,
        });
        /* The clock is read AFTER the payload, because `absorb` stamps itself off the real one. */
        let spoke = Instant::now();
        rig.chat.step(spoke + Duration::from_millis(50));
        rig.calls(|c| assert_eq!(c.reloads, 0, "the page spoke, so it is not stalled"));
        rig.log(|l| assert_eq!(l.state, YtConn::Reading));
    }

    /// A NEW STREAM IS A NEW ROOM: THE OLD LINES AND THE OLD IDS BOTH GO.
    ///
    /// THE DEFECT: navigating and keeping the log. A finished stream's conversation would sit
    /// above the new one's with no seam a reader could see, and the surviving id set would
    /// suppress a genuinely new message that happened to reuse one.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `fresh` branch in `YtChat::tick`'s `Show` arm.
    #[test]
    fn a_new_video_id_clears_the_room() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "first");
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.sink.absorb(batch(&["a", "b"]));
        rig.log(|l| assert_eq!(l.lines.len(), 2));

        rig.chat.start(&ctx, "second");
        rig.chat.step(t0 + Duration::from_millis(10));
        rig.log(|l| {
            assert!(
                l.lines.is_empty(),
                "the old room's lines are not this room's"
            );
        });
        /* And an id from the old room is admitted again rather than swallowed as a repeat. */
        rig.sink.absorb(batch(&["a"]));
        rig.log(|l| assert_eq!(l.lines.len(), 1));
        rig.calls(|c| assert_eq!(c.shown.len(), 2, "navigated, and only twice"));
    }

    /// A PAGE ALREADY ON THE RIGHT STREAM IS LEFT ALONE.
    ///
    /// THE DEFECT: comparing the want against nothing, so every tick calls `show` again. Against
    /// the real pane that is a `load_url` per frame, which restarts the page sixty times a second
    /// and means no chat ever arrives at all.
    ///
    /// THE MUTATION THAT MAKES THIS RED: drop the `same_room` term from `plan`'s
    /// `!showing || !same_room` condition.
    #[test]
    fn a_page_already_on_the_right_stream_is_left_alone() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        let t0 = Instant::now();
        for i in 0..5 {
            rig.chat.step(t0 + Duration::from_millis(i * 10));
        }
        rig.calls(|c| assert_eq!(c.shown.len(), 1, "one build, then nothing"));
    }

    /// THE PAGE IS POINTED AT THIS STREAM'S LIVE CHAT.
    ///
    /// THE DEFECT: building the address here instead of asking `extract::live_chat_url`, which is
    /// how two spellings of one address get into a tree and one of them goes stale.
    ///
    /// THE MUTATION THAT MAKES THIS RED: pass a constant url to `Pane::show` instead of the one
    /// `plan` computed.
    #[test]
    fn the_page_is_pointed_at_this_streams_live_chat() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.chat.step(Instant::now());
        let want = live_chat_url("UdIx8u6qmKo");
        rig.calls(|c| {
            assert_eq!(c.shown.first().map(String::as_str), Some(want.as_str()));
        });
    }

    /// A BUILD THAT FAILED IS NOT RETRIED EVERY FRAME, AND `start` IS THE DOOR BACK IN.
    ///
    /// THE DEFECT: retrying a build on a timer. The ordinary cause of a failed build is a machine
    /// with no WebView2 runtime, and a frame-rate loop against that spends the reader's CPU
    /// forever to learn nothing. This is `player::keeps_syncing`'s argument (player.rs:610-623)
    /// held here by its own test.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the
    /// `if it.problem.is_some() { return Plan::Nothing; }` guard in `plan`.
    #[test]
    fn a_build_that_failed_is_not_retried_every_frame() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.calls(|c| c.refuse = Some(Problem::Failed("no runtime".to_owned())));
        let t0 = Instant::now();
        for i in 0..5 {
            rig.chat.step(t0 + Duration::from_millis(i * 10));
        }
        rig.log(|l| match &l.state {
            YtConn::Stalled(w) => assert_eq!(w, "no runtime"),
            other => panic!("the reason belongs on screen, got {other:?}"),
        });
        rig.calls(|c| {
            assert!(c.shown.is_empty(), "nothing was ever built");
            assert!(!c.live);
        });

        rig.calls(|c| c.refuse = None);
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.chat.step(t0 + Duration::from_millis(100));
        rig.calls(|c| assert!(c.live, "asking again tries once more"));
    }

    /// A REFUSAL IS NEVER RETRIED, NOT EVEN WHEN THE READER ASKS.
    ///
    /// THE DEFECT: treating "there is no WebView2 on this machine" as a transient failure. Every
    /// ask would rebuild an environment that cannot be built, and the sentence on screen would
    /// flicker, which teaches a reader the app is guessing. `after_retry` keeps a refusal
    /// (player.rs:641-659) and this is the test that says so through this surface.
    ///
    /// THE MUTATION THAT MAKES THIS RED: change `after_retry` in `YtChat::start` to
    /// `it.problem = None`.
    #[test]
    fn a_refusal_survives_the_reader_asking_again() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.calls(|c| c.refuse = Some(Problem::Refused("no webview on this platform".to_owned())));
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.calls(|c| c.refuse = None);
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        rig.chat.step(t0 + Duration::from_millis(10));
        rig.calls(|c| {
            assert!(
                !c.live,
                "a refusal is not something asking again can change"
            )
        });
    }

    /// STOPPING TEARS THE PAGE DOWN. No hidden browser is left reading somebody's bandwidth after
    /// they asked it to stop, which is `Player::stop`'s promise (player/surface.rs:144-149).
    ///
    /// THE MUTATION THAT MAKES THIS RED: return `Plan::Nothing` instead of `Plan::Close` from
    /// `plan`'s `want is None` arm.
    #[test]
    fn stopping_closes_the_page() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.chat.stop();
        rig.chat.step(t0 + Duration::from_millis(10));
        rig.calls(|c| {
            assert!(!c.live);
            assert_eq!(c.closes, 1);
        });
        rig.log(|l| assert_eq!(l.state, YtConn::Stopped));
        /* And it stays closed: a stopped surface does not rebuild itself on the next frame. */
        rig.chat.step(t0 + Duration::from_millis(20));
        rig.calls(|c| assert_eq!(c.closes, 1, "closed once, not once per frame"));
    }

    /// A SUSPENDED DOCUMENT IS NAMED, BECAUSE A RELOAD IS NOT A REMEDY FOR IT.
    ///
    /// THE DEFECT: reporting every stall the same way. "YouTube's feed stopped" and "this machine
    /// throttled the renderer" want different answers from whoever reads that line, and only the
    /// first is helped by the reload the watchdog just did. This is the readback posture
    /// `Player::poll_audio` takes about audio (player/surface.rs:560-593).
    ///
    /// THE MUTATION THAT MAKES THIS RED: drop the `suspended` argument from `stall_words` and
    /// always return the bare sentence.
    #[test]
    fn a_suspended_document_says_so_in_the_stall() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "UdIx8u6qmKo");
        let t0 = Instant::now();
        rig.chat.step(t0);
        rig.calls(|c| c.suspended = Some(true));
        rig.chat.step(t0 + Duration::from_millis(200));
        rig.log(|l| match &l.state {
            YtConn::Stalled(w) => assert!(w.contains("SUSPENDED"), "{w}"),
            other => panic!("expected a stall, got {other:?}"),
        });
    }

    /// AN EMPTY VIDEO ID OPENS NOTHING AND SAYS WHY.
    ///
    /// THE DEFECT: building `...&v=` and loading a document with no chat in it, which looks
    /// exactly like a room where nobody is talking. The id is absent for most of every day,
    /// because the channel is only live for part of it.
    ///
    /// THE MUTATION THAT MAKES THIS RED: delete the `want.is_empty()` branch in `YtChat::start`.
    #[test]
    fn an_empty_video_id_opens_nothing_and_says_why() {
        let mut rig = Rig::new(fast());
        let ctx = rig.ctx.clone();
        rig.chat.start(&ctx, "   ");
        rig.chat.step(Instant::now());
        rig.calls(|c| assert!(!c.live));
        rig.log(|l| match &l.state {
            YtConn::Stalled(w) => assert!(w.contains("video id"), "{w}"),
            other => panic!("expected words, got {other:?}"),
        });
    }
}
