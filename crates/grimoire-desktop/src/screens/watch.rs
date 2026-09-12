//! Screen: Watch. The Broken Stoic window. Decisions D1 (the one webview exception), D2 (live
//! status and its sources), D5 (STOIC: Watch live, Videos), D8 (the pill), D9.
//!
//! THE VOCABULARY, WHICH IS `web/app.html`'S AND THE OWNER'S, NOT THIS FILE'S INVENTION.
//!   book            the whole window
//!   pages           the left rail
//!   leaves          everything to the right of the rail
//!   context header  the bar at the top of the leaves: breadcrumb, Pop out, find box
//!   screen          one whole view. One is showing at a time.
//!   folio           that screen's own content, below the context header
//!
//! WHAT THIS SCREEN IS NOW, AND IT IS THE OWNER'S SENTENCE: "leaves should be our player or the
//! offline image". THE FOLIO IS THE PICTURE OR THE VIDEO, EDGE TO EDGE. The heading, the last
//! checked line, Check now, the refusal sentence, the two browser buttons and the VIDEOS band are
//! all gone from the main window's body. They were six ways of saying the one thing a picture with
//! OFFLINE across it says once, and while the stream is up they were six things competing with it.
//!
//! WHERE THE CONTROLS WENT, AND WHY THERE WAS ONLY ONE PLACE FOR THEM. Two constraints agree.
//! NOTHING MAY BE DRAWN OVER THE VIDEO RECT: a native child window owns its rectangle, egui paints
//! underneath it, and the Twitch Developer Services Agreement is read as forbidding app chrome over
//! the player. So a control cannot be laid on the folio while the folio is the video, and a control
//! UNDER the folio would need the folio to stop being edge to edge. The CONTEXT HEADER is the one
//! bar left, and it is where Stop and Sound live now: [`WatchScreen::header`], drawn by `main.rs`
//! beside the breadcrumb and the Pop out control. The header itself stays; losing the breadcrumb
//! and the find box on one screen is a real cost and nobody asked for it.
//!
//! THE FOUR THINGS THE FOLIO CAN BE, in the order it decides them.
//!   1. the video, filling the folio, whenever there is a surface to stage.
//!   2. the channel's own offline artwork, filling the folio, with OFFLINE laid across it, when the
//!      poller says the channel is off and `channel_art` has a picture.
//!   3. the sentence saying why there is neither, in every state the two above do not cover: not
//!      polled yet, live on YouTube with no video id, a player this machine refuses, a build that
//!      failed, an offline channel whose artwork has not arrived. A folio with nothing in it and no
//!      reason for it is the one outcome that is worse than a sentence.
//!   4. beside that sentence, and only where the reader is DECIDING whether to watch here (the feed
//!      is playable and the reader has stopped), the two facts about this player that a reader is
//!      owed before they choose it: [`signed_out_line`], and the profile folder with its tracking
//!      prevention level. They cannot be shown while the player runs, because nothing may be drawn
//!      over the video, so they are shown at the only moment they can be acted on.
//!
//! AND THE STREAM STARTS BY ITSELF NOW. The reader reached a row called "Watch live"; that IS the
//! ask, and a folio that is meant to BE the player but shows a black rectangle until a button is
//! found elsewhere is not the player. The old `watch_here` flag is inverted into `stopped`, so the
//! bandwidth argument it was written for still holds: the app never opens on this screen (`main.rs`
//! starts on the parser), nothing persists the flag, and Stop is one click in the header.
//!
//! THERE IS ONE WINDOW THAT DRAWS THIS SCREEN AND IT IS THE MAIN ONE. There used to be two: a
//! `Host` argument picked between the folio and a status PAGE that the pop-out window drew, and
//! that page had no production caller from the day `windows.rs` started drawing its own picture
//! in picture body. It is deleted, along with the status line, the age, the browser buttons and
//! the VIDEOS band that only it printed. The Watch tool window shares exactly two functions with
//! this module, [`paint_art`] and [`fit_into`], so OFFLINE is laid on the artwork by the same
//! code in both windows; `no_other_window_draws_this_screen` is what holds that line.
//!
//! AND WHEN NOTHING CAN PLAY. `feed_for` answers a live channel with a `player::Feed` and
//! everything else with the SENTENCE saying why, which this screen prints verbatim: not live, not
//! known yet, or live on YouTube with no video id. An offline channel is never handed to a player
//! to render its own idea of offline (Twitch fills an offline channel with other people's streams);
//! the surface is simply not asked for.
//!
//! WHAT IS BORROWED AND WHAT IS OWNED. The state WORD comes from `crate::titlebar::dot_of`
//! through [`state_label`], and the age from `crate::titlebar::age_of`: both are the strip pill's
//! own rules, so the pill above this body and the lines inside it cannot print two words or two
//! numbers for one fact. This file owns the status line and the checked line, which nothing else
//! draws.
//!
//! WHY THE FORMATTERS TAKE A `View` AND NOT THE WATCHER'S `Channel`.
//! `View::of` is the single line that reads the watcher lane's struct. Every formatter and every
//! test below works on primitives, so a field the watcher lane adds or renames touches one
//! function here and nothing else.

use crate::screens::Cx;
use crate::theme::*;
use crate::watcher::Channel;
use chrono::{DateTime, Utc};
use egui::{CornerRadius, FontId, RichText, Stroke, Ui};

/* ------------------------------------------------------------------- the view -- */

/// The screen's reading of one channel, in primitives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub handle: String,
    /// `None` is "never checked, or the last poll failed with nothing known" (contract).
    pub live: Option<bool>,
    pub title: Option<String>,
    pub game: Option<String>,
    pub viewers: Option<u64>,
    /// The live video's id, on the platform that needs one. See [`crate::watcher::Channel::video_id`]
    /// for why YouTube cannot be embedded without it and why Twitch never has one.
    pub video_id: Option<String>,
    pub checked_at: Option<DateTime<Utc>>,
    /// Which poller answered: the checked line names it (D2).
    pub source: String,
    pub error: Option<String>,
}

impl View {
    /// THE ONE PLACE THIS SCREEN READS `crate::watcher::Channel`.
    pub fn of(ch: &Channel) -> View {
        View {
            handle: ch.handle.clone(),
            live: ch.live,
            title: ch.title.clone(),
            game: ch.game.clone(),
            viewers: ch.viewers,
            video_id: ch.video_id.clone(),
            checked_at: ch.checked_at,
            source: ch.source.to_owned(),
            error: ch.error.clone(),
        }
    }
}

/* ------------------------------------------------------------------- the stage -- */

/* THERE IS NO `Host` ANY MORE, AND WHAT IT USED TO PICK BETWEEN WAS ONE LIVE SCREEN AND ONE
 * DEAD ONE.
 *
 * It had two variants. `Body` is the main window's folio. `PopOut` drew a status page in the
 * pop-out window: the state line, the age, Check now, the artwork, two browser buttons, a
 * caption and a VIDEOS band. That page stopped being reachable the day `windows::draw_child`
 * started asking `Slot::is_pip` and returning before any screen is drawn, and NOTHING
 * CONSTRUCTED `Host::PopOut` in the program after that: the only two call sites left were this
 * file's own tests, which is precisely why no dead-code warning ever fired and why the deletion
 * had to be found by grepping for a non-test caller rather than by trusting a green build.
 *
 * IT COST MORE THAN THE LINES. The pop-out page held the second producer of `Ask::CheckLive`,
 * so the argument for keeping Check now in the picture in picture window's chrome ("it is the
 * only producer in the program") read as true and was resting on a function nothing could run.
 * A dead branch does not just sit there; it answers questions wrongly. The poll now lives in
 * `WatchScreen::header`, in the main window, where the rest of this screen's controls are. */

/* THE FLOOR MOVED AND THE ARGUMENTS FOR IT DID NOT. `MIN_STAGE` and `stage_fits` are
 * `crate::screens`' now, because the Videos screen stages a surface into its own folio the same
 * way this one stages the player and a floor only one of them consulted would be a floor with a
 * hole in it. They are re-exported rather than referred to through their new path so that every
 * doc link and every test below, all of which were written against these names, still reads the
 * way it was written. `screens::MIN_STAGE` carries the argument. */
pub use crate::screens::{stage_fits, MIN_STAGE};

/* ------------------------------------------------------------- fitting a picture --
 *
 * WHAT WAS HERE AND WHY IT WENT. This block used to compute a BAND: `art_band`, plus
 * `ART_BELOW`, `MIN_ART_H` and `MAX_ART_H`, the rule for a window that had buttons under the
 * picture and needed to know how much room to leave them. That window was the pop-out page, it
 * is deleted, and nothing else ever called any of it: the body's folio takes the picture EDGE
 * TO EDGE and so has no band to compute, and the picture in picture window is the picture. What
 * is left is the one rule both surviving callers share, which is how a picture is fitted into a
 * rectangle somebody else chose. */

/// The most an image may be blown up past its own pixels. The Twitch avatar rung is 300 pixels
/// square and the YouTube one is 900; drawn at four times its size an avatar is a smear, not a
/// picture, and a smear where a stream would be looks like a fault in the app.
pub const MAX_ART_UPSCALE: f32 = 2.0;

/// The size an image of `px` pixels is drawn at inside a band of `band` points: fitted whole,
/// aspect kept, centred by the caller, and never blown up past [`MAX_ART_UPSCALE`].
///
/// FITTED AND NOT FILLED. Cropping a channel's banner to fill a 16 by 9 band would cut the
/// streamer's own artwork, and a wordmark with its ends sliced off is worse than a margin.
pub fn fit_into(px: egui::Vec2, band: egui::Vec2) -> egui::Vec2 {
    if px.x <= 0.0 || px.y <= 0.0 || band.x <= 0.0 || band.y <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let k = (band.x / px.x).min(band.y / px.y).min(MAX_ART_UPSCALE);
    px * k
}

/// Paint one picture in `band` and caption it honestly.
///
/// SPLIT OUT SO THE PAINT ITSELF CAN BE PROVED. `offline_art` decides WHETHER there is a picture,
/// and that decision reaches a host; this decides what is drawn, and a test can hand it a texture
/// it made itself and read the mesh back out of the frame. Without the split the only evidence
/// that anything is drawn at all would be that a texture was uploaded, which is the exact shape of
/// "it compiles, the tests are green, nothing calls it" this tree keeps finding.
///
/// THE WELL IS PAINTED FIRST AND THE PICTURE IS CENTRED IN IT. A banner that does not fill a 16 by
/// 9 band leaves margins, and margins in the body's own ink read as a hole rather than as a frame.
/// The word laid over the artwork. It is the whole of what this screen says when the channel is
/// down.
const OFFLINE_WORD: &str = "OFFLINE";

/// The picture, with OFFLINE laid across it, and nothing else.
///
/// THE WORD IS ON THE PICTURE AND NOT UNDER IT, and that is the point rather than a flourish.
/// Artwork alone could be read as a stream that has not started yet; a word on it cannot be
/// misread. It is the pattern the platforms themselves use on a channel card, and it lets every
/// other line on this screen go: the heading, the last-checked line, the refusal sentence and the
/// caption were all saying, in six ways, the one thing this says once.
///
/// A SCRIM UNDER THE WORD, because the artwork is the streamer's own and we do not get to know
/// what is behind the text. White on a pale banner is unreadable, so the word sits on a band of
/// the app's own ground at partial alpha: legible over anything, and still plainly the artwork's
/// picture rather than a box we painted over it.
pub fn paint_art(ui: &mut Ui, art: &crate::channel_art::Art, band: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(band, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, SUNK);
    let at = egui::Rect::from_center_size(rect.center(), fit_into(art.px, band));
    ui.painter().image(
        art.texture.id(),
        at,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    /* Sized to the picture, not to the window, so the word keeps its proportion when the body is
     * narrow and the band shrinks with it. */
    let size = (at.height() * 0.13).clamp(14.0, 40.0);
    let font = crate::fonts::display(size);
    let galley = ui
        .painter()
        .layout_no_wrap(OFFLINE_WORD.to_owned(), font, FLARE);
    let pad = egui::vec2(size * 0.9, size * 0.45);
    let scrim =
        egui::Rect::from_center_size(at.center(), galley.rect.size() + pad * 2.0).intersect(at);
    ui.painter()
        .rect_filled(scrim, CornerRadius::ZERO, INK.gamma_multiply(0.72));
    ui.painter()
        .galley(at.center() - galley.rect.size() * 0.5, galley, FLARE);
}

/* ------------------------------------------------------------ the playback state -- */

/// What the playback row and the video band do this frame.
///
/// FOUR ANSWERS FROM ONE RULE, so the row and the band cannot disagree about whether there is a
/// player. Every one of them used to be spelled at its own call site out of `self.watch_here` and
/// an `embeddable` flag, and the two defects that came out of that were both the same shape: a
/// control whose state was derived from whether a surface OUGHT to exist rather than from whether
/// one DOES.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Playback {
    /// Reserve the rest of the body for the video and hand the App a `player::Stage`.
    pub video: bool,
    /// The primary button says Stop, and it is enabled.
    pub stop: bool,
    /// The primary button says Watch here, and it is enabled.
    pub start: bool,
    /// Draw the Sound control.
    pub sound: bool,
}

/// The whole playback state machine, from the only four facts it depends on.
///
/// `asked` is the reader's own flag, `can_play_here` is "this window could host a surface for this
/// feed at all" (the body, a live channel, and a player that has not refused the machine),
/// `playing` is whether a surface EXISTS right now, and `failed` is whether the last attempt at
/// one did not work.
///
/// THE TWO RULES THAT ARE NOT OBVIOUS, AND EACH IS A DEFECT THAT SHIPPED.
///
/// `stop` IS DRIVEN BY `playing`, NEVER BY `can_play_here`. A surface stops being embeddable the
/// instant the channel goes offline or the preferred platform is switched, and none of that
/// destroys the WebView2 child window: it is hidden, and it is still holding somebody's bandwidth.
/// Deciding the Stop control from embeddability meant the one control that could end it was greyed
/// out in exactly the states where it was needed. Stopping is possible in every state where a
/// surface exists, and that is what this line says.
///
/// `video` SURVIVES A FAILURE WHILE THE SURFACE IS ALIVE, and that is the recovery path for a
/// placement call that did not work. The band is what produces the `Stage`, the `Stage` is what
/// makes the App call `Player::sync`, and `Player::sync` is where the failed `set_bounds` is
/// pushed again. Dropping the band on any failure at all is what made a single transient error
/// permanent: it removed the only thing that could have produced the retry. With no surface alive
/// a failure DOES drop the band, because then there is nothing to show and a reserved black
/// rectangle with no webview behind it is a dead frame.
pub fn playback(asked: bool, can_play_here: bool, playing: bool, failed: bool) -> Playback {
    let video = asked && can_play_here && (playing || !failed);
    let stop = playing || video;
    Playback {
        video,
        stop,
        /* One button, and it cannot be both. */
        start: !stop && can_play_here,
        /* The sound flag reaches the surface only through the `Stage`, so the control exists
         * exactly while a stage is being handed over and never as a switch that does nothing. */
        sound: video,
    }
}

/// What can be embedded for this platform right now, or the reason nothing can be.
///
/// THE REASON IS A SENTENCE, NOT A FLAG, because every one of them is different and the screen
/// prints it verbatim. "Not live" and "live but no video id" are not the same problem and a reader
/// who is told the wrong one will go looking in the wrong place.
///
/// A CHANNEL THAT IS NOT LIVE HAS NO SURFACE AT ALL. That is how the offline case is handled
/// honestly: not by embedding a player and letting it show whatever it shows (Twitch fills an
/// offline channel with recommendations for other people's streams, and YouTube has no video to
/// name), but by not asking for one and saying what the poller last knew.
pub fn feed_for(v: &View, on: crate::settings::Platform) -> Result<crate::player::Feed, String> {
    let name = crate::settings::DISPLAY_NAME;
    match v.live {
        None => Err(format!(
            "the live state on {} is not known yet, so there is nothing to play here",
            on.label()
        )),
        Some(false) => Err(format!("{name} is not live on {} right now", on.label())),
        Some(true) => match on {
            /* A Twitch channel IS its player address, so a live channel is always playable. */
            crate::settings::Platform::Twitch => Ok(crate::player::Feed::Twitch {
                login: twitch_login(&v.handle),
            }),
            /* YouTube needs the live VIDEO id; a channel id will not do. When the page said live
             * and the poll did not find an id, that is worth saying out loud rather than showing
             * a dead frame: it means the page's shape changed under the parser. */
            crate::settings::Platform::YouTube => match v.video_id.as_deref() {
                Some(id) if !id.is_empty() => Ok(crate::player::Feed::YouTube {
                    video_id: id.to_owned(),
                }),
                _ => Err(format!(
                    "YouTube says {name} is live but the poll did not find the video id, and a \
                     YouTube channel cannot be embedded without one"
                )),
            },
        },
    }
}

/* ----------------------------------------------------------------- formatters -- */

/// The one line that says who the embedded player is watching as, and it is always the same
/// answer: nobody.
///
/// WHY IT IS UNCONDITIONAL AND NOT A WARNING. The surface runs on WebView2 against a profile this
/// app owns (`player::profile_dir`, under LOCALAPPDATA), and a WebView2 profile cannot be attached
/// to the person's Edge or Chrome profile. There is no state in which the embed carries their
/// Twitch or YouTube session, so this is not a condition to detect, it is a fact about the player,
/// and a fact that is always true is printed rather than guarded.
///
/// AND IT IS NOT AN APOLOGY. An anonymous embedded viewer is a real viewer: a 117 second mid-roll
/// was served into an anonymous cross site embed on this machine, which is a thing that only
/// happens to traffic the platform is monetising. So the line ends on what is true of the view
/// rather than on what is missing from it.
///
/// WHAT IT USED TO SAY, AND WHY THAT SENTENCE CONTRADICTED ITSELF. It read "This player keeps its
/// own store and cannot borrow your browser's session, so sign in on Twitch for chat, channel
/// points and a subscriber's ad-free viewing." The session a reader creates by signing in on
/// Twitch, in their own browser, is exactly the session the first half of that sentence has just
/// said this player can never see, so the second half offered perks the stated mechanism cannot
/// deliver. It was also two wordings deep in a per-platform split (channel points are Twitch's)
/// built entirely on top of that advice, and the split went out with the advice.
///
/// THE THREE MEASURED FACTS, AND ALL THREE ARE IN THE LINE. The embed is ALWAYS anonymous, because
/// WebView2 keeps its own store and cannot be attached to the reader's Edge or Chrome profile.
/// There is no sign-in in this app to put a session in that store either: a login inside our own
/// webview WOULD persist, and no login flow exists, so the line states the absence and promises
/// nothing about it. And an anonymous view still counts and still carries ads, which is the part a
/// reader is owed before they decide this window is the lesser way to watch.
/// The two control labels, named once so the header and the tests cannot drift apart.
pub const SIGN_IN_ON_TWITCH: &str = "Sign in on Twitch";
pub const BACK_TO_THE_PLAYER: &str = "Back to the player";
/// What the control says once there is nothing left to do. Not a button; see `playback_controls`.
pub const SIGNED_IN_ALREADY: &str = "Signed in";

/// What the folio says about the session, while the reader is choosing how to watch.
///
/// IT SAYS WHERE A SIGN-IN IS AND IS NOT, AND BOTH HALVES ARE MEASURED. The player keeps its own
/// WebView2 store, so it never carries the session in the reader's browser; that half is the
/// same on both platforms. What differs is whether a session can be made HERE. On Twitch it can:
/// the header's `Sign in on Twitch` loads Twitch's own page into this player's profile and the
/// embed carries the result. On YouTube it cannot, because Google refuses account sign-in inside
/// an embedded browser outright (403 `disallowed_useragent`, measured), and no wording here may
/// suggest otherwise.
///
/// WHAT IT USED TO SAY. "...has no sign-in..." was true when it was written and stopped being
/// true when the control arrived; the test that held the old sentence was rewritten with it,
/// because a test guarding a sentence that is no longer true is a test guarding a lie.
pub fn signed_out_line(on: crate::settings::Platform) -> String {
    match on {
        crate::settings::Platform::Twitch => format!(
            "Watching signed out: this player keeps its own store, so it never carries the {} \
             session in your browser. {SIGN_IN_ON_TWITCH} above loads Twitch's own page here and \
             the player carries that session from then on; an anonymous view still counts and \
             still carries its ads.",
            on.label()
        ),
        crate::settings::Platform::YouTube => format!(
            "Watching signed out: this player keeps its own store, so it never carries your {} \
             session, and Google refuses account sign-in inside an embedded browser at all; an \
             anonymous view still counts and still carries its ads.",
            on.label()
        ),
    }
}

/// What the main window's folio says while the pop-out is playing the stream.
///
/// IT NAMES THE WINDOW AND NOT THE MECHANISM. "The player moved" or "the surface is reparented"
/// would both be true and neither is what a reader wants; where the picture went is.
pub const PLAYING_IN_THE_POP_OUT: &str = "Playing in the pop-out window.";

/// The one run of text the context header carries, and only in the stranded state: a surface is
/// alive on a channel that can no longer be embedded, so the folio has gone back to the picture and
/// the webview is hidden behind it, still holding a connection. It is the reason the Stop beside it
/// is enabled, and an enabled control with nothing saying what it would close is a control a reader
/// does not press.
///
/// A CONSTANT SO THE TEST CANNOT DRIFT FROM THE BAR. The sentence it replaces was longer, because
/// it had a whole row to itself under the buttons: "{why}. The player is still open and showing
/// nothing; Stop closes it." The header is one line beside a breadcrumb and a find box, and the
/// half that a reader cannot work out from what is on screen is this half. Which channel and why is
/// the picture's business now.
pub const STILL_OPEN: &str = "the player is still open";

/// What the folio says when it has neither a video to show nor a picture to show.
///
/// THE SURFACE'S OWN FAILURE WINS OVER THE FEED'S REFUSAL, because a machine with no WebView2
/// profile cannot play anything whatever the channel is doing, and a reader told "not live right
/// now" would come back at midnight to the same empty rectangle.
///
/// THE LAST ARM IS THE READER'S OWN STOP, and it names the control that undoes it. The feed is
/// playable, the player is fine, and there is no surface because the reader asked for there not to
/// be one; the folio has nothing to draw and the way back is a word in the header.
///
/// THE STRANDED ARM IS NOT HERE ANY MORE. It read "{why}. The player is still open and showing
/// nothing; Stop closes it", and it cannot be in the folio because in that state the folio is the
/// offline PICTURE. It moved, in its short form, to the context header beside the Stop it explains:
/// see [`STILL_OPEN`].
pub fn folio_note(
    problem: Option<&crate::player::Problem>,
    feed: &Result<crate::player::Feed, String>,
) -> String {
    match (problem, feed) {
        (Some(p), _) => p.words().to_owned(),
        (None, Err(why)) => why.clone(),
        (None, Ok(_)) => "Watch here plays the stream in this window, muted.".to_owned(),
    }
}

/* ----------------------------------------------------------------------- urls --
 *
 * ONE IS LEFT, AND IT IS THE ONE THAT IS NOT A URL. `twitch_login` folds a handle the way
 * Twitch's own embed URLs do and `feed_for` calls it to build the player's feed, so it ships.
 * `channel_url`, `chat_url` and `videos_url` do not: they built the pop-out page's browser
 * buttons, that page is deleted, and nothing else in the app opens Twitch in a browser. They
 * were `pub`, which is why no dead-code warning ever named them; a `pub` item in a library
 * crate is reachable as far as rustc is concerned even when the whole program never calls it. */

/// A Twitch login as URLs want it: trimmed, no leading `@`, lower case. Twitch treats logins
/// case-insensitively and its own embed URLs use lower case, so every URL here does too.
pub fn twitch_login(handle: &str) -> String {
    handle.trim().trim_start_matches('@').to_ascii_lowercase()
}

/* THERE IS NO `youtube_url` HERE ANY MORE, AND THAT IS THE POINT OF THE NOTE.
 *
 * It built `https://www.youtube.com/@{handle}` and this screen's button was its one caller. A
 * handle is the half of a YouTube identity that its owner can change; the `UC` id cannot be
 * changed and cannot be reassigned. Two links to one channel that resolve it differently is the
 * same drift a `TWITCH_URL` literal caused beside `TWITCH_HANDLE`, so both links now go through
 * `settings::Platform::url`, which uses `settings::YOUTUBE_CHANNEL_ID`. The `@handle` form is not
 * gone from the app: the WATCHER still fetches `/@{handle}/live`, because that page is the one
 * that carries the live markers it reads, and `watcher::youtube_login` owns that spelling. */

/* ------------------------------------------------------------------ the screen -- */

#[derive(Default)]
pub struct WatchScreen {
    /// Whether the reader has STOPPED the stream. `false` is the default, and that is the change:
    /// the folio IS the player, so arriving on a live channel plays.
    ///
    /// IT WAS `watch_here`, THE OTHER WAY UP, AND THE ARGUMENT FOR IT STILL HOLDS INVERTED. Its doc
    /// read: "Not persisted: a launch that starts a video stream nobody asked for this session is a
    /// launch that costs somebody bandwidth for a window they opened to check a quest." Nothing
    /// about that is weakened here. The app does not open on this screen (`main.rs` starts the body
    /// on the parser) and no flag of this screen's is written to disk, so a launch still plays
    /// nothing; what plays is a reader clicking a rail row named "Watch live", or the live pill,
    /// which are the two ways to reach this screen and are both the ask spelled out. The flag
    /// survives navigation in the other direction now: STOP means stopped, and stepping away and
    /// back does not start the stream over the reader's decision.
    stopped: bool,
    /// Whether sound has been asked for. Starts false and every stream starts muted; what the
    /// screen PRINTS is not this flag but `crate::player::Sound`, which is the browser's own answer
    /// to whether audio is coming out. See the player module's header.
    sound: bool,
    /* THE `signing_in` FIELD IS GONE, and its absence is the point. It was this screen's private
     * opinion about whether a sign-in was happening, kept beside `twitch_auth`'s own, and the two
     * could disagree. The auth state is the single source now; see `folio`. */
}

impl WatchScreen {
    /// Ask this screen to play, without a click on the header's own button.
    ///
    /// THE PILL'S CLICK LANDS HERE, by way of `screens::Ask::WatchHere` and `App::answer`. The
    /// pill is drawn by `titlebar::strip` and nowhere else, which is the main window's title bar
    /// and every tool window's; none of those can reach this screen's private state, and a tool
    /// window's is not even in the main window's pass, so the ask travels as an enum and the App
    /// applies it. Clearing the flag is all it does: whether anything actually plays is still
    /// `feed_for`'s answer, decided on the next frame with the live state in hand. A pill clicked
    /// while the channel is offline clears this and the folio still shows the offline picture,
    /// which is the point.
    ///
    /// IT IS A CLEAR AND NO LONGER A SET, AND IT IS STILL THE WHOLE ASK. A screen that has never
    /// been touched is already un-stopped, so on a live channel the pill's click is only the
    /// navigation; on a screen the reader stopped, it is the way back in, which is the case that
    /// makes this function still worth having.
    ///
    /// SOUND IS NOT TOUCHED. Every stream starts muted (see the player module), and a click on a
    /// status chip is not consent to make noise.
    pub fn play_here(&mut self) {
        self.stopped = false;
    }

    /// THE PLAYBACK STATE, WORKED OUT ONCE AND READ BY BOTH HALVES OF THIS SCREEN.
    ///
    /// The context header draws the controls and the folio draws the picture, and they are two
    /// separate passes over one frame: `main.rs` shows the top panel before the central panel. If
    /// each worked its own answer out the two could disagree, and the disagreement that shows is
    /// the one that matters: a Stop in the header over a folio that has already handed the
    /// rectangle back. So the four facts are read here, in one place, and both callers take what
    /// this returns. `the_header_and_the_folio_never_disagree_about_the_player` reads both out of
    /// one frame.
    ///
    /// THERE IS NO `host` ARGUMENT AND THERE IS NO LONGER A SECOND HOST TO NAME. Both callers are
    /// the body's, and no other window reaches this function, `video_band` or `Cx::stage`; a
    /// surface asked for from a tool window's pass would be built over the MAIN window's body.
    fn state(&self, cx: &Cx, feed: &Result<crate::player::Feed, String>) -> Playback {
        /* COULD THIS WINDOW HOST A SURFACE FOR THIS FEED AT ALL. A player that has FAILED is still
         * a player that could: the refusal (no profile folder, no WebView2) is the only answer no
         * click changes, and `Problem::can_retry` is where that line is drawn. Reading the whole of
         * `problem` here is what greyed out the button that would have retried. */
        let can_play_here = feed.is_ok()
            && cx
                .player
                .problem
                .as_ref()
                .map_or(true, crate::player::Problem::can_retry);
        playback(
            !self.stopped,
            can_play_here,
            cx.player.playing,
            cx.player.problem.is_some(),
        )
    }

    /// THE PLAYBACK CONTROLS, IN THE CONTEXT HEADER. `main.rs` draws this beside the breadcrumb,
    /// and only while the Watch screen is the one showing.
    ///
    /// WHY THEY ARE UP HERE AND NOT ON THE SCREEN THEY DRIVE. The folio is the video, edge to edge,
    /// and nothing may be drawn over a native child window: egui paints UNDER it, so a button laid
    /// on the video is not on top of it, it is invisible. A row of controls above the folio would
    /// mean the folio is not edge to edge. The header is what is left. It is also the one bar that
    /// is on screen in every state of this screen, so a control does not move as the channel
    /// changes.
    ///
    /// NO HOVER TEXT ON ANY OF THESE, AND THAT IS A RULE RATHER THAN AN OMISSION. A tooltip is an
    /// `egui::Area` in `Order::Tooltip`, which `player::floats_above_body` counts as floating over
    /// the body; the folio begins immediately under this bar, so a tooltip dropped from a control
    /// here lands on the video and `occluded_by_overlays` hides the surface for as long as the
    /// pointer rests. A hover that blanks the stream is worse than a terse control, so what a
    /// tooltip would have said is a word beside the control or it is not said.
    ///
    /// WHAT IS OFFERED, AND IT IS THE SMALLEST SET THAT IS HONEST.
    ///   the one button  Stop while a surface exists, Watch here once it does not. Drawn whenever
    ///                   this channel is embeddable at all (`feed.is_ok()`) or a surface exists, so
    ///                   an OFFLINE channel with nothing running carries no playback control
    ///                   whatever, which is half the point of moving them here.
    ///   Sound / Mute    only while a stage is being handed over, so the control exists exactly
    ///                   while there is something for it to reach and never as a switch that does
    ///                   nothing.
    ///   the sound word  `crate::player::Sound::word`, which the player sets from
    ///                   `ICoreWebView2_8::IsDocumentPlayingAudio`, the browser's own answer to
    ///                   whether the document is emitting audio. Asking the embedded player
    ///                   `isMuted()` returns the flag we set it to, which is how a spike concluded
    ///                   there was sound while a person watched a silent window.
    ///   [`STILL_OPEN`]  the STRANDED case, and the one run of text this bar carries. The channel
    ///                   went offline, or the platform was switched, while a surface was up:
    ///                   nothing can be embedded any more, the folio has gone back to the offline
    ///                   picture, and the webview is still there holding a connection. Stop is
    ///                   enabled (see [`playback`]) and a reader owed an enabled control is owed the
    ///                   reason it is there.
    ///
    /// AND WHY THERE IS STILL A "Watch here" AT ALL, when the folio plays by itself. Stop has to be
    /// undoable. `stopped` survives leaving this screen and coming back, deliberately (a stop that
    /// a step through the rail undid would not be a stop), so without a way back the one button
    /// would be a door that only locks. It is the same button, and it cannot be both at once.
    pub fn header(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let on = cx.settings.watch_on;
        let feed = feed_for(&View::of(cx.live.on(on)), on);
        let pb = self.state(cx, &feed);
        /* A control for a channel that can never be embedded is a control for a feature that does
         * not exist, so the PLAYBACK controls are gated on there being something to play. This
         * used to be an early return out of the whole function; it is a gate around one block
         * now, because Check now below it is meaningful in every state and is most meaningful in
         * the state this gate shuts: a channel that cannot be embedded is exactly the one you
         * want to ask about. */
        let playable = pb.stop || pb.start || feed.is_ok();
        ui.add_space(18.0);
        if playable {
            self.playback_controls(ui, cx, &pb);
            ui.add_space(10.0);
        }

        /* CHECK NOW, AND THIS IS WHERE IT LIVES NOW.
         *
         * The watcher polls on its own clock (`watcher::POLL_EVERY`, 90 seconds); this asks it
         * for one poll immediately. The watcher belongs to the App, so the ask travels on
         * `Cx::ask` rather than being called here.
         *
         * IT WAS IN THE PICTURE IN PICTURE WINDOW'S HOVER CHROME AND IT CAME BACK HERE. That
         * window is a picture and its controls are two chips in a corner; a text button in it
         * was the third thing that made its chrome read as a toolbar. It was kept there on the
         * argument that `Ask::CheckLive` had no other producer in the program, which was true
         * only because the unreachable pop-out page still held one. The page is deleted, so the
         * ask needs a REAL producer, and the header is where this screen's other controls
         * already are. */
        if ui
            .add(
                egui::Button::new(
                    RichText::new("Check now")
                        .font(FontId::proportional(12.5))
                        .color(TEXT),
                )
                .fill(PANEL)
                .stroke(Stroke::new(1.0, RULE))
                .corner_radius(CornerRadius::ZERO),
            )
            .on_hover_text(format!(
                "poll now instead of waiting for the next tick ({}s)",
                crate::watcher::POLL_EVERY.as_secs()
            ))
            .clicked()
        {
            cx.ask = crate::screens::Ask::CheckLive;
        }
    }

    /// Stop or Watch here, the sound pair, and the stranded-surface note: everything in the
    /// header that only means anything when there is something to play.
    ///
    /// SPLIT OUT OF `header` WHEN CHECK NOW ARRIVED. The gate used to be an early return, so
    /// every control below it inherited the same condition by accident of position. Making the
    /// gated part a function is what stops the next control added to the header from silently
    /// picking up a rule about the PLAYER that has nothing to do with it.
    fn playback_controls(&mut self, ui: &mut Ui, cx: &mut Cx, pb: &Playback) {
        let up = pb.stop;
        let label = if up { "Stop" } else { "Watch here" };
        let button = egui::Button::new(
            RichText::new(label)
                .font(FontId::proportional(12.5))
                .color(if up { TEXT } else { FLARE }),
        )
        .fill(if up { PANEL } else { PANEL_2 })
        .stroke(Stroke::new(1.0, if up { RULE } else { GOLD_DIM }))
        .corner_radius(CornerRadius::ZERO);
        if ui.add_enabled(pb.stop || pb.start, button).clicked() {
            if pb.stop {
                /* Stop means stop: the surface is dropped, not hidden; the SCREEN goes back to
                 * not-watching, or it would ask for the surface straight back on the next frame and
                 * Stop would be a flicker; and sound goes to off so the next start is muted like
                 * every first start. */
                self.stopped = true;
                self.sound = false;

                cx.ask = crate::screens::Ask::StopPlayer;
            } else {
                /* THE START GOES THROUGH THE ASK RATHER THAN STRAIGHT INTO THE FLAG, and that is
                 * what makes this button a retry as well as a start. `App::answer` clears the flag
                 * (`play_here`) AND gives the player back its go after a build that failed
                 * (`Player::retry`); a local `self.stopped = false` on a screen whose real problem
                 * is a failed build would leave the failure standing, so the second click on a
                 * button offering another attempt would do nothing at all. It is also the same road
                 * every pill in the app takes, so one widget keeps one behaviour. */
                cx.ask = crate::screens::Ask::WatchHere;
            }
        }
        /* SIGN IN ON TWITCH, OR BACK TO THE PLAYER. Drawn exactly while a surface is being handed
         * over (`pb.video`, the same gate as Sound) and only on Twitch: YouTube refuses account
         * sign-in inside an embedded browser, and a button for that would be a control for a
         * feature Google does not permit. The click flips one bit; `folio` does the rest by
         * publishing a different feed. See `WatchScreen::signing_in`. */
        if pb.video && cx.settings.watch_on == crate::settings::Platform::Twitch {
            ui.add_space(8.0);
            let waiting = matches!(cx.auth, crate::twitch_auth::AuthView::Waiting { .. });
            let signed_in = matches!(cx.auth, crate::twitch_auth::AuthView::In { .. });
            let label = if waiting {
                BACK_TO_THE_PLAYER
            } else if signed_in {
                SIGNED_IN_ALREADY
            } else {
                SIGN_IN_ON_TWITCH
            };
            let sign = egui::Button::new(
                RichText::new(label)
                    .font(FontId::proportional(12.5))
                    .color(if waiting || signed_in { TEXT } else { GOLD_HI }),
            )
            .fill(PANEL)
            .stroke(Stroke::new(
                1.0,
                if waiting || signed_in { RULE } else { GOLD_DIM },
            ))
            .corner_radius(CornerRadius::ZERO);
            /* SIGNED IN ALREADY IS A STATEMENT, NOT A BUTTON. There is nothing useful a second
             * sign-in does, and a control that repeats work already done is how somebody ends up
             * signing out by accident. */
            if ui.add_enabled(!signed_in, sign).clicked() {
                if waiting {
                    /* BACK TO THE PLAYER, and the flow keeps running behind it: the polling
                     * thread does not care whether its page is on screen, so a reader who wants
                     * to watch while they fetch their phone loses nothing. */
                    cx.auth_cancel = true;
                } else {
                    cx.auth_begin = true;
                }
            }
        }
        /* THE SOUND CONTROL IS NOT DRAWN OVER A LOGIN PAGE. It would rebuild the page on every
         * click and change nothing a reader could hear, which is a control that does nothing. */
        /* NOT OVER A SIGN-IN PAGE. It would rebuild the page on every click and change nothing
         * anybody could hear, which is a control that does nothing. */
        let waiting_now = matches!(cx.auth, crate::twitch_auth::AuthView::Waiting { .. });
        if pb.sound && !waiting_now {
            ui.add_space(8.0);
            let sound = egui::Button::new(
                RichText::new(if self.sound { "Mute" } else { "Sound on" })
                    .font(FontId::proportional(12.5))
                    .color(TEXT),
            )
            .fill(PANEL)
            .stroke(Stroke::new(1.0, RULE))
            .corner_radius(CornerRadius::ZERO);
            if ui.add(sound).clicked() {
                self.sound = !self.sound;
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(cx.player.sound.word())
                    .font(FontId::monospace(11.0))
                    .color(TEXT_3),
            );
        }
        /* THE STRANDED SURFACE: `stop` is true and the band that would show it is gone, which is
         * the only combination in the table where those two disagree. */
        if pb.stop && !pb.video {
            ui.add_space(10.0);
            ui.label(
                RichText::new(STILL_OPEN)
                    .font(FontId::proportional(11.5))
                    .color(TEXT_3),
            );
        }
    }

    /// The screen, which is the folio and has nothing left to choose between.
    ///
    /// IT IS STILL A FUNCTION RATHER THAN A CALL STRAIGHT TO `folio`, because `folio` is private
    /// and this is what `main.rs` reaches: one public door per screen, the same as every other
    /// screen in `screens`. The choice it used to make is recorded where `Host` used to be.
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.folio(ui, cx);
    }

    /// THE FOLIO: the player, the picture, or the reason there is neither, and nothing else at all.
    ///
    /// EDGE TO EDGE IS NOT THIS FUNCTION'S DOING ALONE. `main.rs` hands the Watch body a central
    /// panel with a ZERO inner margin, where every other screen gets 18 points; that is why the
    /// video and the picture reach the rail and the header rather than sitting in a dark border.
    /// `the_watch_body_is_the_one_screen_drawn_edge_to_edge` holds it there.
    ///
    /// THE ORDER OF THE THREE IS THE WHOLE RULE. A surface first, because a native child window
    /// owns its rectangle and nothing else may be in it. The picture second, and only on
    /// `Some(false)`: on `Some(true)` the reader is one frame from a stream and an offline screen
    /// would be a picture arguing with the header above it, and on `None` the app does not know,
    /// which is not a state to illustrate. This is also what keeps the fetch lazy in the only way
    /// that matters: the network is not touched at all on a live or an unpolled channel. The words
    /// last, because they are what is left when there is nothing to look at.
    fn folio(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let on = cx.settings.watch_on;
        let feed = feed_for(&View::of(cx.live.on(on)), on);
        let pb = self.state(cx, &feed);
        if pb.video {
            let mut feed = feed.expect("pb.video needs can_play_here, which needs feed.is_ok()");
            /* THE SIGN-IN PAGE TAKES THE STREAM'S PLACE WHILE TWITCH IS WAITING FOR THE CODE,
             * AND THAT IS READ FROM THE SHARED AUTH STATE RATHER THAN FROM A BIT OF THIS SCREEN'S
             * OWN.
             *
             * THIS SCREEN USED TO KEEP A `signing_in` FLAG that its own button toggled, entirely
             * separate from the token flow behind the Chat screen's button. Two controls, two
             * states, one of them signing the VIDEO in and the other signing CHAT in, and nothing
             * connecting them. Now there is one state: `twitch_auth` is waiting or it is not, and
             * both screens read it. Clicking sign-in ANYWHERE starts the same flow, this folio
             * shows the page while it is running, and it goes back to the stream by itself when
             * the token lands, because `AuthView::Waiting` stops being true.
             *
             * TWITCH ONLY, still: `feed_for` never produces this feed, and Google refuses account
             * sign-in inside an embedded browser at all. */
            let signing_in = matches!(cx.auth, crate::twitch_auth::AuthView::Waiting { .. })
                && on == crate::settings::Platform::Twitch;
            if let crate::twitch_auth::AuthView::Waiting {
                verification_uri, ..
            } = &cx.auth
            {
                if signing_in {
                    feed = crate::player::Feed::TwitchSignIn {
                        at: verification_uri.clone(),
                    };
                }
            }
            /* THE DEMAND IS PUBLISHED FIRST AND UNCONDITIONALLY, before the branch below decides
             * whether THIS window is the one that hosts it. `main.rs` pairs it with the pop-out's
             * offer; see `player::choose_stage`. */
            cx.demand = Some((feed.clone(), self.sound));
            /* A SIGN-IN PAGE IS STAGED HERE EVEN IF THE VIDEO WAS IN THE POP-OUT A FRAME AGO.
             * `choose_stage` refuses to send that page to the pop-out, so this folio has to
             * reserve the rectangle it will land in, or the page would have no seat at all. */
            if cx.player.hosted_elsewhere && !signing_in {
                /* THE VIDEO IS IN THE POP-OUT, SO THIS FOLIO MUST NOT RESERVE A RECTANGLE FOR IT.
                 * `stage_folio` paints its rect black for the surface to be composited into, and
                 * with the surface in another window that black is a hole in the middle of the
                 * main window with nothing in it and no reason given. The reader is told instead,
                 * in the same place the picture and the refusal sentence go. */
                crate::screens::centred_words(
                    ui,
                    ui.available_rect_before_wrap(),
                    vec![(
                        PLAYING_IN_THE_POP_OUT.to_owned(),
                        FontId::proportional(13.0),
                        TEXT_2,
                    )],
                );
                return;
            }
            /* THE STAGING IS `crate::screens`' AND NOT THIS SCREEN'S ANY MORE. It was
             * `WatchScreen::video_band`, and the Videos screen needs the same rectangle taken the
             * same way with the same floor, the same black, the same occlusion rule and the same
             * repaint tick; the only thing that differs between the two is the feed. See
             * `screens::stage_folio` for why that is shared rather than copied. */
            crate::screens::stage_folio(ui, cx, feed, self.sound);
            return;
        }
        /* THE PICTURE GOES WHERE THE PLAYER WOULD HAVE BEEN. Past the return above there is no
         * surface this frame and no surface is being asked for, so this rectangle is egui's to
         * paint and the two can never contend for it. */
        if cx.live.on(on).live == Some(false) && Self::folio_art(ui, on) {
            return;
        }
        Self::folio_words(ui, cx, &feed);
    }

    /// The channel's own artwork, filling the folio. `false` when there is no picture to fill it
    /// with, which hands the folio on to the words.
    ///
    /// THE BAND IS THE WHOLE FOLIO AND THERE IS NO ARITHMETIC LEFT. `art_band` subtracted the room
    /// that the controls under the picture needed. There are no controls under the picture in
    /// this window, there is no other window with any, and that function is deleted with the page
    /// it served. What survives is the floor:
    /// [`MIN_STAGE`], the player's own, because the folio is now one rectangle serving both and a
    /// window too small to play in is too small to look at a picture in.
    ///
    /// NOTHING IS ASSERTED BY THE PICTURE, and `paint_art` is where that is enforced: the artwork
    /// is the streamer's own upload, no rung is Twitch's generic offline graphic, and the only word
    /// laid over it is OFFLINE, which is the state the caller has already checked.
    ///
    /// THREE WAYS TO HAVE NO PICTURE AND ALL THREE ANSWER THE SAME. No artwork yet (the fetch
    /// answers a frame later), none to be had, or a fetch that failed: `channel_art::artwork` says
    /// `None` to all three and the folio says why in words instead.
    fn folio_art(ui: &mut Ui, on: crate::settings::Platform) -> bool {
        let band = ui.available_size();
        if !stage_fits(band) {
            return false;
        }
        let Some(art) = crate::channel_art::artwork(ui.ctx(), on) else {
            return false;
        };
        paint_art(ui, &art, band);
        true
    }

    /// What the folio says when it has neither a video nor a picture, centred in the folio.
    ///
    /// ONE LINE, USUALLY. [`folio_note`] owns the words and the argument for which of them.
    ///
    /// AND TWO MORE WHERE THE READER IS DECIDING, WHICH IS THE ONLY PLACE THEY CAN GO NOW.
    /// [`signed_out_line`] and the profile folder with its tracking prevention level used to sit
    /// under the playback row while the surface was UP. They cannot: nothing may be drawn over the
    /// video, and the folio is the video. The moment they are actually worth reading is the one
    /// just before, when the feed is playable and the reader has stopped and is deciding whether
    /// this window is the way they want to watch, so that is the state that carries them. The
    /// condition is `feed.is_ok()` with no player problem, which is exactly that state and is also
    /// what keeps the old defect fixed: on an OFFLINE channel these two lines were once printed
    /// under "Broken Stoic is not live on Twitch right now", both true, neither about anything that
    /// existed. An offline or unpolled channel fails `feed.is_ok()` and gets neither.
    ///
    /// THE PROFILE LINE IS STILL GUARDED BY THE PROFILE. `cx.player.profile` is `None` until a
    /// surface has been built at least once, and a path this app has never opened is not a fact
    /// about where a login would be kept, it is a guess.
    fn folio_words(ui: &mut Ui, cx: &Cx, feed: &Result<crate::player::Feed, String>) {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());
        let mut lines: Vec<(String, FontId, egui::Color32)> = Vec::new();
        let refused = cx.player.problem.is_some() || feed.is_err();
        lines.push((
            folio_note(cx.player.problem.as_ref(), feed),
            FontId::proportional(12.5),
            if refused { TEXT_3 } else { TEXT_2 },
        ));
        if !refused {
            lines.push((
                signed_out_line(cx.settings.watch_on),
                FontId::proportional(11.5),
                TEXT_2,
            ));
            /* WHERE A LOGIN WOULD LIVE, AND WHETHER THE COOKIE BLOCK IS OFF. Two facts with exactly
             * one honest form each, and both have a reader with a decision. The folder is the
             * answer to "where did my sign-in go" and it is a path, not a sentence; wry's default
             * would put it beside the binary, which in Program Files is not writable. The level is
             * read BACK from the profile after being set, never the value asked for: 0 is off, and
             * off is what lets the embedded player receive its own cookies (Edge ships Balanced,
             * which is what withholds them on a fresh profile). */
            if let Some(dir) = &cx.player.profile {
                let level = match cx.player.tracking_prevention {
                    Some(0) => "tracking prevention off".to_owned(),
                    Some(n) => format!("tracking prevention still on (level {n})"),
                    None => "tracking prevention not read yet".to_owned(),
                };
                lines.push((
                    format!("{} \u{2022} {level}", dir.display()),
                    FontId::monospace(10.5),
                    TEXT_3,
                ));
            }
        }

        /* THE LAYOUT LOOP IS `crate::screens`' NOW. The Videos folio has the same job in the same
         * shape, one sentence in the middle of a whole folio, and a second copy of the wrap and the
         * centring is how a fix lands on one of them and not the other. What stays here is WHICH
         * lines, which is this screen's own argument and is spelled out above. */
        crate::screens::centred_words(ui, rect, lines);
    }
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    const TW: crate::settings::Platform = crate::settings::Platform::Twitch;

    fn view(live: Option<bool>) -> View {
        View {
            handle: "Broken_Stoic".into(),
            live,
            source: "twitch-gql".into(),
            ..View::default()
        }
    }

    /// A HANDLE IS FOLDED THE WAY TWITCH'S OWN EMBED URLS FOLD IT: trimmed, no leading `@`, lower
    /// case. `feed_for` builds the player's feed from this, so a handle typed with a capital or a
    /// stray space has to reach the embed as the same channel.
    ///
    /// IT USED TO CHECK THREE URL BUILDERS TOO. `channel_url`, `chat_url` and `videos_url` built
    /// the deleted pop-out page's browser buttons and had no other caller; nothing in the app
    /// opens Twitch in a browser now.
    #[test]
    fn a_twitch_handle_is_folded_to_a_login() {
        assert_eq!(twitch_login("Broken_Stoic"), "broken_stoic");
        assert_eq!(twitch_login(" @Broken_Stoic "), "broken_stoic");
        assert_eq!(twitch_login("broken_stoic"), "broken_stoic");
    }

    /// THE YOUTUBE BUTTON OPENS THE CHANNEL ID AND NOT THE HANDLE, and this is the assertion that
    /// keeps a `/@broken_stoic` link from being written back in because it reads more nicely.
    ///
    /// A handle is the changeable half of a YouTube identity. If it is ever changed, a `/@handle`
    /// link stops resolving, or resolves to whoever picked the name up, and nothing in this app
    /// would notice: the WATCHER would go to `unknown` with an error on the pill, but a button is
    /// silent, and a silent button that opens a stranger's channel is the worse of the two.
    #[test]
    fn the_youtube_button_opens_the_channel_id() {
        let url = crate::settings::Platform::YouTube.url();
        assert_eq!(
            url,
            "https://www.youtube.com/channel/UCf4fNJTJt8F1MZAQ2iqIZ9A"
        );
        assert!(
            !url.contains('@'),
            "the handle form is the one that can be taken away: {url}"
        );
        assert!(
            url.ends_with(crate::settings::YOUTUBE_CHANNEL_ID),
            "built from the constant, never spelled out a second time: {url}"
        );
    }

    /// NOTHING THIS SCREEN PRINTS CARRIES AN EM DASH OR AN EN DASH. The owner's rule, held over
    /// every string the screen can still produce.
    ///
    /// THE LIST SHRANK WITH THE POP-OUT PAGE, NOT WITH THE RULE. It used to run over
    /// `status_line`, `checked_line` and `state_label`, which built that page's status band;
    /// they had no production caller once the page went and are deleted. What is left is what
    /// the folio and the header actually put on screen.
    #[test]
    fn nothing_on_screen_carries_a_dash() {
        let mut checked: Vec<String> = vec![
            OFFLINE_WORD.to_owned(),
            STILL_OPEN.to_owned(),
            crate::settings::DISPLAY_NAME.to_owned(),
        ];
        for on in [TW, crate::settings::Platform::YouTube] {
            checked.push(signed_out_line(on));
        }
        /* Every arm of the folio's note, so a dash cannot hide in the one state the author did
         * not open. */
        let refused = crate::player::Problem::Refused(
            "this machine has no WebView2 runtime, so nothing can play here".to_owned(),
        );
        for problem in [None, Some(&refused)] {
            for feed in [
                Ok(crate::player::Feed::Twitch {
                    login: "broken_stoic".into(),
                }),
                Err("no video id".to_owned()),
            ] {
                checked.push(folio_note(problem, &feed));
            }
        }
        assert!(
            checked.len() >= 9,
            "the list stopped covering the screen: {checked:?}"
        );
        for s in checked {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{s}");
        }
    }

    /* ---- the player surface ---- */

    const YT: crate::settings::Platform = crate::settings::Platform::YouTube;

    /// EVERY REFUSAL SAYS SOMETHING DIFFERENT, because the screen prints this sentence and the
    /// three of them send a reader three different places: wait, come back later, or the parser
    /// broke. The unknown state matters most: it is what the screen shows between launch and the
    /// first poll, and reading it as offline would tell somebody their streamer is not on when
    /// nothing has been asked yet.
    #[test]
    fn nothing_is_playable_until_the_poller_says_live() {
        let unknown = feed_for(&view(None), TW).unwrap_err();
        assert!(unknown.contains("not known yet"), "{unknown}");
        assert!(
            unknown.contains("Twitch"),
            "the refusal names the platform: {unknown}"
        );

        let off = feed_for(&view(Some(false)), TW).unwrap_err();
        assert!(off.contains("not live"), "{off}");
        assert!(
            off.contains(crate::settings::DISPLAY_NAME),
            "the refusal names the channel the way a person writes it: {off}"
        );
        assert_ne!(
            unknown, off,
            "unknown is not offline and must not read as it"
        );
    }

    /// A live Twitch channel is playable from the login alone, and the login is normalised on the
    /// way through: a channel IS its player address, so there is nothing to resolve.
    #[test]
    fn a_live_twitch_channel_plays_from_its_login() {
        let mut v = view(Some(true));
        v.handle = "@Broken_Stoic".into();
        assert_eq!(
            feed_for(&v, TW).unwrap(),
            crate::player::Feed::Twitch {
                login: "broken_stoic".into()
            },
            "trimmed, unprefixed, lower case, the same rule the URLs use"
        );
    }

    /// THE ASSERTION THE ORIGINAL PLAN WOULD HAVE FAILED. The plan said to embed YouTube by channel
    /// id through the `/embed/live_stream?channel=` form; that endpoint was measured returning
    /// IFrame API error 150 for a live channel and an offline one alike, reporting its own
    /// `videoId` as the literal string "live_stream". So a live YouTube channel with no video id is
    /// NOT playable, and saying so is the whole difference between a message and a dead frame.
    #[test]
    fn youtube_needs_a_video_id_and_says_so_when_it_has_none() {
        let mut live_with_id = view(Some(true));
        live_with_id.video_id = Some("rFZHOHl-L8A".into());
        assert_eq!(
            feed_for(&live_with_id, YT).unwrap(),
            crate::player::Feed::YouTube {
                video_id: "rFZHOHl-L8A".into()
            }
        );

        let no_id = feed_for(&view(Some(true)), YT).unwrap_err();
        assert!(no_id.contains("video id"), "{no_id}");
        assert_ne!(
            no_id,
            feed_for(&view(Some(false)), YT).unwrap_err(),
            "live-without-an-id is a parser problem and offline is not; they must not share words"
        );

        let mut blank = view(Some(true));
        blank.video_id = Some(String::new());
        assert!(
            feed_for(&blank, YT).is_err(),
            "an empty id would build an embed URL pointing at nothing"
        );

        /* Twitch never carries one and never needs one. */
        let mut tw = view(Some(true));
        tw.video_id = Some("ignored".into());
        assert!(matches!(
            feed_for(&tw, TW).unwrap(),
            crate::player::Feed::Twitch { .. }
        ));
    }

    #[test]
    fn a_stage_smaller_than_the_floor_is_refused() {
        assert!(stage_fits(MIN_STAGE));
        assert!(stage_fits(egui::vec2(1280.0, 720.0)));
        assert!(!stage_fits(egui::vec2(MIN_STAGE.x - 1.0, 720.0)));
        assert!(!stage_fits(egui::vec2(1280.0, MIN_STAGE.y - 1.0)));
        assert!(!stage_fits(egui::Vec2::ZERO));
    }

    fn live_status(live: Option<bool>) -> crate::watcher::Status {
        let mut st = crate::watcher::Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        st.twitch.live = live;
        st
    }

    /// Everything one headless pass of this screen produced, so a test can hold what was PAINTED
    /// and what was ASKED for as well as what was staged. Three tests below need one of the three
    /// each, and three near-copies of the setup is how they would drift apart.
    struct Drawn {
        stage: Option<crate::player::Stage>,
        /// Every run of text painted, in paint order, WITH THE RECTANGLE IT WAS PAINTED IN.
        ///
        /// The rectangle is what lets a test click a named control. Reading a label back proves
        /// the button says "Stop"; only a click proves it is ENABLED, and enabled is half of the
        /// defect these tests are here for (a Stop control greyed out in exactly the state that
        /// needs it). A disabled `egui::Button` still paints its text, so the words alone cannot
        /// tell the two apart.
        runs: Vec<(String, egui::Rect)>,
        /// The pill's stadium: the one shape in this app with a rounded corner (`theme::install`
        /// makes everything else square), which is what makes it findable without reaching for a
        /// private constant in `titlebar`.
        pill: Option<egui::Rect>,
        ask: crate::screens::Ask,
        /// The screen's OWN state after the frame, which no earlier test could see.
        ///
        /// `ask` is what the screen asked the App to do; these two are what the screen did to
        /// ITSELF. Stop has to do both, and an adversarial lens found the second half held by no
        /// test at all: delete the screen's own reset and Stop still raises `StopPlayer`, so every
        /// assertion passed while the screen went on believing it was watching.
        ///
        /// IT IS `!screen.stopped`, SPELLED THE WAY A TEST READS IT. The field turned over when the
        /// folio became the player (a screen plays unless it has been stopped); the QUESTION the
        /// tests ask is still "is this screen watching", so the answer is stored the way it is
        /// asked and the inversion lives on this one line rather than at every assertion.
        watching: bool,
        sound: bool,
        /// The screen asked the App to START a sign-in this pass (`Cx::auth_begin`).
        asked_sign_in: bool,
        /// The screen asked the App to put the video back (`Cx::auth_cancel`).
        asked_back: bool,
        /// How wide and tall the folio was on this pass, so a test can hold the stage or the
        /// picture to the WHOLE of it rather than to a number typed twice.
        folio: egui::Rect,
    }

    impl Drawn {
        /// Just the text, which is what most of these tests read.
        fn words(&self) -> Vec<String> {
            self.runs.iter().map(|(w, _)| w.clone()).collect()
        }
        /// Only the words painted inside the FOLIO, which is the region below the context
        /// header.
        ///
        /// THE TWO REGIONS ARE DIFFERENT PLACES WITH DIFFERENT RULES AND ONE READER KEPT
        /// CONFLATING THEM. The harness draws the header and the folio in one pass, because the
        /// app does, so `words` has always carried both: "Stop" and "Sound on" are the
        /// HEADER's and have never been folio furniture. A test asserting the folio carries no
        /// page furniture was really asserting the whole SCREEN carried none, which made it go
        /// red the moment a control was added to the bar, and would have gone green for a folio
        /// that had quietly moved its furniture up into the header.
        fn folio_words(&self) -> Vec<String> {
            self.runs
                .iter()
                .filter(|(_, r)| self.folio.contains_rect(*r))
                .map(|(w, _)| w.clone())
                .collect()
        }
        /// The centre of the run whose text is exactly `s`, for a test that wants to click it.
        fn spot(&self, s: &str) -> Option<egui::Pos2> {
            self.runs
                .iter()
                .find(|(w, _)| w == s)
                .map(|(_, r)| r.center())
        }
    }

    /// What the App would report about a surface that came up cleanly.
    ///
    /// A DEFAULT `PlayerView` HAS NO PROFILE PATH, which is right (nothing has been built) and
    /// which is why a test about the profile line has to plant one: the line is drawn from
    /// `cx.player.profile`, and against `Default` it would be absent for a reason that has nothing
    /// to do with the condition under test. The path is the real shape (`player::profile_dir`'s
    /// vendor and leaf under LOCALAPPDATA), spelled here rather than called for, so this stays a
    /// test about the SCREEN and does not go reading the machine's environment.
    fn player_up() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            problem: None,
            playing: true,
            sound: Default::default(),
            profile: Some(std::path::PathBuf::from(
                "C:/Users/somebody/AppData/Local/EQLGrimoire/WebView2",
            )),
            tracking_prevention: Some(0),
        }
    }

    /// A PLAYER THAT HAS BEEN BUILT AND IS NOT RUNNING: the reader pressed Stop, so there is no
    /// surface, and the profile folder it used and the tracking level read back off it are still
    /// facts about this machine.
    ///
    /// IT IS `player_up` WITH THE SURFACE GONE, deliberately, because the two differ in exactly the
    /// one field the tests that use it are about (`playing`), and a second hand written fixture
    /// would drift from the first the day a field is added to `PlayerView`.
    fn player_stopped() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            playing: false,
            ..player_up()
        }
    }

    /// A LIVE SURFACE WHOSE LAST PLACEMENT CALL FAILED. `set_bounds` answered `Err` once; the
    /// child window is still up and still playing.
    fn player_misplaced() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            problem: Some(crate::player::Problem::Failed(
                "could not place the player: the window went away".to_owned(),
            )),
            ..player_up()
        }
    }

    /// A BUILD THAT FAILED. There is no surface and there is a sentence saying why, and asking
    /// again is a real thing to do, so this is a `Failed` and not a `Refused`.
    fn player_build_failed() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            problem: Some(crate::player::Problem::Failed(
                "could not create the player surface: no runtime".to_owned(),
            )),
            playing: false,
            ..Default::default()
        }
    }

    /// A MACHINE THAT CAN NEVER HOST ONE: no WebView2 at all, or no per-user data folder to keep
    /// the profile in. No click changes this, so no control may offer to.
    fn player_refused() -> crate::player::PlayerView {
        crate::player::PlayerView {
            hosted_elsewhere: false,
            problem: Some(crate::player::Problem::Refused(
                "embedded playback needs Microsoft WebView2".to_owned(),
            )),
            ..Default::default()
        }
    }

    /// THE THIRD ARGUMENT IS "IS THIS SCREEN WATCHING", AND IT ALWAYS WAS.
    ///
    /// The field behind it turned over when the folio became the player: `watch_here` (default
    /// false, set by a click) became `stopped` (default false, set by Stop). Every call site here
    /// asks the same question it always asked, so the question is what the parameter spells and the
    /// inversion happens once, in `draw_watch_seeded`. Flipping seventeen booleans to mean the
    /// opposite of the word next to them is how a fixture starts lying about its own case.
    fn draw_watch_pass(
        ctx: &egui::Context,
        watching: bool,
        live: &crate::watcher::Status,
        events: Vec<egui::Event>,
    ) -> Drawn {
        draw_watch_with(ctx, watching, live, events, Default::default())
    }

    fn draw_watch_with(
        ctx: &egui::Context,
        watching: bool,
        live: &crate::watcher::Status,
        events: Vec<egui::Event>,
        player: crate::player::PlayerView,
    ) -> Drawn {
        draw_watch_seeded(ctx, TW, watching, false, live, events, player)
    }

    thread_local! {
        /// Read once by the next `draw_watch_seeded` on this thread and cleared. A cell rather
        /// than a seventh parameter, because six callers pass the six that exist and the bit is
        /// wanted by two passes in one test.
        static AUTH_SEED: std::cell::RefCell<crate::twitch_auth::AuthView> =
            const { std::cell::RefCell::new(crate::twitch_auth::AuthView::Out) };
    }

    /// `draw_watch_seeded` with the screen already on the sign-in page.
    fn draw_watch_signing_in(
        ctx: &egui::Context,
        live: &crate::watcher::Status,
        events: Vec<egui::Event>,
        player: crate::player::PlayerView,
    ) -> Drawn {
        AUTH_SEED.with(|s| {
            *s.borrow_mut() = crate::twitch_auth::AuthView::Waiting {
                user_code: "WMCLHMKG".to_owned(),
                verification_uri: "https://www.twitch.tv/activate?device-code=WMCLHMKG".to_owned(),
            }
        });
        draw_watch_seeded(ctx, TW, true, false, live, events, player)
    }

    /// Like `draw_watch_with`, but the screen starts with SOUND ALREADY ON.
    ///
    /// This exists because a test of mine asserted that Stop turns sound off while the fixture
    /// had sound off the whole time, so the assertion could not fail. A mutation that deleted
    /// `self.sound = false` stayed green and caught me out. A reset is only testable against a
    /// state that needs resetting.
    ///
    /// The 17 existing callers keep the old signature deliberately: widening all of them to pass
    /// `false` would be noise at every site to serve one test.
    ///
    /// IT DRAWS THE HEADER AND THE FOLIO, IN THAT ORDER, BECAUSE THE APP DOES.
    /// `main.rs` shows the top panel before the central panel, so the header's click lands on the
    /// screen's own flags before the folio reads them and the two are one frame, not two. A harness
    /// that drew only the folio could not click Stop at all now that Stop is up in the bar, and a
    /// harness that drew them the other way round would hide the fact that a stop takes effect on
    /// the same frame it is pressed. There is one host now, so it always draws both.
    /// The platform the screen is SET TO, which decides whether there is anything to play.
    ///
    /// Every other caller passes Twitch, because `feed_for` builds a Twitch feed from a handle
    /// alone and so a Twitch channel is always embeddable. YouTube needs a video id it only has
    /// while he is live, so YouTube offline is the one reachable case where the header's
    /// PLAYBACK controls are all absent, and that is the case a control drawn after them has to
    /// survive. It used to be an early return out of the whole function.
    /// A pass with the sign-in already complete.
    fn draw_watch_authed(
        ctx: &egui::Context,
        live: &crate::watcher::Status,
        player: crate::player::PlayerView,
    ) -> Drawn {
        AUTH_SEED.with(|s| {
            *s.borrow_mut() = crate::twitch_auth::AuthView::In {
                login: "reviird".to_owned(),
            }
        });
        draw_watch_seeded(ctx, TW, true, false, live, Vec::new(), player)
    }

    fn draw_watch_seeded(
        ctx: &egui::Context,
        on: crate::settings::Platform,
        watching: bool,
        sound: bool,
        live: &crate::watcher::Status,
        events: Vec<egui::Event>,
        player: crate::player::PlayerView,
    ) -> Drawn {
        /* THE THEME GOES ON HERE, NOT AT THE CALLER'S DISCRETION, AND THAT MATTERS FOR ONE
         * ASSERTION IN PARTICULAR. `Drawn::pill` finds the one shape in this app with a rounded
         * corner, which is only true once `theme::install` has squared every widget off; with
         * egui's stock style a plain `ui.button` is rounded too. While the body drew a pill the
         * pill was painted FIRST and `find_map` reached it before any button, so the field was
         * right for the wrong reason. Now that the body draws NO pill, "there is no rounded shape
         * here" is a claim a stock style would make false, so the style the app actually installs
         * is the one every pass in this file runs under. */
        crate::theme::install(ctx);
        let mut settings = crate::settings::Settings {
            watch_on: on,
            ..Default::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut screen = WatchScreen {
            stopped: !watching,
            sound,
        };
        /* THE SIGN-IN STATE IS THE APP'S NOW AND NOT THIS SCREEN'S, so a pass that wants to be
         * mid sign-in seeds the `Cx`. Taken, so it applies to one pass and the next starts
         * signed out, which is what stops one test leaking into the next. */
        let seeded_auth = AUTH_SEED
            .with(|s| std::mem::replace(&mut *s.borrow_mut(), crate::twitch_auth::AuthView::Out));
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
            auth: seeded_auth.clone(),
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
        let mut folio = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            ui.horizontal(|ui| screen.header(ui, &mut cx));
            folio = ui.available_rect_before_wrap();
            screen.ui(ui, &mut cx);
        });
        assert!(!out.shapes.is_empty(), "nothing was painted");
        let shapes = std::mem::take(&mut out.shapes);
        /* headless: there is no renderer to hand the font atlas to, and epaint panics on a dropped
         * delta unless told the drop is deliberate */
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
            pill: flat.iter().find_map(|s| match s {
                egui::Shape::Rect(r) if r.corner_radius.nw > 0 => Some(r.rect),
                _ => None,
            }),
            ask: cx.ask,
            watching: !screen.stopped,
            sound: screen.sound,
            asked_sign_in: cx.auth_begin,
            asked_back: cx.auth_cancel,
            folio,
        }
    }

    fn draw_watch(
        ctx: &egui::Context,
        watch_here: bool,
        live: &crate::watcher::Status,
    ) -> Option<crate::player::Stage> {
        draw_watch_pass(ctx, watch_here, live, Vec::new()).stage
    }

    /// Draws the real screen with the channel live and checks when it reserves a player rect.
    ///
    /// IT USED TO HAVE A THIRD ARM AND THE RULE IT HELD IS NOW HELD BY SHAPE. The arm asserted
    /// that the POP-OUT never reserved a rect, because eframe hands a deferred viewport's
    /// callback the ROOT window's handle and a surface asked for there would be built over the
    /// MAIN window's body, on top of whatever screen was showing. That danger has not gone away;
    /// what has gone away is the way in. `windows.rs` draws its own picture in picture body and
    /// never reaches this screen at all, which
    /// `only_the_main_window_draws_the_watch_screens_header_controls` and
    /// `no_other_window_draws_this_screen` hold by reading the calls in that file.
    ///
    /// AND THE BODY RESERVES ONE WITH NOBODY CLICKING ANYTHING, WHICH IS THE CHANGE.
    /// This used to read "the body reserves nothing until the reader has asked to watch here", and
    /// that sentence is now the wrong way up: the owner asked for the leaves to BE the player, and
    /// a folio that is meant to be the player but holds a black rectangle until a control is found
    /// somewhere else is not the player. So a screen that has never been touched, on a live
    /// channel, stages the surface on its first frame. The absence is still held, at the other end:
    /// a screen the reader has STOPPED reserves nothing, which is what makes Stop a stop.
    #[test]
    fn only_the_body_reserves_a_player_rect_and_it_needs_no_click() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = live_status(Some(true));

        assert!(
            draw_watch(&ctx, false, &live).is_none(),
            "a screen the reader stopped went on staging the surface, so Stop is a flicker"
        );
        let staged = draw_watch(&ctx, true, &live)
            .expect("an untouched screen on a live channel is the player");
        assert_eq!(
            staged.feed,
            crate::player::Feed::Twitch {
                login: "broken_stoic".into()
            }
        );
        assert!(!staged.sound, "every stream starts muted");
        /* `body_rect` AND NOT A FIELD, WHICH IS THE POINT OF THE TYPE. A staged surface now names
         * the WINDOW it goes in, and only the body's seat carries a rectangle in the root's
         * points. Asking for one is therefore also asserting the body is the seat, which is
         * exactly what this test means: nobody popped anything out. */
        let rect = staged
            .body_rect()
            .expect("the body is the seat when no pop-out is open");
        assert!(
            rect.width() > 0.0 && rect.height() > 0.0,
            "the reserved rect has an area: {rect:?}"
        );

        /* THE DEFAULT ITSELF, and not the fixture's reading of it. `draw_watch` builds the screen
         * from a boolean this test hands it, so every assertion above would hold on a screen whose
         * own `Default` was still "stopped" and which therefore played nothing in the running app.
         * `WatchScreen::default` is what `Screens` builds and what `windows.rs` boxes. */
        assert!(
            !WatchScreen::default().stopped,
            "a screen nobody has touched has to be the player, or the folio comes up empty on the \
             one screen the owner asked to be a player"
        );
    }

    /// THE FOLIO IS THE WHOLE OF THE LEAVES BELOW THE HEADER, AND THE STAGE TAKES ALL OF IT.
    ///
    /// EDGE TO EDGE IS TWO FACTS AND THIS IS THE SECOND. `main.rs` gives this body a zero margin
    /// (held by `the_watch_body_is_the_one_screen_drawn_edge_to_edge` in that file); this holds
    /// that the screen then USES what it was given, rather than reserving a polite rectangle inside
    /// it. It is read against the folio the pass measured, never against a number, so a margin
    /// creeping back in either file moves both sides and this still fails.
    #[test]
    fn the_video_fills_the_folio_and_leaves_no_border_of_its_own() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let drawn = draw_watch_pass(&ctx, true, &live_status(Some(true)), Vec::new());
        let staged = drawn.stage.expect("a live channel stages the surface");
        assert_eq!(
            staged
                .body_rect()
                .expect("the body is the seat when no pop-out is open"),
            drawn.folio,
            "the video is inset inside the folio; the owner asked for edge to edge"
        );
    }

    /// THE BODY IS THE ONLY CALLER LEFT, AND NOTHING MAY HAND THIS SCREEN A HOST AGAIN.
    ///
    /// WHAT THIS TEST USED TO BE, AND WHY IT CHANGED. It read two call sites, `main.rs`'s and
    /// `windows.rs`'s, and demanded they pass `Host::Body` and `Host::PopOut` respectively. That
    /// second call is gone: the owner, looking at the popped-out window, said it "is supposed to
    /// be a MINIMAL CHROME always on top resizable window... think like resizeable moveable
    /// picture in picture", and a picture in picture window is not this screen with a different
    /// argument. `windows.rs` draws it itself now (`windows::pip`) and hands this screen nothing
    /// at all; what the two share is the ARTWORK, which is [`paint_art`] and [`fit_into`], called
    /// from there.
    ///
    /// THE DEFECT THE OLD RULE WATCHED FOR IS STILL WATCHED FOR, FROM THE OTHER SIDE. eframe hands
    /// a deferred viewport's callback the ROOT window's handle, so a surface asked for from a tool
    /// window's pass is not built in that window, it is built over the MAIN window's body on top of
    /// whatever screen was showing. Nothing on screen would read as a wrong constant, only a video
    /// sitting on the reader's quest list. The rule that prevented it was "windows.rs must pass
    /// `Host::PopOut`"; the rule now is stronger, because `windows.rs` may not reach this screen at
    /// all, and `stage_folio` and `Cx::stage` are what a surface would have to travel through.
    ///
    /// IT READS THE CALL AND NOT THE PROSE. Both files discuss both variants in comments (the
    /// argument for the split is written down where it is made), so "windows.rs never says
    /// Host::Body" would be a rule about words. `main.rs`'s anchor must appear EXACTLY ONCE: a
    /// rename that moves it has to turn this red rather than pass over a rule matching nothing.
    #[test]
    fn no_other_window_draws_this_screen() {
        /* Test text is cut out first, the same way `reach.rs` and
         * `titlebar::tests::no_pill_is_drawn_with_its_click_thrown_away` do it, so a call inside a
         * `mod tests` can never stand in for the production one. */
        /* Test text is cut off, and then COMMENTS are cut out, and the second half is the half
         * this test kept getting wrong. Every rule below is about CODE: both files discuss the
         * deleted split at length in their own prose, because the argument for a deletion is
         * written where it is made, and `windows::pip_chrome` names one of these tests by path.
         * A rule that matched raw text went red on its own citation. rustfmt owns the layout
         * here, so a comment is a line starting with a slash pair, with a slash-star opener, or,
         * inside a block comment, with a star.
         *
         * THE OPENER IS SPELLED OUT IN WORDS RATHER THAN PUNCTUATION ON PURPOSE: Rust block
         * comments NEST, so writing that sequence in here opens one that never closes and the
         * whole file stops parsing. It did. */
        let cut = |src: &str| -> String {
            let code = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
            code.lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !(t.starts_with("//") || t.starts_with("/*") || t.starts_with("*"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let main = cut(include_str!("../main.rs"));
        let windows = cut(include_str!("../windows.rs"));

        /* THE MAIN WINDOW STILL DRAWS IT, EXACTLY ONCE. Without this the whole test could pass
         * on a program that had stopped drawing the Watch screen at all, which is the shape of
         * every reachability failure in this tree. */
        const ANCHOR: &str = "s.watch.ui(ui, cx)";
        assert_eq!(
            main.matches(ANCHOR).count(),
            1,
            "main.rs has {} call sites matching {ANCHOR:?}; either the anchor moved and this \
             rule is watching nothing, or the body stopped drawing this screen",
            main.matches(ANCHOR).count()
        );

        /* AND NO WINDOW HANDS IT A HOST, BECAUSE THERE IS NO HOST. `Host` had two variants and
         * the second had no production caller for as long as `windows.rs` has drawn its own
         * picture in picture body; it is deleted. A `Host` appearing in either file again means
         * somebody has reintroduced the split that let a tool window ask this screen to stage a
         * surface, which eframe would build over the MAIN window's body. */
        for (name, src) in [("main.rs", &main), ("windows.rs", &windows)] {
            assert_eq!(
                src.matches("Host::").count(),
                0,
                "{name} names a Host again; the Watch tool window is a picture in picture \
                 (`windows::pip`), not this screen with another argument"
            );
        }

        /* AND IT BORROWS EXACTLY TWO THINGS FROM THIS MODULE, WHICH IS THE RULE THAT REPLACED A
         * BROKEN ONE. The first cut of this counted `".ui(ui, cx"` in `windows.rs` and demanded
         * zero, which was red the moment it was written: that file draws the Parser, Plane of
         * Sky and LFG screens and always did, and they are ordinary tool windows that SHOULD be
         * drawn there. The rule that was meant is about THIS screen, and the precise form of it
         * is what `windows.rs` may take from this module at all.
         *
         * `paint_art` and `fit_into` are the artwork, which the picture in picture window shares
         * deliberately so that OFFLINE is laid on by the same function in both windows. Anything
         * else reached from there is this screen leaking back into a window that must not stage
         * a surface, which is the defect the deleted `Host` split existed to prevent. */
        const SHARED: [&str; 2] = ["paint_art", "fit_into"];
        let borrowed: Vec<&str> = windows
            .match_indices("watch::")
            .map(|(i, _)| {
                let rest = &windows[i + "watch::".len()..];
                let end = rest
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .filter(|n| !SHARED.contains(n))
            .collect();
        assert!(
            borrowed.is_empty(),
            "windows.rs reaches into the Watch screen for {borrowed:?}; the picture in picture \
             window shares only {SHARED:?} with it, and a surface asked for from a deferred \
             viewport's pass lands over the MAIN window's body"
        );
        /* AND IT MAY NOT BUILD ONE EITHER, WHICH IS A RULE ABOUT CODE AND NOT ABOUT WORDS. Bare
         * `WatchScreen` will not do as an anchor: `windows.rs` records in its own module doc what
         * that window used to be, and this test's whole point is that prose does not count. These
         * are the three ways the TYPE can be used, and a comment naming it in backticks matches
         * none of them: a type position (`Box<WatchScreen>`), a path (`WatchScreen::default`), and
         * a call or tuple pattern (`WatchScreen(`). */
        for form in ["WatchScreen>", "WatchScreen::", "WatchScreen("] {
            assert_eq!(
                windows.matches(form).count(),
                0,
                "windows.rs uses {form:?}, so it builds a Watch screen again; the Watch tool \
                 window is a picture in picture and draws no screen"
            );
        }
    }

    /// CHECK NOW IS IN THE CONTEXT HEADER, IT IS NOT IN THE FOLIO, AND THE ASK REACHES THE APP.
    ///
    /// `Ask::CheckLive` is the app's only manual poll and `main.rs` answers it with
    /// `self.watcher.refresh()`. It has been raised from two places and BOTH ARE GONE. The pop-out
    /// page in this file had no production caller and is deleted. The other was the picture in
    /// picture window's hover chrome, which the owner refused: "that is still TOO MUCH CHROME".
    /// Cutting it there without putting it back somewhere would have left `main.rs`'s arm for this
    /// ask with nothing to raise it, which is a capability deleted by accident rather than a
    /// window tidied. This is the producer now, and this test is what says so.
    ///
    /// IT IS ASSERTED IN THE HEADER AND ABSENT FROM THE FOLIO, WHICH IS TWO CLAIMS AND NOT ONE.
    /// "leaves should be our player or the offline image": a poll control ON the picture is
    /// exactly the furniture that came out of it, so the region matters as much as the presence.
    ///
    /// AND IT SURVIVES THE GATE ABOVE IT, MEASURED ON THE ONE PLATFORM WHERE THAT GATE SHUTS.
    /// `feed_for` builds a Twitch feed out of a handle alone, so a Twitch channel is always
    /// embeddable and the playback controls always draw; YouTube needs a video id it only has
    /// while he is live. YouTube offline is therefore the single reachable state with no playback
    /// controls at all, it is the state where somebody most wants to ask whether he is on, and it
    /// is the state an early return used to cut this button off in. Testing only Twitch would have
    /// passed against exactly the bug this restructure fixed.
    #[test]
    fn check_now_is_in_the_header_and_asks_the_app_to_poll() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let offline = live_status(Some(false));

        let first = draw_watch_pass(&ctx, false, &offline, Vec::new());
        assert_eq!(
            first.ask,
            crate::screens::Ask::None,
            "a pass with no click raised an ask on its own, so the click below proves nothing"
        );
        assert!(
            first.words().iter().any(|w| w == "Check now"),
            "the app's only manual poll is not on screen: {:?}",
            first.words()
        );
        assert!(
            !first.folio_words().iter().any(|w| w == "Check now"),
            "Check now is painted in the FOLIO, which is the player or the picture: {:?}",
            first.folio_words()
        );

        let spot = first.spot("Check now").expect("it was just found");
        let clicked = draw_watch_pass(
            &ctx,
            false,
            &offline,
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::CheckLive,
            "Check now was clicked and the app was never asked to poll"
        );

        /* THE GATE. YouTube offline: no feed, so no Stop and no Watch here, and this is the state
         * the old early return returned from before it could draw anything else. */
        let yt = crate::settings::Platform::YouTube;
        let gated = draw_watch_seeded(
            &ctx,
            yt,
            false,
            false,
            &offline,
            Vec::new(),
            Default::default(),
        );
        let words = gated.words();
        assert!(
            !words.iter().any(|w| w == "Stop" || w == "Watch here"),
            "this fixture was supposed to have nothing to play, so it is not testing the gate at \
             all: {words:?}"
        );
        assert!(
            words.iter().any(|w| w == "Check now"),
            "the poll went with the playback controls; it is the one control that means something \
             on a channel that cannot be embedded: {words:?}"
        );
    }

    /// THE CONTEXT HEADER'S CONTROLS ARE THE MAIN WINDOW'S, AND NO OTHER WINDOW MAY CALL THEM.
    ///
    /// `WatchScreen::header` is `pub` because `main.rs` is a separate crate root from this module,
    /// and a `pub fn` on a screen is an invitation. The Watch tool window is a picture with two
    /// chips on it (`windows::pip`), so a `header` call in `windows.rs` would put a Stop, a Sound
    /// control and a Check now across the top of a window that can host no surface, driving a
    /// player in the OTHER window. Every one of them would look like it belonged to the picture
    /// under it and none of them would.
    ///
    /// THE CONTROL THAT MAKES THIS SHARPER THAN IT WAS is Check now, which used to be drawn in
    /// that window and now is not. This is the rule that stops it being put back by calling the
    /// header from there, which is the shortest route to exactly the toolbar the owner refused.
    ///
    /// IT READS THE CALL AND NOT THE PROSE, the same way the test above does, and it fences
    /// itself: `main.rs` must have exactly one call, so a rename that moves it turns this red
    /// rather than leaving a rule that matches nothing and passes forever.
    #[test]
    fn only_the_main_window_draws_the_watch_screens_header_controls() {
        /* Test text is cut off, and then COMMENTS are cut out, and the second half is the half
         * this test kept getting wrong. Every rule below is about CODE: both files discuss the
         * deleted split at length in their own prose, because the argument for a deletion is
         * written where it is made, and `windows::pip_chrome` names one of these tests by path.
         * A rule that matched raw text went red on its own citation. rustfmt owns the layout
         * here, so a comment is a line starting with a slash pair, with a slash-star opener, or,
         * inside a block comment, with a star.
         *
         * THE OPENER IS SPELLED OUT IN WORDS RATHER THAN PUNCTUATION ON PURPOSE: Rust block
         * comments NEST, so writing that sequence in here opens one that never closes and the
         * whole file stops parsing. It did. */
        let cut = |src: &str| -> String {
            let code = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
            code.lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !(t.starts_with("//") || t.starts_with("/*") || t.starts_with("*"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let main = cut(include_str!("../main.rs"));
        let windows = cut(include_str!("../windows.rs"));
        assert_eq!(
            main.matches("watch.header(ui, ").count(),
            1,
            "main.rs no longer draws the Watch screen's header controls exactly once, so either \
             the controls are gone from the app or this rule has stopped watching them"
        );
        assert_eq!(
            windows.matches(".header(").count(),
            0,
            "windows.rs draws a Watch header in a window that has no context header and can host \
             no surface"
        );
    }

    /// THE SCREEN DRAWS NO LIVE PILL, BECAUSE THE TITLE STRIP ALREADY DOES.
    ///
    /// The strip is on screen from every screen in the app, so a second pill here would say the
    /// same thing twice to a reader who has just clicked one of them to get here.
    ///
    /// IT USED TO RUN OVER TWO HOSTS AND HALF OF IT IS DELETED WITH THE SECOND ONE. The other
    /// half counted the words the pop-out PAGE printed for each platform: how many times it said
    /// "offline", that it named Twitch and YouTube once each. That page has no production caller
    /// (see the note where `Host` used to be) and is gone, so those counts were a measurement of
    /// a window nobody could open. What is left is the rule that still ships.
    ///
    /// THE FENCE STAYS: a pass that painted nothing at all would have no pill either.
    #[test]
    fn the_screen_draws_no_pill_of_its_own() {
        let mut live = live_status(Some(false));
        live.youtube = crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE);
        assert_eq!(
            live.youtube.live, None,
            "the fixture needs two DIFFERENT states"
        );

        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let drawn = draw_watch_pass(&ctx, false, &live, Vec::new());
        assert!(
            drawn.stage.is_some() || !drawn.words().is_empty(),
            "the screen painted nothing at all, so the absence below is free"
        );
        assert!(
            drawn.pill.is_none(),
            "the screen painted a pill at {:?}; the live pill belongs to the title strip",
            drawn.pill
        );
    }

    /// THE BODY'S FOLIO CARRIES NONE OF THE PAGE FURNITURE, IN ANY STATE, AND THIS IS THE WHOLE
    /// SHAPE OF WHAT THE OWNER ASKED FOR.
    ///
    /// IT READS `folio_words` AND NOT `words`, WHICH IS A CORRECTION AND NOT A RELAXATION. The
    /// harness draws the context header and the folio in one pass because the app does, so the
    /// full run list has always held the header's own controls; this test named the FOLIO in its
    /// title and then measured both. Scoping it to the folio's rectangle is what makes the title
    /// true, and it is stricter in the direction that matters: furniture moved from the folio up
    /// into the header used to satisfy this and now does not, because the header is not in the
    /// rectangle being read.
    ///
    /// "leaves should be our player or the offline image". Six things used to stand between the
    /// reader and that: the heading with the state, the last checked line, Check now, the refusal
    /// sentence, two large browser buttons with a caption, and a VIDEOS band with two more buttons
    /// and a second dated line. Every one of them is a thing this asserts is not painted.
    ///
    /// IT IS DRIVEN OVER EVERY STATE THE FOLIO HAS, because each of the six was drawn from a
    /// different condition and a cut that missed one would be green in the state the author
    /// happened to open. Live and playing (the folio is the video), live and stopped (the folio is
    /// words), offline (the folio is the picture, or its words when a headless pass has no
    /// picture), and never polled.
    ///
    /// THE FENCE MATTERS AS MUCH AS THE ABSENCES. A folio that painted nothing at all would satisfy
    /// every assertion below, and that is exactly the failure this tree keeps finding, so each pass
    /// is required to have produced either a stage or some ink of its own first.
    #[test]
    fn the_body_paints_no_page_furniture_at_all() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let mut offline = live_status(Some(false));
        offline.youtube.checked_at = Some(Utc::now());
        offline.youtube.source = "youtube-page";
        let mut live = live_status(Some(true));
        live.twitch.checked_at = Some(Utc::now());
        live.twitch.source = "twitch-gql";
        live.twitch.title = Some("PoSky keys night".to_owned());

        for (watching, status, what) in [
            (true, live.clone(), "live and playing"),
            (false, live.clone(), "live and stopped"),
            (true, offline.clone(), "offline"),
            (true, live_status(None), "never polled"),
        ] {
            let drawn = draw_watch_pass(&ctx, watching, &status, Vec::new());
            let words = drawn.folio_words();
            assert!(
                drawn.stage.is_some() || !words.is_empty(),
                "{what}: the folio produced neither a stage nor a word, so every absence below is \
                 free"
            );
            /* The status heading, in any of its three forms, and the age line under it. */
            for gone in [
                "Twitch \u{00B7} LIVE",
                "Twitch \u{00B7} offline",
                "Twitch \u{00B7} unknown",
                "last checked",
                "never checked",
                /* CHECK NOW IS IN THE CONTEXT HEADER NOW and this is still the right list to
                 * have it in: the folio is the player or the picture, and a poll control on the
                 * picture is exactly the furniture the owner cut. `folio_words` is what makes
                 * this an assertion about the folio rather than about the screen. */
                "Check now",
                "VIDEOS",
                "Twitch videos",
                "YouTube channel",
                "Open on Twitch",
                "Open chat",
                /* `browser_note()` built this and is deleted with the page that printed it. The
                 * literal stays, because the rule is about what a READER may see and outlives
                 * the function that used to produce it. */
                "Playback opens in your browser",
            ] {
                assert!(
                    !words.iter().any(|w| w.contains(gone)),
                    "{what}: the folio painted {gone:?}, which was the deleted pop-out page: \
                     {words:?}"
                );
            }
        }
    }

    /// AND WHEN THERE IS A PICTURE THE FOLIO SAYS NOTHING AT ALL, WHICH NO HEADLESS PASS CAN SHOW.
    ///
    /// `channel_art::artwork` answers `None` in every test binary by construction (the fetch switch
    /// is a production flag: see `drawing_the_offline_screen_asks_no_host_for_anything`), so the
    /// offline case above always falls through to the words and the "picture instead of a sentence"
    /// rule cannot be read out of a frame. It is held by shape and by two tests that each own half
    /// of it: `folio` returns the moment `folio_art` has painted, so no word can follow a picture in
    /// the same pass, and `the_picture_is_painted_centred_with_offline_laid_over_it` proves the
    /// picture itself paints exactly ONE word, which is OFFLINE.
    ///
    /// WHAT IS LEFT FOR THIS TO PROVE IS THE OTHER HALF: with no picture the folio does not go
    /// silent. An offline channel whose artwork never arrived is the state that used to be carried
    /// by a heading and a refusal sentence, and if the cut had taken the words with the band the
    /// screen would be an empty black rectangle with no reason in it.
    #[test]
    fn an_offline_channel_with_no_picture_still_says_why_the_folio_is_empty() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        assert!(
            !crate::channel_art::fetching_allowed(),
            "artwork would be available here, so this case is not the one it names"
        );
        let drawn = draw_watch_pass(&ctx, true, &live_status(Some(false)), Vec::new());
        assert!(drawn.stage.is_none(), "an offline channel stages nothing");
        assert!(
            drawn
                .words()
                .iter()
                .any(|w| w.contains("is not live on Twitch right now")),
            "the folio has no picture and no reason either, which is a black rectangle: {:?}",
            drawn.words()
        );
    }

    /// `play_here` IS WHAT THE ASK LANDS ON, and it must be enough on its own.
    ///
    /// `App::answer` enters the Watch screen and calls this. If it cleared some second flag the
    /// screen also needed, or if the screen only ever believed its own button, the pill's click
    /// would navigate and then sit there showing "Watch here" to a reader who has already clicked
    /// once.
    ///
    /// IT IS DRIVEN FROM A STOPPED SCREEN, AND THAT IS THE ONLY CASE THAT CAN STILL FAIL. The folio
    /// plays by itself now, so a fresh screen stages a surface whether or not `play_here` does
    /// anything at all: the old shape of this test, which called it on a `Default` and read the rect
    /// back, would be green against an empty function body. The state where the ask still has work
    /// to do is the one where the reader pressed Stop. `stopped` survives leaving this screen and
    /// coming back, deliberately (a stop a step through the rail undid would not be a stop), so the
    /// pill is the way back in and this is the whole of it.
    ///
    /// THE CONTROL COMES FIRST. The same screen is drawn BEFORE the ask and must stage nothing, or
    /// the frame after would prove only that a live channel plays, which is another test's job.
    #[test]
    fn play_here_is_the_way_back_in_after_a_stop() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = live_status(Some(true));
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);

        let mut screen = WatchScreen {
            stopped: true,
            ..WatchScreen::default()
        };
        let mut pass = |screen: &mut WatchScreen| -> Option<crate::player::Stage> {
            let mut cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                player: Default::default(),
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
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
            out.drop_without_applying_deltas();
            cx.stage
        };

        assert!(
            pass(&mut screen).is_none(),
            "a stopped screen staged a surface, so nothing below can prove the ask did anything"
        );
        screen.play_here();
        assert!(
            !screen.sound,
            "the ask does not turn sound on; streams start muted"
        );
        let staged = pass(&mut screen)
            .expect("play_here is the whole of the ask, and it has to be enough to start it");
        assert_eq!(
            staged.feed,
            crate::player::Feed::Twitch {
                login: "broken_stoic".into()
            }
        );
    }

    /// THE SIGNED OUT LINE IS ON SCREEN WHERE THE READER IS CHOOSING, AND ONLY THERE.
    ///
    /// A function that returns the right sentence and is never drawn is this tree's defining
    /// defect, so this reads the words back out of a real frame rather than calling
    /// `signed_out_line` and admiring the result.
    ///
    /// IT USED TO BE DRAWN BESIDE THE RUNNING PLAYER AND IT CANNOT BE ANY MORE. The folio IS the
    /// player, edge to edge, and nothing may be drawn over a native child window. So the sentence
    /// moved to the one moment it is still both true and actionable: the feed is playable, the
    /// player is fine, and the reader has stopped, which is the moment before they decide whether
    /// this window is the way they want to watch. Its own doc names that reader ("what a reader is
    /// owed before they decide this window is the lesser way to watch"), so the line went to the
    /// state that doc describes rather than out of the app.
    ///
    /// EVERY ABSENCE IS FENCED. A pass that painted nothing would satisfy all four negatives, which
    /// is the failure this tree keeps finding, so each one has to have staged a surface or painted
    /// some ink of its own.
    #[test]
    fn the_signed_out_line_is_painted_where_the_reader_is_choosing_and_nowhere_else() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let want = signed_out_line(crate::settings::Platform::Twitch);

        let choosing = draw_watch_with(
            &ctx,
            false,
            &live_status(Some(true)),
            Vec::new(),
            player_stopped(),
        );
        assert!(
            choosing.stage.is_none(),
            "not a discriminating case: this pass is playing, so the line would have nowhere to go"
        );
        assert!(
            choosing.words().iter().any(|w| w == &want),
            "the reader is being offered a player and is never told it watches signed out: {:?}",
            choosing.words()
        );

        for (watching, live, player, why) in [
            (
                true,
                Some(true),
                player_stopped(),
                "the player is up and nothing may be drawn over it",
            ),
            (
                false,
                Some(false),
                player_stopped(),
                "the channel is offline, so nothing is embedded",
            ),
            (
                false,
                None,
                player_stopped(),
                "the live state is not known yet",
            ),
            (
                false,
                Some(true),
                player_refused(),
                "this machine can host no surface at all",
            ),
        ] {
            let drawn = draw_watch_with(&ctx, watching, &live_status(live), Vec::new(), player);
            assert!(
                drawn.stage.is_some() || !drawn.words().is_empty(),
                "{why}: the pass produced neither a stage nor a word, so the absence is free"
            );
            assert!(
                !drawn.words().iter().any(|w| w == &want),
                "the signed out line was painted when {why}: {:?}",
                drawn.words()
            );
        }
    }

    /// NO FACT ABOUT A PLAYER IS PRINTED WHERE THERE IS NO PLAYER TO DESCRIBE, AND THIS IS A DEFECT
    /// THE RUNNING APP SHOWED ME.
    ///
    /// Clicking the pill on an OFFLINE channel turned the screen's flag on without building any
    /// surface, and under "Broken Stoic is not live on Twitch right now" the screen printed the
    /// WebView2 profile path and "tracking prevention not read yet". Both facts were true and
    /// neither was about anything that existed.
    ///
    /// THE CONDITION MOVED WITH THE LINES AND THE DEFECT STAYS SHUT. It used to be "the surface is
    /// up", which is the state that can no longer carry a word at all: the folio is the video and
    /// nothing may be drawn over it. It is now "this feed is playable, this player is fine, and the
    /// reader has stopped", which is where the folio has room and where the two facts are worth
    /// reading. An offline or unpolled channel fails it exactly as it failed the old one, which is
    /// what the negative cases below are for.
    ///
    /// IT ASSERTS ON THE PATH AND THE LEVEL, not on a sentence, because those two runs are what a
    /// reader actually sees, and it checks them PRESENT first so the absences are not free.
    #[test]
    fn the_player_machine_facts_are_printed_only_where_there_is_a_choice_to_make() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let says_profile = |d: &Drawn| {
            d.words()
                .iter()
                .any(|w| w.contains("WebView2") || w.contains("tracking prevention"))
        };

        let choosing = draw_watch_with(
            &ctx,
            false,
            &live_status(Some(true)),
            Vec::new(),
            player_stopped(),
        );
        assert!(
            says_profile(&choosing),
            "the profile folder and the tracking level are gone from the one state that has room \
             for them: {:?}",
            choosing.words()
        );

        for (watching, live, why) in [
            (true, Some(true), "the folio is the video"),
            (true, Some(false), "the channel is offline"),
            (true, None, "the live state is not known yet"),
        ] {
            let drawn = draw_watch_with(
                &ctx,
                watching,
                &live_status(live),
                Vec::new(),
                player_stopped(),
            );
            assert!(
                drawn.stage.is_some() || !drawn.words().is_empty(),
                "{why}: the pass produced neither a stage nor a word, so the absence is free"
            );
            assert!(
                !says_profile(&drawn),
                "{why}: the screen printed the player's profile folder and tracking level anyway: \
                 {:?}",
                drawn.words()
            );
        }
    }

    /// THE WORDS THEMSELVES, AND THE SENTENCE THEY REPLACE CONTRADICTED ITSELF.
    ///
    /// It read "This player keeps its own store and cannot borrow your browser's session, so sign
    /// in on Twitch for chat, channel points and a subscriber's ad-free viewing." Signing in on
    /// Twitch, in the reader's own browser, creates exactly the session the first half has just
    /// said this player can never see, so the advice promised perks the stated mechanism cannot
    /// deliver. Every assertion below is red against that sentence and green against this one.
    ///
    /// WHAT IT MUST NOT DO. Send the reader off to sign in anywhere, which is the contradiction;
    /// offer a login in this app, which does not exist; or apologise, because there is nothing to
    /// apologise for. WHAT IT MUST DO: name the platform whose session is not here, and say that
    /// the view still counts, which is the fact a reader actually weighs.
    ///
    /// THE PER-PLATFORM SPLIT IS GONE WITH THE ADVICE THAT NEEDED IT. Channel points are Twitch's
    /// and a member's perks are YouTube's, and the only reason this line named either was to sell
    /// a sign-in it cannot deliver. A sentence that makes no perks claim needs no perks split.
    #[test]
    fn the_signed_out_line_states_the_anonymous_view_and_contradicts_nothing() {
        use crate::settings::Platform;
        for on in Platform::ALL {
            let s = signed_out_line(on);
            assert!(
                s.starts_with("Watching signed out"),
                "the state comes first: {s}"
            );
            assert!(
                s.contains(on.label()),
                "the reader is told whose session is not in the player: {s}"
            );
            assert!(
                s.contains("still counts"),
                "an anonymous view is a real view, and that is the half a reader weighs: {s}"
            );
            /* THE CONTRADICTION, HELD SHUT, AND IT HAS MOVED. The old sentence said this player
             * had no sign-in, so any instruction to go and make one was a contradiction. There is
             * a sign-in now, HERE, on Twitch, and the contradiction is the other one: sending the
             * reader to their own browser, whose session this player has just said it cannot
             * carry. */
            for elsewhere in [
                "in your browser and",
                "sign in in your browser",
                "in chrome",
                "in edge",
            ] {
                assert!(
                    !s.to_lowercase().contains(elsewhere),
                    "{elsewhere:?} sends the reader to make a session this player cannot carry: {s}"
                );
            }
            for hollow in ["sorry", "unfortunately", "has no sign-in"] {
                assert!(
                    !s.to_lowercase().contains(hollow),
                    "{hollow:?} is an apology, or the sentence the control made false: {s}"
                );
            }
            match on {
                /* THE CONTROL IS NAMED BY ITS OWN LABEL, so the sentence and the button cannot
                 * name two different things. And it is `here`: in this player's profile. */
                Platform::Twitch => {
                    assert!(
                        s.contains(SIGN_IN_ON_TWITCH),
                        "the control is not named: {s}"
                    );
                    assert!(
                        s.contains("here"),
                        "where the session is made is not said: {s}"
                    );
                    assert!(
                        s.contains("Twitch's own page"),
                        "whose page it is, is not said: {s}"
                    );
                }
                /* GOOGLE'S REFUSAL IS A FACT AND IS STATED, and no control is named because
                 * there is none to name. */
                Platform::YouTube => {
                    assert!(
                        s.contains("Google refuses"),
                        "the refusal is not stated: {s}"
                    );
                    assert!(
                        !s.contains(SIGN_IN_ON_TWITCH),
                        "a Twitch control on a YouTube line: {s}"
                    );
                }
            }
            /* the house rule, and this file has failed it before */
            for dash in ['\u{2014}', '\u{2013}'] {
                assert!(!s.contains(dash), "a dash in a UI string: {s}");
            }
        }
        assert_ne!(
            signed_out_line(Platform::Twitch),
            signed_out_line(Platform::YouTube),
            "the line names the platform, so the two cannot be the same string"
        );
    }

    /// SIGN IN ON TWITCH SWAPS THE FOLIO TO TWITCH'S OWN PAGE, AND BACK TO THE PLAYER SWAPS IT
    /// BACK, AND THE APP HOLDS NOTHING IN BETWEEN.
    ///
    /// Three passes, each read off the STAGE the folio handed the App, because the stage's feed
    /// is what the surface is rebuilt from and nothing else is: the words on the button could
    /// change and the page would still be whatever this says it is.
    ///
    /// AND THE THIRD PASS IS THE GATE. On YouTube the control is not drawn at all, because Google
    /// refuses account sign-in in an embedded browser and a button for that would be a control
    /// for a feature that does not exist.
    #[test]
    fn sign_in_on_twitch_loads_twitch_s_own_page_in_the_folio_and_back_returns_the_stream() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let on_air = live_status(Some(true));
        let click = |spot: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        };

        /* 1. SIGNED OUT: the control offers a sign-in and the folio is playing the channel. */
        let first = draw_watch_with(&ctx, true, &on_air, Vec::new(), player_up());
        let spot = first
            .spot(SIGN_IN_ON_TWITCH)
            .unwrap_or_else(|| panic!("{SIGN_IN_ON_TWITCH} is not on screen: {:?}", first.words()));
        assert!(
            matches!(
                first.stage.as_ref().map(|s| &s.feed),
                Some(crate::player::Feed::Twitch { .. })
            ),
            "before the click the folio stages the channel: {:?}",
            first.stage
        );
        assert!(
            !first.asked_sign_in,
            "the screen asked for a sign-in nobody clicked"
        );

        /* 2. THE CLICK ASKS THE APP, AND DOES NOT DECIDE ANYTHING ITSELF. This is the whole
         * change: the screen used to flip a private `signing_in` bit that only it could see,
         * beside `twitch_auth`'s own state that only the Chat screen could see. Two sign-ins with
         * near identical names, and the owner reached for the wrong one three times. Now there is
         * one flow: every control that offers a sign-in raises `Cx::auth_begin`, `App::ui` owns
         * the `Auth`, and both screens read the state it publishes. */
        let clicked = draw_watch_with(&ctx, true, &on_air, click(spot), player_up());
        assert!(
            clicked.asked_sign_in,
            "clicking sign-in asked the App for nothing, so no flow would ever start"
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::None,
            "the sign-in went through `auth_begin` and must not also raise a navigation ask"
        );

        /* 3. WHILE TWITCH IS WAITING FOR THE CODE, the folio shows Twitch's own activation page,
         * and it shows it because the shared auth state says Waiting, not because this screen
         * remembered anything. THE ADDRESS IS THE ONE THE DEVICE FLOW HANDED OUT, code and all:
         * loading a bare `twitch.tv/activate` would ask the reader to type eight characters that
         * nobody showed them. */
        let on_page = draw_watch_signing_in(&ctx, &on_air, Vec::new(), player_up());
        match on_page.stage.as_ref().map(|s| &s.feed) {
            Some(crate::player::Feed::TwitchSignIn { at }) => assert!(
                at.contains("WMCLHMKG"),
                "the activation page was staged without the code in it: {at}"
            ),
            other => panic!("a screen mid sign-in must stage the activation page: {other:?}"),
        }
        assert!(
            !on_page
                .words()
                .iter()
                .any(|w| w == "Sound on" || w == "Mute"),
            "a sound control over a sign-in page is a control that does nothing: {:?}",
            on_page.words()
        );

        /* 4. BACK TO THE PLAYER ASKS FOR THE VIDEO, and does NOT abandon the sign-in: the polling
         * thread keeps running so somebody fetching their phone loses nothing. See
         * `Cx::auth_cancel` and `Auth::unwatch`. */
        let back = on_page
            .spot(BACK_TO_THE_PLAYER)
            .unwrap_or_else(|| panic!("the way back is not on screen: {:?}", on_page.words()));
        let returned = draw_watch_signing_in(&ctx, &on_air, click(back), player_up());
        assert!(
            returned.asked_back,
            "Back to the player asked the App for nothing, so the page would never step aside"
        );

        /* 4b. SIGNED IN, there is nothing left to click, and the folio is the stream again. */
        let done = draw_watch_authed(&ctx, &on_air, player_up());
        assert!(
            done.words().iter().any(|w| w == SIGNED_IN_ALREADY),
            "a signed in reader is not told so: {:?}",
            done.words()
        );
        assert!(
            matches!(
                done.stage.as_ref().map(|s| &s.feed),
                Some(crate::player::Feed::Twitch { .. })
            ),
            "once signed in the folio goes back to the stream by itself: {:?}",
            done.stage
        );

        /* 5. YouTube: no control, by gate. */
        let yt = draw_watch_seeded(
            &ctx,
            crate::settings::Platform::YouTube,
            true,
            false,
            &live_status(Some(true)),
            Vec::new(),
            player_up(),
        );
        assert!(
            !yt.words().iter().any(|w| w == SIGN_IN_ON_TWITCH),
            "a Twitch sign-in control on a YouTube stream: {:?}",
            yt.words()
        );
    }

    /* ---- the playback state machine ---- */

    /// THE WHOLE TRUTH TABLE, and the two rows that used to be wrong are called out by name.
    ///
    /// Sixteen combinations of four booleans, written out rather than generated, because the value
    /// of this test is that a reader can see what each state DOES and disagree with it. The two
    /// defects it holds shut are both "a control decided from whether a surface ought to exist
    /// instead of from whether one does".
    #[test]
    fn the_playback_state_machine_never_strands_a_live_surface_and_never_dead_ends_a_failure() {
        /* (asked, can_play_here, playing, failed) -> (video, stop, start, sound). The type is left
         * to inference: written out it is eight bools wide and clippy's `type_complexity` is
         * right that nobody should have to read that. */
        let rows = [
            /* nothing asked, nothing wrong: the way in, and only the way in. */
            (false, false, false, false, false, false, false, false),
            (false, true, false, false, false, false, true, false),
            /* asked, and it plays. */
            (true, true, false, false, true, true, false, true),
            /* asked on a window or a channel that cannot host it: the button is dead, and it
             * SHOULD be, because there is no surface and no click makes one. */
            (true, false, false, false, false, false, false, false),
            /* THE STRANDED ROW. A surface is alive and it is no longer embeddable (the channel
             * went offline under it, or the platform was switched). The band is gone, and Stop is
             * enabled, because the webview is still there. This row read `stop = false` before. */
            (true, false, true, false, false, true, false, false),
            (false, false, true, false, false, true, false, false),
            /* a live surface on a still-embeddable feed the reader has stopped asking for: the
             * flag and the surface disagree for the frame between the click and the teardown. */
            (false, true, true, false, false, true, false, false),
            (true, true, true, false, true, true, false, true),
            /* THE MISPLACED ROWS. `set_bounds` failed once and the surface is alive. The band
             * SURVIVES, because the band is what produces the Stage that drives the retry. These
             * rows read `video = false` before, which is why the failure was permanent. */
            (true, true, true, true, true, true, false, true),
            (false, true, true, true, false, true, false, false),
            (true, false, true, true, false, true, false, false),
            (false, false, true, true, false, true, false, false),
            /* THE BUILD-FAILED ROWS. No surface, so no band (a reserved black rectangle with
             * nothing behind it is a dead frame) and no Stop. `start` stays TRUE while the machine
             * could host one, and that is the reader's door back in: it raises `Ask::WatchHere`,
             * which clears the failure. This row read `start = false` before. */
            (true, true, false, true, false, false, true, false),
            (false, true, false, true, false, false, true, false),
            /* refused: `can_play_here` is false, so there is no door and none is offered. */
            (true, false, false, true, false, false, false, false),
            (false, false, false, true, false, false, false, false),
        ];
        for (asked, can, playing, failed, video, stop, start, sound) in rows {
            let got = playback(asked, can, playing, failed);
            assert_eq!(
                got,
                Playback {
                    video,
                    stop,
                    start,
                    sound
                },
                "asked={asked} can_play_here={can} playing={playing} failed={failed}"
            );
            assert!(
                !(got.stop && got.start),
                "one button cannot be both doors at once: {got:?}"
            );
            assert!(
                !playing || got.stop,
                "THE INVARIANT: a surface exists and there is no way to stop it"
            );
        }
    }

    /// THE INVARIANT ON ITS OWN, over every input, because a truth table is a list somebody edits.
    #[test]
    fn a_surface_that_exists_can_be_stopped_in_every_state() {
        for asked in [false, true] {
            for can in [false, true] {
                for failed in [false, true] {
                    assert!(
                        playback(asked, can, true, failed).stop,
                        "a live surface with asked={asked} can_play_here={can} failed={failed} \
                         had no enabled Stop"
                    );
                }
            }
        }
    }

    /// THE STRANDED SURFACE, IN A REAL FRAME, AND THE STOP IS REALLY CLICKED.
    ///
    /// The reader is watching, the channel goes offline, and everything that made the player
    /// embeddable is gone: the video, the sound control, the folio itself, which has gone back to
    /// the offline picture. The WebView2 child window is not gone. Before this, the control printed
    /// "Watch here" and greyed it out, so the surface sat there hidden and holding a connection with
    /// no control that could end it and no restart short of quitting the app.
    ///
    /// IT CLICKS RATHER THAN READS, because a disabled button still paints its label. Two passes:
    /// egui resolves interaction against the rects registered on the previous frame, so a press
    /// and release on a widget's first frame reaches nothing.
    ///
    /// THE STOP IT CLICKS IS IN THE CONTEXT HEADER NOW, and that is the reason the harness draws
    /// the header and the folio in one pass: with Stop out of the folio, a test that ran only
    /// `WatchScreen::ui` would find no such run and fail for the wrong reason, or worse, be
    /// "fixed" by dropping the click and reading the label.
    #[test]
    fn a_live_surface_on_a_channel_that_went_offline_can_still_be_stopped() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let offline = live_status(Some(false));

        let first = draw_watch_with(&ctx, true, &offline, Vec::new(), player_up());
        assert!(
            first.stage.is_none(),
            "nothing can be embedded for an offline channel; this case is about the surface that \
             is already up"
        );
        assert!(
            first.words().iter().any(|w| w == "Stop"),
            "the only control that can end a live surface is not on screen: {:?}",
            first.words()
        );
        let spot = first.spot("Stop").expect("Stop was painted");

        let clicked = draw_watch_with(
            &ctx,
            true,
            &offline,
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            player_up(),
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::StopPlayer,
            "Stop is painted but disabled, so the surface cannot be stopped at all"
        );
    }

    /// STOP PUTS THE SCREEN BACK, NOT JUST THE SURFACE.
    ///
    /// Stop does two things and they are easy to confuse: it asks the App to drop the surface
    /// (`Ask::StopPlayer`), and it puts the SCREEN back to not-watching (`stopped = true`) with
    /// sound off so the next start is muted like every first start.
    ///
    /// Only the first half was held by a test. An adversarial lens found that deleting the screen's
    /// own reset left every assertion green: the ask still fired, the App still tore the surface
    /// down, and then the screen, still believing it was watching, asked for it straight back. Stop
    /// became a flicker.
    ///
    /// AND THE SECOND HALF MATTERS MORE THAN IT DID. The folio plays by itself now, so a Stop that
    /// only raised the ask would be undone on the very next frame by the screen it left standing:
    /// the flag is no longer one the reader had to set, it is the one thing keeping the stream off.
    ///
    /// This clicks Stop for real, on a live channel with a surface up, and reads the screen's own
    /// state afterwards rather than only what it asked for. The negative half matters as much:
    /// a frame where Stop was NOT clicked must leave both flags alone, or the test would pass on
    /// a screen that reset itself constantly.
    #[test]
    fn stop_puts_the_screen_back_to_not_watching_and_not_just_the_surface() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let on_air = live_status(Some(true));

        /* WATCHING, WITH SOUND ALREADY ON, so both halves of the reset have something to undo.
         * The first cut of this test used the plain fixture, where sound is off from the start,
         * and its sound assertion therefore could not fail. */
        let first = draw_watch_seeded(&ctx, TW, true, true, &on_air, Vec::new(), player_up());
        assert!(
            first.watching,
            "the fixture is meant to be watching; nothing below tests anything otherwise"
        );
        assert!(
            first.sound,
            "the fixture is meant to have sound ON, or the sound assertion below is vacuous"
        );
        let spot = first.spot("Stop").expect("a live surface paints Stop");

        /* THE CONTROL: another frame with no click. Both flags must be untouched. */
        let idle = draw_watch_seeded(&ctx, TW, true, true, &on_air, Vec::new(), player_up());
        assert!(
            idle.watching,
            "the screen stopped watching without anyone pressing Stop"
        );
        assert!(
            idle.sound,
            "the screen turned sound off without anyone pressing Stop"
        );
        assert_ne!(
            idle.ask,
            crate::screens::Ask::StopPlayer,
            "the screen asked to stop the player with no click at all"
        );

        let clicked = draw_watch_seeded(
            &ctx,
            TW,
            true,
            true,
            &on_air,
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            player_up(),
        );

        assert_eq!(
            clicked.ask,
            crate::screens::Ask::StopPlayer,
            "Stop did not ask the App to drop the surface"
        );
        assert!(
            !clicked.watching,
            "Stop dropped the surface but left the screen watching, so the App is asked for it \
             straight back and Stop is a flicker rather than a stop"
        );
        assert!(
            !clicked.sound,
            "Stop left sound on, so the next start would come up audible instead of muted"
        );
    }

    /// AND THE READER IS TOLD WHY THERE IS A STOP ON A SCREEN THAT IS SHOWING AN OFFLINE PICTURE.
    ///
    /// An enabled control with no explanation beside it is a control a reader does not press.
    ///
    /// THE SENTENCE GOT SHORTER BECAUSE ITS ROOM DID. It read "{why}. The player is still open and
    /// showing nothing; Stop closes it", printed under the playback row where there was a line to
    /// spare. That row is gone: the folio in this state is the offline PICTURE, which cannot carry
    /// words, and the control moved to the context header, which is one line beside a breadcrumb
    /// and a find box. What is left is [`STILL_OPEN`], the half a reader cannot work out from
    /// what is on screen. WHY nothing can be embedded is the picture's job now.
    ///
    /// IT READS THE CONSTANT AND NOT A QUOTATION OF IT, so the bar and this cannot drift, and the
    /// negative half is what makes it a rule rather than a description: with no surface behind it,
    /// nothing says a player is open, and there is no Stop either.
    #[test]
    fn the_stranded_player_says_it_is_still_open() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let offline = live_status(Some(false));

        let stranded = draw_watch_with(&ctx, true, &offline, Vec::new(), player_up());
        assert!(
            stranded.words().iter().any(|w| w == "Stop"),
            "the fixture is meant to be stranded, with a surface that can still be stopped: {:?}",
            stranded.words()
        );
        assert!(
            stranded.words().iter().any(|w| w == STILL_OPEN),
            "an enabled Stop with nothing saying what it would close: {:?}",
            stranded.words()
        );

        let no_surface = draw_watch_with(&ctx, true, &offline, Vec::new(), Default::default());
        assert!(
            !no_surface.words().iter().any(|w| w == STILL_OPEN),
            "there is no player and the screen said one was still open: {:?}",
            no_surface.words()
        );
        assert!(
            !no_surface.words().iter().any(|w| w == "Stop"),
            "there is no player and the header offered a Stop for it: {:?}",
            no_surface.words()
        );
    }

    /// THE HEADER AND THE FOLIO ARE ONE FRAME AND MUST NOT DISAGREE ABOUT THE PLAYER.
    ///
    /// They are two passes now: `main.rs` shows the top panel, then the central panel, and each
    /// used to be free to work its own answer out of `cx.player` and the live status. `state` is the
    /// one place that does it, and this is what holds them to it, because the disagreement is not a
    /// theoretical one: a Sound control in the bar over a folio that has handed the rectangle back
    /// is a switch wired to nothing, and a folio staging a surface with no Stop above it is the
    /// stranded webview all over again.
    ///
    /// THE OBSERVABLE IS THE SOUND CONTROL. `Playback::sound` is `Playback::video` by construction,
    /// and `video` is what produces the stage, so "the Sound control is on screen exactly when a
    /// surface is being staged" is the same claim read from opposite ends of the frame. Four states,
    /// each reaching a different arm of the folio: the video, the words, the stranded picture, and
    /// the plain offline one.
    #[test]
    fn the_header_and_the_folio_never_disagree_about_the_player() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = live_status(Some(true));
        let offline = live_status(Some(false));

        for (watching, status, player, what) in [
            (
                true,
                &live,
                player_stopped(),
                "live, and the folio is the video",
            ),
            (
                false,
                &live,
                player_stopped(),
                "live, and the reader stopped",
            ),
            (
                true,
                &offline,
                player_up(),
                "offline with a surface still alive",
            ),
            (
                true,
                &offline,
                crate::player::PlayerView::default(),
                "offline with nothing running",
            ),
        ] {
            let drawn = draw_watch_with(&ctx, watching, status, Vec::new(), player);
            let has_sound = drawn.words().iter().any(|w| w == "Sound on" || w == "Mute");
            assert_eq!(
                has_sound,
                drawn.stage.is_some(),
                "{what}: the header offers a sound control ({has_sound}) and the folio staged a \
                 surface ({}); one of the two is reading a different playback state: {:?}",
                drawn.stage.is_some(),
                drawn.words()
            );
        }
    }

    /// AN OFFLINE CHANNEL WITH NOTHING RUNNING CARRIES NO PLAYBACK CONTROL AT ALL, AND THAT IS HALF
    /// THE REASON THE CONTROLS MOVED UP HERE.
    ///
    /// The old row drew its one button in every state and greyed it out when it could do nothing,
    /// because it was ON the screen it belonged to and a reader was owed the screen's shape. The
    /// context header is not that: it is the app's own bar, shared with the breadcrumb, the pop-out
    /// control and the find box, and a dead Watch here parked in it on a channel that is off is a
    /// control for a feature that does not exist, in the one place a reader cannot dismiss.
    ///
    /// SO THE RULE IS: nothing to control, nothing in the bar. The states that carry none of it are
    /// offline and never-polled, which are exactly the states where no click could produce a
    /// surface. A REFUSED machine keeps the button and keeps it dead, because there the channel
    /// really is playable and the reason it cannot be played is on the folio; that case is
    /// `a_machine_that_cannot_host_a_surface_is_offered_no_retry`.
    ///
    /// THE FENCE IS THE LIVE PASS. Without it every assertion here would hold on a header that had
    /// stopped drawing anything at all.
    #[test]
    fn the_header_carries_no_playback_control_on_a_channel_that_cannot_be_played() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let controls = |d: &Drawn| -> Vec<String> {
            d.words()
                .into_iter()
                .filter(|w| w == "Stop" || w == "Watch here" || w == "Sound on" || w == "Mute")
                .collect()
        };

        let playing = draw_watch_with(
            &ctx,
            true,
            &live_status(Some(true)),
            Vec::new(),
            player_stopped(),
        );
        assert_eq!(
            controls(&playing),
            vec!["Stop".to_owned(), "Sound on".to_owned()],
            "the fixture is meant to be watching, or the absences below are free: {:?}",
            playing.words()
        );

        for (live, what) in [(Some(false), "offline"), (None, "never polled")] {
            for watching in [false, true] {
                let drawn = draw_watch_with(
                    &ctx,
                    watching,
                    &live_status(live),
                    Vec::new(),
                    crate::player::PlayerView::default(),
                );
                assert!(
                    !drawn.words().is_empty(),
                    "{what}: the pass painted nothing at all, so this proves nothing"
                );
                assert_eq!(
                    controls(&drawn),
                    Vec::<String>::new(),
                    "{what}, watching={watching}: the header parked a playback control on a \
                     channel no click can play: {:?}",
                    drawn.words()
                );
            }
        }
    }

    /// THE WORD BESIDE THE SOUND CONTROL IS THE BROWSER'S ANSWER AND NEVER THE FLAG WE SET.
    ///
    /// THIS IS THE ONE THE OWNER CAUGHT WITH HIS OWN EYES. An earlier build reported sound while he
    /// watched a silent window, because the screen printed what it had ASKED for. The player asks
    /// `ICoreWebView2_8::IsDocumentPlayingAudio` instead, which is the browser saying whether the
    /// document is actually emitting audio, and `Sound::AskedButSilent` is the state where those two
    /// disagree. Nothing in this file held it: `player.rs` proves the three words are different
    /// strings and that the surface computes them, and the SCREEN reading the right one of the
    /// three was true only because the call site said so.
    ///
    /// THE FIXTURE IS THE DISAGREEING STATE, deliberately. The screen's own flag is ON (so its
    /// button reads Mute) and the browser reports silence, so a header printing its own flag would
    /// say "sound on" and a header printing the browser's says "sound on, but no audio is playing
    /// yet". Any state where the two agree could not tell them apart.
    #[test]
    fn the_header_prints_the_browsers_sound_word_and_never_the_flag_it_was_asked_for() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let drawn = draw_watch_seeded(
            &ctx,
            TW,
            true,
            true,
            &live_status(Some(true)),
            Vec::new(),
            crate::player::PlayerView {
                hosted_elsewhere: false,
                sound: crate::player::Sound::AskedButSilent,
                ..player_up()
            },
        );
        assert!(
            drawn.stage.is_some(),
            "the fixture is meant to be playing, or there is no sound control to read"
        );
        assert!(
            drawn.words().iter().any(|w| w == "Mute"),
            "the screen's own flag is on, so its button offers to turn it off: {:?}",
            drawn.words()
        );
        assert!(
            drawn
                .words()
                .iter()
                .any(|w| w == crate::player::Sound::AskedButSilent.word()),
            "the reader asked for sound, the browser says there is none, and the bar does not say \
             so: {:?}",
            drawn.words()
        );
        assert!(
            !drawn
                .words()
                .iter()
                .any(|w| w == crate::player::Sound::On.word()),
            "the bar printed the flag it was asked for instead of the browser's own answer, which \
             is how a build once claimed sound into a silent window: {:?}",
            drawn.words()
        );
        /* AND THE STAGE STILL CARRIES THE FLAG, because the flag is what the surface is driven by;
         * it is only the WORD that comes from the browser. */
        assert!(
            drawn.stage.expect("staged above").sound,
            "sound was asked for and the stage did not carry the request to the surface"
        );
    }

    /// A PLACEMENT THAT FAILED KEEPS THE BAND, WHICH IS WHAT MAKES THE RETRY POSSIBLE.
    ///
    /// The band is the only thing that produces a `player::Stage`; the Stage is the only thing
    /// that makes the App call `Player::sync`; and `Player::sync` is where the failed `set_bounds`
    /// is pushed again. Dropping the band on any failure at all removed the retry, which is how
    /// one transient error became a player that was dead until the process restarted.
    ///
    /// AND A BUILD THAT FAILED DROPS IT, because there is nothing behind the rectangle. Both
    /// halves are here so a fix that just deleted the gate would fail the second.
    #[test]
    fn a_misplaced_surface_keeps_being_staged_and_a_failed_build_is_not_a_black_rectangle() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let live = live_status(Some(true));

        let misplaced = draw_watch_with(&ctx, true, &live, Vec::new(), player_misplaced());
        assert!(
            misplaced.stage.is_some(),
            "a live surface whose last placement failed is never staged again, so the call that \
             would have worked is never made"
        );

        let no_build = draw_watch_with(&ctx, true, &live, Vec::new(), player_build_failed());
        assert!(
            no_build.stage.is_none(),
            "there is no webview, so a reserved rect would be a black hole with nothing in it"
        );
        assert!(
            no_build
                .words()
                .iter()
                .any(|w| w.contains("could not create the player surface")),
            "the reason is not on screen: {:?}",
            no_build.words()
        );
    }

    /// THE DOOR BACK IN AFTER A BUILD THAT FAILED, CLICKED FOR REAL.
    ///
    /// The button must be ENABLED and it must raise `Ask::WatchHere`, because that ask is what
    /// `App::answer` turns into `Player::retry`. A local `self.watch_here = true` would set a flag
    /// that is already set and the reader would press a live-looking button forever.
    #[test]
    fn a_build_that_failed_leaves_an_enabled_watch_here_that_asks_again() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let live = live_status(Some(true));

        let first = draw_watch_with(&ctx, true, &live, Vec::new(), player_build_failed());
        let spot = first
            .spot("Watch here")
            .expect("the way back in is on screen");
        let clicked = draw_watch_with(
            &ctx,
            true,
            &live,
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            player_build_failed(),
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::WatchHere,
            "the button after a failed build is either disabled or wired to nothing that retries"
        );
    }

    /// A REFUSAL OFFERS NOTHING, because nothing it could offer would work. The button is painted
    /// (a reader is owed the shape of the screen) and clicking it asks for nothing.
    #[test]
    fn a_machine_that_cannot_host_a_surface_is_offered_no_retry() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let live = live_status(Some(true));

        let first = draw_watch_with(&ctx, true, &live, Vec::new(), player_refused());
        assert!(
            first.stage.is_none(),
            "a refused player must never be handed a stage"
        );
        let spot = first.spot("Watch here").expect("the button is still drawn");
        let clicked = draw_watch_with(
            &ctx,
            true,
            &live,
            vec![
                egui::Event::PointerMoved(spot),
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: spot,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            player_refused(),
        );
        assert_eq!(
            clicked.ask,
            crate::screens::Ask::None,
            "a machine with no WebView2 was offered a retry that cannot do anything"
        );
    }

    /// `Ask::WatchHere` IS THE RETRY, AND THIS HOLDS THE APP TO ANSWERING IT AS ONE.
    ///
    /// Every test above stops at the ask, because `App::answer` needs an `App` and `App::ui` needs
    /// an `eframe::Frame`, which has no public constructor. What can be proved without one is that
    /// the arm exists and calls the player, so this reads `main.rs` the way
    /// `the_body_and_the_pop_out_each_hand_the_screen_their_own_host` reads both call sites.
    ///
    /// WITHOUT THIS LINE THE WHOLE RECOVERY IS DECORATION: the button is enabled, the ask travels,
    /// `play_here` sets a flag that was already set, and the failure stands.
    #[test]
    fn the_app_answers_the_watch_here_ask_by_giving_the_player_another_go() {
        let src = include_str!("../main.rs");
        let body = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
        let at = body
            .find("Ask::WatchHere => {")
            .expect("main.rs answers Ask::WatchHere");
        let arm = &body[at..];
        let end = arm.find("\n            }").expect("the arm closes");
        let arm = &arm[..end];
        assert!(
            arm.contains("self.player.retry()"),
            "the WatchHere arm never gives the player another go, so a build that failed can \
             never be tried again:\n{arm}"
        );
        assert!(
            arm.contains("self.screens.watch.play_here()"),
            "the WatchHere arm stopped setting the screen's own flag:\n{arm}"
        );
    }

    /* ------------------------------------------------------------ the art band -- */

    #[test]
    fn fit_into_keeps_the_aspect_and_never_smears_an_avatar() {
        /* A 16 by 9 offline screen in a 16 by 9 band fills it exactly. */
        assert_eq!(
            fit_into(egui::vec2(960.0, 540.0), egui::vec2(800.0, 450.0)),
            egui::vec2(800.0, 450.0)
        );
        /* A very wide banner in a 16 by 9 band is bound by the WIDTH and letterboxed. */
        let drawn = fit_into(egui::vec2(960.0, 159.0), egui::vec2(800.0, 450.0));
        assert_eq!(drawn.x, 800.0);
        assert!(
            (drawn.y - 132.5).abs() < 0.1,
            "the aspect is kept, so it is 159/960 of 800: {drawn:?}"
        );
        /* A 300 pixel avatar in a band big enough to blow it up stops at twice its size. Both
         * limits are exercised deliberately: at 420 tall the BAND is what binds and the cap is
         * never reached, so only the taller case can prove the cap is there at all. */
        assert_eq!(
            fit_into(egui::vec2(300.0, 300.0), egui::vec2(1240.0, 420.0)),
            egui::vec2(420.0, 420.0),
            "the band's height binds first here"
        );
        assert_eq!(
            fit_into(egui::vec2(300.0, 300.0), egui::vec2(1240.0, 800.0)),
            egui::vec2(600.0, 600.0),
            "MAX_ART_UPSCALE, or a 300 pixel avatar is drawn four times over"
        );
        assert_eq!(
            fit_into(egui::vec2(0.0, 0.0), egui::vec2(800.0, 450.0)),
            egui::Vec2::ZERO
        );
        assert_eq!(
            fit_into(egui::vec2(960.0, 540.0), egui::Vec2::ZERO),
            egui::Vec2::ZERO
        );
    }

    /// THE PICTURE IS ACTUALLY PAINTED, IN THE RIGHT RECTANGLE, WITH ITS CAPTION UNDER IT.
    ///
    /// This is the assertion that a texture having been uploaded does not make. It builds an
    /// `Art` from a texture the test makes itself, paints it, and reads the textured mesh back out
    /// of the frame's own shapes: the picture's id, the rectangle it covers, and the words beside
    /// it. A `paint_art` that drew nothing would upload exactly as much as one that drew.
    #[test]
    fn the_picture_is_painted_centred_with_offline_laid_over_it() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        /* A 16 by 9 picture, small, so the numbers below are arithmetic rather than a fixture. */
        let image = egui::ColorImage::from_rgba_unmultiplied([96, 54], &[255u8; 96 * 54 * 4]);
        let texture = ctx.load_texture("test-art", image, egui::TextureOptions::LINEAR);
        let art = crate::channel_art::Art {
            kind: crate::channel_art::Kind::TwitchOffline,
            texture: texture.clone(),
            px: egui::vec2(96.0, 54.0),
        };
        let band = egui::vec2(800.0, 450.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| paint_art(ui, &art, band));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
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
        let painted: Vec<egui::Rect> = flat
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Mesh(m) if m.texture_id == texture.id() => Some(m.calc_bounds()),
                _ => None,
            })
            .collect();
        assert_eq!(
            painted.len(),
            1,
            "the picture is painted exactly once, not zero times and not per frame twice"
        );
        let drawn = painted[0];
        /* 96 by 54 fitted into 800 by 450 is bound by MAX_ART_UPSCALE, so it is 192 by 108,
         * centred in the band, whose top left is the ui's cursor at the origin. */
        assert_eq!(drawn.size(), egui::vec2(192.0, 108.0));
        let centre = egui::Rect::from_min_size(egui::Pos2::ZERO, band).center();
        assert!(
            (drawn.center() - centre).length() < 0.5,
            "the picture is centred in its band: {drawn:?} against {centre:?}"
        );
        /* THE WORD IS ON THE PICTURE, and both halves of that matter: that it is painted at all,
         * and that it is painted WITHIN the picture's own rectangle rather than under it, which
         * is what the caption used to do. A test that only checked the word existed would pass
         * on a caption that had simply been reworded. */
        let texts: Vec<(String, egui::Rect)> = flat
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_owned(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => None,
            })
            .collect();
        let words: Vec<&String> = texts.iter().map(|(w, _)| w).collect();

        let (_, where_it_is) = texts
            .iter()
            .find(|(w, _)| w == OFFLINE_WORD)
            .unwrap_or_else(|| panic!("OFFLINE is not painted at all: {words:?}"));
        assert!(
            drawn.contains_rect(*where_it_is),
            "OFFLINE is painted at {where_it_is:?}, outside the picture at {drawn:?}; the word \
             belongs ON the artwork, not under it"
        );
        assert!(
            (where_it_is.center() - drawn.center()).length() < 2.0,
            "OFFLINE is not centred on the picture: {:?} against {:?}",
            where_it_is.center(),
            drawn.center()
        );

        /* AND THE CAPTION IS GONE. It named the rung the artwork came from, which was one more
         * line saying what the picture already showed. The owner asked for the picture and the
         * word and nothing else, so a caption creeping back is a regression this catches. */
        assert!(
            !words
                .iter()
                .any(|w| w.as_str() == crate::channel_art::Kind::TwitchOffline.caption()),
            "the caption came back under the picture: {words:?}"
        );
        assert_eq!(
            words.len(),
            1,
            "the offline picture paints one word and one only: {words:?}"
        );
    }

    /// NO TEST IN THIS CRATE MAY TOUCH TWITCH OR YOUTUBE. Every draw below goes through the offline
    /// case, which is exactly the case that asks for artwork; the switch in `channel_art` is what
    /// stops that being twenty odd requests to two hosts every `cargo test`.
    #[test]
    fn drawing_the_offline_screen_asks_no_host_for_anything() {
        assert!(
            !crate::channel_art::fetching_allowed(),
            "only main calls allow_fetching, and a test binary has no main"
        );
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        /* The whole screen, in the state that wants a picture, twice: a second pass is what would
         * catch a guard that only holds on the first frame. */
        let offline = live_status(Some(false));
        for _ in 0..2 {
            draw_watch(&ctx, false, &offline);
        }
        /* THE COUNT, NOT THE RETURN VALUE. `artwork` answers `None` on its first call whether or
         * not it started a fetch, because the answer arrives a frame later; only the count can
         * tell a screen that asked nobody from a screen that asked Twitch. */
        assert_eq!(
            crate::channel_art::fetches_started(),
            0,
            "drawing the offline case must not reach gql.twitch.tv or youtube.com from a test"
        );
        assert!(
            crate::channel_art::artwork(&ctx, crate::settings::Platform::Twitch).is_none(),
            "with the switch off there is never artwork"
        );
        assert_eq!(crate::channel_art::fetches_started(), 0);
    }

    /// A body pass on a channel that is NOT live reserves nothing, whatever the reader asked for.
    /// This is the offline case handled honestly: no surface at all, and the refusal sentence in
    /// its place, rather than an embedded player showing whatever an offline channel shows.
    #[test]
    fn an_offline_channel_reserves_no_rect_even_with_watch_here_set() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        for live in [Some(false), None] {
            assert!(
                draw_watch(&ctx, true, &live_status(live)).is_none(),
                "live={live:?} has nothing to embed; the screen says so instead"
            );
        }
    }
}
