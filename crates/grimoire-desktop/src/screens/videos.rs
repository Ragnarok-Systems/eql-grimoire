//! Screen: Videos. Broken Stoic's own channel page on YouTube, in the folio, full bleed.
//!
//! THE VOCABULARY IS `web/app.html`'S AND THE OWNER'S, the same as `watch`'s:
//!   book / pages (the rail) / leaves (everything right of the rail) / context header /
//!   screen / folio (a screen's own content, below the context header).
//!
//! WHY THIS FILE EXISTS AT ALL, WHICH IS A DEFECT AND NOT A FEATURE REQUEST.
//! `main.rs::draw_screen` sent BOTH `ScreenId::Watch` and `ScreenId::Videos` to `WatchScreen`. That
//! was harmless while the two rows drew byte-identical screens. It stopped being harmless the day
//! the Watch folio became a full bleed player: clicking the rail row "Videos" opened the TWITCH
//! player under the crumb `stoic/videos`, and the word VIDEOS appeared nowhere in the main window.
//! A row whose name points at nothing is the same defect as a control for a feature that does not
//! exist, and it is worse, because the row is the only thing telling the reader what they clicked.
//!
//! IT IS A YOUTUBE SURFACE ON BOTH SETTINGS, AND THAT IS THE OWNER'S CALL, WORTH UNDERSTANDING.
//! Twitch VODs expire. YouTube is where the archive actually lives. So `settings::Platform` decides
//! what WATCH LIVE plays and must not reach this screen at all; a preference leaking in here would
//! be a bug nobody would think to look for, which is why
//! [`tests::the_channel_page_is_the_same_on_both_platform_settings`] drives both values and reads
//! the staged URL back out of a real frame rather than out of a formatter.
//!
//! ONE SURFACE, SHARED WITH WATCH LIVE, AND NOT A SECOND WEBVIEW.
//! `player::Player` holds ONE `wry::WebView` and one `wry::WebContext`, and every rule around it
//! (`built_for`, `last_bounds`, `last_visible`, `problem`, `Sound`, the placement retry, the build
//! that does not retry itself) is written for one. Two surfaces would be a second WebView2
//! environment beside a game, and a second copy of every one of those rules to get wrong. The cost
//! of one is that stepping between Watch live and Videos is a teardown and a rebuild, because
//! `Player::sync` rebuilds whenever `built_for` changes. That cost is small in the direction it
//! actually falls: what Watch live plays is a LIVE stream, which has no position to lose, so a
//! rebuild puts a reader back exactly where they were. Nothing else in the app pays it, because
//! leaving either screen for a third one only HIDES the surface.
//!
//! THE FOLIO IS THE PAGE, OR IT IS THE REASON THERE IS NO PAGE, AND THERE IS NO THIRD THING.
//! See [`folio_of`]. The page is a real webview surface staged by `screens::stage_folio`, which is
//! the same rectangle, the same floor, the same black, the same occlusion rule, the same physical
//! bounds, the same WebView2 profile and the same tracking prevention level the player gets. The
//! words are the player's own refusal or failure, verbatim, because a machine with no WebView2
//! cannot show a page any more than it can show a stream.
//!
//! WHAT THE CONTEXT HEADER CARRIES AND WHY IT IS UP THERE. Nothing may be drawn over a native child
//! window: egui paints UNDER it, so a control laid on the page is not on top of it, it is gone. The
//! header is the one bar left, exactly as it is for Watch live. See [`header`].
//!
//! THERE IS NO STRUCT HERE AND THAT IS DELIBERATE. Every other screen in this app owns state: a
//! selection, a scroll target, a flag the reader set. This one owns none. There is nothing to stop
//! (a page is not a stream), nothing to choose (one channel, one URL, no platform split) and
//! nothing to remember between frames. A `VideosScreen` with no fields would be a zero sized value
//! threaded through `Screens` to hold nothing, which is the same shape of invention as a control
//! for a feature that does not exist. Free functions, called from `main.rs::draw_screen` and from
//! the context bar, are the whole screen.

use crate::screens::Cx;
use crate::theme::*;
use egui::{CornerRadius, FontId, RichText, Stroke, Ui};

/* ----------------------------------------------------------------------- the feed -- */

/// The one feed this screen ever stages: Broken Stoic's channel page on YouTube.
///
/// BUILT FROM `settings::YOUTUBE_HANDLE` AND NEVER FROM A LITERAL. There is one spelling of this
/// channel's handle in the app, it is verified against the channel id in `settings.rs`, and the
/// watcher fetches `/@{handle}/live` off the same constant on every poll. A second spelling here
/// would be a second thing to rot, and it would rot silently, because this one is only read when
/// somebody opens this screen.
///
/// `settings::Platform` IS NOT CONSULTED AND THERE IS NO ARGUMENT FOR IT TO BE. See the module
/// note: this is the archive, and the archive is on YouTube whichever platform a reader prefers to
/// watch live on.
pub fn feed() -> crate::player::Feed {
    crate::player::Feed::YouTubeChannel {
        handle: crate::settings::YOUTUBE_HANDLE.to_owned(),
    }
}

/* ---------------------------------------------------------------------- the folio -- */

/// What the folio is this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Folio {
    /// The channel page, staged as a surface filling the whole folio.
    Page,
    /// No surface, and the reason, which the folio prints verbatim.
    Words(String),
}

/// The folio's whole decision, from the only two facts it depends on.
///
/// EXHAUSTIVE ON PURPOSE, WITH NO UNREACHABLE ARM. Every combination of "is anything wrong with the
/// player" and "is a surface alive right now" is answered here, and every arm that does not produce
/// a page produces the words for it. A folio with nothing in it and no reason for it is the one
/// outcome worse than a sentence, and an `unreachable!` is how that outcome ships.
///
/// THE MIDDLE ARM IS THE RECOVERY PATH AND IT IS THE ONE THAT IS NOT OBVIOUS. A
/// [`crate::player::Problem::Failed`] with a LIVE surface is a PLACEMENT call that did not work on
/// a webview that is still there. The folio is what produces the `Stage`, the `Stage` is what makes
/// the App call `Player::sync`, and `Player::sync` is where the failed `set_bounds` is pushed
/// again; dropping the page on any failure at all would remove the only thing that could produce
/// the retry and make one transient error permanent. With no surface alive a failure DOES drop the
/// page, because then there is nothing behind the rectangle and a reserved black slab with no
/// webview in it is a dead frame.
///
/// IT IS THE SAME RULE `watch::playback` USES FOR ITS VIDEO BAND, WITH THE READER'S FLAG READ AS
/// "ALWAYS ASKING", because arriving on a row called Videos IS the ask and there is no Stop on this
/// screen for it to be undone by. It is written out rather than delegated so that this screen's
/// answer does not move when Watch live's playback controls change; the two are held together
/// instead by [`tests::the_two_folios_agree_about_when_a_surface_is_staged`], which drives every
/// combination through both and asserts they never disagree. A test that PROVES agreement is worth
/// more than a call that assumes it.
pub fn folio_of(problem: Option<&crate::player::Problem>, playing: bool) -> Folio {
    match problem {
        /* Nothing is wrong: the page. */
        None => Folio::Page,
        /* A refusal is about the MACHINE, not about any one surface: there is no per-user data
         * folder for the WebView2 profile, or there is no WebView2 at all. No click changes it, so
         * no control offers to and the folio says so. */
        Some(p @ crate::player::Problem::Refused(_)) => Folio::Words(p.words().to_owned()),
        /* A failure with a surface still up: see the note above. Keep staging. */
        Some(crate::player::Problem::Failed(_)) if playing => Folio::Page,
        /* A failure with no surface is a BUILD that did not work. There is nothing to place and
         * nothing to show, so the reason is what the folio is. */
        Some(p) => Folio::Words(p.words().to_owned()),
    }
}

/// The folio: the channel page, or the reason there is not one.
///
/// THE PAGE IS STAGED WITH SOUND OFF AND THAT IS NOT A DEFAULT, IT IS THE ONLY VALUE.
/// `Stage::sound` is what the player module turns into a mute flag in an embed URL, and a channel
/// page has no embed URL and no player of ours to mute. The surface is also built without
/// `with_autoplay` on this path (`player::surface::Player::build`), so nothing on the page starts
/// by itself. There is no Sound control on this screen because there is nothing for one to reach,
/// which is the same rule that keeps one off the Watch header when no stage is being handed over.
pub fn folio(ui: &mut Ui, cx: &mut Cx) {
    match folio_of(cx.player.problem.as_ref(), cx.player.playing) {
        Folio::Page => crate::screens::stage_folio(ui, cx, feed(), false),
        Folio::Words(why) => {
            let rect = ui.available_rect_before_wrap();
            ui.allocate_rect(rect, egui::Sense::hover());
            crate::screens::centred_words(
                ui,
                rect,
                vec![
                    (why, FontId::proportional(12.5), TEXT_3),
                    /* THE ADDRESS, BECAUSE IT IS THE ONE THING LEFT THAT A READER CAN ACT ON.
                     * The sentence above says this machine cannot host a surface, and that is a
                     * sentence with no control under it: there is no browser button on this screen
                     * (the folio is the page, and a button under it would mean the folio is not
                     * full bleed). A URL is not a control, it is a fact, and it is the fact that
                     * gets somebody to the archive in the browser they already have. */
                    (
                        crate::player::channel_page_url(crate::settings::YOUTUBE_HANDLE),
                        FontId::monospace(11.0),
                        TEXT_2,
                    ),
                ],
            );
        }
    }
}

/* --------------------------------------------------------------------- the header -- */

/// The one control this screen has, in the context header beside the crumb.
///
/// WHAT IT DOES, IN ONE SENTENCE: it drops the surface, and the folio asks for one again on the
/// very next frame, so the webview is rebuilt pointing at [`feed`] whatever it had wandered off to.
///
/// IT IS `Ask::StopPlayer` AND THERE IS NO SECOND ASK FOR IT. `Player::stop` is exactly "drop the
/// surface and clear a failure that could be retried", and that is precisely what a reload is. A
/// `ReloadPlayer` variant beside it would be a second name with the same body, which is a thing a
/// reader has to work out is not a difference. What differs is the SCREEN: Watch live sets its own
/// `stopped` flag beside the ask, so the surface stays down; this screen has no such flag, so the
/// same ask reads as a reload.
pub const RELOAD: &str = "Reload the channel";

/// The word the header carries when this app's own last look at YouTube did not succeed.
///
/// THE NO NETWORK STORY, AND IT IS THE HONEST ONE RATHER THAN THE FLATTERING ONE.
/// A webview pointed at a page it cannot fetch paints the ENGINE's error, in Edge's voice, and
/// there is nothing this app can do about that rectangle: a native child window owns it and egui
/// paints underneath. What this app CAN do is say, in its own voice, in the one bar it has, whether
/// its own last attempt on YouTube got through. `watcher::Channel::error` is exactly that fact: it
/// is set by a poll that failed and cleared by the next one that succeeded. Beside Edge's error
/// page that answers the reader's actual question, which is whether it is them or it is YouTube;
/// beside a page that loaded, it is a note about the live state and costs nothing.
///
/// THE FOLIO IS NOT GATED ON IT, AND THAT IS THE JUDGEMENT. Refusing to stage the surface when the
/// last poll failed would be this app diagnosing a network from a fact that is not one. The
/// YouTube poller fetches `/@{handle}/live` and fails for transport reasons ("no route to host")
/// AND for content reasons ("did not look like a channel page (no ytInitialData); consent wall or
/// block?"). The second kind is a page that loads perfectly well in a webview, so gating on the
/// error would black out a working page and tell the reader the network was down while they were
/// using it. Two different facts, one string, and the app does not get to guess which it holds.
/// It states what it measured and stages the page anyway.
///
/// IT SAYS WHAT WAS MEASURED AND STOPS THERE, AND THE CLAUSE IT LOST IS THE POINT. It read "the
/// last YouTube check failed, so this page may not load", and that second half was this file
/// inferring something about the PAGE out of a fact about the POLLER, three paragraphs after
/// arguing that the two are different facts. The check failing is measured. Whether this page
/// loads is not, and the reader can see the answer to that in the folio for themselves.
///
/// IT ALSO HAD TO BE SHORTER, MEASURED. Laid out headless at 11.5 points, the old sentence ran to
/// 287 points and this one runs to about 150; the signed out line is 200. The bar carries the
/// crumb, the button (108), one of these, the Pop out control and the find box, and at the
/// narrowest this window can be (`min_inner_size` 880 wide, less `chrome::RAIL_WIDE` at 252) the
/// leaves are 628 points. The worst case is now the PERMANENT line rather than this one. The full
/// error, its age and the rung that produced it are printed in the pop-out window's YouTube line
/// (`watch::checked_line`), which is where detail belongs.
///
/// AND IT REPLACES THE SIGNED OUT LINE RATHER THAN SITTING BESIDE IT. Two runs of text plus a
/// button plus the crumb plus Pop out plus the find box does not fit those 628 points. One slot,
/// and the transient fact takes it from the permanent one while there is a transient fact to tell;
/// [`SIGNED_OUT`] is true every frame and can wait for the next one.
pub const POLL_FAILED: &str = "the last YouTube check failed";

/// Who this surface is watching as, which is nobody, said out loud because on a CHANNEL PAGE it is
/// visible in a way it never is on a player.
///
/// THE PLAYER SCREEN MAKES THE SAME POINT AT LENGTH AND CANNOT MAKE IT HERE.
/// `watch::signed_out_line` is three clauses long and is drawn in the FOLIO, in the one state where
/// the reader is deciding whether to watch in this window. This screen has no such state: the folio
/// is the page from the first frame, nothing may be drawn over it, and there is no decision to
/// inform. The header is the only place, and a header line has to be short.
///
/// WHY IT IS WORTH THE SLOT AT ALL. A channel page draws YouTube's own "Sign in" button, several
/// times, in a webview whose profile belongs to this app (`player::profile_dir`, under
/// LOCALAPPDATA) and not to the reader's browser. Two things follow that a reader cannot see: that
/// button will not pick up the session they already have, and a Google sign-in completed inside it
/// WOULD be kept, by this app, in a folder this app owns. There is no sign-in flow here and this
/// app is not asking for one. Stating the absence promises nothing and is the only warning the
/// shape of the screen allows.
///
/// UNCONDITIONAL, LIKE `watch::signed_out_line`, AND FOR THE SAME REASON. A WebView2 profile cannot
/// be attached to a person's Edge or Chrome profile, so there is no state in which this is false.
/// A fact that is always true is printed rather than guarded.
pub const SIGNED_OUT: &str = "signed out, and there is no sign-in here";

/// Which of the two words the bar carries. See [`POLL_FAILED`] for why there is one slot.
pub fn header_word(youtube: &crate::watcher::Channel) -> &'static str {
    match youtube.error {
        Some(_) => POLL_FAILED,
        None => SIGNED_OUT,
    }
}

/// THE CONTEXT HEADER, drawn by `main.rs` beside the crumb, and only while this screen is showing.
///
/// NO HOVER TEXT ON ANYTHING IN HERE, AND THAT IS A RULE RATHER THAN AN OMISSION. It is
/// `watch::WatchScreen::header`'s rule and it holds for the same mechanical reason: a tooltip is an
/// `egui::Area` in `Order::Tooltip`, which `player::floats_above_body` counts as floating over the
/// body, and the folio begins immediately under this bar. A tooltip dropped from a control here
/// lands on the page and `occluded_by_overlays` hides the surface for as long as the pointer rests
/// on it. A hover that blanks the screen is worse than a terse control.
///
/// THE ONE CONTROL, AND IT ANSWERS BOTH OF THIS SCREEN'S OPEN PROBLEMS WITH ONE ACTION.
///
/// A CHANNEL PAGE IS A REAL BROWSER SURFACE. A person can click a video, a comment, another
/// channel, a search result, and end up anywhere on YouTube, and this app has no back button in its
/// chrome because it has no chrome to put one in. Doing nothing about that was considered and is
/// not defensible HERE, whatever it might be elsewhere: the way back would be to leave the screen
/// and return, and leaving a screen only HIDES the surface, so the reader would come back to
/// whatever they wandered to. The rail row would once again be a row whose name points at nothing,
/// which is the exact defect this screen was built to fix. So there is a way back, it is one word,
/// and it names its destination.
///
/// AND IT IS THE SAME CONTROL A FAILED LOAD NEEDS. A page that could not be fetched shows Edge's
/// error with Edge's refresh button; this is the app's own, in the app's own voice, and it rebuilds
/// the surface rather than reloading the frame, so it also retries a BUILD that failed
/// (`Player::stop` clears a retryable problem). One control, two problems, no invention.
///
/// IT IS ALWAYS ENABLED AND THAT IS NOT A SHRUG. Pressing it while already on the channel reloads
/// the channel, which is a real thing to want when a page came up wrong. The alternative would be
/// to enable it only after the reader had navigated away, which means reading the webview's current
/// address back every frame across a COM boundary to decide whether a button is grey. The control
/// does exactly what it says in every state, so nothing is gained by that and a per-frame readback
/// is spent.
pub fn header(ui: &mut Ui, cx: &mut Cx) {
    ui.add_space(18.0);
    let button = egui::Button::new(
        RichText::new(RELOAD)
            .font(FontId::proportional(12.5))
            .color(TEXT),
    )
    .fill(PANEL)
    .stroke(Stroke::new(1.0, RULE))
    .corner_radius(CornerRadius::ZERO);
    if ui.add(button).clicked() {
        /* See [`RELOAD`]: this screen has no `stopped` flag, so the door that ENDS a stream on the
         * Watch screen RELOADS a page on this one, and the next frame stages [`feed`] again. */
        cx.ask = crate::screens::Ask::StopPlayer;
    }
    ui.add_space(10.0);
    ui.label(
        RichText::new(header_word(&cx.live.youtube))
            .font(FontId::proportional(11.5))
            .color(TEXT_3),
    );
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::{Feed, Problem};

    fn status() -> crate::watcher::Status {
        crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        }
    }

    /// Everything one headless pass of this screen produced.
    struct Drawn {
        stage: Option<crate::player::Stage>,
        runs: Vec<(String, egui::Rect)>,
        ask: crate::screens::Ask,
        folio: egui::Rect,
    }

    impl Drawn {
        fn words(&self) -> Vec<String> {
            self.runs.iter().map(|(w, _)| w.clone()).collect()
        }
        fn spot(&self, s: &str) -> Option<egui::Pos2> {
            self.runs
                .iter()
                .find(|(w, _)| w == s)
                .map(|(_, r)| r.center())
        }
        /// The URL the surface was actually staged at, read back out of the feed the frame
        /// produced. `None` when no surface was staged.
        fn staged_url(&self) -> Option<String> {
            self.stage
                .as_ref()
                .map(|s| crate::player::load(&s.feed, s.sound).0)
        }
    }

    /// One whole frame of this screen: the context header, then the folio, in the order `main.rs`
    /// shows them (the top panel before the central panel), so a click in the bar lands before the
    /// folio reads its consequences.
    fn draw(
        ctx: &egui::Context,
        on: crate::settings::Platform,
        live: &crate::watcher::Status,
        player: crate::player::PlayerView,
        events: Vec<egui::Event>,
    ) -> Drawn {
        crate::theme::install(ctx);
        let mut settings = crate::settings::Settings {
            watch_on: on,
            ..crate::settings::Settings::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live,
            settings: &mut settings,
            ingest: &mut ingest,
            player,
            stage: None,
            demand: None,
            ask: Default::default(),
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        };
        let mut folio_rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            ui.horizontal(|ui| header(ui, &mut cx));
            folio_rect = ui.available_rect_before_wrap();
            folio(ui, &mut cx);
        });
        assert!(!out.shapes.is_empty(), "nothing was painted");
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();

        /* `Shape::Vec` is nested by egui, and a test that read only the top level would find
         * nothing and pass, which is the shape of every reachability failure this tree has had. */
        fn flatten(s: egui::Shape, out: &mut Vec<egui::Shape>) {
            match s {
                egui::Shape::Vec(v) => {
                    for x in v {
                        flatten(x, out);
                    }
                }
                other => out.push(other),
            }
        }
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        Drawn {
            stage: cx.stage,
            runs: flat
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_owned(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                    )),
                    _ => None,
                })
                .collect(),
            ask: cx.ask,
            folio: folio_rect,
        }
    }

    fn player_up() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            problem: None,
            playing: true,
            ..Default::default()
        }
    }

    /// THE TEST THE BRIEF ASKED FOR, AND THE BUG A READER WOULD NEVER THINK TO LOOK FOR.
    ///
    /// `settings::Platform` decides what WATCH LIVE plays. It has no business on this screen: the
    /// archive is on YouTube whichever platform a reader prefers to watch live on. A preference
    /// leaking in here would show up as the Videos row playing Twitch for half the machines in the
    /// world and nothing at all being obviously wrong on the other half.
    ///
    /// IT DRIVES A WHOLE FRAME AND READS THE URL BACK OUT OF THE STAGE, not out of a formatter.
    /// `feed()` ignoring the preference is easy to assert and proves nothing about the PATH: the
    /// defect this replaces was a routing decision two files away. What is asserted is that the
    /// rectangle handed to the App carries the same address under both settings, and that the
    /// address is the channel's.
    #[test]
    fn the_channel_page_is_the_same_on_both_platform_settings() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let mut seen: Vec<String> = Vec::new();
        for on in crate::settings::Platform::ALL {
            let d = draw(&ctx, on, &live, player_up(), Vec::new());
            let url = d
                .staged_url()
                .unwrap_or_else(|| panic!("{on:?} staged no surface at all"));
            assert_eq!(
                url,
                format!(
                    "https://www.youtube.com/@{}",
                    crate::settings::YOUTUBE_HANDLE
                ),
                "the Videos folio must show the channel page, on {on:?}"
            );
            assert!(
                !url.contains("twitch"),
                "the preference reached this screen: {on:?} staged {url}"
            );
            seen.push(url);
        }
        assert_eq!(
            seen[0], seen[1],
            "the two platform settings staged different addresses, so the preference reached a \
             screen it has no business on"
        );
    }

    /// THE ROW FINALLY SHOWS WHAT IT SAYS: the surface this screen stages is a YOUTUBE CHANNEL and
    /// never a player.
    ///
    /// This is the assertion that would have caught the defect as it shipped. `draw_screen` sent
    /// `ScreenId::Videos` to `WatchScreen`, so the Videos row staged `Feed::Twitch` under the crumb
    /// `stoic/videos`. Reading the feed's VARIANT back, and not only its URL, is what pins the kind
    /// of surface rather than the string it happened to produce.
    #[test]
    fn the_videos_folio_stages_a_channel_page_and_never_a_player() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            player_up(),
            Vec::new(),
        );
        let stage = d.stage.as_ref().expect("the folio stages a surface");
        assert!(
            matches!(&stage.feed, Feed::YouTubeChannel { handle } if handle == crate::settings::YOUTUBE_HANDLE),
            "the Videos folio staged {:?}",
            stage.feed
        );
        assert!(
            !stage.sound,
            "a channel page has no player of ours to unmute and must never be staged asking for \
             sound"
        );
    }

    /// A CHANNEL PAGE IS LOADED TOP LEVEL AND IS NEVER PUT IN OUR HOST PAGE.
    ///
    /// MEASURED 2026-09-03: `https://www.youtube.com/@broken_stoic` answers `200` carrying
    /// `X-Frame-Options: SAMEORIGIN`, and none of its three CSP headers carries `frame-ancestors`.
    /// So framing is refused outright for a cross origin parent, and `https://grimoire.localhost`
    /// is a cross origin parent. Reusing the player's host page here would have looked like sharing
    /// and would have produced a blank folio with nothing to say about it.
    #[test]
    fn the_channel_page_is_loaded_top_level_and_nothing_of_ours_is_served() {
        let (url, served) = crate::player::load(&feed(), false);
        assert_eq!(url, "https://www.youtube.com/@broken_stoic");
        assert!(
            served.is_none(),
            "a channel page carries X-Frame-Options: SAMEORIGIN and must not be framed by our own \
             host page; got {served:?}"
        );
        /* And the two framed feeds still are framed, or this assertion would pass for the wrong
         * reason on a build where `load` had stopped serving anything at all. */
        let (u, p) = crate::player::load(
            &Feed::Twitch {
                login: "broken_stoic".into(),
            },
            false,
        );
        assert_eq!(u, crate::player::page_url());
        assert!(p.is_some(), "the twitch player is still framed");
    }

    /// THE FOLIO IS THE WHOLE FOLIO, edge to edge, exactly as the player's is. A page inset by a
    /// margin is a frame this app has drawn around somebody else's site.
    #[test]
    fn the_page_fills_the_folio_and_leaves_no_border_of_its_own() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let d = draw(
            &ctx,
            crate::settings::Platform::YouTube,
            &live,
            player_up(),
            Vec::new(),
        );
        let stage = d.stage.as_ref().expect("the folio stages a surface");
        assert_eq!(
            stage
                .body_rect()
                .expect("the Videos surface is always the body's; it has no pop-out"),
            d.folio,
            "the staged rectangle is not the folio; a border would be egui's ink showing beside a \
             native child window"
        );
    }

    /// THE TWO FOLIOS NEVER DISAGREE ABOUT WHEN A SURFACE IS STAGED.
    ///
    /// [`folio_of`] is written out rather than delegating to `watch::playback`, so that this
    /// screen's answer does not move when Watch live's playback CONTROLS change. This is what pays
    /// for that: every combination of the two facts is driven through both rules and they are held
    /// to the same answer. `watch::playback`'s reader flag is `true` here because arriving on a row
    /// called Videos is the ask and there is no Stop on this screen to undo it.
    ///
    /// A test that PROVES agreement is worth more than a call that assumes it: a call would make
    /// the two identical by construction and would say nothing at all about whether they SHOULD be.
    #[test]
    fn the_two_folios_agree_about_when_a_surface_is_staged() {
        let refused = Problem::Refused("no WebView2".to_owned());
        let failed = Problem::Failed("could not place the player".to_owned());
        let mut checked = 0;
        for problem in [None, Some(&refused), Some(&failed)] {
            for playing in [false, true] {
                let can_play = problem.map_or(true, |p: &Problem| p.can_retry());
                let watch =
                    crate::screens::watch::playback(true, can_play, playing, problem.is_some())
                        .video;
                let here = folio_of(problem, playing) == Folio::Page;
                assert_eq!(
                    here, watch,
                    "the Videos folio and the Watch band disagree for problem {problem:?}, \
                     playing {playing}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 6, "the table was not walked");
    }

    /// A MACHINE THAT CANNOT HOST A SURFACE GETS THE REASON AND THE ADDRESS, NEVER A BLACK SLAB.
    ///
    /// Both halves are asserted. The reason is the player's own sentence, verbatim, because this
    /// screen has no better one and inventing a second wording for one machine fact is how two
    /// screens start contradicting each other. The address is what is left that a reader can act
    /// on: there is no browser button on this screen, because a button under the folio would mean
    /// the folio is not full bleed.
    #[test]
    fn a_machine_with_no_surface_gets_the_reason_and_the_address() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let refused = crate::player::PlayerView {
            problem: Some(Problem::Refused(
                "embedded playback needs Microsoft WebView2".to_owned(),
            )),
            ..Default::default()
        };
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            refused,
            Vec::new(),
        );
        assert!(
            d.stage.is_none(),
            "a refused machine must not be handed a rectangle for a surface that cannot exist"
        );
        let words = d.words();
        assert!(
            words
                .iter()
                .any(|w| w == "embedded playback needs Microsoft WebView2"),
            "the folio must print the player's own reason; got {words:?}"
        );
        assert!(
            words
                .iter()
                .any(|w| w == "https://www.youtube.com/@broken_stoic"),
            "the folio must name the address a reader can still open; got {words:?}"
        );
    }

    /// A BUILD THAT FAILED SAYS SO AND STOPS ASKING; A PLACEMENT THAT FAILED KEEPS ASKING.
    ///
    /// The second half is the recovery path and it is the half that would rot silently: the folio
    /// is the only thing that produces a `Stage`, the `Stage` is the only thing that makes the App
    /// call `Player::sync`, and `sync` is where a failed `set_bounds` is pushed again. A folio that
    /// dropped the page on any failure at all would make one transient error permanent and nothing
    /// on screen would look wrong.
    #[test]
    fn a_failed_placement_keeps_the_page_and_a_failed_build_does_not() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let misplaced = crate::player::PlayerView {
            problem: Some(Problem::Failed(
                "could not place the player: gone".to_owned(),
            )),
            playing: true,
            ..Default::default()
        };
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            misplaced,
            Vec::new(),
        );
        assert!(
            d.stage.is_some(),
            "a live surface whose last placement failed must keep being staged, or nothing is \
             left to retry it"
        );

        let build_failed = crate::player::PlayerView {
            problem: Some(Problem::Failed(
                "could not create the player surface: no runtime".to_owned(),
            )),
            playing: false,
            ..Default::default()
        };
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            build_failed,
            Vec::new(),
        );
        assert!(
            d.stage.is_none(),
            "a build that failed leaves nothing behind the rectangle, so reserving one is a dead \
             frame"
        );
        assert!(
            d.words()
                .iter()
                .any(|w| w == "could not create the player surface: no runtime"),
            "and the reason is what the folio is instead; got {:?}",
            d.words()
        );
    }

    /// THE WAY BACK FROM ANYWHERE ON YOUTUBE IS ONE ENABLED CONTROL, AND IT IS PROVED BY CLICKING.
    ///
    /// Reading the label back proves the bar says "Reload the channel". Only a click proves the
    /// button is ENABLED and wired, and a disabled `egui::Button` paints its text just the same, so
    /// the words alone cannot tell the two apart. The ask is `StopPlayer` because `Player::stop`
    /// is exactly "drop the surface and clear a retryable failure", which on a screen with no
    /// stopped flag is a reload; the folio stages [`feed`] again on the very next frame.
    #[test]
    fn the_reload_control_is_enabled_and_asks_the_app_to_drop_the_surface() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = status();
        let first = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            player_up(),
            Vec::new(),
        );
        let at = first
            .spot(RELOAD)
            .unwrap_or_else(|| panic!("the bar draws no {RELOAD:?}; it drew {:?}", first.words()));
        assert_eq!(
            first.ask,
            crate::screens::Ask::None,
            "nothing was clicked yet"
        );
        let clicked = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &live,
            player_up(),
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::StopPlayer,
            "the reload control must reach the App, or it is a button that does nothing"
        );
    }

    /// THE BAR SAYS WHO IS WATCHING, AND SAYS SOMETHING ELSE WHEN THE APP'S OWN LAST CHECK FAILED.
    ///
    /// Both are the same slot, deliberately: see [`POLL_FAILED`]. The signed out line is true every
    /// frame and can wait; the poll failure is transient and is the one a reader can act on.
    ///
    /// AND THE SURFACE IS STAGED EITHER WAY, which is the half that would be easy to get wrong in
    /// the name of being helpful. This app's YouTube poller fails for transport reasons AND for
    /// content reasons (a consent wall is a page that loads perfectly well in a webview), so
    /// refusing to stage on a failed poll would black out a working page and blame the network.
    #[test]
    fn the_bar_states_the_signed_out_fact_and_yields_it_to_a_failed_check() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);

        let ok = status();
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &ok,
            player_up(),
            Vec::new(),
        );
        assert!(
            d.words().iter().any(|w| w == SIGNED_OUT),
            "the bar must say who this surface is watching as; got {:?}",
            d.words()
        );

        let mut down = status();
        down.youtube.error = Some("no route to host".to_owned());
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &down,
            player_up(),
            Vec::new(),
        );
        let words = d.words();
        assert!(
            words.iter().any(|w| w == POLL_FAILED),
            "a failed check must be said in this app's own voice, not left to the engine's error \
             page; got {words:?}"
        );
        assert!(
            !words.iter().any(|w| w == SIGNED_OUT),
            "there is one slot in this bar and the transient fact takes it; got {words:?}"
        );
        assert!(
            d.stage.is_some(),
            "a failed poll is not a diagnosis of the network, and the page is staged anyway"
        );

        /* AND THE BAR READS THE YOUTUBE CHANNEL, IN THE FRAME AND NOT ONLY IN THE FORMATTER.
         * `header_word` taking the right channel proves nothing about the line that hands it one,
         * and the twitch channel is exactly what a line copied out of `watch::header` would have
         * reached for: everything that screen's bar reads is Twitch's. A Twitch failure is not this
         * screen's business, so the bar carries on saying the permanent thing. */
        let mut twitch_down = status();
        twitch_down.twitch.error = Some("gql: 503".to_owned());
        let d = draw(
            &ctx,
            crate::settings::Platform::Twitch,
            &twitch_down,
            player_up(),
            Vec::new(),
        );
        let words = d.words();
        assert!(
            words.iter().any(|w| w == SIGNED_OUT) && !words.iter().any(|w| w == POLL_FAILED),
            "a Twitch poll failure reached a YouTube screen's bar; got {words:?}"
        );
    }

    /// THE PREFERENCE DOES NOT REACH THE BAR EITHER.
    ///
    /// `header_word` reads the YOUTUBE channel and only ever that one, whatever the reading
    /// preference says. Held here because the twitch channel is the one a copied line would have
    /// reached for: `watch`'s own header and status band are both Twitch's.
    #[test]
    fn the_bar_reads_the_youtube_channel_and_never_the_twitch_one() {
        let mut st = status();
        st.twitch.error = Some("gql: 503".to_owned());
        assert_eq!(
            header_word(&st.youtube),
            SIGNED_OUT,
            "a Twitch failure is not this screen's business"
        );
        st.youtube.error = Some("no route to host".to_owned());
        assert_eq!(header_word(&st.youtube), POLL_FAILED);
        st.youtube.error = None;
        assert_eq!(
            header_word(&st.youtube),
            SIGNED_OUT,
            "a check that succeeded clears it, which is what `watcher::merge` does to the field"
        );
    }

    /// The house forbids both dashes in code, comments and UI strings alike.
    #[test]
    fn no_dashes_anywhere_in_this_file() {
        let src = include_str!("videos.rs");
        for (i, line) in src.lines().enumerate() {
            assert!(!line.contains('\u{2014}'), "em dash on line {}", i + 1);
            assert!(!line.contains('\u{2013}'), "en dash on line {}", i + 1);
        }
    }
}
