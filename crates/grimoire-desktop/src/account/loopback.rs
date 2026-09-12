//! THE LOOPBACK LISTENER: the one socket this app opens for somebody else to connect to, and so the
//! part of the sign-in with the most ways to be wrong.
//!
//! WHAT IT ACCEPTS, AND ONLY THAT. `GET /callback` on `127.0.0.1`, carrying the state this sign-in
//! made, compared in constant time and byte for byte. Everything else is answered and IGNORED
//! without ending the wait:
//!
//!   * another path (a browser asks for `/favicon.ico` on its own) is a 404;
//!   * a callback whose state is missing or different is a 400. It does not cancel the sign-in,
//!     because any local program can connect here, and one that could end a sign-in by guessing
//!     wrong could stop anybody from ever signing in;
//!   * `error=access_denied` with the RIGHT state ends the wait as cancelled, which is the
//!     website's "Use a different account" and the owner closing the approval.
//!
//! 127.0.0.1 AND NEVER 0.0.0.0. Bound to every interface, the port would be reachable from the
//! network, and a code arriving from another machine is exactly what RFC 8252 section 8.3 warns
//! about. `it_listens_on_loopback_only` holds that.
//!
//! NO CONNECTION WAITS ON ANOTHER, BECAUSE BROWSERS PRECONNECT. Chrome and Edge open a spare
//! socket to a host they expect to visit and send nothing on it. A listener that read each
//! connection in turn would sit on that silent socket while the real callback waited behind it.
//! Every socket here is non-blocking, and one loop reads whatever each has sent so far.
//! `a_silent_preconnect_does_not_hold_up_the_real_callback` is the measured version.
//!
//! A LOCAL PROGRAM CANNOT CROWD THE CALLBACK OUT. Any process on this computer can open sockets to
//! this port and hold them. So a connection gets a deadline for its whole head, not a timeout per
//! read (a byte every few seconds would pass every read), and when the house is full it is the
//! OLDEST connection that goes, never the one arriving: a new connection is what a browser coming
//! back looks like. `a_full_house_of_held_sockets_does_not_turn_the_real_callback_away` holds that.

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{ct_eq, safe_word, Hidden};

/// How long a sign-in waits for the browser to come back before it gives up. Five minutes is time
/// to pick a provider, sign in there, and read the approval; longer is a tab somebody forgot.
pub const WAIT_FOR_BROWSER: Duration = Duration::from_secs(5 * 60);

/// How long one connection has, from the moment it is accepted, to send its whole request head.
/// A browser sends it in the same instant it connects.
const HEAD_DEADLINE: Duration = Duration::from_secs(5);
/// The largest request head read. A browser's GET is a few hundred bytes plus any cookies other
/// local apps have set on 127.0.0.1.
const HEAD_LIMIT: usize = 16 * 1024;
/// How long the loop rests when nothing arrived and nothing finished.
const NAP: Duration = Duration::from_millis(25);
/// The most connections held at once. A held connection is a socket and a buffer, not a thread,
/// so a local program opening sockets in a loop costs this app memory it caps, and nothing else.
const MAX_OPEN: usize = 64;
/// The most connections accepted before every open one is read again. Well under `MAX_OPEN`, so a
/// connection is read several times over before enough newer ones can arrive to push it out.
const ACCEPT_BURST: usize = 8;

/// How a wait ended.
#[derive(Debug)]
pub enum Waited {
    /// The website sent a code with the right state. Redacted in `Debug`: see [`Hidden`].
    Code(Hidden),
    /// The website said `access_denied` with the right state.
    Denied,
    /// The website sent another `error` with the right state, reduced to a safe word.
    Refused(String),
    /// The sign-in was cancelled from the app.
    Cancelled,
    /// Nobody came back within the wait.
    TimedOut,
    /// The listener itself failed.
    Broken(String),
}

/// A bound listener. Dropping it closes the port.
pub struct Loopback {
    listener: TcpListener,
    port: u16,
}

impl Loopback {
    /// Bind `127.0.0.1` on a port the OS picks. The OS hands out ephemeral ports well above 1024,
    /// which is the floor the contract sets.
    pub fn bind() -> Result<Loopback, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| {
            format!("Could not open a port on this computer for the browser to come back to: {e}")
        })?;
        /* NON-BLOCKING, so the loop below can look at the cancel flag and the clock between
         * connections instead of parking in `accept` until somebody arrives. */
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Could not prepare the sign-in port: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Could not read the sign-in port: {e}"))?
            .port();
        Ok(Loopback { listener, port })
    }

    /// The port the browser is sent back to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Wait for the browser to come back with `state`, until `cancel` is set or `wait` passes.
    pub fn wait(&self, state: &Hidden, cancel: &AtomicBool, wait: Duration) -> Waited {
        let deadline = Instant::now() + wait;
        /* OLDEST FIRST, so the front is the one that goes when the house is full. */
        let mut open: VecDeque<Conn> = VecDeque::new();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Waited::Cancelled;
            }
            if Instant::now() >= deadline {
                return Waited::TimedOut;
            }
            let mut busy = false;
            for _ in 0..ACCEPT_BURST {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        busy = true;
                        let Some(conn) = Conn::new(stream) else {
                            continue;
                        };
                        if let Some(w) = make_room(&mut open, state) {
                            return w;
                        }
                        open.push_back(conn);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    /* A client that gave up between the SYN and the accept. Theirs, not ours. */
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        busy = true
                    }
                    Err(e) => return Waited::Broken(format!("{e}")),
                }
            }
            let now = Instant::now();
            let mut i = 0;
            while i < open.len() {
                match open[i].pump(now) {
                    Pump::Waiting => i += 1,
                    Pump::Gone => {
                        busy = true;
                        open.remove(i);
                    }
                    Pump::Head(head) => {
                        busy = true;
                        if let Some(conn) = open.remove(i) {
                            if let Some(w) = conn.answer(&head, state) {
                                return w;
                            }
                        }
                    }
                }
            }
            if !busy {
                std::thread::sleep(NAP);
            }
        }
    }
}

/// MAKE ROOM FOR ONE MORE: with the house full, the OLDEST connection goes, never the arriving one.
///
/// READ ONE LAST TIME FIRST, so a head that arrived since the last sweep is served rather than
/// thrown away with its socket. `Some` when that head ended the wait.
fn make_room(open: &mut VecDeque<Conn>, expected: &Hidden) -> Option<Waited> {
    if open.len() < MAX_OPEN {
        return None;
    }
    let mut oldest = open.pop_front()?;
    match oldest.pump(Instant::now()) {
        Pump::Head(head) => oldest.answer(&head, expected),
        Pump::Waiting | Pump::Gone => None,
    }
}

/// One open connection: its socket, what it has sent so far, and when its head must be in.
struct Conn {
    stream: TcpStream,
    head: Vec<u8>,
    deadline: Instant,
}

/// What reading a connection found.
enum Pump {
    /// Not a whole head yet, and still inside its deadline.
    Waiting,
    /// The request head, up to the blank line.
    Head(String),
    /// Closed, broken, too big, not UTF-8, or out of time. Dropped without a reply.
    Gone,
}

impl Conn {
    /// `None` for a socket that cannot be made non-blocking, which is then closed. Set explicitly:
    /// an accepted socket inherits non-blocking from the listener on Windows, and not on Linux.
    fn new(stream: TcpStream) -> Option<Conn> {
        stream.set_nonblocking(true).ok()?;
        Some(Conn {
            stream,
            head: Vec::with_capacity(1024),
            deadline: Instant::now() + HEAD_DEADLINE,
        })
    }

    /// Read whatever has arrived, without waiting for more.
    fn pump(&mut self, now: Instant) -> Pump {
        let mut chunk = [0u8; 1024];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Pump::Gone,
                Ok(n) => {
                    /* ONLY THE NEW BYTES ARE SEARCHED, plus the three before them a blank line
                     * could straddle: a head sent a byte at a time must not cost its square. */
                    let from = self.head.len().saturating_sub(3);
                    self.head.extend_from_slice(&chunk[..n]);
                    if let Some(at) = self.head[from..].windows(4).position(|w| w == b"\r\n\r\n") {
                        self.head.truncate(from + at);
                        return match String::from_utf8(std::mem::take(&mut self.head)) {
                            Ok(head) => Pump::Head(head),
                            Err(_) => Pump::Gone,
                        };
                    }
                    if self.head.len() > HEAD_LIMIT {
                        return Pump::Gone;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Pump::Gone,
            }
        }
        if now >= self.deadline {
            Pump::Gone
        } else {
            Pump::Waiting
        }
    }

    /// Answer a whole head, close the connection, and say whether it ended the wait.
    ///
    /// NEVER WAITS ON THE CLIENT. The reply is a page of a kilobyte or two, which a fresh socket's
    /// send buffer takes whole; a client that somehow will not take it loses its page, and the
    /// outcome stands, because waiting on it would be every other connection waiting too.
    fn answer(mut self, head: &str, expected: &Hidden) -> Option<Waited> {
        let (reply, outcome) = decide(head, expected);
        let mut rest = reply.as_bytes();
        while !rest.is_empty() {
            match self.stream.write(rest) {
                Ok(0) => break,
                Ok(n) => rest = &rest[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = self.stream.flush();
        outcome
    }
}

/// THE WHOLE RULE, AS A PURE FUNCTION: the reply to send, and whether this request ends the wait.
fn decide(head: &str, expected: &Hidden) -> (String, Option<Waited>) {
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (method, target, version) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
    );
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != "/callback" || !version.starts_with("HTTP/1.") {
        return (NOT_FOUND.to_owned(), None);
    }
    let not_this = || {
        page(
            400,
            "Not this sign-in",
            "This link is not for the sign-in Grimoire is waiting on. You can close this tab.",
        )
    };
    let Some(q) = parse_query(query) else {
        return (not_this(), None);
    };
    let matches = q
        .get("state")
        .is_some_and(|s| ct_eq(s.as_bytes(), expected.expose().as_bytes()));
    if !matches {
        return (not_this(), None);
    }
    if let Some(error) = q.get("error") {
        if error == "access_denied" {
            return (
                page(
                    200,
                    "Sign-in cancelled",
                    "Grimoire was not signed in. You can close this tab.",
                ),
                Some(Waited::Denied),
            );
        }
        return (
            page(
                200,
                "Sign-in did not finish",
                "The website did not sign Grimoire in. You can close this tab and try again from \
                 the app.",
            ),
            Some(Waited::Refused(safe_word(error))),
        );
    }
    match q.get("code") {
        Some(code) if !code.is_empty() => (
            page(
                200,
                "Back to Grimoire",
                "Grimoire has what it needs to finish signing in. You can close this tab.",
            ),
            Some(Waited::Code(Hidden::new(code.clone()))),
        ),
        _ => (not_this(), None),
    }
}

/// A query string as name to value, or `None` when it is malformed or names anything twice.
///
/// A REPEATED NAME IS REFUSED rather than first-wins or last-wins: `state=A&state=B` is a request
/// written to confuse a parser, and there is no reading of it that a real redirect produces.
fn parse_query(q: &str) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for pair in q.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if out.insert(decode(k)?, decode(v)?).is_some() {
            return None;
        }
    }
    Some(out)
}

/// `application/x-www-form-urlencoded` decoding: `%XX` and `+`. `None` for a broken escape or
/// bytes that are not UTF-8.
fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b.get(i + 1..i + 3)?;
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\n\
                         Content-Length: 9\r\nConnection: close\r\n\r\nNot found";

/// The page the browser shows after the redirect.
///
/// NO LINKS, NO SCRIPTS, NO REFERRER. The address bar holds the code for as long as the tab is
/// open, so the page loads nothing (`default-src 'none'`) and sends no `Referer` anywhere.
fn page(status: u16, title: &str, line: &str) -> String {
    let reason = match status {
        200 => "OK",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Grimoire</title><style>body{{margin:0;min-height:100vh;display:grid;\
         place-items:center;background:#15120e;color:#e9e1d1;font:16px/1.5 system-ui,sans-serif}}\
         main{{max-width:30rem;padding:2rem;text-align:center}}h1{{font-size:1.3rem;\
         font-weight:600}}</style></head><body><main><h1>{title}</h1><p>{line}</p></main></body>\
         </html>\n"
    );
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const STATE: &str = "Qm9rZW4tU3RvaWMtc3RhdGUtdmFsdWUtZm9yLXRlc3Q";

    fn state() -> Hidden {
        Hidden::new(STATE.to_owned())
    }

    /// Play the browser: one GET, and the whole response back.
    ///
    /// A READ TIMEOUT, SO A BROKEN LISTENER FAILS A TEST RATHER THAN HANGING IT. A wait that ends
    /// too early leaves the rest of a test's visits connected to a backlog nobody accepts, and a
    /// client with no timeout would block there for ever: measured, with `ct_eq` forced true.
    fn visit(port: u16, target: &str) -> String {
        match TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
            Ok(mut s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
                let _ = write!(
                    s,
                    "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                );
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                out
            }
            Err(e) => format!("refused: {e}"),
        }
    }

    /// Visit each target in order on a thread; the join hands back every response.
    fn browse(port: u16, targets: &[&str]) -> std::thread::JoinHandle<Vec<String>> {
        let targets: Vec<String> = targets.iter().map(|t| (*t).to_owned()).collect();
        std::thread::spawn(move || targets.iter().map(|t| visit(port, t)).collect())
    }

    #[test]
    fn it_listens_on_loopback_only() {
        let lb = Loopback::bind().expect("bind");
        let at = lb.listener.local_addr().expect("addr");
        assert_eq!(
            at.ip(),
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            "the sign-in port is reachable from outside this computer"
        );
        assert!(lb.port() >= 1024, "port {}", lb.port());
    }

    /// THE HAPPY PATH: the code comes back, and the page says the tab can close, in plain words.
    #[test]
    fn a_matching_callback_hands_back_the_code_and_says_close_the_tab() {
        let lb = Loopback::bind().expect("bind");
        let b = browse(
            lb.port(),
            &[&format!("/callback?code=THE-CODE&state={STATE}")],
        );
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(20));
        let answers = b.join().expect("browser");
        match got {
            Waited::Code(c) => assert_eq!(c.expose(), "THE-CODE"),
            other => panic!("a good callback ended as {other:?}"),
        }
        let page = &answers[0];
        assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
        assert!(page.contains("close this tab"), "{page}");
        assert!(page.contains("Referrer-Policy: no-referrer"), "{page}");
        assert!(!page.contains('\u{2014}'), "the page has an em-dash in it");
        assert!(!page.contains("THE-CODE"), "the page repeats the code");
    }

    /// THE FAVICON AND ANY OTHER PATH ARE A 404 AND THE FLOW CARRIES ON.
    ///
    /// WHAT MUTATION MAKES THIS RED: returning `Some(..)` for an unknown path, or matching the
    /// path with `starts_with("/callback")`.
    #[test]
    fn a_wrong_path_is_answered_404_and_the_flow_keeps_waiting() {
        let lb = Loopback::bind().expect("bind");
        let b = browse(
            lb.port(),
            &[
                "/favicon.ico",
                &format!("/callbackX?code=EVIL&state={STATE}"),
                &format!("/?code=EVIL&state={STATE}"),
                &format!("/callback?code=GOOD&state={STATE}"),
            ],
        );
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(20));
        let answers = b.join().expect("browser");
        for a in &answers[..3] {
            assert!(a.starts_with("HTTP/1.1 404"), "{a}");
        }
        match got {
            Waited::Code(c) => assert_eq!(c.expose(), "GOOD"),
            other => panic!("{other:?}"),
        }
    }

    /// A STATE THAT DOES NOT MATCH IS REFUSED, AND DOES NOT END THE WAIT EITHER WAY.
    ///
    /// Not a code taken, and not a sign-in cancelled: a local program guessing must not be able
    /// to do either. Every near miss below is one a sloppy comparison accepts.
    ///
    /// WHAT MUTATION MAKES THIS RED: `ct_eq` answering true, a `starts_with` or case-insensitive
    /// state comparison, or ending the wait on a mismatch.
    #[test]
    fn a_state_that_does_not_match_is_refused_and_never_ends_the_wait() {
        let lb = Loopback::bind().expect("bind");
        let near_misses = [
            "/callback?code=EVIL".to_owned(),
            "/callback?code=EVIL&state=".to_owned(),
            format!("/callback?code=EVIL&state={}", STATE.to_lowercase()),
            format!("/callback?code=EVIL&state={}", &STATE[..20]),
            format!("/callback?code=EVIL&state={STATE}x"),
            format!("/callback?code=EVIL&state=x&state={STATE}"),
            format!("/callback?code=EVIL&state={STATE}&state={STATE}"),
            "/callback?error=access_denied&state=wrong".to_owned(),
            "/callback?error=access_denied".to_owned(),
        ];
        let targets: Vec<&str> = near_misses.iter().map(String::as_str).collect();
        let b = browse(lb.port(), &targets);
        let got = lb.wait(
            &state(),
            &AtomicBool::new(false),
            Duration::from_millis(1500),
        );
        let answers = b.join().expect("browser");
        assert!(
            matches!(got, Waited::TimedOut),
            "a callback with the wrong state ended the wait as {got:?}"
        );
        for (a, t) in answers.iter().zip(&near_misses) {
            assert!(a.starts_with("HTTP/1.1 400"), "{t} was answered {a}");
        }
    }

    /// `access_denied` WITH THE RIGHT STATE IS A CANCEL.
    #[test]
    fn access_denied_with_the_right_state_ends_as_denied() {
        let lb = Loopback::bind().expect("bind");
        let b = browse(
            lb.port(),
            &[&format!("/callback?error=access_denied&state={STATE}")],
        );
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(20));
        let answers = b.join().expect("browser");
        assert!(matches!(got, Waited::Denied), "{got:?}");
        assert!(answers[0].contains("not signed in"), "{}", answers[0]);
    }

    /// Another error ends the wait, and only a safe word of it survives.
    #[test]
    fn another_error_ends_as_a_refusal_in_a_safe_word() {
        let lb = Loopback::bind().expect("bind");
        let b = browse(
            lb.port(),
            &[&format!(
                "/callback?error=server%3Cscript%3E_error&state={STATE}"
            )],
        );
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(20));
        b.join().expect("browser");
        match got {
            Waited::Refused(w) => assert_eq!(w, "serverscript_error"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cancelling_ends_the_wait_promptly() {
        let lb = Loopback::bind().expect("bind");
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c.store(true, Ordering::SeqCst);
        });
        let t = Instant::now();
        let got = lb.wait(&state(), &cancel, Duration::from_secs(60));
        assert!(matches!(got, Waited::Cancelled), "{got:?}");
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn nobody_coming_back_times_out() {
        let lb = Loopback::bind().expect("bind");
        let got = lb.wait(
            &state(),
            &AtomicBool::new(false),
            Duration::from_millis(200),
        );
        assert!(matches!(got, Waited::TimedOut), "{got:?}");
    }

    /// A SILENT PRECONNECT DOES NOT HOLD UP THE REAL CALLBACK.
    ///
    /// WHAT MUTATION MAKES THIS RED: reading a connection until its head is in (a blocking read)
    /// instead of reading only what has arrived. The silent socket then holds the loop for
    /// `HEAD_DEADLINE`.
    #[test]
    fn a_silent_preconnect_does_not_hold_up_the_real_callback() {
        let lb = Loopback::bind().expect("bind");
        let port = lb.port();
        let silent = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("preconnect");
        let b = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            visit(port, &format!("/callback?code=FAST&state={STATE}"))
        });
        let t = Instant::now();
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(20));
        assert!(matches!(got, Waited::Code(_)), "{got:?}");
        assert!(
            t.elapsed() < HEAD_DEADLINE - Duration::from_secs(2),
            "the real callback waited {:?} behind a silent socket",
            t.elapsed()
        );
        b.join().expect("browser");
        drop(silent);
    }

    /// A FULL HOUSE OF SOCKETS A LOCAL PROGRAM HOLDS OPEN DOES NOT TURN THE REAL CALLBACK AWAY.
    ///
    /// Any process on this computer can reach the port without knowing the state. It opens as many
    /// connections as the listener will serve and sends nothing, or a byte now and then. The real
    /// callback arriving behind them must still be read, answered, and end the wait with its code.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping a new connection when the house is full (the single
    /// use code is lost with it, and the wait ends `TimedOut`), or making room only when an old
    /// connection's deadline passes, which is after the browser has already been refused.
    #[test]
    fn a_full_house_of_held_sockets_does_not_turn_the_real_callback_away() {
        let lb = Loopback::bind().expect("bind");
        let port = lb.port();
        let stop = Arc::new(AtomicBool::new(false));
        let hog = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut held: Vec<TcpStream> = (0..MAX_OPEN)
                    .filter_map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok())
                    .collect();
                /* HALF OF THEM TRICKLE, so a listener that only drops silent sockets is caught. */
                while !stop.load(Ordering::SeqCst) {
                    for s in held.iter_mut().step_by(2) {
                        let _ = s.write_all(b"G");
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                held.len()
            })
        };
        let b = std::thread::spawn(move || {
            /* Long enough for the listener to have taken every held socket first. */
            std::thread::sleep(Duration::from_millis(500));
            visit(port, &format!("/callback?code=REAL&state={STATE}"))
        });
        let got = lb.wait(&state(), &AtomicBool::new(false), Duration::from_secs(4));
        let answer = b.join().expect("browser");
        stop.store(true, Ordering::SeqCst);
        assert_eq!(
            hog.join().expect("hog"),
            MAX_OPEN,
            "the hog did not fill the house"
        );
        match got {
            Waited::Code(c) => assert_eq!(c.expose(), "REAL"),
            other => panic!("the real callback behind held sockets ended as {other:?}"),
        }
        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "the browser got {answer:?}"
        );
    }

    /// THE OLDEST CONNECTION IS READ ONE LAST TIME BEFORE IT MAKES ROOM, AND A SILENT ONE IS CLOSED.
    ///
    /// Driven on `make_room` itself with a full house of real accepted sockets, because through
    /// `wait` the sweep almost always reads a head before eviction could reach it, and a test that
    /// depends on losing that race proves nothing.
    ///
    /// WHAT MUTATION MAKES THIS RED: `make_room` dropping the oldest without reading it (a head
    /// that arrived since the last sweep is thrown away), or making no room at all.
    #[test]
    fn the_oldest_connection_is_read_once_more_before_it_makes_room() {
        let lb = Loopback::bind().expect("bind");
        let port = lb.port();
        let connect = || {
            let s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
            let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
            s
        };
        let accept = |open: &mut VecDeque<Conn>| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                if let Ok((s, _)) = lb.listener.accept() {
                    open.push_back(Conn::new(s).expect("non-blocking"));
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("a connection was never accepted");
        };

        /* THE OLDEST HAS ITS WHOLE HEAD IN, UNREAD. The backlog hands connections out in order. */
        let mut oldest = connect();
        write!(
            oldest,
            "GET /callback?code=OLDEST&state={STATE} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        )
        .expect("write");
        let mut open = VecDeque::new();
        accept(&mut open);
        let silent: Vec<TcpStream> = (1..MAX_OPEN)
            .map(|_| {
                let s = connect();
                accept(&mut open);
                s
            })
            .collect();
        assert_eq!(open.len(), MAX_OPEN);

        let got = make_room(&mut open, &state());
        assert_eq!(open.len(), MAX_OPEN - 1, "a full house made no room");
        match got {
            Some(Waited::Code(c)) => assert_eq!(c.expose(), "OLDEST"),
            other => panic!("the oldest connection's head was thrown away: {other:?}"),
        }
        let mut answer = String::new();
        let _ = oldest.read_to_string(&mut answer);
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer:?}");

        /* NOW THE OLDEST IS SILENT: it goes, with no outcome, and its socket is closed. */
        let _newest = connect();
        accept(&mut open);
        assert!(make_room(&mut open, &state()).is_none());
        assert_eq!(open.len(), MAX_OPEN - 1);
        let mut sink = [0u8; 16];
        let mut first_silent = &silent[0];
        match first_silent.read(&mut sink) {
            Ok(0) => {}
            Err(e)
                if !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            other => panic!("the evicted silent socket is still open: {other:?}"),
        }
        assert!(
            make_room(&mut VecDeque::new(), &state()).is_none(),
            "room was made in an empty house"
        );
    }

    /// A SOCKET THAT TRICKLES ITS HEAD A BYTE AT A TIME IS CLOSED AT THE HEAD DEADLINE.
    ///
    /// A per-read timeout is not a deadline: one byte every few seconds passes every read, and a
    /// 16 KB head at that pace holds its connection for hours.
    ///
    /// WHAT MUTATION MAKES THIS RED: timing out each read instead of the whole head.
    #[test]
    fn a_trickling_socket_is_closed_at_the_head_deadline() {
        let lb = Loopback::bind().expect("bind");
        let port = lb.port();
        let t = std::thread::spawn(move || {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
            let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
            let opened = Instant::now();
            let mut sink = [0u8; 64];
            while opened.elapsed() < HEAD_DEADLINE + Duration::from_secs(4) {
                if s.write_all(b"G").is_err() {
                    return Some(opened.elapsed());
                }
                match s.read(&mut sink) {
                    Ok(_) => return Some(opened.elapsed()),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return Some(opened.elapsed()),
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            None
        });
        let got = lb.wait(
            &state(),
            &AtomicBool::new(false),
            HEAD_DEADLINE + Duration::from_secs(5),
        );
        assert!(matches!(got, Waited::TimedOut), "{got:?}");
        match t.join().expect("trickler") {
            Some(after) => assert!(
                after < HEAD_DEADLINE + Duration::from_secs(2),
                "the trickling socket was closed only after {after:?}"
            ),
            None => panic!("a socket trickling its head held its connection past the deadline"),
        }
    }

    /// Only a GET, and only HTTP/1.x, reach the rule at all.
    #[test]
    fn only_a_get_reaches_the_callback() {
        let good = format!("/callback?code=C&state={STATE}");
        for head in [
            format!("POST {good} HTTP/1.1"),
            format!("HEAD {good} HTTP/1.1"),
            format!("GET {good}"),
            format!("get {good} HTTP/1.1"),
        ] {
            let (reply, out) = decide(&head, &state());
            assert!(reply.starts_with("HTTP/1.1 404"), "{head}: {reply}");
            assert!(out.is_none(), "{head}");
        }
        let (_, out) = decide(&format!("GET {good} HTTP/1.1"), &state());
        assert!(matches!(out, Some(Waited::Code(_))));
    }

    #[test]
    fn a_query_is_decoded_and_a_broken_one_is_refused() {
        let q = parse_query("code=a%2Bb+c&state=x").expect("valid");
        assert_eq!(q["code"], "a+b c");
        assert!(parse_query("code=%zz").is_none());
        assert!(parse_query("code=%4").is_none());
        assert!(parse_query("code=%FF").is_none(), "not UTF-8");
        assert!(parse_query("state=a&state=a").is_none());
    }
}
