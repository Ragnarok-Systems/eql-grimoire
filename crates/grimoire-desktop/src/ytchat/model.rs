//! What the YouTube bridge says, and how the two chats become one column.
//!
//! WHAT THIS FILE IS. Two halves and nothing else. The first turns the JSON string that
//! `ytchat::extract::EXTRACT_JS` posts over `window.ipc` into `YtMessage`s, counting every item it
//! cannot read instead of dropping it. The second, `interleave`, takes the Twitch log this crate
//! already keeps (`chat::Log::lines`) and the YouTube log the bridge keeps, and produces the single
//! ordered column the merged screen draws. There is no I/O here, no thread, no webview and no
//! clock beyond one `SystemTime::now()` per received batch, so every rule below is testable
//! without a network and every test in this file runs offline.
//!
//! `YtMessage` MIRRORS `chat::Event` DELIBERATELY, INCLUDING WHAT IT LEAVES OUT. `chat::Piece`
//! (chat.rs:208-217) is the OWNED mirror of the borrowed `irc::emotes::Span`, because an event
//! outlives the read buffer its spans pointed into. `YtPiece` is owned for the same reason and one
//! step further removed: the page's DOM node is gone by the time the IPC string reaches us, so
//! there is nothing left to borrow from at all. The shapes are kept the same on purpose, so a
//! screen that already knows how to lay out `Vec<chat::Piece>` needs one more arm and not a second
//! layout.
//!
//!
//! ==================== THE ORDERING DECISION, WHICH IS THE WHOLE FILE ====================
//!
//! Two live rooms, one column. Three candidate rules were on the table and the evidence rules two
//! of them out outright.
//!
//! (a) ORDER EVERYTHING BY WHEN IT REACHED THIS APP. Refused, and not on taste: for the YouTube
//!  half, arrival order is not even a well defined answer. The page hands us rows through a
//!  `MutationObserver`, and YouTube smooths its own appends: `isSmoothed_` is true, the poll
//!  cadence is a flat `timeoutMs: 10000`, and the observed lag from send to DOM append was 2.1
//!  to 5.8 seconds normally and 44 to 66 seconds after a scroll pause flushed its off DOM
//!  buffer. Worse than late, it is out of order WITHIN the platform: two nodes were appended in
//!  the same millisecond carrying stamps 20 seconds apart. Twitch, over a TLS socket, is behind
//!  by the round trip. So rule (a) does not merely add a delay to one feed, it applies a
//!  systematic 5 to 15 second bias in one direction, which is exactly long enough to draw a
//!  Twitch reply above the YouTube line it is replying to. The whole reason this merged feed
//!  exists is that the two rooms are different people (measured: no overlap at all between the
//!  five Twitch names and the five YouTube names sampled in the same window) talking about the
//!  same stream, so getting "who spoke first" backwards is not a cosmetic fault.
//!
//! (b) ORDER EVERYTHING BY THE PLATFORM'S OWN STAMP. Refused, and this file is not the first to
//!  refuse it. `chat::Event::sent_ms` (chat.rs:265-272) carries the argument already: in
//!  irc_busy.txt, #zackrawrr delivered 1788555473902 and then 1788555473897, five milliseconds
//!  backwards, in arrival order. Sorting a single platform's log by its own stamp reorders that
//!  pair against the order the room actually saw. The rule is stated there as law and this file
//!  obeys it rather than relitigating it.
//!
//! (c) WHAT THIS FILE DOES. The two refusals above are not the same refusal, and noticing that is
//!  the decision:
//!
//!  On Twitch, arrival order is the truth and the stamp is the liar.
//!  On YouTube, the stamp is the truth and arrival order is the liar.
//!
//!  So neither key is used for both jobs. THE RULE IS:
//!
//!  1. The Twitch half is walked exactly as it arrived and is never reordered by anything.
//!     That is chat.rs's law, obeyed by construction rather than by care.
//!  2. The YouTube half is ordered by `timestampUsec`, stably, so that rows the page handed
//!     over in the wrong order are put back into the order they were said in, and rows that
//!     share a stamp keep the order the DOM gave them.
//!  3. BETWEEN the two platforms, rows are compared on a single common clock: Unix
//!     milliseconds. Twitch supplies `tmi-sent-ts` directly. YouTube supplies `timestampUsec`
//!     divided by a thousand.
//!  4. A row that cannot be dated does not stop the other feed. See `twitch_at_ms` and
//!     `youtube_at_ms` for the two different fallbacks and why they differ.
//!
//!  THE ASYMMETRY IS THE CODE AND NOT JUST THE COMMENT. `interleave` walks the Twitch deque
//!  untouched and sorts the YouTube deque, and those two lines are the two sentences above.
//!
//!  THE ARGUMENT THAT MAKES THIS SOUND, IN ONE SENTENCE: five milliseconds of backwards jitter
//!  is fatal for ordering two messages from the same room five milliseconds apart, and utterly
//!  irrelevant for deciding whether a Twitch line came before a YouTube line eight seconds
//!  away. The stamp is precise enough for the cross platform question and not for the intra
//!  platform one, so it is asked only the question it can answer.
//!
//! WHAT THE RULE GETS WRONG, SAID PLAINLY AND NOT BURIED.
//!
//!   * A YouTube row does not always land at the bottom. It lands where its stamp puts it, which
//!     is above any Twitch row spoken after it, so the reader sees a row appear one or two lines up
//!     rather than at the very end. The depth is bounded by the cross platform delivery gap: at the
//!     measured combined rate (3.1 a minute on YouTube plus 3.68 a minute on Twitch, one message
//!     every 8.6 seconds) a typical 5 to 15 second gap is 0 to 2 rows, and the worst measured case,
//!     a 66 second scroll pause flush, is about 7. That is the price of rule (c) and it is paid
//!     knowingly. The thing that would be unreadable is a feed whose ALREADY DRAWN rows swap places
//!     under the reader's eyes, and that cannot happen here: see `interleave`'s note on why the
//!     output is insertion only, and the test that pins it.
//!   * Two messages a few hundred milliseconds apart on different platforms may come out in the
//!     wrong order, because Twitch's accept clock and YouTube's accept clock are independent and
//!     the skew between them has never been measured from this machine. This is not fixable from
//!     here and it does not matter: no reader can tell who spoke first across two rooms inside a
//!     second.
//!   * IF YOUTUBE'S STAMP DISAPPEARS, THE FEED DEGRADES SILENTLY. `timestampUsec` is reachable only
//!     through Polymer's private state (measured today at `el.inst.data.timestampUsec`; the
//!     `el.__data` path everybody assumes does not exist on build f82dea74), so a YouTube deploy
//!     can take it away from every row at once. When that happens `youtube_at_ms` falls back to
//!     when the batch arrived and the merged feed quietly becomes rule (a). Nothing counts that
//!     today. `YtMessage::ts_usec` is `None` on every affected row, so a screen CAN see it, but no
//!     tripwire in this file fires. That is a named gap, not an oversight.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chat;

/* ---------------------------------------------------------------- the shapes -- */

/// Which room a merged row came out of.
///
/// IT EXISTS FOR THE ROW CHROME AND NOTHING ELSE. The merged screen paints a badge and an accent
/// before it knows or cares which body shape it is about to lay out, and one `badge(ui, source)`
/// helper taking this is less code than threading the platform out of a match that has already
/// destructured the message. `Merged::source` is how a match arm that HAS destructured still names
/// it. If the screen ends up deciding the badge inside the same match it uses for the body, then
/// this enum and that method are both dead and they should be cut together, not kept because they
/// were in a contract.
/// `Default` IS TWITCH BECAUSE THAT IS THE HALF THAT CAN SEND. The Chat screen keeps one of
/// these for where the next message goes, and a screen built by `Screens::default()` must open on
/// a destination that works rather than on one that has to explain itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Twitch,
    YouTube,
}

/// What kind of thing a YouTube row is. Two arms, because the screen has two layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YtKind {
    /// `yt-live-chat-text-message-renderer`. Somebody talked. 90 of the 91 rows in the measured
    /// capture.
    Chat,
    /// Somebody paid: `yt-live-chat-paid-message-renderer` (a superchat) or
    /// `yt-live-chat-membership-item-renderer` (a membership).
    ///
    /// THE TWO ARE ONE ARM AND THAT IS A DECISION WITH A COST. They are not the same event: one is
    /// a one off payment with an amount attached and the other is a recurring membership. They are
    /// folded together because the alternative on offer was folding a membership into `Chat`, and
    /// that is the mistake `chat::Kind::Highlight` exists to name (chat.rs:176-180): the whole
    /// point of the event is that it looks different, and a client that draws it as an ordinary
    /// line has taken the money and given nothing back. Drawn with the paid emphasis a membership
    /// is slightly overstated; drawn as chat it is erased. If the screen ever needs to tell them
    /// apart, this enum grows a third arm and `parse_batch` stops mapping "member" here.
    Paid,
}

/// ONE RUN OF A YOUTUBE MESSAGE BODY, OWNED. The mirror of `chat::Piece` (chat.rs:208-217), one arm
/// for one arm, so a screen laying out a merged column has one shape in its head and not two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YtPiece {
    /// Text to draw as text. Never empty: `parse_batch` drops empty runs rather than emitting them.
    Text(String),
    /// An emote. `name` is the `alt` off the `<img>`, which for the unicode emotes measured today
    /// is the literal glyph ("bee", "money with wings") and for a channel emote would be the
    /// `:_name:` form.
    ///
    /// THE CDN URL IS DELIBERATELY NOT CARRIED, AND THAT IS THE ONE PLACE THIS FILE REFUSES TO COPY
    /// ITS MIRROR. `chat::Piece::Emote` carries an `id` that no production line of this crate ever
    /// reads: nothing here fetches from an emote CDN yet, screens/chat.rs draws
    /// `Piece::Emote { name, .. }`, and the field survives only because rustc's dead code pass is
    /// blind to fields behind the `PartialEq` derive (lib.rs, check 1's stated blind spot) and
    /// reach.rs only scans structs. Adding a `url` here would be the same unread field with the
    /// same two blind spots covering for it, in a file whose crate calls uncalled code its
    /// signature defect. THE COST IS REAL AND IS NOT WAVED AWAY: Twitch can rebuild an image URL
    /// from an id, YouTube cannot rebuild one from alt text, so the day something fetches emote
    /// images the extractor has to hand the URL over again. That is the order Cargo.toml's own
    /// notes demand of every entry in this crate, a thing arrives WITH the code that uses it and
    /// not before, and it is cheaper than shipping a field on the promise of a caller.
    Emote { name: String },
}

/// One row out of the YouTube bridge. Built once by `parse_batch` and then read only.
///
/// IT DERIVES `Clone` AND `Debug` AND NOTHING ELSE, FOR THE REASON WRITTEN AT chat.rs:236-242.
/// `PartialEq`, `Eq`, `Hash`, `PartialOrd` and `Ord` each expand to an impl that reads every field,
/// which blinds rustc's dead code pass to a field no screen ever draws; this crate lost fourteen
/// fields to exactly that. Keeping those derives off keeps the lint's eyes open. The tests below
/// compare field by field for the same reason.
#[derive(Clone, Debug)]
pub struct YtMessage {
    /// The renderer element's `.id`, an opaque string such as
    /// "ChwKGkNOcXoyZnVhMXBZREZZRVNkZ1lkVWdVTjF3". THE ONLY DEDUP KEY THERE IS, which is why a row
    /// without one is refused rather than kept: a reload replays 75 to 77 already seen rows and the
    /// Top chat to Live chat switch replays the whole list, so an undedupable row is a row that
    /// will be drawn again and again.
    pub id: String,
    /// `#author-name`, as YouTube renders it, "@KolbyBuchanan" and the at sign included. It is left
    /// exactly as the page had it: whether the merged column strips the at sign to sit level with
    /// Twitch's bare logins is a layout decision and not a parsing one.
    pub author: String,
    /// The `author-type` attribute. Observed values today are "", "member" and "owner";
    /// "moderator" is documented by YouTube and did not appear in the sampled window, so a screen
    /// must still handle it. Superchat renderers carry NO such attribute at all (measured `null`,
    /// not empty string), and the extractor substitutes "owner" there off `author-is-owner`.
    pub author_type: String,
    /// The whole message as text, emote names inlined.
    ///
    /// IT IS NOT `#message.textContent` AND MUST NOT BE. The measured defect: `textContent` drops
    /// the emote `<img>` entirely and leaves the gap behind, so "put everything in Bee (bee) tier"
    /// comes back as "put everything in Bee  tier" with a double space and a missing word. The
    /// extractor rebuilds it from the alt text, and when a batch arrives without a `body` key at
    /// all `parse_batch` rebuilds it from `pieces` the same way.
    pub body: String,
    pub kind: YtKind,
    /// `timestampUsec`, YouTube's own microsecond clock, when the page would give it up.
    ///
    /// OPTIONAL AND NEVER LOAD BEARING, which is a measured requirement and not caution. It lives
    /// in Polymer's private state, at `el.inst.data.timestampUsec` on build f82dea74; the
    /// `el.__data.data.timestampUsec` path that everyone reaches for first does not exist there at
    /// all (`'__data' in el` is false), so a reader written against only that path gets `None` for
    /// 100% of rows today. It was present on 91 of 91 rows in the measured capture through the
    /// paths the extractor actually tries, and it can vanish on any YouTube deploy. See
    /// `youtube_at_ms` for what happens then, and the module note for the tripwire this file does
    /// not have.
    pub ts_usec: Option<i64>,
    /// WHAT SOMEBODY PAID, EXACTLY AS YOUTUBE PRINTED IT, or `None` for a message that cost
    /// nothing.
    ///
    /// IT ARRIVES WITH THE CODE THAT DRAWS IT, which is why this field did not exist until now.
    /// `extract.rs` has always posted an `amount` and nothing read it, so a superchat reached the
    /// screen looking like an ordinary message with a gold tint and the money missing. The
    /// workflow's verifier found that and deliberately did NOT add the field, because a field with
    /// no reader is the defect this crate keeps catching; `screens::chat::yt_row` draws it now.
    ///
    /// A STRING AND NEVER A NUMBER, AND THAT IS NOT LAZINESS. The measured capture carried
    /// `"\u{a5}2,000"`. Parsing that means knowing the currency symbol, the grouping separator and
    /// the decimal separator for every locale YouTube renders in, getting one wrong turns 2,000 yen
    /// into 2 yen, and there is nothing this app does with the NUMBER anyway: it shows what was
    /// paid, it does not add it up. YouTube has already formatted it for the viewer who is reading
    /// it, so the honest thing is to pass their string through untouched.
    pub amount: Option<String>,
    /// `body` cut into text and emote runs. Empty when the page gave no `pieces` array, in which
    /// case a screen draws `body`, exactly as screens/chat.rs:347 already does when
    /// `chat::Event::spans` is empty.
    pub pieces: Vec<YtPiece>,
    /// UNIX MILLISECONDS AT THE MOMENT THIS ROW'S BATCH CROSSED THE IPC BOUNDARY. Set once by
    /// `parse_batch` and never touched again.
    ///
    /// THIS IS THE FIELD THE INTERFACE SKETCH CALLED `at`, AND IT IS NOT AN `Instant`, WHICH IS THE
    /// ONE DEVIATION IN THIS FILE WORTH ARGUING. Its entire job is to be compared with a Twitch
    /// row's `chat::Event::sent_ms`, which is Unix milliseconds off Twitch's own clock.
    /// `std::time::Instant` is an opaque monotonic reading with no defined relationship to the
    /// epoch and no way to be compared with an `i64` of epoch milliseconds, so an `Instant` here
    /// would be a field that cannot do the only thing it is for. There is no third option in which
    /// both sides carry an `Instant`: `chat::Event` records no arrival time, this lane may not edit
    /// chat.rs, and inventing one at merge time would stamp every row with the instant of the
    /// current frame, which is the same value for all of them and a different value next frame.
    ///
    /// THE COST OF USING THE WALL CLOCK, NAMED: a system clock step (an NTP correction, a laptop
    /// waking up) moves the fallback key of whatever batch straddles it. It cannot reorder anything
    /// within either feed, because a merge never reorders within a side, and it does not touch the
    /// stamped path at all. That is a small, bounded, one time wobble in exchange for the only
    /// clock the two platforms both speak.
    pub seen_ms: i64,
}

/// What one IPC payload turned into. `refused` and `last_refusal` are the tripwire.
///
/// COUNTED, NEVER SILENTLY DROPPED, and the sentence is copied from chat.rs:395-403 because the
/// failure is identical: a parser that quietly discards what it does not understand looks perfect
/// right up to the release where the shape changes and a tenth of the chat vanishes with nothing on
/// screen and nothing in the log. YouTube's chat is a Polymer DOM this app reads over the shoulder
/// of a page it does not control, so "the shape changed" is not a hypothetical failure mode here,
/// it is the expected one.
#[derive(Clone, Debug)]
pub struct Batch {
    pub kept: Vec<YtMessage>,
    pub refused: u32,
    pub last_refusal: Option<String>,
}

impl Batch {
    fn empty() -> Batch {
        Batch {
            kept: Vec::new(),
            refused: 0,
            last_refusal: None,
        }
    }

    /// Count one thing this file could not read, and keep the last reason in words.
    fn refuse(&mut self, why: String) {
        self.refused = self.refused.saturating_add(1);
        self.last_refusal = Some(why);
    }
}

/// One row of the merged column, borrowed from whichever log it came out of.
///
/// IT BORROWS AND DOES NOT OWN, WHICH IS THE POINT. chat.rs's module note (chat.rs:21-26) records
/// why `Watcher::status`'s clone the whole state every frame posture was NOT copied to the chat
/// log: cloning two thousand `String`s and their span vectors at 60 Hz costs more than drawing
/// them. A merged column is that same log plus a second one, so `interleave` is called inside both
/// `with_log` closures and hands back references into them. Two pointers per row, no allocation
/// beyond the `Vec` itself, and the borrow checker is what stops a row outliving the lock.
///
/// LOCK ORDER, SINCE THERE ARE NOW TWO. The caller nests `ChatReader::with_log` outside
/// `YtChat::with_log`. Nothing in either module takes the other's lock, so the order cannot
/// deadlock today, and writing it down is what keeps that true when a third reader appears.
#[derive(Clone, Copy, Debug)]
pub enum Merged<'a> {
    Twitch(&'a chat::Event),
    YouTube(&'a YtMessage),
}

impl Merged<'_> {
    /// Which room this row came out of. See `Source` for why this is worth a method.
    pub fn source(&self) -> Source {
        match self {
            Merged::Twitch(_) => Source::Twitch,
            Merged::YouTube(_) => Source::YouTube,
        }
    }
}

/* --------------------------------------------------------------- the parsing -- */

/// The first 120 characters of something that went wrong.
///
/// It is the same six lines as chat.rs:1175, on purpose and not by accident: a refusal from this
/// module and a refusal from the IRC reader end up in the same kind of line on the same screen, and
/// they should clip the same way. Making chat.rs's copy `pub` for one caller would widen that
/// module's API to save six lines here.
fn clip(s: &str) -> String {
    let mut out: String = s.chars().take(120).collect();
    if out.len() < s.len() {
        out.push_str("...");
    }
    out
}

/// Unix milliseconds now. One call per batch, shared by every row in it, because they genuinely did
/// all cross the boundary together.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Turn one `window.ipc.postMessage` payload into rows, counting what it cannot read.
///
/// THE PAYLOAD IS ALWAYS A JSON ARRAY OF ITEMS, because the page coalesces at 250 ms and posts the
/// whole burst as one string. That is not an optimisation this file can opt out of: wry's IPC shim
/// only carries strings (`TryGetWebMessageAsString`, and a non string makes the whole COM handler
/// return early with nothing logged), and a scroll pause flush released six rows in one callback in
/// the measured run.
///
/// WHAT IS REFUSED, AND THE ONE THING THAT IS NOT. A payload that is not JSON, a payload that is not
/// an array, an item that is not an object, an item whose `kind` is missing or unknown, an item
/// with no `id`, and a `pieces` entry whose `t` is neither "text" nor "emote" are all counted.
/// `kind:"ping"` is NOT: it is the extractor's own liveness heartbeat carrying the succeeded poll
/// count, it is understood perfectly well, and counting a thing this file understands would poison
/// the tripwire so that a real shape change later goes unnoticed in the noise. That is the mistake
/// chat.rs's `other_viewers_joining_are_neither_logged_nor_counted_as_refusals` test exists to stop.
///
/// THE PING'S `polls` VALUE IS DROPPED HERE AND THAT IS A KNOWN GAP. It is the only honest liveness
/// signal the bridge has, because message silence is not a stall (a 93 second gap was measured on a
/// healthy, actively polling feed), and a reload watchdog wants it. `Batch` has nowhere to put it
/// and adding a field the surface does not read would be the exact defect this crate keeps
/// catching. When the watchdog is built, `Batch` grows the field WITH the code that reads it.
pub fn parse_batch(json: &str) -> Batch {
    let mut out = Batch::empty();

    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            out.refuse(format!("a batch that is not JSON: {e}: {}", clip(json)));
            return out;
        }
    };
    let items = match v.as_array() {
        Some(a) => a,
        None => {
            out.refuse(format!("a batch that is not an array: {}", clip(json)));
            return out;
        }
    };

    /* ONE CLOCK READING FOR THE WHOLE BATCH. Every row in it arrived in the same IPC callback, so
     * giving them different arrival stamps would invent a resolution the boundary does not have,
     * and it would make `interleave`'s fallback order depend on how long this loop took. */
    let seen_ms = now_ms();

    for it in items {
        let Some(obj) = it.as_object() else {
            out.refuse(format!(
                "an item that is not an object: {}",
                clip(&it.to_string())
            ));
            continue;
        };
        let raw_kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let kind = match raw_kind {
            "text" => YtKind::Chat,
            /* A superchat and a membership are both "somebody paid", see `YtKind::Paid`. */
            "paid" | "member" => YtKind::Paid,
            /* The heartbeat. Understood, nothing to keep, and NOT a refusal. */
            "ping" => continue,
            "" => {
                out.refuse(format!("an item with no kind: {}", clip(&it.to_string())));
                continue;
            }
            other => {
                out.refuse(format!(
                    "an item of a kind this reader does not know, {other:?}"
                ));
                continue;
            }
        };

        let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").trim();
        if id.is_empty() {
            /* Without the renderer's own id there is no dedup key, and a row that cannot be
             * deduped is a row that gets drawn again on every reload and on every Top chat to Live
             * chat switch. Refusing it is louder than drawing it forever. */
            out.refuse(format!(
                "a {raw_kind} item with no id, which is the only dedup key there is: {}",
                clip(&it.to_string())
            ));
            continue;
        }

        let mut pieces: Vec<YtPiece> = Vec::new();
        if let Some(arr) = obj.get("pieces").and_then(|v| v.as_array()) {
            for p in arr {
                match p.get("t").and_then(|v| v.as_str()) {
                    Some("text") => {
                        let t = p.get("v").and_then(|v| v.as_str()).unwrap_or("");
                        if !t.is_empty() {
                            pieces.push(YtPiece::Text(t.to_owned()));
                        }
                    }
                    Some("emote") => {
                        let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if !n.is_empty() {
                            pieces.push(YtPiece::Emote { name: n.to_owned() });
                        }
                    }
                    other => {
                        /* COUNTED, BUT THE MESSAGE IS STILL KEPT. A run this file cannot cut is a
                         * degraded LAYOUT, not a lost message: `body` was built by the page from
                         * every child node including this one, and a screen draws `body` when
                         * `pieces` is short of it. Refusing the whole row over one unknown run
                         * would lose a message that is entirely readable, and saying nothing would
                         * hide the shape change. Both halves matter. */
                        out.refuse(format!(
                            "a message run of an unknown type, {other:?}, in item {id}"
                        ));
                    }
                }
            }
        }

        /* AN ABSENT `body` KEY AND AN EMPTY ONE ARE DIFFERENT THINGS. Empty is a real answer: the
         * page builds `body` itself and a message can genuinely have no text. Absent means the
         * extractor did not send the field, and then the only text there is lives in `pieces`, so
         * it is rebuilt the same way the page would have. Folding the two would turn every
         * emote-only row into a blank line the day the extractor stops sending `body`. */
        let body = match obj.get("body").and_then(|v| v.as_str()) {
            Some(s) => s.to_owned(),
            None => pieces
                .iter()
                .map(|p| match p {
                    YtPiece::Text(t) => t.as_str(),
                    YtPiece::Emote { name } => name.as_str(),
                })
                .collect(),
        };

        /* A NUMBER FROM JAVASCRIPT ARRIVES AS EITHER, so both are tried. `1788572284975162` is
         * under 2^53 and survives an f64 round trip exactly, which is why the float arm is a
         * fallback and not a corruption. A zero or negative stamp is not a date, it is the page
         * having nothing to say, and it is folded into the same `None` as an absent key so that
         * `youtube_at_ms` has one case to handle and not two. */
        let ts_usec = obj
            .get("ts")
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
            .filter(|u| *u > 0);

        out.kept.push(YtMessage {
            id: id.to_owned(),
            /* EMPTY IS `None`, NOT `Some("")`. The script sends `null` for an unpaid row, but a
             * page that ever renders an empty `#purchase-amount` would otherwise put a blank
             * badge on screen beside a message nobody paid for. */
            amount: obj
                .get("amount")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            author: obj
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_owned(),
            author_type: obj
                .get("authorType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            body,
            kind,
            ts_usec,
            pieces,
            seen_ms,
        });
    }

    out
}

/* ------------------------------------------------------------- the interlace -- */

/// When a Twitch row happened, in Unix milliseconds, for the cross platform comparison ONLY.
///
/// `sent_ms` is Twitch's own accept clock and it is present on every PRIVMSG, USERNOTICE and
/// CLEARCHAT in both captures. It is NOT used to order Twitch against Twitch: chat.rs:265-272
/// records the pair that goes five milliseconds backwards, and `interleave` walks the Twitch deque
/// without sorting it at all, so the law holds by construction rather than by care. The YouTube
/// side IS sorted, and the reason the two are treated in opposite ways is the whole module note.
///
/// AN UNDATABLE ROW KEYS TO THE EPOCH, WHICH IS "AS EARLY AS POSSIBLE", AND THAT IS THE SAFE END.
/// The rows this happens to are the ones chat.rs never stamps, chiefly `Kind::Server`, a NOTICE from
/// tmi.twitch.tv saying the channel is suspended or the capability request was rejected. Keying it
/// early cannot move it ahead of its own predecessors, because the merge emits the Twitch deque in
/// its own order no matter what the keys say; the ONLY thing this number decides for such a row is
/// whether the other feed is allowed past it. Answering "we do not know when this was, so do not
/// hold YouTube behind it" is the answer that cannot strand a live feed. The opposite choice, dating
/// an undatable row as late as possible, freezes every YouTube row behind one unstamped NOTICE until
/// the next Twitch line arrives, which in a room at 3.68 messages a minute is a visible stall with
/// no cause on screen.
fn twitch_at_ms(e: &chat::Event) -> i64 {
    e.sent_ms.unwrap_or(0)
}

/// When a YouTube row happened, in Unix milliseconds.
///
/// THE STAMP FIRST, ARRIVAL ONLY AS THE FALLBACK, AND THE FALLBACK IS THE INTERESTING HALF.
/// `ts_usec` is what makes rule (c) work, and it can be taken away by a YouTube deploy from every
/// row at once, because it is read out of Polymer's private state. When that happens this function
/// returns `seen_ms` instead and the merged feed degrades, all at once and for the whole platform,
/// to rule (a), ordering YouTube by when it reached this app. That is the right degradation: it is
/// the honest second best answer, it is stable frame to frame because `seen_ms` is frozen into the
/// row when it arrives rather than recomputed, and it never strands the YouTube half at the top or
/// the bottom of the column the way a sentinel date would.
///
/// DIVIDING MICROSECONDS BY A THOUSAND LOSES NOTHING THAT WAS EVER REAL. The two platforms' clocks
/// are independent and their skew has never been measured from this machine, so sub millisecond
/// precision on one side of the comparison is precision about nothing.
fn youtube_at_ms(m: &YtMessage) -> i64 {
    match m.ts_usec {
        Some(u) => u / 1000,
        None => m.seen_ms,
    }
}

/// Merge the two logs into the one column the screen draws. Oldest first.
///
/// THE TWITCH SIDE IS WALKED AND THE YOUTUBE SIDE IS SORTED, AND THAT ONE LINE OF DIFFERENCE IS
/// THE WHOLE ORDERING RULE.
///
/// Twitch is walked in the deque's own order, which is arrival order, which chat.rs:265-272 proves
/// is the order the room saw: 1788555473902 arrived before 1788555473897 and the stamp is a liar
/// about which came first. Nothing in this function can move a Twitch row past another Twitch row,
/// so that law holds by construction and not by care. A `sort_by_key` over the concatenation of the
/// two feeds, stable or not, would break exactly that pair, which is why one is not used for the
/// merge.
///
/// YouTube is the mirror image and gets the opposite treatment. Its deque holds DOM append order,
/// and DOM append order is measurably NOT send order: the page smooths its own appends, and after a
/// scroll pause released its off DOM buffer two rows were appended in the SAME millisecond carrying
/// stamps 20 seconds apart. Walking that order would draw those two backwards. So the YouTube side
/// is stably sorted by `youtube_at_ms` before the merge: stable, so that rows which genuinely share
/// a key, every row of one batch once the stamp is lost, keep the order the DOM gave them, which is
/// the only evidence left at that point.
///
/// THE OUTPUT IS INSERTION ONLY: APPENDING TO EITHER FEED NEVER TRANSPOSES TWO ROWS ALREADY DRAWN.
/// This is the property that decides whether a merged feed is readable, and it survives the sort,
/// which is worth proving rather than hoping. The relative order of any two rows in the output is
/// fixed by one of three rules, and every one of them reads only those two rows: two Twitch rows go
/// by their positions in a deque that only grows at the end; two YouTube rows go by their keys, and
/// by their deque positions when the keys are equal; a Twitch row and a YouTube row go by their two
/// keys, with the tie to Twitch. No rule consults a row that has not arrived yet, so a new row can
/// be INSERTED anywhere in the column but cannot make two rows already in it swap. The reader sees
/// a late line appear a row or two above the bottom; the reader never sees the column shuffle
/// itself. The test named for this goes red if a future edit makes any key depend on another row,
/// for instance by clamping both feeds to one shared running maximum.
///
/// THERE IS NO MONOTONE CLAMP ON THE TWITCH KEY, DELIBERATELY. The obvious guard, forcing that
/// side's keys to be non decreasing so a backwards stamp cannot pull a row forward, was written and
/// then cut, because no input could be found that it changes: the merge already refuses to move a
/// Twitch row ahead of its own predecessors, so raising a backwards key to its predecessor's value
/// alters no comparison the merge actually makes. The house rule is that every guard must be shown
/// to fail against a deliberate defect, and this one could not be, so it is not shipped.
///
/// A TIE BETWEEN THE PLATFORMS GOES TO TWITCH. It is arbitrary and it is fixed, and fixed is the
/// part that matters: the merged column is recomputed from scratch on every frame, so a tie broken
/// any way that is not a pure function of the inputs would make two rows flicker past each other at
/// the frame rate.
pub fn interleave<'a>(
    twitch: &'a VecDeque<chat::Event>,
    youtube: &'a VecDeque<YtMessage>,
) -> Vec<Merged<'a>> {
    /* THE SORT IS OVER INDICES AND NOT OVER THE ROWS. `youtube` is borrowed, the caller holds it
     * under a lock, and a merged column is drawn every frame; copying up to a log's worth of
     * `String`s to sort them is the cost chat.rs:21-26 refused for exactly this reason. A `usize`
     * per row is eight bytes, and the list is already nearly sorted every frame because the page
     * hands rows over roughly in order, which is the case a merge sort is fastest on. */
    let mut order: Vec<usize> = (0..youtube.len()).collect();
    /* `sort_by_key` IS THE STABLE SORT, and the stability is load bearing, not incidental. When
     * `timestampUsec` goes away every row of a batch shares one `seen_ms`, and stability is what
     * keeps that batch in DOM order instead of an arbitrary one. `sort_unstable_by_key` here would
     * be a silent and unreproducible reordering on precisely the day the bridge is already
     * degraded. */
    order.sort_by_key(|&j| youtube_at_ms(&youtube[j]));

    let mut out: Vec<Merged<'a>> = Vec::with_capacity(twitch.len() + youtube.len());
    let mut i = 0usize;
    let mut j = 0usize;
    loop {
        let head_y = order.get(j).map(|&at| &youtube[at]);
        let take_twitch = match (twitch.get(i), head_y) {
            (Some(t), Some(y)) => twitch_at_ms(t) <= youtube_at_ms(y),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_twitch {
            out.push(Merged::Twitch(&twitch[i]));
            i += 1;
        } else {
            out.push(Merged::YouTube(&youtube[order[j]]));
            j += 1;
        }
    }
    out
}

/* ------------------------------------------------------------------ the tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /* THE FIXTURES ARE THE MEASURED SHAPES. The ids, the author names, the emote alt text, the
     * "put everything in Bee (bee) tier" body with its double space defect and the "2000 yen"
     * superchat amount are all from the run of the extraction script against
     * youtube.com/live_chat?is_popout=1&v=UdIx8u6qmKo on 2026-09-04, which posted 14 batches and
     * 91 messages with 0 duplicate ids. The five millisecond backwards Twitch pair is from
     * irc_busy.txt by way of chat.rs:265-272. Nothing here is a shape invented by the person
     * writing the test, which is where a parser goes to pass while production fails. */

    /// The emote bearing message exactly as the extractor posted it, including the CDN url this
    /// file deliberately drops.
    const EMOTE_ITEM: &str = r#"{"kind":"text","id":"ChwKGkNOcXoyZnVhMXBZREZZRVNkZ1lkVWdVTjF3","author":"@cardigansrule","authorType":"","body":"put everything in Bee 🐝 tier","ts":1788572284975162,"pieces":[{"t":"text","v":"put everything in Bee "},{"t":"emote","name":"🐝","url":"https://fonts.gstatic.com/s/e/notoemoji/15.1/1f41d/72.png"},{"t":"text","v":" tier"}],"amount":null}"#;

    const PLAIN_ITEM: &str = r#"{"kind":"text","id":"ChwKGkNPX0hxNGFiMXBZREZjWVNkZ1lkTmNJRlVR","author":"@kkixx_stixx","authorType":"member","body":"hello from youtube","ts":1788572304975162,"pieces":[{"t":"text","v":"hello from youtube"}],"amount":null}"#;

    const PAID_ITEM: &str = r#"{"kind":"paid","id":"ChwKGkNQTEsxNGFiMXBZREZTOFNkZ1lkTFo0RmFn","author":"@Vad_eem","authorType":"","body":"thanks for the stream","ts":1788572314975162,"pieces":[{"t":"text","v":"thanks for the stream"}],"amount":"¥2,000"}"#;

    /// The heartbeat the extractor emits every 20 seconds so a watchdog can tell a quiet room from
    /// a stalled continuation.
    const PING_ITEM: &str = r#"{"kind":"ping","id":"ping:1788572604823","author":"","authorType":"","body":"","ts":1788572604823000,"pieces":[],"amount":null,"polls":12}"#;

    fn tw(body: &str, sent_ms: Option<i64>) -> chat::Event {
        chat::Event {
            kind: chat::Kind::Chat,
            who: "someone".to_owned(),
            color: None,
            body: body.to_owned(),
            spans: Vec::new(),
            notice: None,
            sent_ms,
            badges: String::new(),
            secs: None,
        }
    }

    fn yt(id: &str, ts_usec: Option<i64>, seen_ms: i64) -> YtMessage {
        YtMessage {
            /* Test fixtures build unpaid rows; the paid path has its own test. */
            amount: None,
            id: id.to_owned(),
            author: "@viewer".to_owned(),
            author_type: String::new(),
            body: id.to_owned(),
            kind: YtKind::Chat,
            ts_usec,
            pieces: Vec::new(),
            seen_ms,
        }
    }

    /// A fingerprint that names a row without depending on its position, so two runs of
    /// `interleave` over different inputs can be compared for relative order.
    fn marks(rows: &[Merged<'_>]) -> Vec<String> {
        rows.iter()
            .map(|m| match m {
                Merged::Twitch(e) => format!("t:{}", e.body),
                Merged::YouTube(y) => format!("y:{}", y.id),
            })
            .collect()
    }

    /// Is `small` a subsequence of `big`, in order? The exact statement of "nothing was
    /// transposed, things were only inserted".
    fn is_subsequence(small: &[String], big: &[String]) -> bool {
        let mut it = big.iter();
        small.iter().all(|s| it.any(|b| b == s))
    }

    /* ----------------------------------------------------------- parse_batch -- */

    /// A REAL BATCH KEEPS WHAT IT UNDERSTANDS AND COUNTS WHAT IT DOES NOT.
    ///
    /// THE DEFECT: a `continue` where the `refused` increment should be. The parser then looks
    /// perfect, because the three good rows still arrive, and the day YouTube renames `#author-name`
    /// or ships a renderer this file has never heard of, a tenth of the chat goes missing with
    /// nothing on screen and nothing in the log to say so. This is the same tripwire chat.rs keeps
    /// for the IRC side and it is kept for the same reason.
    ///
    /// THE MUTATION THAT MAKES IT RED: change either refusal arm in `parse_batch` (the unknown
    /// `kind`, or the empty `id`) from `out.refuse(..)` to a bare `continue`.
    #[test]
    fn a_batch_keeps_what_it_understands_and_counts_what_it_cannot_read() {
        let no_id = r#"{"kind":"text","id":"","author":"@ghost","body":"no id","ts":1788572324975162,"pieces":[]}"#;
        let tombstone =
            r#"{"kind":"yt-live-chat-tombstone","id":"abc","body":"a shape from the future"}"#;
        let json = format!(
            "[{},{},{},{},{}]",
            EMOTE_ITEM, PLAIN_ITEM, PAID_ITEM, no_id, tombstone
        );
        let b = parse_batch(&json);
        assert_eq!(b.kept.len(), 3, "the three readable rows are kept");
        assert_eq!(
            b.refused, 2,
            "the row with no id and the unknown renderer are each counted"
        );
        let why = b.last_refusal.unwrap_or_default();
        assert!(
            why.contains("tombstone"),
            "the last refusal says what it could not read, was {why:?}"
        );
        /* THE TRIVIAL PASS THIS STOPS: a parser that refused everything would satisfy the count
         * above by accident, so the kept rows are checked for content and not just length. */
        assert_eq!(b.kept[1].author, "@kkixx_stixx");
        assert_eq!(b.kept[1].author_type, "member");
        assert_eq!(b.kept[1].body, "hello from youtube");
    }

    /// AN EMOTE IS A RUN OF ITS OWN AND THE BODY IS NOT `textContent`.
    ///
    /// THE DEFECT: reading the body off `#message.textContent`. That silently drops the emote
    /// `<img>` and leaves the space it sat in, so the measured line comes back as
    /// "put everything in Bee  tier", one word short and with a double space, and every emote in
    /// every message in the app disappears. The extractor rebuilds the body from the alt text and
    /// this test pins that the rebuilt body and the cut runs agree.
    ///
    /// THE MUTATION THAT MAKES IT RED: drop the `Some("emote")` arm in `parse_batch`'s piece loop,
    /// or make it push a `YtPiece::Text`.
    #[test]
    fn an_emote_is_its_own_run_and_the_body_still_contains_it() {
        let b = parse_batch(&format!("[{}]", EMOTE_ITEM));
        assert_eq!(b.refused, 0, "a measured row is not a refusal");
        let m = &b.kept[0];
        assert_eq!(m.body, "put everything in Bee 🐝 tier");
        assert_eq!(
            m.pieces,
            vec![
                YtPiece::Text("put everything in Bee ".to_owned()),
                YtPiece::Emote {
                    name: "🐝".to_owned()
                },
                YtPiece::Text(" tier".to_owned()),
            ],
            "three runs, and the middle one is not text"
        );
        assert_eq!(m.ts_usec, Some(1788572284975162));
    }

    /// WITH NO `body` KEY THE BODY IS REBUILT FROM THE RUNS, EMOTE NAMES INCLUDED.
    ///
    /// THE DEFECT: defaulting an absent `body` to the empty string. The extractor sends `body`
    /// today, so nothing would look wrong until the day it stops or a future extractor sends runs
    /// only, and then every row in the merged column is a blank line under a name. Rebuilding from
    /// the runs is the same reconstruction the page does and it is one line.
    ///
    /// THE MUTATION THAT MAKES IT RED: replace the `None =>` arm of the `body` match with
    /// `String::new()`.
    #[test]
    fn a_batch_with_no_body_key_rebuilds_the_body_from_its_runs() {
        let json = r#"[{"kind":"text","id":"xyz","author":"@a","pieces":[{"t":"text","v":"gg "},{"t":"emote","name":"🐝"},{"t":"text","v":" wp"}]}]"#;
        let b = parse_batch(json);
        assert_eq!(b.refused, 0);
        assert_eq!(b.kept[0].body, "gg 🐝 wp");
    }

    /// A MISSING STAMP IS KEPT AND IS NOT A REFUSAL.
    ///
    /// THE DEFECT: refusing a row with no `ts`. `timestampUsec` is read out of Polymer's private
    /// state and can go away on any YouTube deploy, so refusing on its absence would refuse the
    /// entire YouTube feed at once on a day when every message is perfectly readable. The module
    /// note states it as a rule: the microsecond stamp is optional and never load bearing.
    ///
    /// THE MUTATION THAT MAKES IT RED: add a `ts_usec.is_none()` branch to `parse_batch` that calls
    /// `out.refuse` and `continue`s.
    #[test]
    fn a_message_with_no_stamp_is_kept_and_is_not_counted_as_a_refusal() {
        let json = r#"[{"kind":"text","id":"nots","author":"@a","body":"hi","pieces":[{"t":"text","v":"hi"}]},
                       {"kind":"text","id":"zero","author":"@b","body":"yo","ts":0,"pieces":[]}]"#;
        let b = parse_batch(json);
        assert_eq!(b.refused, 0, "an undated message is still a message");
        assert_eq!(b.kept.len(), 2);
        assert_eq!(b.kept[0].ts_usec, None, "absent is None");
        assert_eq!(
            b.kept[1].ts_usec, None,
            "a zero stamp is the page having nothing to say, folded into the same None"
        );
        /* Both rows must still be datable for the merge, which is what `seen_ms` is for. */
        let now = now_ms();
        assert!(
            (b.kept[0].seen_ms - now).abs() < 60_000,
            "the arrival stamp is filled in even when the platform's is not"
        );
        assert_eq!(
            b.kept[0].seen_ms, b.kept[1].seen_ms,
            "one clock reading for the whole batch, because they arrived together"
        );
    }

    /// THE WATCHDOG PING IS SKIPPED WITHOUT POISONING THE REFUSAL COUNT.
    ///
    /// THE DEFECT: two opposite mistakes, exactly as chat.rs found with other viewers' JOINs.
    /// Counting a heartbeat this file understands perfectly well poisons the tripwire, so that a
    /// real shape change later drowns in a refusal every twenty seconds. Keeping it puts an empty
    /// row from nobody in the middle of the conversation every twenty seconds.
    ///
    /// THE MUTATION THAT MAKES IT RED: remove the `"ping" => continue` arm so the ping falls
    /// through to the unknown kind refusal, or map it to `YtKind::Chat`.
    #[test]
    fn the_liveness_ping_is_neither_drawn_nor_counted_as_a_refusal() {
        let b = parse_batch(&format!("[{},{}]", PING_ITEM, PLAIN_ITEM));
        assert_eq!(b.refused, 0, "a heartbeat is understood, not refused");
        assert_eq!(b.kept.len(), 1, "and it is not a row anybody reads");
        assert_eq!(b.kept[0].id, "ChwKGkNPX0hxNGFiMXBZREZjWVNkZ1lkTmNJRlVR");
    }

    /// A SUPERCHAT AND A MEMBERSHIP ARE NOT ORDINARY LINES.
    ///
    /// THE DEFECT: mapping every renderer to `YtKind::Chat`. The reader paid for the row
    /// specifically so it would look different, and a client that draws it as chat has taken the
    /// money and given nothing back, which is the argument `chat::Kind::Highlight` already carries
    /// at chat.rs:176-180.
    ///
    /// THE MUTATION THAT MAKES IT RED: change the `"paid" | "member"` arm to yield `YtKind::Chat`.
    #[test]
    fn a_superchat_and_a_membership_are_a_kind_of_their_own() {
        let member = r#"{"kind":"member","id":"mem1","author":"@c","body":"welcome","ts":1788572334975162,"pieces":[]}"#;
        let json = format!("[{},{},{}]", PAID_ITEM, member, PLAIN_ITEM);
        let b = parse_batch(&json);
        assert_eq!(b.refused, 0);
        assert_eq!(b.kept[0].kind, YtKind::Paid);
        assert_eq!(b.kept[1].kind, YtKind::Paid);
        /* THE TRIVIAL PASS THIS STOPS: a parser that marked everything paid would satisfy the two
         * lines above, so an ordinary message from the same batch is checked. */
        assert_eq!(b.kept[2].kind, YtKind::Chat, "an ordinary message is chat");
    }

    /// A PAYLOAD THAT IS NOT A JSON ARRAY IS ONE COUNTED REFUSAL AND NOT A PANIC.
    ///
    /// THE DEFECT: `serde_json::from_str(..).unwrap()` or `.unwrap_or_default()`. This function runs
    /// inside the wry IPC closure, which runs on the UI thread, so a panic here takes the whole app
    /// down, and a silent default turns "the bridge is posting garbage" into "chat is quiet". Every
    /// script on the YouTube page can reach `window.ipc`, so a payload that is not ours is a thing
    /// that will actually happen.
    ///
    /// THE MUTATION THAT MAKES IT RED: replace either early return in `parse_batch` with
    /// `return Batch::empty()`.
    #[test]
    fn a_payload_that_is_not_a_batch_is_counted_rather_than_swallowed() {
        for bad in ["not json at all", "{\"k\":\"rows\"}", ""] {
            let b = parse_batch(bad);
            assert!(b.kept.is_empty(), "nothing readable in {bad:?}");
            assert_eq!(b.refused, 1, "and it is counted, for {bad:?}");
            assert!(
                b.last_refusal.is_some(),
                "with a reason in words, for {bad:?}"
            );
        }
        /* THE TRIVIAL PASS THIS STOPS: a parser that refused every payload would satisfy the loop
         * above, so a well formed empty batch must NOT be a refusal. An empty array is what the
         * page posts when a mutation batch turned out to hold nothing this file wants. */
        let ok = parse_batch("[]");
        assert_eq!(ok.refused, 0, "an empty batch is well formed");
        assert!(ok.kept.is_empty());
    }

    /// EXACTLY WHAT THE SCRIPT POSTS, CAPTURED FROM THE SCRIPT ITSELF AND NOT RETYPED.
    ///
    /// `ytchat::extract::EXTRACT_JS` was pulled out of its raw literal and run under node against a
    /// stub DOM shaped like the measured page: the three renderers the kind table admits, the
    /// guidelines card the table refuses, a row whose Polymer state is absent, and the heartbeat.
    /// This string is the one `window.ipc.postMessage` argument that run produced, byte for byte.
    /// Every other fixture above is one item of it; this is the whole payload as the boundary
    /// actually carries it.
    const POSTED_BY_THE_SCRIPT: &str = r#"[{"kind":"text","id":"ChwKGkNOcXoyZnVhMXBZREZZRVNkZ1lkVWdVTjF3","author":"@cardigansrule","authorType":"","body":"put everything in Bee 🐝 tier","ts":1788572284975162,"pieces":[{"t":"text","v":"put everything in Bee "},{"t":"emote","name":"🐝","url":"https://fonts.gstatic.com/x.png"},{"t":"text","v":" tier"}],"amount":null},{"kind":"text","id":"ChwKGkNPX0hxNGFiMXBZREZjWVNkZ1lkTmNJRlVR","author":"@kkixx_stixx","authorType":"member","body":"hello from youtube","ts":1788572304975162,"pieces":[{"t":"text","v":"hello from youtube"}],"amount":null},{"kind":"paid","id":"ChwKGkNQTEsxNGFiMXBZREZTOFNkZ1lkTFo0RmFn","author":"@Vad_eem","authorType":"owner","body":"thanks for the stream","ts":1788572314975162,"pieces":[{"t":"text","v":"thanks for the stream"}],"amount":"¥2,000"},{"kind":"member","id":"mem1","author":"@c","authorType":"","body":"welcome","ts":1788572334975162,"pieces":[{"t":"text","v":"welcome"}],"amount":null},{"kind":"text","id":"nostamp","author":"@d","authorType":"","body":"no polymer state here","ts":null,"pieces":[{"t":"text","v":"no polymer state here"}],"amount":null},{"kind":"ping","id":"ping:1788575086220","author":"","authorType":"","body":"","ts":1788575086220000,"pieces":[],"amount":null,"polls":0}]"#;

    /// THE SCRIPT AND THIS PARSER AGREE, KEY FOR KEY, INCLUDING THE NULLS.
    ///
    /// THE DEFECT THIS EXISTS FOR, AND IT IS THE ONE NOTHING ELSE IN THE CRATE COULD CATCH. The
    /// page side of this bridge is a JavaScript string constant and the Rust side reads its keys by
    /// name out of a `serde_json::Value`. Neither half is a type the other can see, so renaming
    /// `authorType`, dropping `pieces`, or emitting a kind the table does not carry compiles
    /// perfectly on both sides and shows up on the reader's screen as a chat that has gone calm.
    /// The two halves were also written by two people who never saw each other's file, which is
    /// exactly the situation where "the shapes obviously match" is a claim and not a fact.
    ///
    /// IT HAS TWO HALVES BECAUSE ONE ALONE PROVES NOTHING. The payload above is a SNAPSHOT: a
    /// rename in `EXTRACT_JS` after this was captured would leave it untouched and the parse half
    /// green. So the second half asserts that the script still spells every key this parser reaches
    /// for, which is what makes drift on the page side visible from here.
    ///
    /// THE MUTATION THAT MAKES IT RED: rename any key on either side. `authorType` to `author_type`
    /// in `EXTRACT_JS`'s returned object literal turns the second half red; the same rename in
    /// WHAT SOMEBODY PAID SURVIVES THE PARSE.
    ///
    /// THE DEFECT THIS EXISTS FOR SHIPPED. `extract.rs` posted an `amount` for every superchat
    /// and `YtMessage` had no field to put it in, so the money was dropped between the page and
    /// the screen and a paid message drew as an ordinary one. Nothing failed, nothing warned; the
    /// only reason it was ever noticed is that a reviewer read the script`s object literal against
    /// the parser field by field.
    ///
    /// THE STRING IS PASSED THROUGH UNTOUCHED, currency symbol and separators included. See the
    /// field`s own note on why parsing it would be a way to turn 2,000 yen into 2.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the `amount` read in `parse_batch`, or `filter`ing
    /// the empty string differently.
    #[test]
    fn what_somebody_paid_reaches_the_screen() {
        let posted = r#"[
          {"id":"a","kind":"paid","author":"@big","authorType":"","body":"take my money",
           "ts":1788572604823000,"pieces":[],"amount":"¥2,000"},
          {"id":"b","kind":"text","author":"@thrifty","authorType":"","body":"hi",
           "ts":1788572604824000,"pieces":[],"amount":null},
          {"id":"c","kind":"paid","author":"@odd","authorType":"","body":"blank",
           "ts":1788572604825000,"pieces":[],"amount":"   "}
        ]"#;
        let got = parse_batch(posted);
        assert_eq!(got.kept.len(), 3, "{:?}", got.last_refusal);

        assert_eq!(
            got.kept[0].amount.as_deref(),
            Some("¥2,000"),
            "the sum somebody paid was dropped between the page and the screen, which is how a \
             superchat ends up drawn as an ordinary message"
        );
        assert_eq!(got.kept[0].kind, YtKind::Paid);
        assert_eq!(
            got.kept[1].amount, None,
            "an unpaid message must carry no amount at all"
        );
        assert_eq!(
            got.kept[2].amount, None,
            "a blank amount is `None`, not `Some(\"\")`: an empty badge beside a message nobody \
             paid for is worse than no badge"
        );
    }

    /// `parse_batch`'s `obj.get("authorType")` turns the `author_type` assertion red.
    #[test]
    fn what_the_script_posts_is_exactly_what_this_parser_reads() {
        let b = parse_batch(POSTED_BY_THE_SCRIPT);
        assert_eq!(
            b.refused, 0,
            "the script's own output must not be a refusal: {:?}",
            b.last_refusal
        );
        /* Six items went out and five rows come back: the heartbeat is understood and skipped, and
         * the guidelines card never reached Rust at all because the kind table refused it in the
         * page. Those are two different refusals in two different files and neither is counted. */
        assert_eq!(b.kept.len(), 5);
        assert_eq!(
            b.kept.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec![
                "ChwKGkNOcXoyZnVhMXBZREZZRVNkZ1lkVWdVTjF3",
                "ChwKGkNPX0hxNGFiMXBZREZjWVNkZ1lkTmNJRlVR",
                "ChwKGkNQTEsxNGFiMXBZREZTOFNkZ1lkTFo0RmFn",
                "mem1",
                "nostamp",
            ],
            "no guidelines card and no ping among them"
        );
        assert_eq!(
            b.kept.iter().map(|m| m.kind).collect::<Vec<_>>(),
            vec![
                YtKind::Chat,
                YtKind::Chat,
                YtKind::Paid,
                YtKind::Paid,
                YtKind::Chat
            ],
            "text is chat, and both paid and member are paid"
        );
        /* The emote row survives the crossing whole: the body the page rebuilt from alt text, and
         * the three runs cut out of the same walk. */
        assert_eq!(b.kept[0].body, "put everything in Bee 🐝 tier");
        assert_eq!(
            b.kept[0].pieces,
            vec![
                YtPiece::Text("put everything in Bee ".to_owned()),
                YtPiece::Emote {
                    name: "🐝".to_owned()
                },
                YtPiece::Text(" tier".to_owned()),
            ]
        );
        assert_eq!(b.kept[0].ts_usec, Some(1788572284975162));
        /* THE SUPERCHAT'S OWNER SUBSTITUTION CROSSES INTACT. A paid renderer carries no
         * `author-type` attribute at all, so the page substitutes "owner" off `author-is-owner`,
         * and this is the only place that substitution can be seen from the Rust side. */
        assert_eq!(b.kept[2].author_type, "owner");
        assert_eq!(b.kept[1].author_type, "member");
        /* A JSON `null` stamp is `None` and is not a refusal, which is the whole reason `ts` is
         * allowed to be null in the first place. */
        assert_eq!(b.kept[4].ts_usec, None);

        /* THE SECOND HALF: the script still spells what this parser reads.
         *
         * EVERY NEEDLE CARRIES ITS VALUE AND NOT JUST ITS KEY, AND THAT IS NOT PEDANTRY. The
         * script writes each key TWICE, once in `read`'s returned row and once in the heartbeat
         * `ping` pushes, so a bare `"authorType:"` is still present after `read`'s copy has been
         * renamed and this half was measured going green against exactly that mutation. Pinning
         * the value beside the key is what separates the two literals. `t`, `v` and `name` are
         * checked in the run shape they appear in for the same reason: one letter on its own
         * matches somewhere in 170 lines of JavaScript and proves nothing. */
        let js = crate::ytchat::extract::EXTRACT_JS;
        for key in [
            "kind: kind",
            "id: id",
            "author: an ?",
            "authorType: n.getAttribute(",
            "body: body",
            "ts: usec(n)",
            "pieces: pieces",
            "{ t: \"text\", v:",
            "{ t: \"emote\", name: name",
            "kind: \"ping\"",
        ] {
            assert!(
                js.contains(key),
                "the script no longer writes {key:?}, so this parser is reading a key that is not \
                 posted any more"
            );
        }
    }

    /* ------------------------------------------------------------ interleave -- */

    /// THE CASE THE WHOLE ORDERING RULE EXISTS FOR: A YOUTUBE LINE SPOKEN FIRST IS DRAWN FIRST,
    /// EVEN THOUGH IT REACHED THIS APP TEN SECONDS AFTER THE TWITCH REPLY.
    ///
    /// The numbers are the measured ones. YouTube's page smooths its own appends and polls on a
    /// flat ten second timer, so the lag from send to DOM append was 2.1 to 5.8 seconds normally;
    /// ten seconds is an ordinary, not a worst case, figure. Twitch over a socket is effectively
    /// immediate. So a YouTube question asked at t and a Twitch answer sent at t plus three seconds
    /// reach this app in the wrong order, and rule (a), ordering by arrival, would draw the answer
    /// above the question.
    ///
    /// THE MUTATION THAT MAKES IT RED: make `youtube_at_ms` return `m.seen_ms` unconditionally,
    /// which is exactly rule (a) for the YouTube half.
    #[test]
    fn a_youtube_line_spoken_first_is_drawn_first_even_though_it_arrived_last() {
        let spoken = 1788572284975i64;
        let mut twitch = VecDeque::new();
        /* Sent three seconds after the YouTube line, delivered at once. */
        twitch.push_back(tw("answering the question above", Some(spoken + 3_000)));
        let mut youtube = VecDeque::new();
        /* Spoken first, seen by us ten seconds later. */
        youtube.push_back(yt("question", Some(spoken * 1000), spoken + 10_000));

        let rows = interleave(&twitch, &youtube);
        assert_eq!(
            marks(&rows),
            vec![
                "y:question".to_owned(),
                "t:answering the question above".to_owned()
            ],
            "the question must be above the answer"
        );
    }

    /// TWITCH'S FIVE MILLISECONDS BACKWARDS IS NOT REORDERED, AND THE MERGE STILL PLACES A YOUTUBE
    /// ROW BETWEEN TWITCH ROWS.
    ///
    /// 1788555473902 then 1788555473897 is the real pair from irc_busy.txt that chat.rs:265-272
    /// records: the stamp is when Twitch accepted the message, not when it went out, and arrival
    /// order is the order the room saw. A merged column that sorted by stamp would swap them.
    ///
    /// THE MUTATION THAT MAKES IT RED: replace `interleave`'s two way merge with a
    /// `sort_by_key(|m| at_ms(m))` over the concatenation of both feeds, stable or not.
    #[test]
    fn the_twitch_pair_that_goes_backwards_on_the_wire_keeps_its_arrival_order() {
        let mut twitch = VecDeque::new();
        twitch.push_back(tw("first as the room saw it", Some(1788555473902)));
        twitch.push_back(tw("second as the room saw it", Some(1788555473897)));
        twitch.push_back(tw("much later", Some(1788555480000)));
        let mut youtube = VecDeque::new();
        youtube.push_back(yt("between", Some(1788555475000 * 1000), 1788555485000));

        let rows = interleave(&twitch, &youtube);
        assert_eq!(
            marks(&rows),
            vec![
                "t:first as the room saw it".to_owned(),
                "t:second as the room saw it".to_owned(),
                "y:between".to_owned(),
                "t:much later".to_owned(),
            ],
            "arrival order within Twitch, and the YouTube row still lands between"
        );
    }

    /// THE YOUTUBE HALF IS PUT BACK INTO THE ORDER IT WAS SAID IN, NOT THE ORDER THE PAGE HANDED IT
    /// OVER, AND ROWS THAT SHARE A STAMP KEEP THE ORDER THE DOM GAVE THEM.
    ///
    /// THE DEFECT: walking the YouTube deque in its own order, the way the Twitch deque is walked.
    /// It looks symmetrical and it is wrong, because the two feeds are not symmetrical. YouTube
    /// smooths its own appends and buffers off DOM while the list is scroll paused, and when that
    /// buffer flushed, two rows were appended in the SAME millisecond carrying stamps 20 seconds
    /// apart. A merged column that trusted DOM order would draw that pair backwards and there would
    /// be nothing on screen to say so.
    ///
    /// THE MUTATION THAT MAKES IT RED: delete the `order.sort_by_key(..)` line in `interleave`, or
    /// change it to `sort_unstable_by_key`, which breaks the second half of this test rather than
    /// the first.
    #[test]
    fn the_youtube_half_is_ordered_by_stamp_and_ties_keep_dom_order() {
        let t0 = 1788572284975i64;
        let twitch: VecDeque<chat::Event> = VecDeque::new();
        let mut youtube = VecDeque::new();
        /* Both appended in the same millisecond by the pause flush, 20 seconds apart in truth, and
         * handed over in the wrong order. */
        youtube.push_back(yt("said second", Some((t0 + 20_000) * 1000), t0 + 60_000));
        youtube.push_back(yt("said first", Some(t0 * 1000), t0 + 60_000));
        assert_eq!(
            marks(&interleave(&twitch, &youtube)),
            vec!["y:said first".to_owned(), "y:said second".to_owned()],
            "the stamp decides, not the order the DOM handed them over in"
        );

        /* THE OTHER HALF, AND IT IS THE ONE `sort_unstable_by_key` BREAKS. Once the stamp is gone
         * every row of a batch shares one arrival reading, and then DOM order is the only evidence
         * left about which was said first. A stable sort keeps it; an unstable one throws it away
         * on the exact day the bridge is already degraded. */
        let mut lost = VecDeque::new();
        for id in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            lost.push_back(yt(id, None, t0 + 60_000));
        }
        assert_eq!(
            marks(&interleave(&twitch, &lost)),
            vec![
                "y:one".to_owned(),
                "y:two".to_owned(),
                "y:three".to_owned(),
                "y:four".to_owned(),
                "y:five".to_owned(),
                "y:six".to_owned(),
                "y:seven".to_owned(),
                "y:eight".to_owned(),
            ],
            "one batch, one arrival stamp, so DOM order must survive the sort"
        );
    }

    /// APPENDING TO EITHER FEED NEVER TRANSPOSES TWO ROWS THAT WERE ALREADY DRAWN.
    ///
    /// This is the readability property, and it is the reason the ordering rule is allowed to place
    /// a late YouTube row above a Twitch row at all. A reader can live with a line appearing one or
    /// two rows up. A reader cannot read a column that shuffles itself. The statement asserted here
    /// is the exact one: the previous output is a SUBSEQUENCE of the next output, so rows may be
    /// inserted between old rows but no two old rows ever swap.
    ///
    /// THE MUTATION THAT MAKES IT RED: make either key depend on the other feed, for example by
    /// clamping both sides to one shared running maximum before merging, or by giving a row with no
    /// stamp the key of the newest row seen anywhere.
    #[test]
    fn appending_to_either_feed_only_inserts_and_never_transposes() {
        let t0 = 1788572284975i64;
        let mut twitch = VecDeque::new();
        let mut youtube = VecDeque::new();
        twitch.push_back(tw("a", Some(t0)));
        youtube.push_back(yt("p", Some((t0 + 8_000) * 1000), t0 + 18_000));
        let first = marks(&interleave(&twitch, &youtube));

        /* A Twitch line sent between the two, which under the rule belongs in the middle. */
        twitch.push_back(tw("b", Some(t0 + 4_000)));
        let second = marks(&interleave(&twitch, &youtube));

        /* A YouTube line spoken before everything, arriving now, which is the scroll pause flush
         * case: 44 to 66 seconds of lag was measured after one. */
        youtube.push_back(yt("q", Some((t0 + 1_000) * 1000), t0 + 60_000));
        let third = marks(&interleave(&twitch, &youtube));

        assert!(
            is_subsequence(&first, &second),
            "first {first:?} must survive inside second {second:?}"
        );
        assert!(
            is_subsequence(&second, &third),
            "second {second:?} must survive inside third {third:?}"
        );
        /* THE TRIVIAL PASS THIS STOPS: an `interleave` that always appended every Twitch row before
         * every YouTube row would satisfy every subsequence check above, so the actual insertions
         * are pinned. Both new rows really did land in the middle. */
        assert_eq!(
            third,
            vec![
                "t:a".to_owned(),
                "y:q".to_owned(),
                "t:b".to_owned(),
                "y:p".to_owned()
            ],
            "the two late arrivals sit where they were spoken"
        );
    }

    /// WITH NO STAMPS AT ALL THE YOUTUBE HALF FALLS BACK TO WHEN IT ARRIVED, NOT TO THE TOP AND NOT
    /// TO THE BOTTOM.
    ///
    /// This is the day YouTube renames the Polymer field. Every YouTube row loses `ts_usec` at once,
    /// and the merged feed has to degrade to something a reader can still read. It degrades to rule
    /// (a), ordering YouTube by arrival, which is the honest second best answer and is stable
    /// because `seen_ms` was frozen into the row when it arrived.
    ///
    /// THE MUTATION THAT MAKES IT RED: make `youtube_at_ms` return `0` or `i64::MAX` when `ts_usec`
    /// is `None`. Either sentinel piles the whole YouTube feed at one end of the column.
    #[test]
    fn youtube_with_no_stamps_at_all_falls_back_to_when_it_arrived() {
        let t0 = 1788572284975i64;
        let mut twitch = VecDeque::new();
        twitch.push_back(tw("early", Some(t0)));
        twitch.push_back(tw("late", Some(t0 + 20_000)));
        let mut youtube = VecDeque::new();
        youtube.push_back(yt("middle", None, t0 + 10_000));

        assert_eq!(
            marks(&interleave(&twitch, &youtube)),
            vec![
                "t:early".to_owned(),
                "y:middle".to_owned(),
                "t:late".to_owned()
            ],
            "an undated YouTube row sits where it arrived, between the two Twitch rows"
        );
    }

    /// AN UNDATABLE TWITCH LINE DOES NOT HOLD THE OTHER FEED BEHIND IT.
    ///
    /// `chat::Event::sent_ms` is `None` on the arms Twitch does not stamp, chiefly `Kind::Server`, a
    /// NOTICE from tmi.twitch.tv. Dating such a row as late as possible would park it at the head of
    /// the merge and freeze every YouTube row behind it until the next Twitch line arrives, which in
    /// a room measured at 3.68 messages a minute is a stall of tens of seconds with no cause
    /// anywhere on screen. Dating it at the epoch cannot do that, and cannot move it ahead of its
    /// own predecessors either, because the Twitch deque is walked and never sorted.
    ///
    /// THE MUTATION THAT MAKES IT RED: change `twitch_at_ms` to `e.sent_ms.unwrap_or(i64::MAX)`.
    #[test]
    fn an_undatable_twitch_line_does_not_hold_the_youtube_feed_behind_it() {
        let t0 = 1788572284975i64;
        let mut twitch = VecDeque::new();
        let mut notice = tw("this channel is in followers only mode", None);
        notice.kind = chat::Kind::Server;
        twitch.push_back(notice);
        let mut youtube = VecDeque::new();
        youtube.push_back(yt("still flowing", Some(t0 * 1000), t0 + 10_000));

        assert_eq!(
            marks(&interleave(&twitch, &youtube)),
            vec![
                "t:this channel is in followers only mode".to_owned(),
                "y:still flowing".to_owned()
            ],
            "the undated notice is drawn and the YouTube row is not stranded behind it"
        );

        /* And the row after it is still placed on its own stamp, so one undated line does not
         * swallow the ordering of the lines around it. */
        twitch.push_back(tw("much later", Some(t0 + 30_000)));
        assert_eq!(
            marks(&interleave(&twitch, &youtube)),
            vec![
                "t:this channel is in followers only mode".to_owned(),
                "y:still flowing".to_owned(),
                "t:much later".to_owned()
            ]
        );
    }

    /// THE SAME TWO LOGS PRODUCE THE SAME COLUMN EVERY TIME, TIES INCLUDED.
    ///
    /// The merged column is recomputed from scratch on every frame. A tie broken by anything that is
    /// not a pure function of the inputs, a hash order or a clock reading taken during the merge,
    /// would make two rows flicker past each other sixty times a second. Ties are not exotic once
    /// the stamp is lost: a whole batch shares one `seen_ms`.
    ///
    /// THE MUTATION THAT MAKES IT RED: break the tie in `interleave` with anything that varies
    /// between calls, or flip the comparison to `<` so that equal keys take the YouTube row and the
    /// documented rule and the code disagree.
    #[test]
    fn an_exact_tie_goes_to_twitch_and_the_column_is_the_same_on_every_call() {
        let t0 = 1788572284975i64;
        let mut twitch = VecDeque::new();
        twitch.push_back(tw("same instant", Some(t0)));
        let mut youtube = VecDeque::new();
        youtube.push_back(yt("also same instant", Some(t0 * 1000), t0));

        let once = marks(&interleave(&twitch, &youtube));
        let twice = marks(&interleave(&twitch, &youtube));
        assert_eq!(once, twice, "the merge is a pure function of its inputs");
        assert_eq!(
            once,
            vec![
                "t:same instant".to_owned(),
                "y:also same instant".to_owned()
            ],
            "a tie goes to Twitch, which is arbitrary but must be fixed"
        );
    }

    /// EVERY MERGED ROW NAMES THE ROOM IT CAME OUT OF.
    ///
    /// THE DEFECT: a `source` that disagrees with the row it is attached to, which paints a Twitch
    /// badge on a YouTube line and makes the whole merged column untrustworthy about the one fact
    /// only it can tell the reader.
    ///
    /// THE MUTATION THAT MAKES IT RED: swap the two arms of `Merged::source`.
    #[test]
    fn a_merged_row_names_the_platform_it_came_from() {
        let mut twitch = VecDeque::new();
        twitch.push_back(tw("t", Some(1)));
        let mut youtube = VecDeque::new();
        youtube.push_back(yt("y", Some(2000), 2));
        let rows = interleave(&twitch, &youtube);
        assert_eq!(rows[0].source(), Source::Twitch);
        assert_eq!(rows[1].source(), Source::YouTube);
    }
}
