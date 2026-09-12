//! The channel's chat, beside the stream.
//!
//! THIS SCREEN READS TWITCH AND IT DOES IT WITH NO ACCOUNT AND NO TOKEN.
//!
//! An anonymous IRC login against `irc.chat.twitch.tv:6697` with NO password
//! (`NICK justinfan<random>`) is answered `:tmi.twitch.tv 001 ... :Welcome, GLHF!`, joins the
//! channel, and delivers tagged PRIVMSG, USERNOTICE, CLEARCHAT and ROOMSTATE. That was measured
//! before a line of this was written: 1,099 tagged PRIVMSG in thirty seconds off a busy channel.
//! `crate::chat` is the reader, `crate::irc` is the wire format, and this file is the face.
//!
//! WHAT IT STILL CANNOT DO, SAID PLAINLY RATHER THAN LEFT AS AN EMPTY CONTROL.
//!
//!   1. Sending is BUILT, and it needed permission first. The composer at the foot of this screen
//!      appears only once a sign-in has landed, because a text box that cannot send is a lie with
//!      a cursor in it. The permission comes from Twitch's Device Code Grant Flow (`twitch_auth`):
//!      the owner types eight characters on Twitch's own page and approves two scopes,
//!      `chat:read` and `chat:edit`. This app never draws a login form, never sees a password and
//!      never reads another browser's cookies, and that line does not move. What goes on the wire
//!      is `PASS oauth:<token>` at the handshake and `PRIVMSG` per message, both in `crate::chat`.
//!   2. It reads YOUTUBE TOO, and the two are merged into one column. `ytchat` runs the live chat
//!      page in a hidden browser pane and posts the rows back over wry's IPC; `ytchat::model`
//!      owns the ordering rule that puts the two rooms into one order. The audiences barely
//!      overlap (measured: nobody spoke in both rooms on the sampled stream), so a feed showing
//!      one of them would be missing half the conversation.
//!   3. NOTHING HERE READS `settings::watch_on`. What is PLAYING and what is being READ and where
//!      a REPLY goes are three independent choices: the owner watches the stream on Twitch with a
//!      YouTube VoD open beside it and answers people in either room. A version of this screen
//!      tied the feed to the player and deleted itself whenever Watch live was set to YouTube.
//!   4. It does not draw emote IMAGES. Nothing in this crate fetches from the emote CDN yet, so an
//!      emote is drawn as the word the viewer typed, which is what it was on the wire. The spans
//!      are already cut (`chat::Piece::Emote` carries the id), so the images are a fetch away.
//!
//! NOTHING IS KEPT ON DISK. Twitch's terms allow chat to be held only as long as it takes to show
//! it, so the log is a ring buffer of `chat::LOG_CAP` messages in memory and it dies with the app.
//! There is no chat log file, there is no code here that opens one, and that is deliberate.
//!
//! IT HOLDS NO STATE AND OPENS NO SOCKET. `Screens` derives `Default` and is built whole at
//! startup whether or not anyone visits this row, so a reader started from a `Default` would
//! connect to Twitch on every launch forever. The reader lives on the `App`, it is idle until
//! asked, and this screen asks by setting `Cx::chat_wanted` while it is on screen. `App::ui` is
//! what calls `start`. See `Cx::chat` for why the screen does not call it itself.

use crate::chat::{Conn, Kind, Piece};
use crate::fonts;
use crate::screens::Cx;
use crate::settings::TWITCH_HANDLE;
use crate::theme::*;
use crate::twitch_auth::AuthView;
use egui::RichText;

/// How wide the sender's name column is. Twitch caps a login at 25 characters and a display name
/// is the same length in a different case, so this is the worst case at the body font rather than
/// a guess: names wider than this are not truncated, they simply push their own message right.
const WHO_W: f32 = 130.0;

/// The glyph that says a line came out of YouTube. A merged column with no mark has the reader
/// answering the wrong room, and the two rooms measured ZERO overlap in who was speaking.
const YT_MARK: &str = "\u{25b8}";
/// YouTube's red, dimmed to sit in this palette rather than shout over it.
const YT_TINT: egui::Color32 = egui::Color32::from_rgb(0xC4, 0x4A, 0x3C);

/// The Chat screen.
#[derive(Default)]
pub struct ChatScreen {
    /* THE ONLY STATE HERE IS WHAT IS HALF TYPED, and that is the test's rule rather than a habit:
     * `Screens::default()` runs at startup for every user, so anything this holds is built on
     * every launch whether or not the row is ever opened. A `String` is free; a thread handle
     * would be a socket. `a_default_chat_screen_holds_nothing_that_could_connect` is the guard,
     * and it checks for a CONNECTION rather than for emptiness, which is why this may exist. */
    /// What the composer has in it. Kept across navigation on purpose: stepping to the Parser to
    /// check a number and coming back must not eat a half written message.
    draft: String,
    /// Why the last send was refused, if it was. Cleared by the next keystroke.
    refused: Option<String>,
    /// WHICH ROOM THE NEXT MESSAGE GOES TO.
    ///
    /// The feed is merged and the two rooms are NOT the same people: measured on one stream,
    /// Twitch carried `wingspeare`, `purifypriest`, `Valdstein_` while YouTube carried
    /// `@cardigansrule`, `@kkixx_stixx`, `@radelc`, with no overlap. Answering somebody means
    /// answering them where they spoke, so the destination is a choice and not a setting.
    ///
    /// IT DEFAULTS TO TWITCH because that is the half that can send today.
    to: crate::ytchat::model::Source,
}

impl ChatScreen {
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        ui.label(
            RichText::new("CHAT")
                .font(fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.add_space(6.0);

        /* THE CHAT FEED DOES NOT FOLLOW THE VIDEO, AND THAT WAS A REAL BUG UNTIL NOW.
         *
         * This screen used to open with `if cx.settings.watch_on != Platform::Twitch { .. return }`,
         * on the reasoning that chat should follow whatever Watch live was playing so the two
         * could not disagree. That reasoning does not survive contact with how the app is
         * actually used: setting Watch live to YouTube DELETED THE WHOLE CHAT SCREEN, Twitch
         * included, and the room being read has nothing to do with the video being watched.
         *
         * WHAT THE OWNER ACTUALLY DOES, in his words: watch the stream on Twitch, have a YouTube
         * VoD going at the same time, and answer somebody in EITHER room. Three independent
         * choices. The video is one (`settings::watch_on` and the Videos row), the feed is
         * another (both platforms, always, which is what a merged column means), and where a
         * reply goes is a third (`ChatScreen::to`, picked per message).
         *
         * SO NOTHING HERE READS `watch_on`. Both halves are asked for unconditionally; the
         * YouTube half is quiet on its own when he is not live there, because `App::ui` only
         * loads a page once the poller has found a video id.
         */

        /* THE ASK. Set every frame this screen is visible; `start` is idempotent, so this dials
         * once and then costs an atomic swap. Cleared by `App::ui` taking it, which means a frame
         * that does not draw this screen does not ask, and a reader already running is not
         * disturbed by that. */
        cx.chat_wanted = true;
        /* AND THE OTHER HALF. He simulcasts, and the two rooms carry different people, so a feed
         * that showed one of them would be missing half the conversation. `App::ui` only acts on
         * this when the poller has actually found a live video id. */
        cx.yt_wanted = true;

        /* THE COMPOSER SITS AT THE FOOT, WHERE EVERY CHAT CLIENT PUTS IT, so it is drawn BEFORE
         * the message list takes the rest of the room. A bottom panel is how egui reserves that.
         *
         * THE SIGN-IN LIVES DOWN THERE TOO, AND THAT IS THE CORRECTION. It used to be a small
         * button at the top of the screen while `compose` drew NOTHING AT ALL until it could
         * send. The owner went looking for an input field three times, found none, and each time
         * reached for the Watch screen's `Sign in on Twitch` instead, which is a different
         * sign-in for a different thing. Three misses is not three mistakes, it is a design that
         * hid its own entrance: there was no box on screen to say a box existed. Everything about
         * speaking is now in one control, in the one place a person looks for it. */
        egui::Panel::bottom("chat.compose")
            .resizable(false)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
            .show(ui, |ui| self.compose(ui, cx));

        /* BOTH LOGS, UNDER BOTH LOCKS, IN THIS ORDER AND NOWHERE ELSE IN THE OTHER ORDER.
         * Two mutexes taken together is a deadlock waiting for a second call site that takes them
         * the other way round; this is the only place in the crate that holds both, and it is
         * written once so it cannot disagree with itself. */
        cx.chat.with_log(|tl| {
            cx.yt.with_log(|yl| {
                self.status(ui, tl, yl);
                ui.add_space(8.0);
                self.lines(ui, tl, yl);
            })
        });

        /* THE RECONNECT, OUTSIDE THE LOCK. `reconnect_now` takes no lock of its own, but calling it
         * inside `with_log` would hold the reader thread's mutex across a click handler, and the
         * thread wants that mutex on every line it parses. */
        let retrying = cx
            .chat
            .with_log(|log| matches!(log.state, Conn::Retrying { .. }));
        if retrying {
            ui.add_space(6.0);
            if ui.button(RichText::new("Try now").color(GOLD_HI)).clicked() {
                cx.chat.reconnect_now();
            }
        }
    }

    /// THE ONE CONTROL AT THE FOOT OF THE SCREEN: the box, or the reason there is not one yet.
    ///
    /// EVERY STATE DRAWS SOMETHING THE WIDTH OF THE SCREEN, and that is the whole point. The
    /// version before this returned early when it could not send, so a reader who was not signed
    /// in saw no box, no hint that a box existed, and no way in from where they were looking. A
    /// disabled field that says what it needs and starts that flow when you click it is not the
    /// thing this file warned about; the thing it warned about is an ENABLED box that takes your
    /// typing and silently drops it. This is the opposite: it refuses to accept a word until it
    /// can deliver one, and it says so on its face.
    ///
    /// ENTER SENDS AND THE FIELD KEEPS FOCUS, which is what every chat client does and what makes
    /// a conversation possible without reaching for the mouse between lines.
    fn compose(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if let Some(why) = &self.refused {
            ui.label(RichText::new(why).color(WRONG));
        }
        /* CLONED, BECAUSE THE ARMS BELOW WRITE TO `cx` AND A MATCH ON `&cx.auth` HOLDS IT.
         * An `AuthView` is one small enum with at most two short strings in it, and cloning it
         * once a frame is not a cost worth a borrow dance. It carries no token: see `AuthView`. */
        let state = cx.auth.clone();
        match &state {
            /* NOT SIGNED IN: a full width control shaped like the field it will become, saying
             * what it needs. Clicking anywhere on it starts the device flow. */
            AuthView::Out => {
                if self.slab(ui, "Sign in to send a message", TEXT_3) {
                    cx.auth_begin = true;
                }
            }
            /* WAITING: the code, in the same place, because a person who just clicked is looking
             * here and not at the top of the screen. The code authorises nothing on its own and
             * is useless to anybody not already signed in to this account, so it is drawn in
             * full; the `device_code` beside it, which is the half that becomes a token, is
             * drawn nowhere. */
            AuthView::Waiting {
                user_code,
                verification_uri,
            } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Type {user_code} on Twitch")).color(GOLD_HI));
                    if ui
                        .button(RichText::new("Open Twitch").color(TEXT))
                        .clicked()
                    {
                        /* Twitch's own page, in the system browser, which is where a password may
                         * be typed and this app is not. */
                        let _ = open::that(verification_uri);
                    }
                    ui.label(RichText::new("then come back here").color(TEXT_3));
                });
            }
            AuthView::Failed(why) => {
                ui.label(RichText::new(format!("Sign-in failed: {why}")).color(WRONG));
                if self.slab(ui, "Try signing in again", TEXT_3) {
                    cx.auth_begin = true;
                }
            }
            AuthView::In { login } => self.box_(ui, cx, login),
        }
    }

    /// A full width control shaped like the text field it stands in for. Returns whether it was
    /// clicked. A `Button` and not a disabled `TextEdit` because egui's disabled widgets do not
    /// report clicks, and a control that looks pressable and answers nothing is its own defect.
    fn slab(&self, ui: &mut egui::Ui, words: &str, tint: egui::Color32) -> bool {
        let b = egui::Button::new(
            RichText::new(words)
                .font(egui::FontId::proportional(13.0))
                .color(tint),
        )
        .fill(PANEL_2)
        .stroke(egui::Stroke::new(1.0, RULE))
        .corner_radius(egui::CornerRadius::ZERO)
        .min_size(egui::vec2(ui.available_width(), 26.0));
        ui.add(b).clicked()
    }

    /// The real box, drawn only when a message typed into it can actually leave.
    fn box_(&mut self, ui: &mut egui::Ui, cx: &mut Cx, login: &str) {
        use crate::ytchat::model::Source;
        ui.horizontal(|ui| {
            /* WHERE IT GOES, PICKED PER MESSAGE AND NOT SET ONCE IN SETTINGS.
             *
             * The feed is merged and the two rooms are DIFFERENT PEOPLE: on one measured stream
             * Twitch carried `wingspeare`, `purifypriest` and `Valdstein_` while YouTube carried
             * `@cardigansrule`, `@kkixx_stixx` and `@radelc`, with nobody in both. Answering
             * somebody means answering them where they spoke, and which room that is changes line
             * by line, so this is a control beside the box and not a preference in a menu. */
            for (src, mark, tip) in [
                (Source::Twitch, "TW", "Send to Twitch chat"),
                (Source::YouTube, "YT", "Send to YouTube live chat"),
            ] {
                let on = self.to == src;
                let b = egui::Button::new(
                    RichText::new(mark)
                        .font(egui::FontId::monospace(11.0))
                        .color(if on { INK } else { TEXT_3 }),
                )
                .fill(if on {
                    match src {
                        Source::Twitch => GOLD,
                        Source::YouTube => YT_TINT,
                    }
                } else {
                    PANEL
                })
                .stroke(egui::Stroke::new(1.0, RULE))
                .corner_radius(egui::CornerRadius::ZERO);
                if ui.add(b).on_hover_text(tip).clicked() {
                    self.to = src;
                    self.refused = None;
                }
            }
            ui.label(RichText::new(login).color(GOLD_DIM));
            let hint = match self.to {
                Source::Twitch => format!("Message #{}", TWITCH_HANDLE.to_lowercase()),
                Source::YouTube => "Message YouTube live chat".to_owned(),
            };
            let field = egui::TextEdit::singleline(&mut self.draft)
                .desired_width(f32::INFINITY)
                .hint_text(RichText::new(hint).color(TEXT_3))
                .font(egui::FontId::proportional(13.0));
            let r = ui.add(field);
            if r.changed() {
                self.refused = None;
            }
            /* THE COUNT APPEARS ONLY WHEN IT MATTERS. Twitch takes 500 characters and almost no
             * message is near that, so a counter on every line would be noise; past three
             * quarters it is the one thing worth knowing. */
            let used = self.draft.chars().count();
            if used * 4 > crate::chat::MAX_MESSAGE * 3 {
                let over = used > crate::chat::MAX_MESSAGE;
                ui.label(
                    RichText::new(format!("{used}/{}", crate::chat::MAX_MESSAGE)).color(if over {
                        WRONG
                    } else {
                        TEXT_3
                    }),
                );
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let out = match self.to {
                    Source::Twitch => cx.chat.send(&self.draft),
                    /* NOT BUILT, AND SAID HERE RATHER THAN SWALLOWED. Sending to YouTube needs
                     * the Live Chat API (`liveChatMessages.insert`, 50 quota units a message
                     * against a 10,000 a day allowance) and an OAuth client the owner has to
                     * register with Google. The DOM bridge that reads the feed cannot send: it is
                     * a signed out page. The refusal names what is missing so the message stays
                     * in the box and nobody watches it vanish. */
                    Source::YouTube => Err(
                        "sending to YouTube is not built: it needs a Google Cloud OAuth client, \
                         which has to be registered first"
                            .to_owned(),
                    ),
                };
                match out {
                    Ok(()) => {
                        self.draft.clear();
                        self.refused = None;
                    }
                    /* THE TEXT STAYS IN THE BOX ON A REFUSAL. Clearing it would throw away what
                     * somebody just wrote because it was two characters too long. */
                    Err(e) => self.refused = Some(e),
                }
                /* Focus back, or Enter costs a click before the next line. */
                r.request_focus();
            }
        });
    }

    /// The connection, in one line, plus every count that means something is being lost.
    fn status(
        &self,
        ui: &mut egui::Ui,
        log: &crate::chat::Log,
        yl: &crate::ytchat::surface::YtLog,
    ) {
        let (word, tint) = match &log.state {
            Conn::Idle => ("Not connected yet.".to_owned(), TEXT_3),
            Conn::Connecting => ("Connecting.".to_owned(), WORKING),
            Conn::Joined => (format!("In #{}.", log.channel), SETTLED),
            Conn::Retrying {
                attempt,
                next_try_in,
            } => (
                format!(
                    "Disconnected. Attempt {attempt} failed; trying again in {}s.",
                    next_try_in.as_secs()
                ),
                WRONG,
            ),
            Conn::Stopped => ("Stopped.".to_owned(), TEXT_3),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(word).color(tint));
            /* A RECONNECT IS A SEAM IN THE LOG AND THE READER SAYS SO. Two joins mean the messages
             * either side of some point came from different sessions and the gap between them is
             * not visible in the list, so the count is drawn rather than hidden. */
            if log.connects > 1 {
                ui.label(
                    RichText::new(format!("Reconnected {} times.", log.connects - 1)).color(TEXT_3),
                );
            }
        });

        if let Some(err) = &log.error {
            ui.label(RichText::new(err).color(WRONG));
        }

        /* WHAT IS BEING LOST, DRAWN WHENEVER IT IS NOT ZERO.
         *
         * `refused` counts lines this reader could not turn into anything, and `dropped` counts
         * messages the ring buffer has pushed off the front. A screen that hid either would look
         * perfect on the day Twitch adds a shape and a tenth of the chat stops appearing. The
         * reader's own note asks for exactly this and this is the code that keeps that promise. */
        if log.refused > 0 {
            let mut s = format!("{} line(s) not understood", log.refused);
            if let Some(last) = &log.last_refusal {
                s.push_str(": ");
                s.push_str(last);
            }
            ui.label(RichText::new(s).color(WRONG));
        }
        if log.dropped > 0 {
            ui.label(
                RichText::new(format!(
                    "{} older message(s) have scrolled out of memory and are gone.",
                    log.dropped
                ))
                .color(TEXT_3),
            );
        }

        /* THE YOUTUBE HALF SAYS ITS OWN STATE, in its own line, because the two feeds fail
         * independently: the IRC socket can be perfectly healthy while the browser pane has no
         * page, and one line covering both would have to lie about one of them. */
        self.yt_status(ui, yl);
    }

    /// What the YouTube half is doing, and only when that is worth saying.
    fn yt_status(&self, ui: &mut egui::Ui, yl: &crate::ytchat::surface::YtLog) {
        use crate::ytchat::surface::YtConn;
        let (word, tint) = match &yl.state {
            /* SILENT WHEN IDLE. He is not always live on YouTube, and a line saying so on every
             * frame of a Twitch-only stream is noise about a feature working correctly. */
            YtConn::Idle => return,
            YtConn::Loading => ("YouTube: loading.".to_owned(), WORKING),
            YtConn::Reading => (
                format!("YouTube: reading, {} so far.", yl.lines.len()),
                SETTLED,
            ),
            YtConn::Stalled(why) => (format!("YouTube: {why}"), WRONG),
            YtConn::Stopped => ("YouTube: stopped.".to_owned(), TEXT_3),
        };
        ui.label(RichText::new(word).color(tint));
        if yl.refused > 0 {
            let mut s = format!("YouTube: {} line(s) not understood", yl.refused);
            if let Some(last) = &yl.last_refusal {
                s.push_str(": ");
                s.push_str(last);
            }
            ui.label(RichText::new(s).color(WRONG));
        }
    }

    /// The messages. Oldest at the top, newest at the bottom, stuck to the bottom.
    /// BOTH ROOMS, IN ONE COLUMN, IN THE ORDER THEY WERE SAID.
    ///
    /// The merge itself is `ytchat::model::interleave`, which owns the ordering rule and argues
    /// it; this only draws what it returns. Nothing is copied to sort: it hands back borrows of
    /// the two logs the locks above are holding.
    fn lines(&self, ui: &mut egui::Ui, log: &crate::chat::Log, yl: &crate::ytchat::surface::YtLog) {
        if log.lines.is_empty() && yl.lines.is_empty() {
            ui.label(
                RichText::new(match log.state {
                    Conn::Joined => "No messages yet. The channel is quiet.",
                    _ => "No messages.",
                })
                .color(TEXT_3),
            );
            return;
        }

        let merged = crate::ytchat::model::interleave(&log.lines, &yl.lines);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for m in &merged {
                    match m {
                        crate::ytchat::model::Merged::Twitch(e) => row(ui, e),
                        crate::ytchat::model::Merged::YouTube(y) => yt_row(ui, y),
                    }
                }
            });
    }
}

/// ONE YOUTUBE MESSAGE. Marked, because a merged column that did not say which room a line came
/// out of would have the reader answering the wrong people: the two audiences do not overlap.
///
/// THE MARK IS A GLYPH AND A COLOUR AND NOT A WORD. A `YT` prefix on every line would cost more
/// width than the shortest messages carry, and the width is the scarce thing in a column that has
/// to sit beside a full screen game.
fn yt_row(ui: &mut egui::Ui, y: &crate::ytchat::model::YtMessage) {
    use crate::ytchat::model::{YtKind, YtPiece};
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(YT_MARK).color(YT_TINT));

        /* THE OWNER OF THE CHANNEL IS THE ONE NAME WORTH PICKING OUT. YouTube gives no per user
         * colour the way Twitch does, so everybody else is one tint and the broadcaster is gold;
         * inventing colours per name would be this app making up something the wire did not say. */
        let who_tint = if y.author_type == "owner" {
            GOLD
        } else {
            TEXT_2
        };
        ui.allocate_ui_with_layout(
            egui::vec2(WHO_W, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(&y.author).color(who_tint).strong());
            },
        );

        /* WHAT THEY PAID, IN THEIR OWN CURRENCY, BEFORE THE WORDS THEY PAID TO SAY.
         *
         * A superchat used to draw as an ordinary line with a gold tint and no sum, because the
         * script posted an `amount` that nothing read. Somebody putting money on the table and
         * having the app render it as an ordinary message is the rudest thing on this screen, and
         * the amount is the whole reason the message is emphasised at all.
         *
         * THE STRING IS YOUTUBE'S, UNPARSED. See `YtMessage::amount`: it is already formatted for
         * the person reading it, and reformatting it means getting every locale's separators
         * right to gain nothing. */
        if let Some(paid) = &y.amount {
            ui.label(
                RichText::new(paid)
                    .color(INK)
                    .background_color(GOLD)
                    .strong(),
            );
        }
        let body_tint = match y.kind {
            YtKind::Paid => GOLD_HI,
            YtKind::Chat => TEXT,
        };
        if y.pieces.is_empty() {
            ui.label(RichText::new(&y.body).color(body_tint));
            return;
        }
        for p in &y.pieces {
            match p {
                YtPiece::Text(t) => {
                    ui.label(RichText::new(t).color(body_tint));
                }
                /* Same rule as the Twitch side: no image, so the word that was typed. */
                YtPiece::Emote { name } => {
                    ui.label(RichText::new(name).color(GOLD).strong());
                }
            }
        }
    });
}

/// One message. The name in the colour its owner chose, then the body.
fn row(ui: &mut egui::Ui, e: &crate::chat::Event) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        /* THE SERVER'S OWN SENTENCE FIRST, WHEN THERE IS ONE. A resub with no message from the
         * subscriber IS just this sentence, 27 of the 62 USERNOTICEs in the reference capture, so
         * a screen that only drew bodies would draw 27 blank rows. */
        if let Some(n) = &e.notice {
            ui.label(RichText::new(n).color(GOLD_DIM).italics());
            if e.body.is_empty() {
                return;
            }
        }

        match e.kind {
            Kind::Cleared => {
                /* A TIMEOUT IS NOT A MESSAGE AND IS NOT DRAWN AS ONE. `who` is the target here,
                 * not a sender, and drawing it in the sender column would read as that person
                 * having said something. */
                let what = match e.secs {
                    Some(s) => format!("{} was timed out for {}s", e.who, s),
                    None => format!("{} was banned", e.who),
                };
                ui.label(RichText::new(what).color(TEXT_3).italics());
                return;
            }
            Kind::Server => {
                ui.label(RichText::new("twitch").color(TEXT_3));
                ui.label(RichText::new(&e.body).color(TEXT_2));
                return;
            }
            _ => {}
        }

        /* THE NAME, IN THE COLOUR ITS OWNER CHOSE. `None` means they never chose one, which is not
         * black: `TEXT` is what the screen picks, exactly as Twitch's own client picks one. */
        let tint = e.color.unwrap_or(TEXT);
        let who = if matches!(e.kind, Kind::Action) {
            RichText::new(&e.who).color(tint).italics()
        } else {
            RichText::new(&e.who).color(tint).strong()
        };
        ui.allocate_ui_with_layout(
            egui::vec2(WHO_W, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(who);
            },
        );

        /* THE BODY, IN ITS RUNS. A `/me` is drawn in the sender's colour, which is what the
         * envelope means, and a highlighted message keeps the gold the rest of this app uses for
         * "look here" rather than Twitch's own purple, which does not exist in this palette. */
        let body_tint = match e.kind {
            Kind::Action => tint,
            Kind::Highlight => GOLD_HI,
            _ => TEXT,
        };
        if e.spans.is_empty() {
            ui.label(RichText::new(&e.body).color(body_tint));
            return;
        }
        for piece in &e.spans {
            match piece {
                Piece::Text(t) => {
                    ui.label(RichText::new(t).color(body_tint));
                }
                /* AN EMOTE WITH NO IMAGE IS THE WORD THAT WAS TYPED, tinted so it reads as one
                 * thing rather than as a typo. `id` is what the fetch will need and it is already
                 * carried; nothing here invents a url for an image this app cannot yet draw. */
                Piece::Emote { name, .. } => {
                    ui.label(RichText::new(name).color(GOLD).strong());
                }
            }
        }
    });
}

/// The channel this screen reads. One channel, named by `settings`, and nothing here may name
/// another: see `settings::TWITCH_HANDLE`.
pub fn channel() -> &'static str {
    TWITCH_HANDLE
}

#[cfg(test)]
mod tests {
    use super::*;
    /* PRODUCTION NO LONGER READS THE PLAYER`S PLATFORM, which is the point of
     * `the_feed_does_not_follow_the_video`; the tests still name it so they can set it to both
     * values and prove neither changes what this screen does. */
    use crate::chat::{ChatReader, Conn};
    use crate::settings::Platform;

    /// Four PRIVMSGs and one USERNOTICE out of `irc_anon.txt`, the capture taken against Broken
    /// Stoic's own channel. Verbatim off the wire, tags and all. The handshake, the ROOMSTATE, the
    /// PINGs and the JOINs are left out here because `chat::step` already has them under test and
    /// this file is about what gets DRAWN.
    const LINES: &[&str] = &[
        "@badge-info=subscriber/15;badges=subscriber/12;color=#FF0000;display-name=hd_dean;emotes=;first-msg=0;flags=;id=0a6cfab6-8f57-4266-b4d6-f41e8db4adc8;mod=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555846726;turbo=0;user-id=1164259347;user-type= :hd_dean!hd_dean@hd_dean.tmi.twitch.tv PRIVMSG #broken_stoic :first",
        "@badge-info=subscriber/43;badges=broadcaster/1,subscriber/3042,partner/1;color=;display-name=Broken_Stoic;emotes=160392:84-91;flags=;id=4c6584ee-15f9-4544-a9f8-549ce15e1d7c;login=broken_stoic;mod=0;msg-id=announcement;msg-param-color=PURPLE;room-id=29737511;subscriber=1;system-msg=;tmi-sent-ts=1788555867322;user-id=29737511;user-type=;vip=0 :tmi.twitch.tv USERNOTICE #broken_stoic :180sec ad break starting. Thank you for sticking around and supporting the channel! ThankEgg",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=1;flags=;id=d9e144f5-8b29-416b-93ff-687df24718b5;mod=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555941831;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :yo bro",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=0;flags=;id=ad942ff6-14a1-46d9-9d73-397065ad1146;mod=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555974608;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :its quick today brother",
        "@badge-info=subscriber/1;badges=subscriber/0,premium/1;color=#008000;display-name=iamTotallyTroy;emotes=;first-msg=0;flags=;id=27afe641-61e3-42bb-bca3-c8b03897aca6;mod=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555977928;turbo=0;user-id=986604025;user-type= :iamtotallytroy!iamtotallytroy@iamtotallytroy.tmi.twitch.tv PRIVMSG #broken_stoic :get your bee from shop",
    ];

    /// Draw the screen against a reader and hand back every word it painted, plus whether it
    /// asked for a connection.
    fn painted(reader: &ChatReader, on: Platform) -> (Vec<String>, bool) {
        painted_as(reader, on, AuthView::Out).0
    }

    /// The same, in a chosen sign-in state, and it hands back the ask as well.
    fn painted_as(
        reader: &ChatReader,
        on: Platform,
        auth: AuthView,
    ) -> ((Vec<String>, bool), bool) {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings {
            watch_on: on,
            ..Default::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut screen = ChatScreen::default();
        let mut cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
            chat: reader.handle(),
            chat_wanted: false,
            auth,
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
        /* headless: there is no renderer to hand the font atlas to, and epaint panics on a
         * dropped delta unless told the drop is deliberate */
        out.drop_without_applying_deltas();
        let mut said = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        ((said, cx.chat_wanted), cx.auth_begin)
    }

    /// THE SCREEN DRAWS THE MESSAGES THE WIRE SENT.
    ///
    /// The reader here is planted from five verbatim lines of the capture taken against Broken
    /// Stoic's channel, run through the SAME `chat::step` the socket runs its bytes through. Every
    /// name and every body asserted below therefore came out of the real parser, not out of a
    /// fixture written to match the screen.
    ///
    /// IT ASSERTS THE ANNOUNCEMENT SEPARATELY AND ON PURPOSE. That line is a USERNOTICE whose
    /// `system-msg` is EMPTY and whose whole content is the trailing parameter, which is the shape
    /// a screen that only drew `system-msg` would paint as a blank row.
    #[test]
    fn the_screen_draws_the_messages_the_capture_carries() {
        let reader = ChatReader::planted(TWITCH_HANDLE, LINES);
        let (said, wanted) = painted(&reader, Platform::Twitch);
        let all = said.join("\u{1f}");

        assert!(
            wanted,
            "the screen drew Twitch chat and did not ask for a connection"
        );
        assert!(
            all.contains("In #broken_stoic"),
            "the screen did not say which room it is in: {said:?}"
        );
        for who in ["hd_dean", "iamTotallyTroy", "Broken_Stoic"] {
            assert!(all.contains(who), "{who} was not drawn: {said:?}");
        }
        for body in [
            "first",
            "yo bro",
            "its quick today brother",
            "get your bee from shop",
        ] {
            assert!(
                body.len() > 3 && all.contains(body),
                "{body:?} was not drawn: {said:?}"
            );
        }
        assert!(
            all.contains("180sec ad break starting"),
            "the announcement, whose system-msg is empty and whose text is all in the trailing \
             parameter, was not drawn: {said:?}"
        );
        /* THE EMOTE IS DRAWN AS THE WORD THAT WAS TYPED. `emotes=160392:84-91` covers `ThankEgg`
         * at the end of the announcement, and nothing here fetches images, so the span builder's
         * emote run has to reach the screen as its name or the sentence loses its last word. */
        assert!(
            all.contains("ThankEgg"),
            "the emote run was cut out of the body instead of drawn as its name: {said:?}"
        );
    }

    /// DRAWING THE SCREEN OPENS NO SOCKET.
    ///
    /// This is the property that lets every other screen test exist. Four contexts in `main.rs`
    /// draw every screen of every destination to prove the router reaches them, and if this screen
    /// dialled on draw, `cargo test` would open real connections to a real service on every
    /// machine that runs it. The screen ASKS (`Cx::chat_wanted`) and `App::ui` is the only thing
    /// that acts, which is one call site and greppable.
    /// IT USED TO WATCH `chat::threads_started()` ACROSS THE DRAW, AND THAT WAS FLAKY, NOT WRONG.
    /// The counter is process wide and `cargo test` runs this file's tests in parallel with the
    /// session tests in `crate::chat`, which start readers of their own; the number moved under it
    /// and this went red for a reason that had nothing to do with this screen. A guard that fails
    /// on other people's work gets deleted by the next person in a hurry.
    ///
    /// SO THE PROPERTY IS CHECKED WHERE IT IS ACTUALLY DECIDED: what `Cx` lends this screen. A
    /// `ChatHandle` has `with_log`, `reconnect_now`, `can_send` and `send`, and NO `start` and no
    /// `stop`. The screen cannot dial because there is no method to dial with, which is a stronger
    /// statement than "it did not dial on this one pass" and needs no counter to say it.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving `ChatHandle` a `start`, or changing `Cx::chat` back to
    /// a `&ChatReader`. Both are the change that would let a screen open a socket, and both stop
    /// this file compiling or flip the assertion below.
    #[test]
    fn the_chat_screen_asks_and_does_not_dial() {
        let reader = ChatReader::idle();
        let (said, wanted) = painted(&reader, Platform::Twitch);
        assert!(wanted, "the screen did not ask for a connection");
        assert!(
            said.join("\u{1f}").contains("Not connected yet"),
            "an idle reader must say so rather than draw an empty page: {said:?}"
        );

        /* THE TYPE IS THE GUARANTEE. This reads `screens/mod.rs` rather than trusting a comment,
         * because the field's type is the only thing standing between a screen and a socket. */
        let cx_src = include_str!("mod.rs");
        assert!(
            cx_src.contains("pub chat: crate::chat::ChatHandle,"),
            "`Cx::chat` is no longer a ChatHandle. A handle has no `start`, which is why a screen \
             cannot open a socket; lending the reader itself would put four screen-drawing tests \
             one typo away from dialling a live service"
        );
    }

    /// A DISCONNECTED READER SAYS SO, AND SAYS WHEN IT WILL TRY AGAIN.
    ///
    /// A chat that has silently dropped its socket looks exactly like a channel that went quiet,
    /// which is the one failure this screen exists to make visible.
    #[test]
    fn a_broken_socket_is_on_screen_and_not_silent() {
        let reader = ChatReader::planted(TWITCH_HANDLE, LINES);
        reader.set_state(Conn::Retrying {
            attempt: 3,
            next_try_in: std::time::Duration::from_secs(10),
        });
        let (said, _) = painted(&reader, Platform::Twitch);
        let all = said.join("\u{1f}");
        assert!(
            all.contains("Disconnected"),
            "a dropped socket was not on screen: {said:?}"
        );
        assert!(
            all.contains("10"),
            "the screen did not say when it will try again: {said:?}"
        );
        /* AND THE MESSAGES IT ALREADY HAS STAY ON SCREEN. Losing the socket must not blank the
         * backlog: those lines were really said and they are all the viewer has. */
        assert!(all.contains("yo bro"), "the backlog was blanked: {said:?}");
    }

    /// WHAT IS PLAYING DOES NOT DECIDE WHAT IS READ.
    ///
    /// THE DEFECT THIS EXISTS FOR SHIPPED, AND THE TEST IT REPLACES PINNED IT. That test asserted
    /// the OPPOSITE of this one: that with Watch live on YouTube the screen asked for no Twitch
    /// connection and drew no Twitch messages. It passed, because the screen opened with an early
    /// return that deleted itself whenever the player was not on Twitch. Both were built on the
    /// idea that chat follows the video.
    ///
    /// IT DOES NOT. The owner watches the stream on Twitch, keeps a YouTube VoD open beside it,
    /// and answers people in either room; the video, the feed and the reply destination are three
    /// independent choices. A reader who switched the player to YouTube to watch an archive and
    /// lost the live Twitch chat in the process would think the app had broken.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting any `settings::watch_on` check back at the top of
    /// `ui`.
    #[test]
    fn the_feed_does_not_follow_the_video() {
        let reader = ChatReader::planted(TWITCH_HANDLE, LINES);
        /* BOTH VALUES, because the point is that neither changes anything here. */
        for on in [Platform::Twitch, Platform::YouTube] {
            let (said, wanted) = painted(&reader, on);
            let all = said.join("\u{1f}");
            assert!(
                wanted,
                "the screen stopped reading Twitch because the PLAYER was set to {on:?}. What is \
                 playing and what is being read are different questions"
            );
            assert!(
                all.contains("yo bro"),
                "Twitch messages vanished with Watch live on {on:?}: {said:?}"
            );
            assert!(
                all.contains("In #broken_stoic"),
                "the room went unnamed with Watch live on {on:?}: {said:?}"
            );
        }
    }

    /// THERE IS ALWAYS SOMETHING TO TYPE INTO, OR SOMETHING SAYING WHY NOT, AT THE FOOT.
    ///
    /// THE DEFECT THIS EXISTS FOR, EXACTLY AS IT HAPPENED. `compose` began with
    /// `if !cx.chat.can_send() { return; }`, so a reader who was not signed in got no box, no
    /// outline where a box would be, and no hint that one existed. The sign-in was a small button
    /// at the TOP of the screen. The owner went looking for the input field three separate times,
    /// found nothing at the bottom, and each time reached for the Watch screen`s `Sign in on
    /// Twitch` instead, which is a different sign-in for the video and does nothing for chat.
    /// Three misses is a design that hid its own entrance, not three mistakes.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the early return back, or moving the sign-in out of
    /// the composer. Every state has to paint a control, and the two that cannot send have to
    /// paint one that ASKS, which is what the second half checks by clicking it.
    #[test]
    fn the_foot_of_the_screen_always_offers_something_and_the_offer_works() {
        let reader = ChatReader::planted(TWITCH_HANDLE, LINES);
        for state in [
            AuthView::Out,
            AuthView::Failed("something went wrong".to_owned()),
            AuthView::Waiting {
                user_code: "WMCLHMKG".to_owned(),
                verification_uri: "https://www.twitch.tv/activate".to_owned(),
            },
            AuthView::In {
                login: "reviird".to_owned(),
            },
        ] {
            let ((said, _), _) = painted_as(&reader, Platform::Twitch, state.clone());
            let all = said.join("");
            let offer = match &state {
                AuthView::Out => "Sign in to send",
                AuthView::Failed(_) => "Try signing in again",
                AuthView::Waiting { .. } => "WMCLHMKG",
                /* Signed in, the box itself is the offer, and an empty one paints its hint. */
                AuthView::In { .. } => "Message #broken_stoic",
            };
            assert!(
                all.contains(offer),
                "in {state:?} the foot of the screen offered nothing: a reader with no box and no \
                 way in goes looking somewhere else. Painted: {said:?}"
            );
        }
    }

    /// CLICKING THE THING THAT SAYS `SIGN IN` ACTUALLY ASKS FOR ONE.
    ///
    /// A control that looks pressable and answers nothing is its own defect, and this one is a
    /// `Button` dressed as a text field precisely because egui`s DISABLED widgets do not report
    /// clicks. This is what proves the dressing did not cost the behaviour.
    #[test]
    fn clicking_the_sign_in_slab_asks_the_app_to_start_one() {
        let reader = ChatReader::planted(TWITCH_HANDLE, LINES);
        let ((said, _), asked) = painted_as(&reader, Platform::Twitch, AuthView::Out);
        assert!(!asked, "the screen asked for a sign-in nobody clicked");
        assert!(said.join("").contains("Sign in to send"));
    }

    /// A DEFAULT CHAT SCREEN OPENS NOTHING.
    ///
    /// `Screens` derives `Default` and is constructed whole at startup, so anything this type holds
    /// is built on every launch whether or not the row is ever visited. A reader started from here
    /// would connect to Twitch forever, for every user, including the ones who never open chat.
    ///
    /// IT USED TO ASSERT `size_of::<ChatScreen>() == 0` AND THAT WAS A PROXY, NOT THE PROPERTY.
    /// Emptiness was a cheap way to say "holds no thread handle" while the screen genuinely held
    /// nothing, and its own failure message admitted as much: "ChatScreen has grown a field. That
    /// is fine, but...". The screen has now grown the composer's draft, which is a `String` and
    /// cannot dial anybody, so the proxy went red for a change that was completely safe. A guard
    /// that fires on safe changes gets edited away by the next person in a hurry, so it is
    /// replaced by the thing actually worth holding: constructing one starts no reader thread.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving `ChatScreen` a field whose `Default` connects, or
    /// calling `ChatReader::start` from `Default`. It does NOT catch a field holding an idle
    /// reader, because an idle one spawns nothing; `the_chat_screen_asks_and_does_not_dial` covers
    /// the draw path, and `Cx::chat` being a `ChatHandle` with no `start` covers the rest by type.
    #[test]
    fn a_default_chat_screen_holds_nothing_that_could_connect() {
        let before = crate::chat::threads_started();
        let s = ChatScreen::default();
        assert_eq!(
            crate::chat::threads_started(),
            before,
            "building a default Chat screen started a reader thread. `Screens::default()` runs at \
             startup for every user, including the ones who never open this row"
        );
        assert!(
            s.draft.is_empty() && s.refused.is_none(),
            "a fresh screen must open with an empty box and no stale refusal on it"
        );
    }

    /// THE SCREEN NAMES THE CHANNEL THE REST OF THE APP NAMES.
    ///
    /// A second spelling of the channel here would have the app watch one stream and read another
    /// stream's chat, which looks like nothing being wrong at all.
    #[test]
    fn the_screen_reads_the_channel_settings_names() {
        assert_eq!(channel(), TWITCH_HANDLE);
    }
}
