//! Twitch chat, read anonymously on a background thread, off the UI.
//!
//! WHAT THIS FILE IS. One thread, one socket, one channel. It dials irc.chat.twitch.tv:6697 over
//! TLS, asks for the two capabilities that carry the information a viewer can see, logs in with no
//! account and no password, joins the channel, answers every PING, and pushes what the server said
//! into an `Arc<Mutex<Log>>` the UI reads a frame at a time. It reconnects with a backoff when the
//! socket dies, and it never writes a byte to disk.
//!
//! THE SHAPE IS `watcher.rs`, DELIBERATELY AND ALMOST LINE FOR LINE. The live status poller in this
//! crate already solved "a background thread that must be promptly stoppable and cheap to read from
//! the paint loop", and a second answer to the same question is a second set of bugs. Taken from
//! it: the named `thread::Builder` (watcher.rs:615), the `stop` and `nudge` `AtomicBool`s
//! (watcher.rs:601-603), the nap that sleeps in 250 ms slices so a stop lands within a quarter
//! second rather than at the end of a long wait (watcher.rs:643-657), the poison tolerant `lock`
//! that hands back the guard from a panicked thread rather than taking the UI down with it
//! (watcher.rs:679-681), the "the thread would not spawn, so say so in the shared state rather than
//! leave it looking like a result that has not landed yet" branch (watcher.rs:657-664), and the
//! `Drop` that sets `stop` and does NOT join, because a read can be mid flight and the UI thread
//! must not stall on it (watcher.rs:672-676).
//!
//! ONE THING IS DELIBERATELY NOT COPIED. `Watcher::status()` clones the whole shared state every
//! frame, and that is right for two small structs. This log holds up to `LOG_CAP` messages, and
//! cloning two thousand `String`s and their span vectors at 60 Hz would cost more than drawing
//! them. So the reader lends the log under the lock, `ChatReader::with_log`, and the screen reads
//! what it needs inside the closure. The lock is held for one frame's read of a `VecDeque`, and the
//! only other holder is a reader thread that takes it once per socket read.
//!
//! WHY NO `twitch.tv/membership`. The capability set is `twitch.tv/tags twitch.tv/commands` and
//! that is a measurement, not a preference. Both captures are on disk. The one taken WITH
//! membership (irc_anon.txt, one quiet channel, 23 lines) spends four of its lines on other
//! viewers' JOINs in the two minutes it covers. The one taken WITHOUT it (irc_busy.txt, seven of
//! the busiest channels on the service, 6094 lines over eleven minutes) contains exactly seven JOIN
//! lines, one per channel, every one of them the echo of our own. Membership buys a viewer a list
//! of names nothing on this screen draws and, in a channel with tens of thousands of chatters, the
//! majority of the bytes. The echo of our own JOIN still arrives without it, which is what this
//! file uses to know it is in the room.
//!
//! A DROPPED SOCKET IS AN ERROR THE UI CAN SEE, NEVER SILENCE. This is the rule watcher.rs:13-19
//! states for live status and it holds here for the same reason: "nobody is talking" is a fact
//! about the channel and "the connection died" is a fact about this machine, and a client that
//! draws the second as the first shows an empty, calm, wrong chat while the socket is gone. So a
//! failed session sets `Log::error` and moves `Log::state` to `Conn::Retrying`, and it does NOT
//! touch `Log::lines`. The lines already read stay on screen across a reconnect, because they were
//! true when they arrived and a reconnect does not make them false.
//!
//! NOTHING HERE TOUCHES DISK. Twitch's Developer Agreement permits chat to be held for as long as
//! it takes to operate the feature and no longer, so the log is a `VecDeque` in memory, bounded by
//! COUNT, and it dies with the process. There is no path from this file to `std::fs`. The Chat
//! screen already promises the reader exactly this in words.
//!
//! THREE THINGS COME FROM THE SIBLING PARSING PIECES and they are named in exactly one function,
//! `step`, so that a rename touches one place: `split_line`, which cuts a raw line into tags,
//! prefix, command and trailing; `Tags::parse` with a `get` by key, over the tags blob; and
//! `spans`, which turns a body and an `emotes` tag into drawable runs. Nothing else in this file
//! knows they exist.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{Color32, ViewportId};

/* THE THREE PIECES OF THE WIRE FORMAT, EACH PARSED IN ITS OWN MODULE AND CALLED HERE.
 *
 * The module note above names the seam as `split_line`, `Tags::parse` and `spans`. Two of those
 * three were the names this file was written against and NOT the names the modules ended up with,
 * which is what a seam between separately written pieces looks like when it is first closed:
 * `irc::line` calls the splitter `parse`, and `irc::tags` calls the map `TagMap`. The alias below
 * keeps this file reading the way its own note describes while pointing at exactly one
 * implementation. There is no second splitter and no second tag parser in this crate; the check is
 * `grep -rn 'pub fn parse' src/irc/`, which answers with two lines, one per module. */
use crate::irc::emotes;
use crate::irc::line::parse as split_line;
use crate::irc::tags::TagMap;

/* ------------------------------------------------------------- the constants -- */

/// Where Twitch's IRC bridge lives, and the TLS port. 6667 exists and is plaintext; it is not
/// offered here, because a plaintext fallback is a downgrade an attacker on the path can force.
pub const IRC_HOST: &str = "irc.chat.twitch.tv";
pub const IRC_PORT: u16 = 6697;

/// The capability request, byte for byte as the busy capture's `CAP * ACK` echoes it back.
///
/// `tags` is what carries the display name, the colour, the emote positions, the badges and the
/// server's own timestamp; `commands` is what carries USERNOTICE, CLEARCHAT, ROOMSTATE, NOTICE and
/// RECONNECT. Without `tags` a message is an anonymous grey line. Without `commands` a timeout and
/// a sub are invisible. See the module note for why membership is not in this list.
pub const CAP_REQ: &str = "CAP REQ :twitch.tv/tags twitch.tv/commands";

/// How many messages the log keeps. Chosen from the captures rather than from taste.
///
/// #zackrawrr, the busiest channel in irc_busy.txt, produced 3681 messages in 660 seconds, which is
/// 5.6 a second sustained, and its worst single second held 38. Two thousand lines is therefore
/// about six minutes of the busiest chat that was measured, and about eighteen hours of Broken
/// Stoic's own measured rate (four messages in the 131 seconds irc_anon.txt covers). Six minutes is
/// more scrollback than a viewer reads back through, and the cost is bounded: the mean message body
/// in the busy capture is 24.9 bytes and the longest is 347, so two thousand entries with their
/// names, badges and span vectors sit comfortably under a megabyte.
///
/// IT IS A COUNT AND NOT A BYTE BUDGET ON PURPOSE. A byte budget makes the amount of scrollback a
/// viewer has depend on how wordy the channel is, so the same app would hold twenty minutes of one
/// chat and ninety seconds of another and a bug report would be impossible to reproduce. A count is
/// the thing the reader can actually perceive.
pub const LOG_CAP: usize = 2000;

/// The socket read timeout, which is also the loop's tick. It is what makes a stop prompt: the
/// thread cannot be sitting in a blocking read for longer than this, so `Drop` costs at most a
/// quarter second of a thread nobody is waiting on. Same quarter second as watcher.rs:656.
pub const READ_SLICE: Duration = Duration::from_millis(250);

/// How long a connected, joined socket may stay silent before it is declared dead.
///
/// MEASURED: Twitch sends its own PING every four minutes and forty seconds. In irc_busy.txt the
/// two PINGs sit at server timestamps 1788555717404 and 1788555995913, 278.5 seconds apart, and the
/// first arrives 272.7 seconds after the first line of the capture. Seven minutes is a shade over
/// one and a half of those intervals, so a live connection can never trip it, and a socket that a
/// middlebox has quietly black holed, which produces no error and no EOF, is caught inside seven
/// minutes instead of never. Without this a half open TCP connection reads as a quiet channel
/// forever, which is the exact conflation the module note forbids.
pub const IDLE_LIMIT: Duration = Duration::from_secs(420);

/// How long the whole login may take: the `001` welcome and then the `376` end of MOTD, after which
/// the JOIN goes out and a `366` or the JOIN echo confirms it. In both captures every one of those
/// lines arrives inside the first handful of milliseconds. Fifteen seconds is not a performance
/// budget, it is the difference between "Twitch is slow" and "this socket is connected to something
/// that will never answer".
pub const HANDSHAKE_LIMIT: Duration = Duration::from_secs(15);

/// The floor between repaints. See `Coalescer`: the point is that a raid cannot become a repaint
/// storm.
pub const REPAINT_EVERY: Duration = Duration::from_millis(100);

/// The longest single line this reader will accept before it gives up on the connection.
///
/// IRCv3 allows 8191 bytes of tags plus the 512 byte message, and the longest line in the busy
/// capture is 1081 bytes, so 16 KiB is roughly sixteen times the worst thing that was actually seen
/// and about double the worst thing the grammar permits. It is not really a line limit: it is the
/// cap that stops a peer which never sends a newline from growing this thread's buffer until the
/// process dies.
pub const MAX_LINE: usize = 16 * 1024;

/// How long to wait for the TCP handshake. `TcpStream::connect` has no timeout of its own and a
/// black holed address hangs it for the operating system's full SYN retry schedule, which on
/// Windows is around twenty seconds and on Linux longer still, all of it spent with the screen
/// saying "connecting".
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The reconnect ladder, in seconds, flattening at its last rung forever after.
///
/// It is short at the front because the common failure is a laptop lid or a wifi hop and the viewer
/// is looking at the screen when it happens, and it flattens at a minute because the uncommon
/// failure is Twitch being down, and a client that keeps hammering a service that is down is part
/// of why it stays down.
pub const BACKOFF: [u64; 7] = [1, 2, 5, 10, 20, 30, 60];

/* ---------------------------------------------------------------- the shapes -- */

/// What kind of thing the server said. Every one of these is a shape that appears in the captures,
/// except `Server`, which says so on itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A PRIVMSG: somebody talked.
    Chat,
    /// A PRIVMSG that arrived wrapped in the `\x01ACTION ... \x01` envelope, which is what `/me`
    /// sends. 42 of them in the busy capture. The envelope is stripped, see `de_action`.
    Action,
    /// A PRIVMSG the sender spent channel points to highlight, `msg-id=highlighted-message`. 16 in
    /// the busy capture. It is a kind of its own because the whole point of it is that it is drawn
    /// differently; a client that renders it as an ordinary line has taken the viewer's points and
    /// given nothing back.
    Highlight,
    /// A USERNOTICE: a sub, a resub, a gift, an announcement, a watch streak milestone. `notice`
    /// carries the server's own sentence and `body` carries the viewer's message, and they are
    /// genuinely two different things: see `Event::notice`.
    Notice,
    /// A CLEARCHAT: somebody was timed out or banned, or the whole room was wiped. `who` is the
    /// target and `secs` is the timeout length when there was one.
    Cleared,
    /// An IRC NOTICE from tmi.twitch.tv. NOT PRESENT IN EITHER CAPTURE, which is the honest state
    /// of this arm: it is what a suspended channel, a rejected capability request or a rate limit
    /// answers with, and dropping it would turn every one of those into an empty chat with no
    /// reason on screen.
    Server,
}

/// ONE RUN OF A MESSAGE BODY, OWNED.
///
/// `irc::emotes::Span` is the same thing BORROWED: it points into the body and into the `emotes`
/// tag, so cutting a line copies no text. That is right for the splitter and impossible here. An
/// `Event` owns its `body` and then crosses a channel from the reader thread into the ring buffer
/// the screen draws from, where it outlives the read buffer those spans pointed into. A struct
/// holding both a `String` and a slice of that same `String` is self referential and does not
/// compile, which is the borrow checker stating the lifetime problem up front rather than the
/// program discovering it at run time.
///
/// IT IS BUILT BY COPYING FROM THE SPAN BUILDER AND NEVER BY CUTTING THE BODY AGAIN. `pieces`
/// below maps one arm per `Span` variant and does no arithmetic. Where an emote starts and ends
/// stays the span builder's business, including the inclusive CODE POINT ranges Twitch sends and
/// the byte offsets Rust slices with; a second walk over the body here would be a second
/// implementation of that conversion, which agrees until it does not. This is the rule `FightRow`
/// follows for `secs` and `headline`, for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Text to draw as text. Never empty: the span builder guarantees it.
    Text(String),
    /// An emote. `id` is what Twitch calls it in the CDN url, `name` is the text it replaces.
    ///
    /// `name` IS DRAWN WHEN THERE IS NO IMAGE, which is every emote today: nothing in this crate
    /// fetches from the emote CDN yet. A reader that dropped the name and kept only the id would
    /// leave a hole in the sentence rather than the word the viewer typed.
    Emote { id: String, name: String },
}

/// The owned mirror of `irc::emotes::spans`, one arm per variant and no arithmetic of its own.
fn pieces(body: &str, emote_tag: &str) -> Vec<Piece> {
    emotes::spans(body, emote_tag)
        .into_iter()
        .map(|s| match s {
            emotes::Span::Text(t) => Piece::Text(t.to_owned()),
            emotes::Span::Emote { id, name } => Piece::Emote {
                id: id.to_owned(),
                name: name.to_owned(),
            },
        })
        .collect()
}

/// One entry in the log. Built once by the reader thread and then read only.
///
/// IT DERIVES `Clone` AND `Debug` AND NOTHING ELSE, WHICH IS A REACHABILITY DECISION. lib.rs
/// records that rustc's dead code lint is blind to a field on a struct that derives `PartialEq`,
/// `Eq`, `Hash`, `PartialOrd` or `Ord`, because the derive reads every field, and that this crate
/// lost fourteen unreachable fields to exactly that blind spot. Keeping those derives off this
/// struct keeps the lint's eyes open: a field here that the Chat screen never draws is reported as
/// dead rather than hidden behind a `==`. The tests below compare fields by name for the same
/// reason.
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: Kind,
    /// The `display-name` tag, which is the capitalisation the sender chose ("iamTotallyTroy",
    /// "Broken_Stoic"), falling back to `login` and then to the nick out of the message prefix, and
    /// for a CLEARCHAT the login of whoever was timed out.
    pub who: String,
    /// The sender's chosen name colour, when they have chosen one. `color=` arrives EMPTY for users
    /// who never picked one, which the capture shows on Broken Stoic's own line, and an empty tag
    /// means "the client decides", not black.
    pub color: Option<Color32>,
    /// The text a person typed. For an `Action` the `\x01ACTION` envelope is already off. For a
    /// `Notice` with no message from the viewer it is empty and `notice` is what there is to draw.
    pub body: String,
    /// `body` cut into text and emote runs by the span builder. Empty when there is no body.
    pub spans: Vec<Piece>,
    /// The server's own sentence about a USERNOTICE, from `system-msg`, unescaped.
    ///
    /// IT IS A SEPARATE FIELD BECAUSE THE CAPTURE SHOWS THE TWO CANNOT BE ONE. 27 of the 62
    /// USERNOTICEs in irc_busy.txt have no trailing parameter at all: a resub with no message from
    /// the subscriber IS the whole line, and `system-msg` is the only text in it. Meanwhile the
    /// announcement in irc_anon.txt has `system-msg=` EMPTY and its entire content in the trailing.
    /// Fold them into one string and one of those two shapes draws as a blank row.
    pub notice: Option<String>,
    /// `tmi-sent-ts`, the server's own millisecond clock, when the line carried one.
    ///
    /// THE LOG IS NOT SORTED BY IT AND MUST NOT BE. In irc_busy.txt, #zackrawrr delivers
    /// 1788555473902 and then 1788555473897, five milliseconds backwards, in arrival order. The
    /// stamp is when Twitch accepted the message, not when it went out, and the two orders differ.
    /// Arrival order is the order the room saw, and it is the order the deque preserves.
    pub sent_ms: Option<i64>,
    /// The raw `badges` tag, "moderator/1,subscriber/138,bits-charity/1". Kept as the server sent
    /// it: which of them this app draws, and at what size, is the screen's business and not a
    /// decision worth baking into the reader.
    pub badges: String,
    /// A CLEARCHAT's `ban-duration` in seconds. `None` on a permanent ban, and on everything that
    /// is not a CLEARCHAT. All 31 CLEARCHATs in the busy capture carry a duration; the permanent
    /// case is what the tag's absence means.
    pub secs: Option<u32>,
}

impl Event {
    /// The empty shell every arm fills in. Not `Default`, because an `Event` with no `kind` is not
    /// a thing that should be constructible from outside the one function that builds one.
    fn blank(kind: Kind) -> Event {
        Event {
            kind,
            who: String::new(),
            color: None,
            body: String::new(),
            spans: Vec::new(),
            notice: None,
            sent_ms: None,
            badges: String::new(),
            secs: None,
        }
    }
}

/// The room's own settings, from ROOMSTATE.
///
/// `followers_only` is minutes and `-1` means the mode is off, which is Twitch's encoding and not
/// this file's: irc_busy.txt carries -1, 1, 10, 15 and 1440 across its seven channels.
#[derive(Clone, Debug)]
pub struct Room {
    pub emote_only: bool,
    pub followers_only: i64,
    pub r9k: bool,
    pub slow: u32,
    pub subs_only: bool,
}

impl Room {
    fn unknown() -> Room {
        Room {
            emote_only: false,
            followers_only: -1,
            r9k: false,
            slow: 0,
            subs_only: false,
        }
    }

    /// Apply the keys a ROOMSTATE actually carried and leave the rest alone.
    ///
    /// WHY A PATCH AND NOT A REPLACEMENT. The ROOMSTATE that follows a JOIN is complete, and both
    /// captures only ever show that one. The ROOMSTATE that follows a moderator flipping a switch
    /// carries ONLY the key that changed, and a client that overwrote the whole struct from it
    /// would answer "is this channel followers only?" with the default the moment somebody turned
    /// slow mode on, and tell a viewer they may talk when they may not. Nothing in the captures
    /// proves this arm, which is said plainly rather than left for a reader to assume it was
    /// measured.
    fn apply(&mut self, p: &RoomPatch) {
        if let Some(v) = p.emote_only {
            self.emote_only = v;
        }
        if let Some(v) = p.followers_only {
            self.followers_only = v;
        }
        if let Some(v) = p.r9k {
            self.r9k = v;
        }
        if let Some(v) = p.slow {
            self.slow = v;
        }
        if let Some(v) = p.subs_only {
            self.subs_only = v;
        }
    }
}

/// The keys one ROOMSTATE carried. `None` means the line did not mention it.
#[derive(Clone, Debug)]
struct RoomPatch {
    emote_only: Option<bool>,
    followers_only: Option<i64>,
    r9k: Option<bool>,
    slow: Option<u32>,
    subs_only: Option<bool>,
}

/// Where the reader is. The UI draws this; it is the whole of the difference between a quiet
/// channel and a broken socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conn {
    /// `start` has never been called. The Chat screen sits here until its first draw.
    Idle,
    /// A socket is being dialled, or the login is in progress.
    Connecting,
    /// In the room. Lines arriving.
    Joined,
    /// The last session failed. `attempt` counts consecutive failures and `next_try_in` is what the
    /// thread is currently sleeping, so the screen can count it down instead of saying nothing.
    Retrying { attempt: u32, next_try_in: Duration },
    /// The thread has gone: `stop` was set, or it could not be spawned at all.
    Stopped,
}

/// What the UI reads. Lives behind the reader's mutex; borrow it with `ChatReader::with_log`.
#[derive(Clone, Debug)]
pub struct Log {
    /// The channel this reader is on, lowercase and with no leading `#`.
    pub channel: String,
    pub state: Conn,
    /// Why the last session ended, in words, cleared only when a new one joins. Set ALONGSIDE a
    /// full `lines`, never instead of it.
    pub error: Option<String>,
    /// Oldest first, arrival order. See `Event::sent_ms` for why it is not sorted.
    pub lines: VecDeque<Event>,
    pub room: Option<Room>,
    /// How many lines the cap has pushed off the front. A count and not a flag, so the screen can
    /// say "and 12043 older lines are gone" rather than implying the log is complete.
    pub dropped: u64,
    /// How many lines arrived that this reader could not turn into anything.
    ///
    /// COUNTED, NEVER SILENTLY DROPPED. A parser that quietly discards what it does not understand
    /// looks perfect right up to the release where Twitch adds a shape and a tenth of the chat
    /// vanishes with nothing on screen and nothing in the log. This number and `last_refusal` are
    /// the tripwire, and the screen is expected to show them when they are not zero.
    pub refused: u64,
    pub last_refusal: Option<String>,
    /// How many times this reader has joined the room. Two or more means it reconnected, and the
    /// lines either side of the seam came from different sessions.
    pub connects: u64,
}

impl Log {
    fn idle(channel: &str) -> Log {
        Log {
            channel: channel.to_owned(),
            state: Conn::Idle,
            error: None,
            lines: VecDeque::new(),
            room: None,
            dropped: 0,
            refused: 0,
            last_refusal: None,
            connects: 0,
        }
    }

    /// Append, and drop the oldest when the cap is passed. The cap is a parameter rather than the
    /// constant so a test can prove eviction without pushing two thousand real lines through it.
    fn push(&mut self, e: Event, cap: usize) {
        self.lines.push_back(e);
        while self.lines.len() > cap {
            if self.lines.pop_front().is_some() {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
    }
}

/* ------------------------------------------------------------------ the wire -- */

/// Something that can be dialled to get a duplex byte pipe. The seam exists so the reader's loop
/// can be driven by the bytes of a real capture in a test instead of by a socket to Twitch, which
/// is the only way the framing, the PONG and the reconnect can be asserted at all.
pub trait Wire: Send {
    fn dial(&mut self) -> Result<Box<dyn Duplex>, String>;
}

/// A connected pipe.
pub trait Duplex: Send {
    /// Fill `into`. `Ok(0)` means the peer closed, which is the one thing a caller must not read as
    /// "nothing happened". A read that merely reached its timeout must be an `Err` whose kind is
    /// `WouldBlock` or `TimedOut`, never `Ok(0)`.
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize>;
    /// Send one IRC line. The CRLF is this method's job, so no caller can forget it.
    fn send(&mut self, line: &str) -> std::io::Result<()>;
}

/// The production wire: TCP to irc.chat.twitch.tv:6697 with rustls on top.
pub struct TlsWire {
    pub host: &'static str,
    pub port: u16,
}

impl Default for TlsWire {
    fn default() -> TlsWire {
        TlsWire {
            host: IRC_HOST,
            port: IRC_PORT,
        }
    }
}

/// The TLS client config, built once for the life of the process.
///
/// `ClientConfig::builder()` is the short spelling and it PANICS when the process has no default
/// crypto provider, or has two. This crate forbids a panic in non test code and, worse, the number
/// of providers compiled in is decided by the feature flags of every crate in the tree, so the
/// short spelling makes an unrelated dependency bump able to turn chat into a crash.
/// `builder_with_provider` names ring, which is the provider ureq already pulls in, and returns a
/// `Result` this function can put words to.
fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    static CFG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CFG.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if roots.is_empty() {
            /* An empty trust store does not fail to build. It builds a client that rejects
             * everything, and the reader would then report a certificate error forever with no hint
             * that the roots, rather than Twitch, are the problem. */
            return Err("the bundled root certificates are empty".to_owned());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS could not be configured: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Arc::new(cfg))
    })
    .clone()
}

impl Wire for TlsWire {
    fn dial(&mut self) -> Result<Box<dyn Duplex>, String> {
        let cfg = tls_config()?;
        let addr = format!("{}:{}", self.host, self.port);

        /* RESOLVE, THEN CONNECT WITH A TIMEOUT. `TcpStream::connect(&str)` takes no timeout, so a
         * black holed address holds this thread for the operating system's whole SYN retry
         * schedule with the screen saying "connecting" the entire time. `connect_timeout` needs a
         * resolved `SocketAddr`, hence the two steps, and every resolved address is tried because
         * irc.chat.twitch.tv answers with several and an unreachable IPv6 route is an ordinary
         * thing on a home network. */
        let addrs = addr
            .to_socket_addrs()
            .map_err(|e| format!("{addr} could not be resolved: {e}"))?;
        let mut last = format!("{addr} resolved to no addresses");
        let mut sock = None;
        for a in addrs {
            match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(e) => last = format!("{a} refused the connection: {e}"),
            }
        }
        let sock = match sock {
            Some(s) => s,
            None => return Err(last),
        };

        /* The read timeout IS the loop's tick, see READ_SLICE. The write timeout only has to exceed
         * any plausible send, since the only things ever written are four short lines and a PONG
         * every four and a half minutes. */
        sock.set_read_timeout(Some(READ_SLICE))
            .map_err(|e| format!("the read timeout could not be set: {e}"))?;
        sock.set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("the write timeout could not be set: {e}"))?;
        /* Nagle would hold a twenty byte PONG waiting for company that is not coming. */
        let _ = sock.set_nodelay(true);

        let server = rustls::pki_types::ServerName::try_from(self.host)
            .map_err(|e| format!("{} is not a valid server name: {e}", self.host))?
            .to_owned();
        let conn = rustls::ClientConnection::new(cfg, server)
            .map_err(|e| format!("the TLS session could not be started: {e}"))?;
        Ok(Box::new(Tls {
            s: rustls::StreamOwned::new(conn, sock),
        }))
    }
}

struct Tls {
    s: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
}

impl Duplex for Tls {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        match self.s.read(into) {
            Ok(n) => Ok(n),
            Err(e) if quiet(&e) => {
                /* A REAL BUG THIS BRANCH EXISTS TO STOP, AND IT IS WINDOWS ONLY, WHICH IS THIS
                 * APP'S MAIN PLATFORM. rustls' `Stream::read` loops on `complete_io` and, when the
                 * socket will not yield, forgives the error and hands over the plaintext it has
                 * already decrypted ONLY if the error kind is `WouldBlock`. A socket read timeout
                 * on Windows reports `TimedOut`, not `WouldBlock`. So the sequence "server sends
                 * one record, then goes quiet" leaves a complete, decrypted, perfectly good chat
                 * line sitting in rustls' own reader while every read returns TimedOut, forever,
                 * until IDLE_LIMIT fires and this reader reports a disconnection that never
                 * happened. Draining the connection's reader by hand is the fix; when it is empty
                 * it says so with its own WouldBlock and the timeout is reported as the timeout it
                 * was. */
                match self.s.conn.reader().read(into) {
                    Ok(n) => Ok(n),
                    Err(_) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    fn send(&mut self, line: &str) -> std::io::Result<()> {
        self.s.write_all(line.as_bytes())?;
        self.s.write_all(b"\r\n")?;
        self.s.flush()
    }
}

/// Is this the read timeout expiring rather than the connection failing? Both kinds are here
/// because the platforms disagree: Unix reports `WouldBlock` and Windows reports `TimedOut` for the
/// same expired `SO_RCVTIMEO`, and treating either as a failure would reconnect four times a second
/// on that platform.
fn quiet(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/* ---------------------------------------------------------------- the tuning -- */

/// Every number the loop consults, in one struct, so the tests can run the real loop over real
/// captured bytes in milliseconds instead of minutes. This is `Watcher::spawn` taking an interval:
/// the production path takes `Tuning::default()` and nothing else exists to be got wrong.
#[derive(Clone, Debug)]
pub struct Tuning {
    pub log_cap: usize,
    pub read_slice: Duration,
    pub idle_limit: Duration,
    pub handshake_limit: Duration,
    pub repaint_every: Duration,
    pub max_line: usize,
    pub backoff: Vec<Duration>,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning {
            log_cap: LOG_CAP,
            read_slice: READ_SLICE,
            idle_limit: IDLE_LIMIT,
            handshake_limit: HANDSHAKE_LIMIT,
            repaint_every: REPAINT_EVERY,
            max_line: MAX_LINE,
            backoff: BACKOFF.iter().map(|s| Duration::from_secs(*s)).collect(),
        }
    }
}

/* ---------------------------------------------------------------- the reader -- */

/// How many reader threads this process has started. Logged when one starts, which is the same
/// thing `channel_art::fetches_started` does and for the same reason: a screen that starts a second
/// connection on every draw looks identical to one that started a single connection, right up until
/// Twitch rate limits the address.
/// A READ ONLY VIEW OF A READER'S LOG, CLONEABLE, THAT CANNOT DIAL AND CANNOT HANG UP.
///
/// WHY THE READER ITSELF CANNOT TRAVEL. `ChatReader` owns a thread: dropping one sets the stop
/// flag, so it is deliberately neither `Clone` nor copyable, and it lives in exactly one place,
/// the `App`. But the Chat screen is drawn in TWO windows now. A tool window is a deferred egui
/// viewport whose context (`windows::ChildCx`) is built by cloning what the root holds, and a
/// thread owner cannot be cloned into it.
///
/// SO THE LOG TRAVELS AND THE THREAD DOES NOT. This is two `Arc` clones, which is what a `Cx`
/// costs per frame to carry it, and it is exactly the read half: `with_log` to draw, and
/// `reconnect_now` to nudge a backoff, which is safe from anywhere because it only sets a flag a
/// running thread reads. There is no `start` and no `stop` on it, so no window except the one
/// that owns the reader can open a socket or close one. That is the same rule `Cx::chat_wanted`
/// expresses for the body, enforced by the type rather than by a comment.
#[derive(Clone)]
pub struct ChatHandle {
    shared: Arc<Mutex<Log>>,
    nudge: Arc<AtomicBool>,
    /// Present once a sign-in has landed. The screen asks `can_send`; nothing hands the token out.
    creds: Arc<Mutex<Option<Creds>>>,
    outbox: Arc<Mutex<VecDeque<String>>>,
}

impl ChatHandle {
    /// A handle onto a log nothing writes to: the empty state, and what every context that must
    /// NAME a chat handle without having a reader behind it uses.
    ///
    /// IT REPLACES A `OnceLock<ChatReader>` CALLED `never()`. That existed because `Cx` borrowed
    /// a `&ChatReader` and a temporary would not live for the frame; a handle is owned and cheap,
    /// so the shared static is gone and with it the worry about what a process wide reader might
    /// accumulate. The eight test contexts and any window with no reader behind it use this.
    pub fn idle() -> ChatHandle {
        ChatHandle {
            shared: Arc::new(Mutex::new(Log::idle(""))),
            nudge: Arc::new(AtomicBool::new(false)),
            creds: Arc::new(Mutex::new(None)),
            outbox: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Read the log. A borrow under the lock rather than a clone: see the module note.
    pub fn with_log<R>(&self, f: impl FnOnce(&Log) -> R) -> R {
        f(&lock(&self.shared))
    }

    /// Stop waiting out the backoff and dial now. Returns immediately, and does nothing at all
    /// when no thread is running to read the flag.
    pub fn reconnect_now(&self) {
        self.nudge.store(true, Ordering::Relaxed);
    }

    /// Whether this app may speak. False until a sign-in lands.
    pub fn can_send(&self) -> bool {
        lock_creds(&self.creds).is_some()
    }

    /// The login we would speak as, for the composer to show. Never the token.
    pub fn speaking_as(&self) -> Option<String> {
        lock_creds(&self.creds).as_ref().map(|c| c.login.clone())
    }

    /// QUEUE A MESSAGE. Returns the reason it cannot go, in words the composer prints.
    ///
    /// IT QUEUES AND DOES NOT SEND, because the socket belongs to the reader thread and a UI
    /// thread writing to it would be two writers on one stream. The queue is drained by the
    /// session loop between reads, which is at most `read_slice` away.
    ///
    /// EVERY REFUSAL IS CHECKED HERE RATHER THAN DISCOVERED ON THE WIRE. Twitch drops an
    /// overlong message silently, which on screen looks exactly like a message that was sent and
    /// that nobody answered, so the length is refused where the text can still be edited. A
    /// newline is refused because IRC frames on CRLF: a message containing one is not one
    /// message, it is an injection of a second command, and this is the one place that can stop
    /// that.
    pub fn send(&self, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("nothing to send".to_owned());
        }
        if text.chars().count() > MAX_MESSAGE {
            return Err(format!(
                "that is {} characters and Twitch takes {MAX_MESSAGE}",
                text.chars().count()
            ));
        }
        if text.contains('\n') || text.contains('\r') {
            return Err("a message cannot contain a line break".to_owned());
        }
        if !self.can_send() {
            return Err("not signed in".to_owned());
        }
        self.outbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(text.to_owned());
        Ok(())
    }
}

fn lock_creds(m: &Mutex<Option<Creds>>) -> std::sync::MutexGuard<'_, Option<Creds>> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

static THREADS: AtomicUsize = AtomicUsize::new(0);

pub fn threads_started() -> usize {
    THREADS.load(Ordering::Relaxed)
}

/// WHO WE ARE ON THE WIRE, WHEN WE ARE ANYBODY.
///
/// Reading needs none of this: an anonymous `NICK justinfan<digits>` with no password is answered
/// and delivers everything. SENDING needs a real login and a token with `chat:edit`, which is
/// what `twitch_auth` gets by device code. Absent means anonymous, which is the normal state and
/// not a failure.
///
/// THE NICK IS NOT DECORATIVE AND CANNOT BE GUESSED. Twitch requires the `NICK` to be the login
/// the token belongs to; a mismatch is refused at the handshake. That is why `twitch_auth`
/// spends a call on `/oauth2/validate`: the token does not carry its owner's name in a form this
/// app may read, so it asks.
#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    pub login: String,
    token: String,
}

impl Creds {
    pub fn new(login: &str, token: &str) -> Creds {
        Creds {
            login: login.to_owned(),
            token: token.to_owned(),
        }
    }
}

/// REDACTED BY HAND, for the reason `twitch_auth::Tokens` is: one `{:?}` in a log line or a panic
/// message would write the thing that speaks as the owner into a file.
impl std::fmt::Debug for Creds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Creds")
            .field("login", &self.login)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// The longest message Twitch will take. Anything past this is rejected by the service, so it is
/// refused here with words instead, where the sender can still edit it.
pub const MAX_MESSAGE: usize = 500;

/// The handle the Chat screen holds.
///
/// IT DOES NOT IMPLEMENT `Default` AND THAT IS THE POINT. `Screens` derives `Default` and is built
/// whole at startup for every user whether or not the Chat row is ever visited, so anything that
/// opens a socket from a `Default` opens it on every launch forever, including for the people who
/// never look at chat. `ChatReader::idle()` is the constructor, it opens nothing, and `start` is
/// the only door to the network. `screens::chat` has a test that fails the day that changes.
pub struct ChatReader {
    shared: Arc<Mutex<Log>>,
    stop: Arc<AtomicBool>,
    /// "Stop waiting out the backoff and try now", for a Retry button. Same mechanism as
    /// `Watcher::refresh`.
    nudge: Arc<AtomicBool>,
    /// Whether `start` has already run. `swap` makes the check and the claim one operation, so two
    /// threads calling `start` in the same instant still produce one socket.
    started: AtomicBool,
    /// Who we are, when we are anybody. Read at DIAL time, not at start time, because the sign-in
    /// usually completes after the reader is already connected anonymously.
    creds: Arc<Mutex<Option<Creds>>>,
    /// Messages waiting to go out. Drained by the session loop, which is the only thread holding
    /// the socket.
    outbox: Arc<Mutex<VecDeque<String>>>,
    /// Set when the credentials change, to make the current session hang up and dial again so the
    /// handshake can carry them.
    redial: Arc<AtomicBool>,
}

impl ChatReader {
    /// A reader that has done nothing. No thread, no socket, no allocation worth naming.
    pub fn idle() -> ChatReader {
        ChatReader {
            shared: Arc::new(Mutex::new(Log::idle(""))),
            stop: Arc::new(AtomicBool::new(false)),
            nudge: Arc::new(AtomicBool::new(false)),
            started: AtomicBool::new(false),
            creds: Arc::new(Mutex::new(None)),
            outbox: Arc::new(Mutex::new(VecDeque::new())),
            redial: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Connect to `channel` and start reading. Idempotent: the second and every later call is a no
    /// operation, and a later call naming a DIFFERENT channel is refused out loud rather than
    /// quietly.
    ///
    /// WHY A LATER CALL IS NOT A CHANNEL SWITCH. The intended caller is the Chat screen's `ui`,
    /// which runs on every frame it is visible. If `start` reconnected whenever the channel it was
    /// handed differed from the one it was on, one frame with a momentarily wrong channel, and a
    /// settings field being edited is exactly that, would tear down a live socket and dial a new
    /// one, sixty times a second. Switching channels is `stop()` and a fresh `ChatReader`, and the
    /// refusal is written into the log so nobody has to guess why the wrong chat is on screen.
    pub fn start(&self, ctx: &egui::Context, channel: &str) {
        self.start_with(
            ctx,
            channel,
            Box::new(TlsWire::default()),
            Tuning::default(),
        )
    }

    /// The same reader over a caller supplied wire and caller chosen numbers. What the tests use to
    /// drive the real loop with the real captures and no network.
    pub fn start_with(
        &self,
        ctx: &egui::Context,
        channel: &str,
        mut wire: Box<dyn Wire>,
        tune: Tuning,
    ) {
        let want = normalise(channel);
        if self.started.swap(true, Ordering::SeqCst) {
            let mut lg = lock(&self.shared);
            if lg.channel != want {
                let refusal = format!(
                    "this reader is already on #{}; it will not be moved to #{} in place",
                    lg.channel, want
                );
                log::warn!("twitch chat: {refusal}");
                lg.error = Some(refusal);
            }
            return;
        }

        {
            let mut lg = lock(&self.shared);
            lg.channel = want.clone();
            lg.state = Conn::Connecting;
        }

        let shared = self.shared.clone();
        let stop = self.stop.clone();
        let nudge = self.nudge.clone();
        let creds = self.creds.clone();
        let outbox = self.outbox.clone();
        let redial = self.redial.clone();
        let ctx = ctx.clone();

        THREADS.fetch_add(1, Ordering::Relaxed);
        log::info!(
            "twitch chat: reading #{want} ({} reader thread(s) this run)",
            threads_started()
        );

        let spawned = thread::Builder::new()
            .name("twitch-chat".to_owned())
            .spawn(move || {
                let w = Wired {
                    shared: &shared,
                    stop: &stop,
                    nudge: &nudge,
                    creds: &creds,
                    outbox: &outbox,
                    redial: &redial,
                    ctx: &ctx,
                };
                run(&mut *wire, &w, &want, &tune);
            });
        if let Err(e) = spawned {
            /* No thread means no chat, ever. Say so, rather than leaving `Connecting` on screen to
             * look like a socket that has not answered yet. Same branch as watcher.rs:657. */
            let mut lg = lock(&self.shared);
            lg.state = Conn::Stopped;
            lg.error = Some(format!("the chat reader thread could not be started: {e}"));
        }
    }

    /// A read only view of this reader's log, for the windows that draw it.
    ///
    /// ONE IMPLEMENTATION OF `with_log`, NOT TWO. This type used to carry its own; it delegates
    /// now, so the lock discipline is written once and a screen holding a handle and a screen
    /// holding the reader cannot read the log two different ways.
    pub fn handle(&self) -> ChatHandle {
        ChatHandle {
            shared: self.shared.clone(),
            nudge: self.nudge.clone(),
            creds: self.creds.clone(),
            outbox: self.outbox.clone(),
        }
    }

    /// Read the log. See [`ChatHandle::with_log`], which this is.
    pub fn with_log<R>(&self, f: impl FnOnce(&Log) -> R) -> R {
        f(&lock(&self.shared))
    }

    /// HAND THE READER A SIGN-IN, AND MAKE IT DIAL AGAIN SO THE HANDSHAKE CARRIES IT.
    ///
    /// The reader is almost always already connected anonymously when this arrives: reading needs
    /// no account, so the socket comes up on the first frame of the Chat screen, and the sign-in
    /// completes whenever the owner finishes typing a code. IRC has no way to become somebody
    /// else on an open connection, so the only way to start speaking is to hang up and log in
    /// again, which costs the reader nothing (the log is kept across a reconnect on purpose).
    ///
    /// IDEMPOTENT ON THE SAME CREDENTIALS. `App::ui` hands these over on every frame once the
    /// sign-in lands, and a redial per frame would be a reconnect storm against Twitch.
    pub fn set_creds(&self, creds: Creds) {
        let mut g = lock_creds(&self.creds);
        if g.as_ref() == Some(&creds) {
            return;
        }
        *g = Some(creds);
        drop(g);
        self.redial.store(true, Ordering::Relaxed);
        self.nudge.store(true, Ordering::Relaxed);
    }

    /// Ask the thread to finish at its next wake. Does not block; see `Drop`.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// A READER HOLDING A LOG BUILT FROM RAW WIRE LINES, AND NO THREAD. Test only.
    ///
    /// Screens are drawn headless in this crate's tests to prove they paint what they claim, and
    /// the Chat screen cannot be drawn against anything real without either a socket or a log.
    /// This is the log: it runs the REAL `step` over the REAL capture lines, so what the screen
    /// paints in a test is what the wire produces, and it opens nothing.
    ///
    /// `started` IS CLAIMED, which is the part that matters for a test that draws through a `Cx`.
    /// A screen that asks for a connection sets `Cx::chat_wanted`, and if some future frame acted
    /// on that flag against this reader, `start` would find the claim already made and return
    /// without dialling. The guarantee is structural rather than a convention about test order.
    #[cfg(test)]
    pub fn planted(channel: &str, raw: &[&str]) -> ChatReader {
        let reader = ChatReader::idle();
        reader.started.store(true, Ordering::SeqCst);
        {
            let mut lg = lock(&reader.shared);
            lg.channel = normalise(channel);
            lg.state = Conn::Joined;
            lg.connects = 1;
            for line in raw {
                match step(line) {
                    Step::Keep(e) => lg.push(*e, LOG_CAP),
                    Step::Refused(why) => {
                        lg.refused += 1;
                        lg.last_refusal = Some(why);
                    }
                    _ => {}
                }
            }
        }
        reader
    }

    /// Put this reader into a state the screen has to draw differently. Test only.
    #[cfg(test)]
    pub fn set_state(&self, state: Conn) {
        lock(&self.shared).state = state;
    }
}

impl Drop for ChatReader {
    /// Sets the flag and does NOT join, exactly as `Watcher` does. The thread can be a quarter
    /// second into a read or a whole `CONNECT_TIMEOUT` into a dial, and the UI thread dropping a
    /// screen must not stall for either.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A poisoned mutex still holds a perfectly good `Log`; a panic on the reader thread must not take
/// the UI thread with it. watcher.rs:679.
fn lock(m: &Mutex<Log>) -> std::sync::MutexGuard<'_, Log> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `#Broken_Stoic` and `broken_stoic ` are the same room. Twitch logins are lowercase.
fn normalise(channel: &str) -> String {
    channel
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_ascii_lowercase()
}

/// A justinfan login. No password is sent and none is needed: both captures were taken with a bare
/// `NICK justinfan<digits>` and answered `:tmi.twitch.tv 001 justinfan41337 :Welcome, GLHF!`.
///
/// The number comes off the clock because this crate has no `rand` and does not need one: it only
/// has to avoid colliding with another anonymous reader from the same address, not be unguessable.
/// Five digits is what both captures used.
fn anon_nick() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("justinfan{}", 10_000 + (n % 90_000))
}

/* ------------------------------------------------------------ the repaint valve -- */

/// Turns "a line arrived" into "ask the UI to repaint, but not more often than `REPAINT_EVERY`".
///
/// THE STORM THIS EXISTS TO STOP IS MEASURED. #zackrawrr's worst single second in irc_busy.txt held
/// 38 messages, and a raid is worse than the worst second of an ordinary stream by an order of
/// magnitude. Each repaint lays out and paints the whole log. Coalescing at 100 ms caps the demand
/// at ten frames a second no matter how bad chat gets, and ten frames is above the rate at which a
/// person can read anything. The reader also drains a whole socket read before asking at all, so
/// two hundred lines that arrive in one packet cost one frame and not two hundred.
///
/// AND THE TRAILING LINE STILL ARRIVES, which is the half a naive rate limiter gets wrong. If the
/// last message of a burst falls inside a window that has already been spent, dropping the request
/// would leave that line unpainted until something else happened to wake the UI, which in a chat
/// that has just gone quiet could be minutes. `dirty` survives the refusal, and the loop calls
/// `due` on every tick, so the line lands one `READ_SLICE` later at worst.
/// `pub(crate)` AND NOT PRIVATE, BECAUSE A SECOND BURSTING SOURCE ARRIVED.
///
/// It was private while chat was the only thing in this app that produced events faster than a
/// person can read them. An artifact download is the second: `verify::copy_sealed` reports
/// progress once per 64 KiB, which for the measured 10,874,880 byte release binary is 166 events
/// from a thread that is not drawing. `updater::run` uses this one rather than writing a second
/// valve, because two copies of a rate limiter are two places for the trailing-event bug to be
/// fixed in only one of. It is still not exported from the crate.
pub(crate) struct Coalescer {
    dirty: bool,
    last: Instant,
}

impl Coalescer {
    pub(crate) fn new(now: Instant) -> Coalescer {
        Coalescer {
            dirty: false,
            /* Backdating by the window means the very first line is painted at once rather than a
             * tenth of a second after the connection came up. */
            last: now.checked_sub(REPAINT_EVERY).unwrap_or(now),
        }
    }

    pub(crate) fn dirtied(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn due(&mut self, now: Instant, every: Duration) -> bool {
        if self.dirty && now.duration_since(self.last) >= every {
            self.dirty = false;
            self.last = now;
            return true;
        }
        false
    }
}

/// Wake the UI. `request_repaint_of(ViewportId::ROOT)` and not `request_repaint()`, because this
/// runs on a thread that is inside no viewport's callback and the plain call would target whatever
/// viewport egui last had in hand. windows.rs:873 names the root the same way for the same reason.
fn wake(ctx: &egui::Context) {
    ctx.request_repaint_of(ViewportId::ROOT);
}

/* ---------------------------------------------------------------- the parsing -- */

/// What one line of the protocol means to this reader.
#[derive(Debug)]
enum Step {
    /// Something for the log. Boxed: an `Event` is much the largest variant and every other one
    /// would pay for it on the stack.
    Keep(Box<Event>),
    /// A PING. The payload is what must come back in the PONG.
    Ping(String),
    /// A ROOMSTATE's keys.
    Room(Box<RoomPatch>),
    /// `001`: the login was accepted.
    Welcome,
    /// `376`: the message of the day is over, which is where both captures show a client sending
    /// its JOIN.
    MotdEnd,
    /// Our own JOIN echo, or `366` End of NAMES. Either means we are in the room.
    InRoom,
    /// The server is about to restart and wants the client to come back.
    Reconnect,
    /// Understood, and nothing to show: the other numerics, CAP, PART, the NAMES list, and anybody
    /// else's JOIN.
    Skip,
    /// Not something this reader can read, with the reason. Counted, never silently dropped.
    Refused(String),
}

/// Split one line and decide what it is. The ONLY place in this file that touches the sibling
/// parsing pieces, which is deliberate: if their names move, three call sites move, all of them
/// inside this function.
fn step(raw: &str) -> Step {
    /* TWO REFUSALS OF THIS FILE'S OWN, BEFORE THE SPLITTER IS ASKED. A line that is all tags and no
     * command is what a torn write produces, since the tag blob is by far the longest part of a
     * tagged line and contains no spaces at all; and a line that splits to an empty command is not
     * addressed to anything. Deciding both here rather than relying on how strict the splitter
     * happens to be keeps "a truncated line is counted" a property of this file. */
    if raw.starts_with('@') && !raw.contains(' ') {
        return Step::Refused(format!("tags with no command: {}", clip(raw)));
    }
    let line = match split_line(raw) {
        Some(l) => l,
        None => return Step::Refused(format!("a line that is not IRC: {}", clip(raw))),
    };
    if line.command.is_empty() {
        return Step::Refused(format!("a line with no command: {}", clip(raw)));
    }
    /* AN UNTAGGED LINE FALLS BACK TO AN EMPTY MAP RATHER THAN BEING REFUSED. `TagMap::parse`
     * refuses an empty blob, and correctly: `@ COMMAND` is a sender that wrote the tag marker and
     * then no tags, which is a torn line. But `line.tags` is `None` on every PING and every server
     * numeric of the welcome burst, and turning those into refusals would count a healthy
     * connection's first six lines as corruption. See `TagMap::default`. */
    let tags = TagMap::parse(line.tags.unwrap_or("")).unwrap_or_default();
    let trailing = line.trailing.unwrap_or("");

    match line.command {
        "PING" => {
            /* ANSWER WITH WHAT WAS SENT, NOT WITH A CONSTANT. Both captures say
             * `PING :tmi.twitch.tv`, and a constant would pass every test written against them and
             * then fail the day Twitch puts a token in it. A client that misses a PONG is
             * disconnected, and that disconnection looks exactly like a channel that went quiet,
             * which is the failure this whole file is arranged to prevent. */
            let payload = if trailing.is_empty() {
                "tmi.twitch.tv"
            } else {
                trailing
            };
            Step::Ping(payload.to_owned())
        }
        "PRIVMSG" => {
            let mut e = match de_action(trailing) {
                Some(inner) => {
                    let mut e = Event::blank(Kind::Action);
                    e.body = inner.to_owned();
                    e
                }
                None => {
                    let kind = if tags.get("msg-id") == Some("highlighted-message") {
                        Kind::Highlight
                    } else {
                        Kind::Chat
                    };
                    let mut e = Event::blank(kind);
                    e.body = trailing.to_owned();
                    e
                }
            };
            /* THE EMOTE INDICES ARE AGAINST THE STRIPPED BODY, AND THAT IS MEASURED, NOT ASSUMED.
             * irc_busy.txt carries `emotes=305954156:76-83` on an ACTION whose trailing is
             * `\x01ACTION [Trivia] ... PogChamp\x01`. Characters 76 to 83 of the text WITHOUT the
             * eight character `\x01ACTION ` prefix are exactly "PogChamp"; with the prefix left on
             * they land eight characters early, in the middle of a word. Every emote in every /me
             * in the app would be drawn over the wrong text, and /me is 42 lines of the capture. So
             * the strip happens above and the spans are built from what is left. */
            e.spans = pieces(&e.body, tags.get("emotes").unwrap_or(""));
            e.who = who_of(&tags, line.prefix);
            e.color = hex_color(tags.get("color").unwrap_or(""));
            e.badges = tags.get("badges").unwrap_or("").to_owned();
            e.sent_ms = tags.get("tmi-sent-ts").and_then(|s| s.parse::<i64>().ok());
            Step::Keep(Box::new(e))
        }
        "USERNOTICE" => {
            let mut e = Event::blank(Kind::Notice);
            e.body = trailing.to_owned();
            e.spans = pieces(&e.body, tags.get("emotes").unwrap_or(""));
            e.who = who_of(&tags, line.prefix);
            e.color = hex_color(tags.get("color").unwrap_or(""));
            e.badges = tags.get("badges").unwrap_or("").to_owned();
            e.sent_ms = tags.get("tmi-sent-ts").and_then(|s| s.parse::<i64>().ok());
            /* `system-msg` arrives escaped, and EMPTY on an announcement, which is why the empty
             * case becomes `None` rather than `Some("")`: a `Some("")` would draw a blank line
             * above the announcement's real text. */
            e.notice = match tags.get("system-msg") {
                Some(s) if !s.is_empty() => Some(unescape_tag(s)),
                _ => None,
            };
            Step::Keep(Box::new(e))
        }
        "CLEARCHAT" => {
            let mut e = Event::blank(Kind::Cleared);
            /* The target login is the trailing parameter. No trailing at all is the whole room
             * being wiped, which no line of either capture shows, so `who` is left empty and the
             * screen has to say "the chat was cleared" for that case rather than name nobody. */
            e.who = trailing.to_owned();
            e.secs = tags.get("ban-duration").and_then(|s| s.parse::<u32>().ok());
            e.sent_ms = tags.get("tmi-sent-ts").and_then(|s| s.parse::<i64>().ok());
            Step::Keep(Box::new(e))
        }
        "NOTICE" => {
            let mut e = Event::blank(Kind::Server);
            e.body = trailing.to_owned();
            Step::Keep(Box::new(e))
        }
        "ROOMSTATE" => Step::Room(Box::new(RoomPatch {
            emote_only: tags.get("emote-only").map(|v| v == "1"),
            followers_only: tags
                .get("followers-only")
                .and_then(|v| v.parse::<i64>().ok()),
            r9k: tags.get("r9k").map(|v| v == "1"),
            slow: tags.get("slow").and_then(|v| v.parse::<u32>().ok()),
            subs_only: tags.get("subs-only").map(|v| v == "1"),
        })),
        "RECONNECT" => Step::Reconnect,
        "001" => Step::Welcome,
        "376" => Step::MotdEnd,
        /* `366`, the end of the NAMES list, only arrives after a JOIN has been accepted, so it
         * confirms the room as surely as the JOIN echo does. `376` does NOT: it only says the
         * greeting is over, which is when the JOIN goes out. Two numerics, two meanings, kept
         * apart, because a reader that treated 376 as "in the room" would clear the error and start
         * the idle clock before it had actually joined anything. */
        "366" | "JOIN" => Step::InRoom,
        _ => Step::Skip,
    }
}

/// The name to draw. `display-name` is the sender's own capitalisation and is present on every
/// tagged line in both captures; `login` and then the nick out of the prefix are what is left when
/// it is not, and an empty string is better than the word "unknown", which a viewer would read as a
/// username.
fn who_of(tags: &TagMap<'_>, prefix: Option<&str>) -> String {
    match tags.get("display-name") {
        Some(d) if !d.is_empty() => d.to_owned(),
        _ => match tags.get("login") {
            Some(l) if !l.is_empty() => l.to_owned(),
            _ => nick_of(prefix.unwrap_or("")).to_owned(),
        },
    }
}

/// The nick out of an IRC prefix. `hd_dean!hd_dean@hd_dean.tmi.twitch.tv` is `hd_dean`, and
/// `tmi.twitch.tv`, which is the prefix on every server sent line in both captures, has no nick at
/// all and must not be drawn as one.
fn nick_of(prefix: &str) -> &str {
    match prefix.find('!') {
        Some(i) => &prefix[..i],
        None => "",
    }
}

/// Strip the `\x01ACTION ... \x01` envelope `/me` sends, or say this was not one.
///
/// The closing `\x01` is optional here on purpose: a message truncated at Twitch's 500 character
/// limit loses it, and a `/me` with no terminator is still a `/me`.
fn de_action(body: &str) -> Option<&str> {
    let inner = body.strip_prefix("\u{1}ACTION ")?;
    Some(inner.strip_suffix('\u{1}').unwrap_or(inner))
}

/// `#FF0000` to a colour. `None` for the empty tag, which is what the server sends for a user who
/// never picked one, and for anything that is not six hex digits behind a hash.
///
/// AN EMPTY `color=` IS NOT BLACK. Broken Stoic's own line in irc_anon.txt carries `color=;`, and a
/// reader that parsed that as 0x000000 would paint the broadcaster's name invisible on this app's
/// near black background. `None` means "the screen chooses", which is what Twitch's own client does
/// with it.
fn hex_color(tag: &str) -> Option<Color32> {
    let hex = tag.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| -> Option<u8> { u8::from_str_radix(hex.get(at..at + 2)?, 16).ok() };
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// Undo IRCv3 tag escaping: `\s` is a space, `\:` a semicolon, `\r`, `\n` and `\\` themselves.
///
/// WHAT THE CAPTURE ACTUALLY EXERCISES. 215 lines of irc_busy.txt contain `\s` and NOT ONE contains
/// `\:`, which is expected: a semicolon inside a tag value is rare and a display name cannot hold
/// one. The other four escapes are implemented from the grammar and are not proved by any byte on
/// disk, which is said here rather than left to look measured. A trailing lone backslash is
/// dropped, which is what the grammar says to do with it.
fn unescape_tag(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some(':') => out.push(';'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// For a refusal message: enough of the line to recognise it and no more, because a refusal is
/// logged and a 16 KiB line in the log is its own problem.
fn clip(s: &str) -> String {
    let mut out: String = s.chars().take(120).collect();
    if out.len() < s.len() {
        out.push_str("...");
    }
    out
}

/* -------------------------------------------------------------------- the loop -- */

/// Everything a session needs that is shared with the rest of the app. Bundled because it grew
/// past the point where seven positional parameters could be read at a call site without
/// counting them, and because `run` and `session` want the same set.
pub(crate) struct Wired<'a> {
    pub shared: &'a Arc<Mutex<Log>>,
    pub stop: &'a AtomicBool,
    pub nudge: &'a AtomicBool,
    pub creds: &'a Mutex<Option<Creds>>,
    pub outbox: &'a Mutex<VecDeque<String>>,
    pub redial: &'a AtomicBool,
    pub ctx: &'a egui::Context,
}

/// The reconnect loop. Runs until `stop`.
fn run(wire: &mut dyn Wire, w: &Wired<'_>, channel: &str, tune: &Tuning) {
    let (shared, stop, nudge, ctx) = (w.shared, w.stop, w.nudge, w.ctx);
    let mut attempt: u32 = 0;
    while !stop.load(Ordering::Relaxed) {
        {
            let mut lg = lock(shared);
            lg.state = Conn::Connecting;
        }
        wake(ctx);

        let outcome = session(wire, w, channel, tune);
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let why = match outcome {
            /* `session` returns `Ok` only when `stop` was set, and that was just checked, so this
             * arm is a session that ended with no reason attached. Reporting it as a failure with
             * vague words still beats looping in silence. */
            Ok(()) => "the session ended".to_owned(),
            Err(e) => e,
        };
        attempt = attempt.saturating_add(1);
        let wait = backoff(&tune.backoff, attempt);
        {
            let mut lg = lock(shared);
            /* THE LINES STAY. A reconnect does not make what was already said untrue, and a chat
             * that empties itself every time the wifi hiccups is a chat nobody trusts. Only the
             * state and the reason move. */
            lg.state = Conn::Retrying {
                attempt,
                next_try_in: wait,
            };
            lg.error = Some(why.clone());
        }
        log::info!("twitch chat: #{channel} disconnected ({why}); retrying in {wait:?}");
        wake(ctx);

        if !nap(wait, stop, nudge, tune.read_slice) {
            break;
        }
    }

    let mut lg = lock(shared);
    lg.state = Conn::Stopped;
    drop(lg);
    wake(ctx);
}

/// The ladder, flattening at its last rung. `attempt` counts from one.
fn backoff(ladder: &[Duration], attempt: u32) -> Duration {
    if ladder.is_empty() {
        return Duration::from_secs(1);
    }
    let i = (attempt.max(1) as usize - 1).min(ladder.len() - 1);
    ladder.get(i).copied().unwrap_or(Duration::from_secs(1))
}

/// Sleep in slices so a stop lands within one slice rather than at the end of a minute long
/// backoff. Returns false when the caller should stop. watcher.rs:645-657, and the `nudge` is what
/// a "Try now" button pulls.
fn nap(how_long: Duration, stop: &AtomicBool, nudge: &AtomicBool, slice: Duration) -> bool {
    let deadline = Instant::now() + how_long;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        if nudge.swap(false, Ordering::Relaxed) {
            return true;
        }
        thread::sleep(slice.min(READ_SLICE));
    }
    true
}

/// One connection, from dial to death. `Ok(())` means `stop` was set; every other ending is an
/// `Err` with words a person can read, because that string is what the screen shows.
fn session(wire: &mut dyn Wire, w: &Wired<'_>, channel: &str, tune: &Tuning) -> Result<(), String> {
    let (shared, stop, ctx) = (w.shared, w.stop, w.ctx);
    let mut pipe = wire.dial()?;

    /* WHO WE ARE, DECIDED AT DIAL TIME AND NOT AT START TIME. The reader connects anonymously on
     * the Chat screen's first frame and the sign-in lands minutes later, so reading this here is
     * what lets the redial pick it up. See `ChatReader::set_creds`. */
    let creds = lock_creds(w.creds).clone();
    w.redial.store(false, Ordering::Relaxed);

    /* THE ORDER IS THE CAPTURE'S ORDER. Capabilities first, so the very first PRIVMSG is already
     * tagged; the login second; and the JOIN is NOT sent here. It waits for the end of the
     * message of the day below, because that is where both captures show a client sending it.
     *
     * PASS GOES BEFORE NICK AND THAT IS NOT A STYLE CHOICE: IRC authenticates on the password
     * that arrived before the nick, and a NICK sent first is registered anonymously and cannot be
     * upgraded on the same connection.
     *
     * THE TOKEN IS NEVER LOGGED, and the only place it is formatted is this line. There is no
     * `log::debug` of the handshake anywhere in this file for exactly that reason. */
    pipe.send(CAP_REQ)
        .map_err(|e| format!("the capability request could not be sent: {e}"))?;
    let nick = match &creds {
        Some(c) => {
            pipe.send(&format!("PASS oauth:{}", c.token))
                .map_err(|e| format!("the sign-in could not be sent: {e}"))?;
            c.login.clone()
        }
        None => anon_nick(),
    };
    pipe.send(&format!("NICK {nick}"))
        .map_err(|e| format!("the login could not be sent: {e}"))?;

    let mut pending: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut buf = [0u8; 8192];
    let opened = Instant::now();
    let mut last_byte = Instant::now();
    let mut coalesce = Coalescer::new(Instant::now());
    let mut welcomed = false;
    let mut sent_join = false;
    let mut in_room = false;

    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }

        /* A SIGN-IN LANDED WHILE THIS SOCKET WAS OPEN. IRC cannot become somebody else in place,
         * so the only way to start speaking is to hang up and log in again. `run` reconnects
         * immediately and keeps the lines that are already on screen. */
        if w.redial.load(Ordering::Relaxed) {
            return Err("signing in".to_owned());
        }

        /* WHAT THE COMPOSER QUEUED. This thread owns the socket, so it is the only thread that
         * may write to it; the UI pushes onto the outbox and this drains it. Only when logged in:
         * an anonymous connection is refused by Twitch with a NOTICE, and queueing a message the
         * moment before a sign-in completes must not throw it away, so it waits for the redial. */
        if in_room && creds.is_some() {
            loop {
                let next = w
                    .outbox
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pop_front();
                let Some(text) = next else { break };
                if let Err(e) = pipe.send(&format!("PRIVMSG #{channel} :{text}")) {
                    /* PUT IT BACK. The socket is going; `run` will redial and this message goes
                     * out on the next one rather than vanishing with no trace, which is what a
                     * reader typing into a chat during a wifi hiccup would otherwise get. */
                    w.outbox
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push_front(text);
                    return Err(format!("the message could not be sent: {e}"));
                }
                /* IT WENT OUT, SO PUT IT ON SCREEN OURSELVES.
                 *
                 * TWITCH DOES NOT ECHO YOUR OWN MESSAGE BACK TO YOU. It delivers a PRIVMSG to
                 * everybody else in the room and answers the sender with a USERSTATE, which
                 * carries the badges and colour the message was sent with and NOT the message.
                 * IRCv3 has an `echo-message` capability for exactly this and Twitch does not
                 * implement it, so a client that only draws what arrives shows every message in
                 * the room except its own. The owner saw precisely that: the line reached real
                 * Twitch and never appeared in this app.
                 *
                 * AFTER THE SEND AND NOT BEFORE, so a line that never left is never drawn as
                 * though it had. On a failure the arm above puts it back in the outbox and this
                 * is not reached, which is what stops a message appearing twice when the redial
                 * sends it for real.
                 *
                 * THE SPANS ARE CUT THE SAME WAY EVERY OTHER LINE IS, with an empty `emotes` tag:
                 * this app cannot know which words Twitch will resolve to emotes for us, so ours
                 * draw as the text we typed. That is what the other side sees for an emote this
                 * app has no image for anyway.
                 *
                 * NO `sent_ms`: that field is the SERVER`s clock and there is no server reply to
                 * take one from. Inventing a local one would put a number in a field documented
                 * as Twitch`s own. */
                if let Some(me) = &creds {
                    let mut e = Event::blank(Kind::Chat);
                    e.who = me.login.clone();
                    e.spans = pieces(&text, "");
                    e.body = text;
                    let mut lg = lock(shared);
                    lg.push(e, tune.log_cap);
                    drop(lg);
                    wake(ctx);
                }
            }
        }

        let n = match pipe.read(&mut buf) {
            Ok(0) => return Err("the server closed the connection".to_owned()),
            Ok(n) => n,
            Err(e) if quiet(&e) => 0,
            Err(e) => return Err(format!("the connection failed while reading: {e}")),
        };
        if n > 0 {
            last_byte = Instant::now();
            pending.extend_from_slice(&buf[..n]);
        } else {
            /* The socket's own read timeout is what paces this loop, so this sleep normally costs
             * nothing that matters. It is here as a floor: a `Duplex` whose read returns at once
             * with nothing, which is what a socket with no timeout set on it does, would otherwise
             * spin this thread at one hundred percent of a core with no symptom but a hot laptop. */
            thread::sleep(tune.read_slice / 8);
        }

        /* THE TWO DEADLINES, AND BOTH OF THEM EXIST TO TURN SILENCE INTO WORDS.
         *
         * A socket that connects and then never completes the login is the shape a captive portal
         * and a hung proxy both take, and without the first deadline this reader sits on
         * "Connecting" forever. A socket that joins and then goes silent is the shape a half open
         * TCP connection takes after a NAT has forgotten it, and without the second one this reader
         * sits on "Joined" forever showing a chat that stopped hours ago. Both end as an `Err`,
         * which is to say as something the viewer can see. */
        if !in_room && opened.elapsed() > tune.handshake_limit {
            let stage = if !welcomed {
                "the welcome"
            } else if !sent_join {
                "the end of the message of the day"
            } else {
                "the join"
            };
            return Err(format!(
                "connected, but {stage} never arrived within {}s",
                tune.handshake_limit.as_secs()
            ));
        }
        if in_room && last_byte.elapsed() > tune.idle_limit {
            return Err(format!(
                "the connection went silent for {}s, which is longer than Twitch's own four and \
                 a half minute ping, so this is not a quiet channel",
                tune.idle_limit.as_secs()
            ));
        }

        /* DRAIN EVERY COMPLETE LINE AND KEEP THE REMAINDER. A TCP read boundary falls wherever it
         * likes: a five kilobyte read can end in the middle of a tag blob, and the next line's
         * first half shares a read with the previous line's second half. Anything that parses `buf`
         * per read rather than per newline loses a message on every boundary, and the loss is
         * proportional to how busy the channel is, so it looks like a working client on a quiet
         * stream and eats a tenth of a raid. `framing_survives_any_read_boundary` holds this. */
        while let Some(cut) = pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=cut).collect();
            let text = match std::str::from_utf8(&raw) {
                Ok(t) => t.trim_end_matches('\n').trim_end_matches('\r'),
                Err(e) => {
                    /* Twitch validates UTF-8 on the way in, so this should be unreachable, which is
                     * exactly why it is counted rather than assumed away. */
                    let mut lg = lock(shared);
                    lg.refused = lg.refused.saturating_add(1);
                    lg.last_refusal = Some(format!("a line that is not UTF-8: {e}"));
                    continue;
                }
            };
            if text.is_empty() {
                continue;
            }

            match step(text) {
                Step::Keep(e) => {
                    let mut lg = lock(shared);
                    lg.push(*e, tune.log_cap);
                    drop(lg);
                    coalesce.dirtied();
                }
                Step::Ping(payload) => {
                    /* A CLIENT THAT DOES NOT PONG IS DISCONNECTED, and that disconnection arrives
                     * as a socket that has simply stopped speaking, which is indistinguishable from
                     * a channel where nobody is talking. This one line is the difference. */
                    pipe.send(&format!("PONG :{payload}"))
                        .map_err(|e| format!("the keepalive could not be sent: {e}"))?;
                }
                Step::Room(patch) => {
                    let mut lg = lock(shared);
                    let mut room = lg.room.clone().unwrap_or_else(Room::unknown);
                    room.apply(&patch);
                    lg.room = Some(room);
                    drop(lg);
                    coalesce.dirtied();
                }
                Step::Welcome => welcomed = true,
                Step::MotdEnd => {
                    if !sent_join {
                        pipe.send(&format!("JOIN #{channel}"))
                            .map_err(|e| format!("the channel could not be joined: {e}"))?;
                        sent_join = true;
                    }
                }
                Step::InRoom => {
                    if !in_room {
                        in_room = true;
                        let mut lg = lock(shared);
                        lg.state = Conn::Joined;
                        /* THE ERROR IS CLEARED ONLY HERE, at the moment there is a working socket
                         * to replace it. Clearing it at dial time would blank the reason on screen
                         * for every attempt of a backoff that is failing, which is exactly when a
                         * viewer is reading it. */
                        lg.error = None;
                        lg.connects = lg.connects.saturating_add(1);
                        drop(lg);
                        log::info!("twitch chat: joined #{channel} as {nick}");
                        wake(ctx);
                    }
                }
                Step::Reconnect => {
                    /* Twitch says this before it restarts a chat server. It is in NEITHER capture,
                     * so this arm is written from the protocol and is not proved by any byte on
                     * disk. Ending the session hands it to the backoff ladder, which is the
                     * behaviour that was wanted anyway. */
                    return Err("the server asked the client to reconnect".to_owned());
                }
                Step::Skip => {}
                Step::Refused(why) => {
                    let mut lg = lock(shared);
                    lg.refused = lg.refused.saturating_add(1);
                    lg.last_refusal = Some(why);
                }
            }
        }

        if pending.len() > tune.max_line {
            /* A peer that has sent more than MAX_LINE with no newline in it is not speaking IRC.
             * Ending the session throws the buffer away with it, which is the point: the
             * alternative is a Vec that grows until the process dies. */
            return Err(format!(
                "the server sent {} bytes with no end of line in them",
                pending.len()
            ));
        }

        if coalesce.due(Instant::now(), tune.repaint_every) {
            wake(ctx);
        }
    }
}

/* ------------------------------------------------------------------ the tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /* THE FIXTURES ARE REAL BYTES. Every line below is verbatim from a capture taken against
     * irc.chat.twitch.tv on 2026-09-04 with an anonymous login. Nothing here was typed by hand,
     * which matters: a hand written IRC fixture is where a parser goes to pass while production
     * fails, because the person writing it writes the shape they already believe in. The two files
     * are irc_anon.txt (3.4 KB, one channel, the whole handshake) and irc_busy.txt (2.5 MB, seven
     * channels, 6094 lines, 38 messages in its worst single second). */

    /// irc_anon.txt in full, all 23 lines, in order. One channel, the complete handshake, a
    /// ROOMSTATE, two PINGs, an announcement USERNOTICE and four PRIVMSGs.
    const ANON: &[&str] = &[
        ":tmi.twitch.tv CAP * ACK :twitch.tv/tags twitch.tv/commands twitch.tv/membership",
        ":tmi.twitch.tv 001 justinfan73921 :Welcome, GLHF!",
        ":tmi.twitch.tv 002 justinfan73921 :Your host is tmi.twitch.tv",
        ":tmi.twitch.tv 003 justinfan73921 :This server is rather new",
        ":tmi.twitch.tv 004 justinfan73921 :-",
        ":tmi.twitch.tv 375 justinfan73921 :-",
        ":tmi.twitch.tv 372 justinfan73921 :You are in a maze of twisty passages, all alike.",
        ":tmi.twitch.tv 376 justinfan73921 :>",
        ":justinfan73921!justinfan73921@justinfan73921.tmi.twitch.tv JOIN #broken_stoic",
        ":justinfan73921.tmi.twitch.tv 353 justinfan73921 = #broken_stoic :justinfan73921",
        ":justinfan73921.tmi.twitch.tv 366 justinfan73921 #broken_stoic :End of /NAMES list",
        "@emote-only=0;followers-only=-1;r9k=0;room-id=29737511;slow=0;subs-only=0 :tmi.twitch.tv ROOMSTATE #broken_stoic",
        ":groundofacesstaging!groundofacesstaging@groundofacesstaging.tmi.twitch.tv JOIN #broken_stoic",
        ":streamelements!streamelements@streamelements.tmi.twitch.tv JOIN #broken_stoic",
        ":broken_stoic!broken_stoic@broken_stoic.tmi.twitch.tv JOIN #broken_stoic",
        ":classy_viking!classy_viking@classy_viking.tmi.twitch.tv JOIN #broken_stoic",
        "PING :tmi.twitch.tv",
        "@badge-info=subscriber/15;badges=subscriber/12,campaign-29737511-6496c7fd-09fc-47fb-9442-b99933723e28-mw/1;client-nonce=b2bb7aa898314e2b9a3b3aee169d5a3f;color=#FF0000;display-name=hd_dean;emotes=;first-msg=0;flags=;id=0a6cfab6-8f57-4266-b4d6-f41e8db4adc8;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555846726;turbo=0;user-id=1164259347;user-type= :hd_dean!hd_dean@hd_dean.tmi.twitch.tv PRIVMSG #broken_stoic :first",
        "@badge-info=subscriber/43;badges=broadcaster/1,subscriber/3042,partner/1;color=;display-name=Broken_Stoic;emotes=160392:84-91;flags=;id=4c6584ee-15f9-4544-a9f8-549ce15e1d7c;login=broken_stoic;mod=0;msg-id=announcement;msg-param-color=PURPLE;room-id=29737511;subscriber=1;system-msg=;tmi-sent-ts=1788555867322;user-id=29737511;user-type=;vip=0 :tmi.twitch.tv USERNOTICE #broken_stoic :180sec ad break starting. Thank you for sticking around and supporting the channel! ThankEgg",
        "PING :tmi.twitch.tv",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;client-nonce=7a5826cad06240aea58760bafb5f82f7;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=1;flags=;id=d9e144f5-8b29-416b-93ff-687df24718b5;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555941831;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :yo bro",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;client-nonce=1beec3dab1eb45f493df60530d68587f;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=0;flags=;id=ad942ff6-14a1-46d9-9d73-397065ad1146;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555974608;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :its quick today brother",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;client-nonce=9b5c7fedaa3f40e3a02237e045aa4839;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=0;flags=;id=27afe641-61e3-42bb-bca3-c8b03897aca6;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555977928;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :get your bee from shop",
    ];

    /// The five log worthy lines in ANON: four PRIVMSG and one USERNOTICE. Everything else is the
    /// handshake, a ROOMSTATE, two PINGs, the NAMES list and five JOINs.
    const ANON_EVENTS: usize = 5;

    /// A CLEARCHAT from irc_busy.txt: a 300 second timeout in #zackrawrr.
    const BUSY_CLEARCHAT: &str = "@ban-duration=300;room-id=552120296;target-user-id=438259749;tmi-sent-ts=1788555614579 :tmi.twitch.tv CLEARCHAT #zackrawrr :koishi_mm";

    /// A `/me` from irc_busy.txt with an emote in it. `\u{1}` is the literal 0x01 byte the capture
    /// holds; the rest is verbatim. `emotes=305954156:76-83` is the number that matters.
    const BUSY_ACTION: &str = "@badge-info=;badges=vip/1,glhf-pledge/1;color=#5F9EA0;display-name=ThePositiveBot;emotes=305954156:76-83;first-msg=0;flags=;id=47950aad-783d-4435-9895-186459f59b0f;mod=0;returning-chatter=0;room-id=71092938;subscriber=0;tmi-sent-ts=1788555453509;turbo=0;user-id=425363834;user-type=;vip=1 :thepositivebot!thepositivebot@thepositivebot.tmi.twitch.tv PRIVMSG #xqc :\u{1}ACTION [Trivia] aintnoway_deadass, you got the answer right! It was \"Led Zeppelin\" PogChamp\u{1}";

    /// A resub from irc_busy.txt with NO trailing parameter. 27 of the capture's 62 USERNOTICEs
    /// have this shape.
    const BUSY_RESUB_NO_BODY: &str = "@badge-info=subscriber/6;badges=subscriber/6,no_audio/1;color=#0000FF;display-name=wanenri;emotes=;flags=;id=47834a15-c654-4b16-91a8-b1ab9ebab277;login=wanenri;mod=0;msg-id=resub;msg-param-cumulative-months=6;msg-param-months=0;msg-param-multimonth-duration=1;msg-param-multimonth-tenure=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\\sSubscription\\s(jynxzi);msg-param-sub-plan=Prime;msg-param-was-gifted=false;room-id=411377640;subscriber=1;system-msg=wanenri\\ssubscribed\\swith\\sPrime.\\sThey've\\ssubscribed\\sfor\\s6\\smonths!;tmi-sent-ts=1788555459216;user-id=412419058;user-type=;vip=0 :tmi.twitch.tv USERNOTICE #jynxzi";

    /// A channel points highlighted message from irc_busy.txt.
    const BUSY_HIGHLIGHT: &str = "@badge-info=;badges=pichu/1;color=#DAA520;display-name=daemonlupusrex312;emotes=;first-msg=0;flags=;id=a8a43dba-ae58-4fbd-b437-0851f605325b;mod=0;msg-id=highlighted-message;returning-chatter=0;room-id=552120296;subscriber=0;tmi-sent-ts=1788555502056;turbo=0;user-id=207168990;user-type= :daemonlupusrex312!daemonlupusrex312@daemonlupusrex312.tmi.twitch.tv PRIVMSG #zackrawrr :Because ramming planes into a civilian tower vs bombing military bases is equivalent what a clown";

    /// A ROOMSTATE from irc_busy.txt with followers only actually on: 15 minutes, in #jynxzi.
    const BUSY_ROOMSTATE: &str = "@emote-only=0;followers-only=15;r9k=0;room-id=411377640;slow=0;subs-only=0 :tmi.twitch.tv ROOMSTATE #jynxzi";

    /// Twenty five consecutive `tmi-sent-ts` values from #zackrawrr in irc_busy.txt, in the order
    /// they arrived. Note the pair at index 22 and 23: 1788555473902 comes off the wire BEFORE
    /// 1788555473897, which is why the log keeps arrival order and never sorts by this number.
    const ZACK_BURST_MS: [i64; 25] = [
        1788555470389,
        1788555470622,
        1788555470693,
        1788555471038,
        1788555471100,
        1788555471239,
        1788555471522,
        1788555471592,
        1788555471847,
        1788555472112,
        1788555472422,
        1788555472500,
        1788555472588,
        1788555472652,
        1788555472812,
        1788555473083,
        1788555473274,
        1788555473291,
        1788555473488,
        1788555473675,
        1788555473757,
        1788555473885,
        1788555473902,
        1788555473897,
        1788555474092,
    ];

    fn wire_bytes(lines: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for l in lines {
            v.extend_from_slice(l.as_bytes());
            v.extend_from_slice(b"\r\n");
        }
        v
    }

    /// What the tape does once it has played out.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum End {
        /// The peer closes: `Ok(0)`.
        Close,
        /// The peer stays connected and says nothing, which is the half open socket and the captive
        /// portal both.
        Quiet,
    }

    struct Tape {
        bytes: Vec<u8>,
        at: usize,
        chunk: usize,
        end: End,
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl Duplex for Tape {
        fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
            if self.at >= self.bytes.len() {
                return match self.end {
                    End::Close => Ok(0),
                    End::Quiet => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "nothing to read",
                    )),
                };
            }
            let n = self.chunk.min(into.len()).min(self.bytes.len() - self.at);
            into[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        }
        fn send(&mut self, line: &str) -> std::io::Result<()> {
            let mut g = self.sent.lock().unwrap_or_else(|p| p.into_inner());
            g.push(line.to_owned());
            Ok(())
        }
    }

    struct Replay {
        takes: VecDeque<Vec<u8>>,
        chunk: usize,
        end: End,
        sent: Arc<Mutex<Vec<String>>>,
        dials: Arc<AtomicUsize>,
        /// Flipped when the script runs out, so a test that drives `run` ends on a fact rather than
        /// on a timer.
        exhausted: Arc<AtomicBool>,
    }

    impl Wire for Replay {
        fn dial(&mut self) -> Result<Box<dyn Duplex>, String> {
            self.dials.fetch_add(1, Ordering::Relaxed);
            match self.takes.pop_front() {
                Some(bytes) => Ok(Box::new(Tape {
                    bytes,
                    at: 0,
                    chunk: self.chunk,
                    end: self.end,
                    sent: self.sent.clone(),
                })),
                None => {
                    self.exhausted.store(true, Ordering::Relaxed);
                    Err("the script has no more connections in it".to_owned())
                }
            }
        }
    }

    fn fast() -> Tuning {
        Tuning {
            log_cap: LOG_CAP,
            read_slice: Duration::from_millis(1),
            idle_limit: Duration::from_millis(150),
            handshake_limit: Duration::from_millis(150),
            repaint_every: Duration::from_millis(0),
            max_line: MAX_LINE,
            backoff: vec![Duration::from_millis(1)],
        }
    }

    /// A SIGNED IN SESSION THAT SENDS ONE MESSAGE, and everything it did.
    ///
    /// The anonymous `play` above cannot reach any of this: sending is gated on credentials, so a
    /// harness with none can never exercise the outbox, the PRIVMSG or the echo.
    fn play_signed_in(lines: &[&str], say: &str, tune: &Tuning) -> (Log, Vec<String>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut wire = Replay {
            takes: VecDeque::from(vec![wire_bytes(lines)]),
            chunk: 4096,
            end: End::Close,
            sent: sent.clone(),
            dials: Arc::new(AtomicUsize::new(0)),
            exhausted: Arc::new(AtomicBool::new(false)),
        };
        let shared = Arc::new(Mutex::new(Log::idle("broken_stoic")));
        let stop = AtomicBool::new(false);
        let ctx = egui::Context::default();
        let creds = Mutex::new(Some(Creds::new("reviird", "NOT-A-REAL-TOKEN")));
        let outbox = Mutex::new(VecDeque::from(vec![say.to_owned()]));
        let redial = AtomicBool::new(false);
        let nudge = AtomicBool::new(false);
        let w = Wired {
            shared: &shared,
            stop: &stop,
            nudge: &nudge,
            creds: &creds,
            outbox: &outbox,
            redial: &redial,
            ctx: &ctx,
        };
        let _ = session(&mut wire, &w, "broken_stoic", tune);
        let log = lock(&shared).clone();
        let wrote = sent.lock().unwrap_or_else(|p| p.into_inner()).clone();
        (log, wrote)
    }

    /// A SIGNED IN HANDSHAKE SENDS THE PASSWORD BEFORE THE NICK, AND THE MESSAGE WE SEND COMES
    /// BACK ONTO OUR OWN SCREEN.
    ///
    /// TWITCH DOES NOT ECHO YOUR OWN PRIVMSG. It delivers the line to everybody else and answers
    /// the sender with a USERSTATE, which carries the badges the message was sent with and not the
    /// message. IRCv3's `echo-message` capability exists for exactly this and Twitch does not
    /// implement it, so a client that draws only what arrives shows every message in the room
    /// except its own. The owner hit this: the line reached real Twitch and never appeared here.
    ///
    /// PASS BEFORE NICK IS NOT STYLE. IRC registers the connection on the password that arrived
    /// before the nick; a NICK sent first is registered anonymously and cannot be upgraded on the
    /// same socket, so the order is load bearing and asserted by index.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the echo block after the send, or swapping the two
    /// handshake lines.
    #[test]
    fn a_signed_in_session_logs_in_then_speaks_and_shows_its_own_words() {
        let (log, wrote) = play_signed_in(ANON, "hello from the grimoire", &fast());

        let pass = wrote
            .iter()
            .position(|l| l.starts_with("PASS "))
            .expect("a signed in session must send a password");
        let nick = wrote
            .iter()
            .position(|l| l.starts_with("NICK "))
            .expect("every session sends a nick");
        assert!(
            pass < nick,
            "NICK went out before PASS, so Twitch registers this connection anonymously and it \
             can never speak: {wrote:?}"
        );
        assert_eq!(
            wrote[nick], "NICK reviird",
            "the nick must be the login the token belongs to; Twitch refuses a mismatch"
        );
        assert!(
            wrote
                .iter()
                .any(|l| l == "PRIVMSG #broken_stoic :hello from the grimoire"),
            "the queued message never went on the wire: {wrote:?}"
        );

        /* AND IT IS ON OUR OWN SCREEN. */
        let mine: Vec<&Event> = log.lines.iter().filter(|e| e.who == "reviird").collect();
        assert_eq!(
            mine.len(),
            1,
            "our own message is missing from our own log. Twitch never echoes it back, so the \
             app has to put it there: {:?}",
            log.lines
                .iter()
                .map(|e| (&e.who, &e.body))
                .collect::<Vec<_>>()
        );
        assert_eq!(mine[0].body, "hello from the grimoire");
        assert!(
            !mine[0].spans.is_empty(),
            "our own line was not cut into pieces, so it would draw blank beside every other line"
        );
        assert_eq!(
            mine[0].sent_ms, None,
            "`sent_ms` is the SERVER's clock and there is no server reply to take one from; \
             inventing a local number would put a lie in a documented field"
        );
    }

    /// A MESSAGE THAT NEVER LEFT IS NEVER DRAWN AS THOUGH IT HAD, AND IS NOT LOST EITHER.
    ///
    /// The echo goes AFTER the send for this reason. A socket that dies mid-send puts the text
    /// back in the outbox so the redial carries it; drawing it first would show it once now and
    /// once again when it really goes.
    ///
    /// WHAT MUTATION MAKES THIS RED: moving the echo above the `pipe.send` call.
    #[test]
    fn a_message_that_could_not_be_sent_is_kept_and_not_shown() {
        /* A wire whose send always fails, so the drain hits its error arm on the first message. */
        struct Deaf {
            sent: Arc<Mutex<Vec<String>>>,
        }
        impl Duplex for Deaf {
            fn read(&mut self, _into: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "quiet"))
            }
            fn send(&mut self, line: &str) -> std::io::Result<()> {
                self.sent
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(line.to_owned());
                if line.starts_with("PRIVMSG") {
                    return Err(std::io::Error::other("the socket went"));
                }
                Ok(())
            }
        }
        struct DeafWire {
            sent: Arc<Mutex<Vec<String>>>,
        }
        impl Wire for DeafWire {
            fn dial(&mut self) -> Result<Box<dyn Duplex>, String> {
                Ok(Box::new(Deaf {
                    sent: self.sent.clone(),
                }))
            }
        }

        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut wire = DeafWire { sent: sent.clone() };
        let shared = Arc::new(Mutex::new(Log::idle("broken_stoic")));
        /* In the room already, so the drain runs on the first pass with no handshake to wait for. */
        let stop = AtomicBool::new(false);
        let ctx = egui::Context::default();
        let creds = Mutex::new(Some(Creds::new("reviird", "NOT-A-REAL-TOKEN")));
        let outbox = Mutex::new(VecDeque::from(vec!["this one does not make it".to_owned()]));
        let redial = AtomicBool::new(false);
        let nudge = AtomicBool::new(false);
        let w = Wired {
            shared: &shared,
            stop: &stop,
            nudge: &nudge,
            creds: &creds,
            outbox: &outbox,
            redial: &redial,
            ctx: &ctx,
        };
        let _ = session(&mut wire, &w, "broken_stoic", &fast());

        let log = lock(&shared).clone();
        assert!(
            !log.lines.iter().any(|e| e.who == "reviird"),
            "a message that failed to send was drawn as though it had gone"
        );
        assert_eq!(
            outbox.lock().unwrap_or_else(|p| p.into_inner()).len(),
            1,
            "a message that failed to send was thrown away instead of kept for the redial"
        );
    }

    /// Play one connection through `session` on this thread and hand back the log, the lines the
    /// reader wrote, and how the session ended.
    fn play(
        lines: &[&str],
        chunk: usize,
        end: End,
        tune: &Tuning,
    ) -> (Log, Vec<String>, Result<(), String>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut wire = Replay {
            takes: VecDeque::from(vec![wire_bytes(lines)]),
            chunk,
            end,
            sent: sent.clone(),
            dials: Arc::new(AtomicUsize::new(0)),
            exhausted: Arc::new(AtomicBool::new(false)),
        };
        let shared = Arc::new(Mutex::new(Log::idle("broken_stoic")));
        let stop = AtomicBool::new(false);
        let ctx = egui::Context::default();
        /* ANONYMOUS, which is what every one of these framing tests is about; the authenticated
         * handshake has its own test. */
        let creds = Mutex::new(None);
        let outbox = Mutex::new(VecDeque::new());
        let redial = AtomicBool::new(false);
        let nudge = AtomicBool::new(false);
        let w = Wired {
            shared: &shared,
            stop: &stop,
            nudge: &nudge,
            creds: &creds,
            outbox: &outbox,
            redial: &redial,
            ctx: &ctx,
        };
        let out = session(&mut wire, &w, "broken_stoic", tune);
        let log = lock(&shared).clone();
        let wrote = sent.lock().unwrap_or_else(|p| p.into_inner()).clone();
        (log, wrote, out)
    }

    /// A LINE THAT STRADDLES A TCP READ BOUNDARY IS STILL ONE LINE.
    ///
    /// THE DEFECT: parsing the read buffer per read instead of per newline. A `read` returns
    /// whatever the kernel has, which on a busy channel routinely ends in the middle of a tag blob,
    /// so the tail of one message and the head of the next share a buffer. A reader that does not
    /// carry the remainder forward loses a message on every boundary, and the rate of loss rises
    /// with how busy the channel is, which means it looks flawless on a quiet stream and eats a
    /// tenth of a raid. Replaying the same real bytes at five wildly different chunk sizes,
    /// including one byte at a time, is what makes that impossible to hide.
    #[test]
    fn framing_survives_any_read_boundary() {
        let tune = fast();
        let mut seen: Vec<(usize, u64, String)> = Vec::new();
        for chunk in [1usize, 3, 7, 512, 65536] {
            let (log, _, _) = play(ANON, chunk, End::Close, &tune);
            let bodies: String = log
                .lines
                .iter()
                .map(|e| e.body.clone())
                .collect::<Vec<_>>()
                .join("|");
            seen.push((log.lines.len(), log.refused, bodies));
        }
        /* THE TRIVIAL PASS THIS STOPS: every chunk size agreeing on ZERO events would satisfy "they
         * all agree", so the expected count is named outright before the agreement is asserted. */
        assert_eq!(
            seen[0].0, ANON_EVENTS,
            "the capture holds four PRIVMSG and one USERNOTICE"
        );
        assert_eq!(seen[0].1, 0, "no line of a real capture should be refused");
        assert!(
            seen[0].2.contains("get your bee from shop"),
            "the last message of the capture must be in the log"
        );
        for (i, s) in seen.iter().enumerate() {
            assert_eq!(
                (s.0, s.1, s.2.as_str()),
                (seen[0].0, seen[0].1, seen[0].2.as_str()),
                "chunk size index {i} produced a different log from the same bytes"
            );
        }
    }

    /// EVERY PING IS ANSWERED, WITH ITS OWN PAYLOAD, AND THE JOIN IS SENT EXACTLY ONCE.
    ///
    /// THE DEFECT: no PONG, or one PONG for two PINGs, or a JOIN re-sent on every numeric. Twitch
    /// drops a client that misses a keepalive, and the drop arrives as a socket that has simply
    /// stopped speaking, which on screen is indistinguishable from a channel where nobody is
    /// talking. The capture holds two real PINGs, so the count is two and not "at least one". A
    /// JOIN sent twice is a rate limit strike against the address.
    #[test]
    fn every_ping_is_answered_and_the_room_is_joined_once() {
        let (_, wrote, _) = play(ANON, 4096, End::Close, &fast());
        assert_eq!(
            wrote,
            vec![
                "CAP REQ :twitch.tv/tags twitch.tv/commands".to_owned(),
                wrote.get(1).cloned().unwrap_or_default(),
                "JOIN #broken_stoic".to_owned(),
                "PONG :tmi.twitch.tv".to_owned(),
                "PONG :tmi.twitch.tv".to_owned(),
            ],
            "the wire should carry exactly the capabilities, the login, one join, and one pong per \
             ping, in that order"
        );
        /* The login carries a clock derived number so it cannot be an equality in the list above,
         * which means slot one would otherwise accept literally anything, including nothing. */
        let nick = wrote.get(1).cloned().unwrap_or_default();
        assert!(
            nick.starts_with("NICK justinfan") && nick.len() > "NICK justinfan".len(),
            "the login must be an anonymous justinfan with a number, was {nick:?}"
        );
        assert!(
            !wrote.iter().any(|l| l.starts_with("PASS")),
            "an anonymous read needs no password and must never send one"
        );
        assert!(
            !wrote.iter().any(|l| l.contains("membership")),
            "membership is the majority of the bytes and buys this screen nothing"
        );
    }

    /// A SOCKET THAT DIES IS AN ERROR ON SCREEN, NEVER A QUIET CHANNEL.
    ///
    /// THE DEFECT: watcher.rs:13-19 names it for live status and it is the same one here. A reader
    /// that ends a session without setting `error` leaves a full, calm, stale chat on screen while
    /// the connection is gone, at the exact moment the viewer needs to know the app has lost the
    /// stream. The `Err` is the whole product requirement.
    #[test]
    fn a_dropped_socket_becomes_words_and_not_silence() {
        let (log, _, out) = play(ANON, 4096, End::Close, &fast());
        let why = match out {
            Ok(()) => panic!("a closed socket must not be reported as a clean stop"),
            Err(e) => e,
        };
        assert!(
            why.contains("closed"),
            "the reason must name what happened, was {why:?}"
        );
        /* THE TRIVIAL PASS THIS STOPS: a reader that reported an error on EVERY session would
         * satisfy the assertion above. A session that had failed before joining could not have
         * counted a connect or kept any lines. */
        assert_eq!(log.connects, 1, "it did join before it was dropped");
        assert_eq!(
            log.lines.len(),
            ANON_EVENTS,
            "the lines that did arrive are still there"
        );
    }

    /// A CONNECTED BUT SILENT SOCKET IS AN ERROR TOO, AND IT TAKES A DEADLINE TO NOTICE.
    ///
    /// THE DEFECT: a half open TCP connection, which is what a NAT that has forgotten the flow
    /// leaves behind, never returns an error and never returns EOF. It simply reads nothing,
    /// forever. Without the idle deadline this reader sits on `Joined` showing a chat that stopped
    /// hours ago, and no amount of staring at the screen reveals it. The number is anchored to a
    /// measurement: Twitch's own PING arrived 278.5 seconds apart in irc_busy.txt, so real silence
    /// that long cannot happen on a live connection.
    #[test]
    fn a_connection_that_goes_silent_is_reported_rather_than_waited_on_forever() {
        let (log, _, out) = play(ANON, 4096, End::Quiet, &fast());
        let why = match out {
            Ok(()) => panic!("silence must not be reported as a clean stop"),
            Err(e) => e,
        };
        assert!(
            why.contains("silent"),
            "the reason must say the connection went silent, was {why:?}"
        );
        assert_eq!(
            log.lines.len(),
            ANON_EVENTS,
            "the messages that did arrive before the silence are kept"
        );
        assert_eq!(log.connects, 1, "it had joined; this is not a failed dial");
    }

    /// A LOGIN THAT NEVER COMPLETES IS AN ERROR, NOT AN EMPTY CHAT.
    ///
    /// THE DEFECT: this reader sends its JOIN when the message of the day ends, which is where both
    /// captures show a client sending it. If that line never comes, a reader with no handshake
    /// deadline sits connected and joined to nothing, drawing an empty room forever, which a viewer
    /// reads as "chat is dead tonight". The fixture is the real capture with the three lines that
    /// end the greeting and confirm the room taken out, and nothing else changed.
    #[test]
    fn a_handshake_that_never_finishes_is_reported_and_not_drawn_as_an_empty_room() {
        let no_motd_end: Vec<&str> = ANON
            .iter()
            .copied()
            .filter(|l| !l.contains(" 376 ") && !l.contains(" 366 ") && !l.contains(" JOIN "))
            .collect();
        let (log, wrote, out) = play(&no_motd_end, 4096, End::Quiet, &fast());
        let why = match out {
            Ok(()) => panic!("an incomplete login must not be reported as a clean stop"),
            Err(e) => e,
        };
        assert!(
            why.contains("never arrived"),
            "the reason must say what did not arrive, was {why:?}"
        );
        assert_eq!(log.connects, 0, "it never got into the room");
        assert!(
            !wrote.iter().any(|l| l.starts_with("JOIN")),
            "no end of MOTD means no join was sent, which is what the error is about"
        );
    }

    /// A RECONNECT KEEPS EVERY LINE THE LOG ALREADY HAD.
    ///
    /// THE DEFECT: rebuilding the shared state at the top of the reconnect loop, which is the
    /// obvious way to write it and empties the chat every time the wifi hiccups. The lines were
    /// true when they arrived and a new socket does not make them false. Two connections of the
    /// same real capture must leave twice the messages: not the same number, and not one lot.
    #[test]
    fn a_reconnect_keeps_the_lines_the_log_already_had() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let dials = Arc::new(AtomicUsize::new(0));
        let exhausted = Arc::new(AtomicBool::new(false));
        let mut wire = Replay {
            takes: VecDeque::from(vec![wire_bytes(ANON), wire_bytes(ANON)]),
            chunk: 4096,
            end: End::Close,
            sent,
            dials: dials.clone(),
            exhausted: exhausted.clone(),
        };
        let shared = Arc::new(Mutex::new(Log::idle("broken_stoic")));
        let ctx = egui::Context::default();
        /* `run` normally ends only when `stop` is set. The scripted wire sets it the moment it is
         * asked for a connection it does not have, so this runs to a fixed end with no timer, no
         * sleeping thread and nothing to flake. */
        let nudge = AtomicBool::new(false);
        let creds = Mutex::new(None);
        let outbox = Mutex::new(VecDeque::new());
        let redial = AtomicBool::new(false);
        let w = Wired {
            shared: &shared,
            stop: &exhausted,
            nudge: &nudge,
            creds: &creds,
            outbox: &outbox,
            redial: &redial,
            ctx: &ctx,
        };
        run(&mut wire, &w, "broken_stoic", &fast());
        let log = lock(&shared).clone();
        assert_eq!(
            log.lines.len(),
            ANON_EVENTS * 2,
            "both sessions' lines must be in the log; half this number means it was cleared"
        );
        assert_eq!(log.connects, 2, "it joined the room twice");
        assert_eq!(
            dials.load(Ordering::Relaxed),
            3,
            "two scripted connections and the one that stops the loop"
        );
        assert!(
            log.error.is_some(),
            "the last thing that happened was a failure and the screen must be able to say so"
        );
        assert_eq!(log.state, Conn::Stopped);
    }

    /// THE LOG IS BOUNDED BY COUNT AND SAYS HOW MUCH IT THREW AWAY.
    ///
    /// THE DEFECT: an unbounded `VecDeque`. A viewer who leaves the app on a busy channel overnight
    /// is 5.6 messages a second, measured on #zackrawrr, which is two hundred thousand messages by
    /// morning. The second half of the defect is silent eviction: a log that drops the oldest line
    /// and says nothing looks complete, so nobody can tell a gap from the start of the stream.
    #[test]
    fn the_log_is_capped_and_counts_what_it_dropped() {
        let mut tune = fast();
        tune.log_cap = 3;
        let (log, _, _) = play(ANON, 4096, End::Close, &tune);
        assert_eq!(log.lines.len(), 3, "the cap holds");
        assert_eq!(
            log.dropped,
            (ANON_EVENTS - 3) as u64,
            "what fell off the front is counted, not silently gone"
        );
        /* THE TRIVIAL PASS THIS STOPS: a log that kept the FIRST three would also be length three.
         * The newest message in the capture is the one that must have survived. */
        let last = log.lines.back().map(|e| e.body.clone()).unwrap_or_default();
        assert_eq!(
            last, "get your bee from shop",
            "eviction takes the oldest, never the newest"
        );
    }

    /// A `/me` HANDS THE SPAN BUILDER THE BODY THE EMOTE INDICES ARE MEASURED AGAINST.
    ///
    /// THE DEFECT: passing the raw trailing, `\x01ACTION ...\x01`, to the span builder. The emote
    /// indices Twitch sends are offsets into the text WITHOUT that eight character envelope. On
    /// this real line `emotes=305954156:76-83` selects "PogChamp" only after the strip; with the
    /// envelope left on it selects eight characters that end in the middle of a word, so every
    /// emote in every `/me` in the app is drawn over the wrong text. 42 lines of irc_busy.txt are
    /// actions.
    #[test]
    fn an_action_is_unwrapped_before_the_emote_offsets_are_used() {
        let e = match step(BUSY_ACTION) {
            Step::Keep(e) => e,
            other => panic!("a real PRIVMSG should be kept, got {other:?}"),
        };
        assert_eq!(e.kind, Kind::Action);
        assert!(
            !e.body.starts_with('\u{1}'),
            "the envelope must be off the body"
        );
        /* The assertion that actually pins the offset. 76 and 83 are the capture's own numbers. */
        let at: String = e.body.chars().skip(76).take(8).collect();
        assert_eq!(
            at, "PogChamp",
            "characters 76 to 83 of the stripped body are the emote the tag names"
        );
        assert_eq!(e.who, "ThePositiveBot");
    }

    /// A RESUB WITH NO MESSAGE FROM THE SUBSCRIBER STILL HAS WORDS TO DRAW.
    ///
    /// THE DEFECT: treating `system-msg` and the trailing parameter as the same field. 27 of the 62
    /// USERNOTICEs in irc_busy.txt have no trailing at all, so a screen that draws only the
    /// trailing renders a blank row for nearly half the subs in a channel. The escaping is the
    /// other half: `\s` is a space, and a client that skips the unescape prints
    /// `wanenri\ssubscribed\swith\sPrime.`
    #[test]
    fn a_resub_with_no_user_message_still_carries_the_servers_sentence() {
        let e = match step(BUSY_RESUB_NO_BODY) {
            Step::Keep(e) => e,
            other => panic!("a real USERNOTICE should be kept, got {other:?}"),
        };
        assert_eq!(e.kind, Kind::Notice);
        assert_eq!(e.body, "", "this line genuinely has no user message");
        assert_eq!(
            e.notice.as_deref(),
            Some("wanenri subscribed with Prime. They've subscribed for 6 months!"),
            "the server's own sentence, unescaped, is what there is to draw"
        );
        assert_eq!(e.who, "wanenri");
    }

    /// AN ANNOUNCEMENT'S TEXT IS IN THE TRAILING AND ITS `system-msg` IS EMPTY.
    ///
    /// THE DEFECT: the mirror image of the test above, and the reason the two fields cannot be
    /// folded into one. Broken Stoic's own announcement in irc_anon.txt carries `system-msg=` with
    /// nothing after it and the whole message in the trailing. A reader that preferred `system-msg`
    /// whenever the tag was present would draw an empty row for every announcement, and an empty
    /// `Some("")` in `notice` would put a blank line above the text.
    #[test]
    fn an_announcement_keeps_its_text_even_though_the_system_message_is_empty() {
        let raw = ANON
            .iter()
            .find(|l| l.contains("msg-id=announcement"))
            .copied()
            .unwrap_or("");
        let e = match step(raw) {
            Step::Keep(e) => e,
            other => panic!("the capture's announcement should be kept, got {other:?}"),
        };
        assert_eq!(e.kind, Kind::Notice);
        assert!(
            e.body.starts_with("180sec ad break starting."),
            "the announcement's words are the trailing, was {:?}",
            e.body
        );
        assert_eq!(
            e.notice, None,
            "an empty system-msg is nothing to draw, not a blank line to draw"
        );
        /* Broken Stoic's own line carries `color=;`. Painting that as black would make the
         * broadcaster's name invisible on this app's ink background. */
        assert_eq!(e.color, None, "an empty colour tag is not black");
        assert_eq!(e.who, "Broken_Stoic");
    }

    /// THE OTHER VIEWERS' JOINS ARE SKIPPED, NOT REFUSED, AND NOT DRAWN.
    ///
    /// THE DEFECT: two of them, and they are opposite mistakes. Counting a line the reader
    /// understands perfectly well as a refusal poisons the tripwire that is supposed to say
    /// "Twitch changed something", so a real change later goes unnoticed in the noise. Pushing it
    /// into the log puts "streamelements joined the channel" in the middle of the conversation.
    /// irc_anon.txt was captured with membership on, so it holds four real other viewer JOINs.
    #[test]
    fn other_viewers_joining_are_neither_logged_nor_counted_as_refusals() {
        let (log, _, _) = play(ANON, 4096, End::Close, &fast());
        assert_eq!(log.refused, 0, "every line of the capture is understood");
        assert_eq!(
            log.lines.len(),
            ANON_EVENTS,
            "the four other JOINs, the NAMES list and the MOTD are not chat"
        );
        assert!(
            !log.lines.iter().any(|e| e.who == "streamelements"),
            "a viewer joining is not something the log holds"
        );
    }

    /// A TIMEOUT NAMES ITS TARGET AND ITS LENGTH.
    ///
    /// THE DEFECT: dropping CLEARCHAT, or reading its target from the wrong place. The target login
    /// is the trailing parameter and the duration is a tag, and a client that shows a timeout with
    /// no name attached tells a viewer somebody was silenced without saying who, which is worse
    /// than saying nothing at all.
    #[test]
    fn a_timeout_carries_who_and_for_how_long() {
        let e = match step(BUSY_CLEARCHAT) {
            Step::Keep(e) => e,
            other => panic!("a real CLEARCHAT should be kept, got {other:?}"),
        };
        assert_eq!(e.kind, Kind::Cleared);
        assert_eq!(e.who, "koishi_mm");
        assert_eq!(e.secs, Some(300));
        assert_eq!(e.sent_ms, Some(1788555614579));
    }

    /// A HIGHLIGHTED MESSAGE IS NOT AN ORDINARY ONE.
    ///
    /// THE DEFECT: ignoring `msg-id=highlighted-message` on a PRIVMSG. The viewer spent channel
    /// points on it specifically so it would look different; a client that renders it as an
    /// ordinary line has taken the points and delivered nothing. 16 of them in irc_busy.txt.
    #[test]
    fn a_channel_points_highlight_is_a_kind_of_its_own() {
        let e = match step(BUSY_HIGHLIGHT) {
            Step::Keep(e) => e,
            other => panic!("a real PRIVMSG should be kept, got {other:?}"),
        };
        assert_eq!(e.kind, Kind::Highlight);
        /* THE TRIVIAL PASS THIS STOPS: a reader that marked EVERY message a highlight would satisfy
         * the line above, so an ordinary message from the other capture is checked here. */
        let plain = ANON
            .iter()
            .find(|l| l.contains("PRIVMSG #broken_stoic :first"))
            .copied()
            .unwrap_or("");
        match step(plain) {
            Step::Keep(e) => assert_eq!(e.kind, Kind::Chat, "an untagged message is ordinary"),
            other => panic!("a real PRIVMSG should be kept, got {other:?}"),
        }
    }

    /// A ROOMSTATE THAT MENTIONS ONE KEY LEAVES THE OTHERS ALONE.
    ///
    /// THE DEFECT: replacing the whole `Room` from each ROOMSTATE. The one that follows a JOIN is
    /// complete, which is all either capture contains, but the one Twitch sends when a moderator
    /// flips a switch carries only the key that changed. A wholesale replacement would answer "is
    /// this channel followers only?" with the default the instant somebody turned slow mode on, and
    /// the screen would tell a viewer they may talk when they may not. The two real ROOMSTATEs used
    /// here differ in exactly the field that matters.
    #[test]
    fn a_partial_roomstate_does_not_reset_the_settings_it_did_not_mention() {
        let full = ANON
            .iter()
            .find(|l| l.contains("ROOMSTATE"))
            .copied()
            .unwrap_or("");
        let mut room = Room::unknown();
        match step(BUSY_ROOMSTATE) {
            Step::Room(p) => room.apply(&p),
            other => panic!("a real ROOMSTATE should be a room patch, got {other:?}"),
        }
        assert_eq!(
            room.followers_only, 15,
            "#jynxzi is fifteen minute followers only"
        );
        /* Now a patch that mentions only slow mode, built from the real #broken_stoic ROOMSTATE's
         * own `slow=0` key with the rest of its keys taken back out. */
        let mut only_slow = match step(full) {
            Step::Room(p) => *p,
            other => panic!("a real ROOMSTATE should be a room patch, got {other:?}"),
        };
        only_slow.emote_only = None;
        only_slow.followers_only = None;
        only_slow.r9k = None;
        only_slow.subs_only = None;
        room.apply(&only_slow);
        assert_eq!(
            room.followers_only, 15,
            "a patch that never mentioned followers-only must not turn it off"
        );
        assert_eq!(room.slow, 0, "and the key it did mention did land");
    }

    /// A TRUNCATED LINE IS COUNTED, NEVER SILENTLY DROPPED.
    ///
    /// THE DEFECT: a parser that returns early on anything it cannot read. It looks perfect until
    /// Twitch changes a shape, and then a slice of the chat disappears with nothing on screen and
    /// nothing in the log to say why. The bytes here are a real capture line cut short, which is
    /// what a server that dies mid write actually produces, and the cut lands inside the tag blob
    /// because that is by far the longest run of a tagged line.
    #[test]
    fn a_line_the_reader_cannot_read_is_counted_and_named() {
        let whole = ANON
            .iter()
            .find(|l| l.contains("PRIVMSG #broken_stoic :first"))
            .copied()
            .unwrap_or("");
        let torn = &whole[..40];
        let torn_lines = [torn];
        let (log, _, _) = play(&torn_lines, 4096, End::Close, &fast());
        assert_eq!(log.lines.len(), 0, "half a line is not a message");
        assert_eq!(log.refused, 1, "and it is counted rather than forgotten");
        assert!(
            log.last_refusal.is_some(),
            "with the reason kept for the screen"
        );
        /* THE TRIVIAL PASS THIS STOPS: a reader that refused everything would also satisfy the
         * above, so the same line, intact, must NOT be refused. */
        let whole_lines = [whole];
        let (ok, _, _) = play(&whole_lines, 4096, End::Close, &fast());
        assert_eq!(ok.refused, 0, "the same line, intact, is understood");
        assert_eq!(ok.lines.len(), 1);
    }

    /// THE REPAINT VALVE CAPS THE FRAME RATE A RAID CAN DEMAND, AND STILL PAINTS THE LAST LINE.
    ///
    /// THE DEFECT: one `request_repaint` per message. #zackrawrr's worst second in irc_busy.txt
    /// held 38 messages, and a raid is an order of magnitude worse; each repaint lays out and
    /// paints the entire log. The second half of the defect is the fix for the first done badly: a
    /// plain rate limiter throws away the request that arrives inside a spent window, so the last
    /// message before a chat goes quiet is never painted at all. The timings driving this are the
    /// real arrival stamps of 25 consecutive #zackrawrr messages, 3.7 seconds of real chat.
    #[test]
    fn the_repaint_valve_coalesces_a_burst_without_losing_the_last_line() {
        let base = Instant::now();
        let start = ZACK_BURST_MS[0];
        let mut c = Coalescer::new(base);
        let mut at_ms: Vec<u64> = Vec::new();
        for ms in ZACK_BURST_MS {
            /* Clamped at zero because the capture's stamps are not monotonic: 1788555473902 arrives
             * before 1788555473897. That is also why the log keeps arrival order. */
            let delta = (ms - start).max(0) as u64;
            c.dirtied();
            if c.due(base + Duration::from_millis(delta), REPAINT_EVERY) {
                at_ms.push(delta);
            }
        }
        assert!(!at_ms.is_empty(), "the burst must have been painted at all");
        /* THE DISCRIMINATING ASSERTION. A repaint per message is exactly 25, so anything fewer is
         * coalescing and 25 is the storm. A "no more than one per 100 ms" bound alone would NOT
         * catch it, because 3.7 seconds allows 38 and 25 slips under that. */
        assert!(
            at_ms.len() < ZACK_BURST_MS.len(),
            "a repaint per message is the storm this exists to stop, got {} for {}",
            at_ms.len(),
            ZACK_BURST_MS.len()
        );
        for w in at_ms.windows(2) {
            assert!(
                w[1] - w[0] >= REPAINT_EVERY.as_millis() as u64,
                "two repaints {} ms apart is under the floor",
                w[1] - w[0]
            );
        }
        /* THE HALF THAT IS EASY TO GET WRONG. One more real line lands five milliseconds after the
         * last repaint, inside a spent window. The valve must refuse it AND must not lose it. */
        let last = at_ms.last().copied().unwrap_or(0);
        c.dirtied();
        assert!(
            !c.due(base + Duration::from_millis(last + 5), REPAINT_EVERY),
            "a request inside a spent window is refused"
        );
        let later = last + 5 + REPAINT_EVERY.as_millis() as u64;
        assert!(
            c.due(base + Duration::from_millis(later), REPAINT_EVERY),
            "and the refused request is still delivered on the next tick, not dropped"
        );
    }

    /// STARTING TWICE OPENS ONE CONNECTION, AND AN IDLE READER OPENS NONE.
    ///
    /// THE DEFECT: `ChatReader::start` called from a screen's `ui`, which runs on every frame.
    /// Without the `swap`, a visible Chat screen opens sixty sockets a second to Twitch, which is a
    /// rate limit on the address and then a ban. The other half is `Screens::default()`, which this
    /// crate builds whole at startup for every user: a reader that connected from `Default` would
    /// open a socket on every launch for people who never open chat.
    #[test]
    fn starting_twice_opens_one_connection_and_an_idle_reader_opens_none() {
        let before = threads_started();
        let r = ChatReader::idle();
        assert_eq!(
            threads_started(),
            before,
            "an idle reader has started no thread"
        );
        assert_eq!(r.with_log(|l| l.state.clone()), Conn::Idle);

        let ctx = egui::Context::default();
        let make = |dials: Arc<AtomicUsize>| -> Box<dyn Wire> {
            Box::new(Replay {
                takes: VecDeque::new(),
                chunk: 4096,
                end: End::Close,
                sent: Arc::new(Mutex::new(Vec::new())),
                dials,
                exhausted: Arc::new(AtomicBool::new(false)),
            })
        };
        let dials = Arc::new(AtomicUsize::new(0));
        r.start_with(&ctx, "#Broken_Stoic", make(dials.clone()), fast());
        r.start_with(&ctx, "broken_stoic", make(dials.clone()), fast());
        assert_eq!(
            threads_started(),
            before + 1,
            "the second start is a no operation"
        );
        /* The channel is normalised, so `#Broken_Stoic` and `broken_stoic` are the same room and
         * the second call must NOT record a complaint about being moved. */
        assert_eq!(r.with_log(|l| l.channel.clone()), "broken_stoic");
        r.stop();

        let other = ChatReader::idle();
        other.start_with(&ctx, "a", make(dials.clone()), fast());
        other.start_with(&ctx, "b", make(dials), fast());
        let complaint = other.with_log(|l| l.error.clone()).unwrap_or_default();
        assert!(
            complaint.contains("will not be moved"),
            "a second start naming a different channel must be refused in words, was {complaint:?}"
        );
        other.stop();
    }

    /// A COLOUR TAG IS SIX HEX DIGITS OR IT IS NOTHING.
    ///
    /// THE DEFECT: parsing `color=` as black. Every colour asserted here is one the captures
    /// actually carry, including the empty one on Broken Stoic's own line, and this app paints on a
    /// near black background, so a name resolved to 0x000000 is a name nobody can see.
    #[test]
    fn an_empty_colour_tag_is_absent_rather_than_black() {
        assert_eq!(hex_color("#FF0000"), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(hex_color("#008000"), Some(Color32::from_rgb(0, 128, 0)));
        assert_eq!(hex_color("#1E90FF"), Some(Color32::from_rgb(30, 144, 255)));
        assert_eq!(hex_color("#5F9EA0"), Some(Color32::from_rgb(95, 158, 160)));
        assert_eq!(
            hex_color(""),
            None,
            "the tag the capture sends for no colour"
        );
        /* THE TRIVIAL PASS THIS STOPS: `None` for everything would satisfy the line above. */
        assert!(hex_color("#DAA520").is_some());
        assert_eq!(hex_color("#FFF"), None, "three digits is not this format");
        assert_eq!(hex_color("#GGGGGG"), None);
    }

    /// A SERVER PREFIX IS NEVER DRAWN AS A USERNAME.
    ///
    /// THE DEFECT: falling back to the prefix without checking it is a user prefix. Every
    /// USERNOTICE and CLEARCHAT in both captures is prefixed `:tmi.twitch.tv`, so a reader that
    /// took the text before the `!` and did not notice there was no `!` would attribute every sub
    /// in the channel to a user called "tmi.twitch.tv".
    #[test]
    fn the_server_prefix_is_never_drawn_as_a_username() {
        assert_eq!(nick_of("hd_dean!hd_dean@hd_dean.tmi.twitch.tv"), "hd_dean");
        assert_eq!(nick_of("tmi.twitch.tv"), "");
        assert_eq!(nick_of("justinfan73921.tmi.twitch.tv"), "");
        /* And the whole path: the capture's USERNOTICE is prefixed by the server and named by its
         * tags, which is the case the fallback must never be reached for. */
        let raw = ANON
            .iter()
            .find(|l| l.contains("msg-id=announcement"))
            .copied()
            .unwrap_or("");
        match step(raw) {
            Step::Keep(e) => assert_eq!(e.who, "Broken_Stoic"),
            other => panic!("the capture's announcement should be kept, got {other:?}"),
        }
    }
}
