//! A REAL CONNECTION TO REAL TWITCH, RUN BY HAND.
//!
//! `#[ignore]`, and that is the whole design of this file. Every other test in this crate is
//! forbidden from touching the network: `cargo test` runs on other people's machines and in CI, and
//! a suite that opens sockets to a live service is a suite that fails when the service is down,
//! rate limits the build machine, and cannot be trusted to be measuring what it says. So the unit
//! tests drive the reader over a replayed capture and this one file dials for real, only when
//! somebody asks for it:
//!
//!     cargo test -p grimoire-desktop --test live_twitch -- --ignored --nocapture
//!
//! WHAT IT PROVES THAT A REPLAY CANNOT. That the TLS handshake against `irc.chat.twitch.tv:6697`
//! completes with the rustls provider this crate compiles, that Twitch accepts an ANONYMOUS login
//! with no password, that it honours the capability request, and that tagged lines arrive and come
//! out of `chat::step` as events. The parser is already proven against a capture; what is not
//! provable that way is that the service still behaves the way the capture says it did.
//!
//! IT READS AND IT SENDS NOTHING. Anonymous IRC has no account behind it, no credential is involved
//! anywhere in this file, and the reader has no code path that writes a message. It joins a public
//! channel, reads what that channel is broadcasting to everybody, prints a count, and hangs up.
//!
//! NOTHING IS WRITTEN TO DISK. Same rule as the screen: the log is in memory and dies with the
//! process.

use grimoire_desktop::chat::{ChatReader, Conn};
use std::time::{Duration, Instant};

/// Which channel to listen to. Defaults to the app's own; override to watch a busier one, which is
/// the only way to see message traffic when Broken Stoic is offline.
fn channel() -> String {
    std::env::var("EQL_LIVE_CHANNEL")
        .unwrap_or_else(|_| grimoire_desktop::settings::TWITCH_HANDLE.to_owned())
}

/// How long to listen. Long enough for the handshake and some traffic, short enough to be a test.
fn window() -> Duration {
    Duration::from_secs(
        std::env::var("EQL_LIVE_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(25),
    )
}

#[test]
#[ignore = "dials real Twitch; run by hand with --ignored"]
fn a_real_anonymous_login_is_accepted_and_lines_arrive() {
    let ch = channel();
    let ctx = egui::Context::default();
    let reader = ChatReader::idle();
    reader.start(&ctx, &ch);

    let deadline = Instant::now() + window();
    let mut joined_at = None;
    let mut last = 0usize;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
        let (state, n, refused, err, last_refusal) = reader.with_log(|l| {
            (
                l.state.clone(),
                l.lines.len(),
                l.refused,
                l.error.clone(),
                l.last_refusal.clone(),
            )
        });
        if state == Conn::Joined && joined_at.is_none() {
            joined_at = Some(Instant::now());
            println!("joined #{ch}");
        }
        if n != last {
            /* Print only what arrived since the last look, so a busy channel does not reprint its
             * whole backlog four times a second. */
            reader.with_log(|l| {
                for e in l.lines.iter().skip(last) {
                    let body = if e.body.is_empty() {
                        e.notice.clone().unwrap_or_default()
                    } else {
                        e.body.clone()
                    };
                    println!("  [{:?}] {}: {}", e.kind, e.who, body);
                }
            });
            last = n;
        }
        if let Some(e) = &err {
            println!("error: {e}");
        }
        if refused > 0 {
            println!("refused {refused}: {last_refusal:?}");
        }
    }

    let (state, n, refused, connects, last_refusal) = reader.with_log(|l| {
        (
            l.state.clone(),
            l.lines.len(),
            l.refused,
            l.connects,
            l.last_refusal.clone(),
        )
    });
    reader.stop();

    println!("\n--- #{ch} after {:?} ---", window());
    println!("state    {state:?}");
    println!("joined   {connects} time(s)");
    println!("messages {n}");
    println!("refused  {refused}  {last_refusal:?}");

    /* THE ASSERTION IS THE HANDSHAKE, NOT THE TRAFFIC. A channel can be genuinely silent for
     * twenty five seconds, and failing this test for that would be asserting something about
     * Broken Stoic's viewers rather than about this code. Being IN THE ROOM is what the anonymous
     * login and the capability request buy, and that is what is checked. */
    assert_eq!(
        state,
        Conn::Joined,
        "an anonymous login to #{ch} did not end up in the room; the reader says {state:?}"
    );
    assert!(connects >= 1, "never joined");
    /* AND NOTHING WAS UNREADABLE. A live line the parser cannot take is the failure this whole
     * file exists to catch: it means the service changed shape since the capture was taken. */
    assert_eq!(
        refused, 0,
        "{refused} live line(s) could not be parsed, last was {last_refusal:?}. The capture the \
         unit tests replay no longer describes what Twitch sends"
    );
}

/// THE SCREEN, DRAWN AGAINST A LIVE SOCKET, PRINTED.
///
/// The unit tests draw this screen against a replayed capture and assert the words. This draws it
/// against whatever the channel is saying right now and prints every string it painted, which is
/// the one check that cannot be faked by a fixture: the reader, the parser, the owned spans and the
/// screen all have to work for anything to come out.
#[test]
#[ignore = "dials real Twitch; run by hand with --ignored"]
fn the_screen_drawn_against_a_live_socket() {
    let ch = channel();
    let ctx = egui::Context::default();
    grimoire_desktop::fonts::install(&ctx);
    grimoire_desktop::theme::install(&ctx);

    let reader = ChatReader::idle();
    reader.start(&ctx, &ch);
    std::thread::sleep(window());

    let live = grimoire_desktop::watcher::Status {
        twitch: grimoire_desktop::watcher::Channel::unchecked(&ch),
        youtube: grimoire_desktop::watcher::Channel::unchecked(
            grimoire_desktop::settings::YOUTUBE_HANDLE,
        ),
    };
    let mut settings = grimoire_desktop::settings::Settings::default();
    let mut ingest = grimoire_desktop::ingest::Ingest::new(&settings);
    let mut screen = grimoire_desktop::screens::chat::ChatScreen::default();
    let mut cx = grimoire_desktop::screens::Cx {
        data: None,
        railed: false,
        data_err: None,
        live: &live,
        settings: &mut settings,
        ingest: &mut ingest,
        chat: reader.handle(),
        chat_wanted: false,
        auth: Default::default(),
        auth_begin: false,
        auth_cancel: false,
        yt: Default::default(),
        yt_wanted: false,
        player: Default::default(),
        stage: None,
        demand: None,
        ask: Default::default(),
    };
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(520.0, 900.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
    let shapes = std::mem::take(&mut out.shapes);
    out.drop_without_applying_deltas();
    reader.stop();

    let mut said = Vec::new();
    let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
    while let Some(sh) = stack.pop() {
        match sh {
            egui::Shape::Vec(v) => stack.extend(v),
            egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
            _ => {}
        }
    }
    said.reverse();
    println!("\n=== THE CHAT SCREEN, AS PAINTED ===");
    for s in &said {
        println!("{s}");
    }
    assert!(cx.chat_wanted, "the screen did not ask for a connection");
    assert!(!said.is_empty(), "the screen painted no text at all");
}

/// THE REAL WIRE AGAINST THE REAL ENDPOINT, PRINTING WHAT IT ACTUALLY GETS.
///
/// The unit tests drive a fake wire that answers `(400, body)` because that is what a wire is
/// supposed to do. This one asks `Https` itself, so a difference between the two shows up here
/// rather than as "Twitch answered 400" on somebody's screen. It stops before anybody has to type
/// anything: one device request and one poll, which is guaranteed to be `authorization_pending`.
#[test]
#[ignore = "dials real Twitch; run by hand with --ignored"]
fn the_production_wire_reads_a_pending_authorisation() {
    use grimoire_desktop::twitch_auth::{self, Https, Step};
    let cid = grimoire_desktop::settings::TWITCH_CLIENT_ID;
    let scopes = grimoire_desktop::settings::TWITCH_CHAT_SCOPES;
    let mut w = Https::default();

    let d = match twitch_auth::begin(&mut w, cid, scopes) {
        Ok(d) => d,
        Err(e) => panic!("the device request failed: {e}"),
    };
    println!(
        "device ok: user_code={} interval={:?}",
        d.user_code, d.interval
    );
    println!("verification_uri={}", d.verification_uri);

    let step = twitch_auth::poll(&mut w, cid, scopes, &d.device_code);
    println!("first poll -> {step:?}");
    assert_eq!(
        step,
        Step::Pending,
        "the production wire did not read `authorization_pending` out of a 400. That is the whole \
         protocol: every poll before the owner finishes typing is a 400 with that message in its \
         body, and reading it as anything else refuses the sign-in on its first attempt"
    );
}
