//! Every screen the nav can reach. Decision D9.
//!
//! One shared context, `Cx`, is handed to every screen's `ui`. It is a bundle of borrows rather
//! than an owner: the App owns the snapshot, the watcher, the settings and the ingest, and a
//! screen sees them for exactly one frame. That is what keeps a screen from squirrelling away a
//! stale copy of the data and drawing last week's record count over this week's file.
pub mod analysis;
pub mod chat;
pub mod commission;
pub mod dashboards;
pub mod dashgrid;
pub mod dps;
pub mod exalt;
pub mod gear;
pub mod inventory;
pub mod items;
pub mod lfg;
pub mod live;
pub mod logs;
pub mod night;
pub mod parser;
pub mod quests;
pub mod reports;
pub mod sky;
pub mod spells;
pub mod unlocks;
pub mod valet;
pub mod videos;
pub mod watch;
pub mod widgets;
pub mod zones;

/// A request one screen makes of the App: go and show this thing on the screen that owns it, or
/// do the one thing only the App can (poke the watcher).
///
/// WHY A FIELD AND NOT A CALLBACK. A quest row names a zone and an item, and both have screens of
/// their own under FIND. The screen drawing the row does not know which nav slot those screens sit
/// in and should not: it sets `ask`, the integrator reads it after the frame and does the routing.
/// One enum, read once per frame, reset by the reader.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Ask {
    #[default]
    None,
    /// Open the Zones screen on this zone, by its display name as the snapshot spells it.
    ShowZone(String),
    /// Open the Items screen on this item, by its display name as the snapshot spells it.
    ShowItem(String),
    /// Open the Quests screen on this quest, by its title.
    ShowQuest(String),
    /// Open the Spells screen on this spell, by the name spells.json spells it.
    ShowSpell(String),
    /// Poll the live status now rather than at the next tick. The watcher belongs to the App;
    /// `Cx` lends its status and nothing else.
    CheckLive,
    /// Show Broken Stoic in the main window's BODY and start the stream there.
    ///
    /// THE PILL RAISES THIS, from four places: the main window's title strip, the footer, the
    /// Watch screen's own status row, and every tool window's strip. Clicking it used to hand the
    /// channel's URL to the system browser; the owner asked for the stream to appear in the main
    /// viewing area instead, and the surface can only be attached to the ROOT window's handle,
    /// which only `App::ui` holds. So the pill reports the click, this carries it, and
    /// `App::answer` enters the Watch screen and turns its `watch_here` on.
    ///
    /// IT IS NOT A PROMISE THAT ANYTHING PLAYS. An offline or not yet polled channel is never
    /// handed to a player (`screens::watch::feed_for` refuses, with the reason in words), so this
    /// lands the reader on the screen that says what the poller last knew. That is the honest
    /// outcome of a click on an offline pill, and it is what the pill's own hover says will happen.
    WatchHere,
    /// Tear the player surface down. Leaving the Watch screen only HIDES it, because coming back a
    /// moment later should not restart the stream; this is the door that actually drops it, and
    /// the Watch screen's Stop button is what opens it.
    ///
    /// AND THE VIDEOS SCREEN'S "Reload the channel" RAISES THE SAME ASK, WHICH IS NOT A REUSE OF
    /// CONVENIENCE. `Player::stop` is exactly "drop the surface, and clear a failure that could be
    /// retried", and that is precisely what a reload is. What differs is the SCREEN, not the
    /// action: `watch` sets its own `stopped` flag beside this ask, so the surface stays down;
    /// `videos` has no such flag and asks for a surface again on the very next frame, so the same
    /// door reads as a reload and rebuilds the webview at the channel page whatever it had wandered
    /// off to. A second variant with an identical body would be a name a reader has to work out is
    /// not a difference.
    StopPlayer,
    /// OPEN A DESTINATION, AND THE SECTION OF IT NAMED HERE.
    ///
    /// # THE SECTION IS A NAME AND NOT AN INDEX, DELIBERATELY
    ///
    /// The log parser`s section order has already moved once in this tree, and the last literal
    /// section index went with it. An index that no longer names what it named opens the wrong
    /// page silently; a name that no longer exists is a jump that does nothing, which the
    /// dashboard`s `every_tile_opens_a_section_that_exists` catches at build time.
    ///
    /// `None` FOR A DESTINATION WITH NO SECTIONS, which is a real answer and not a gap: the kill
    /// tracker and the loot list are top level rows. Passing a section for one of those would be
    /// a promise about a rail that has nothing to select.
    ///
    /// RAISED BY `screens::dashboards`, whose every tile is a window onto one larger page. The
    /// existing `Show*` asks each name a RECORD and let the target screen jump to it; this one
    /// names a PLACE, which is the thing none of them could say.
    Open(crate::nav::ScreenId, Option<&'static str>),
    /// OPEN SETTINGS, WHICH HAS NO `ScreenId`: it is a `main::Body` the persona gear opens, and
    /// the gear is at the foot of the MAIN window's rail.
    ///
    /// # THIS EXISTS BECAUSE AN EMPTY STATE WAS DESCRIBING A TRIP THE READER COULD NOT MAKE
    ///
    /// `screens::parser::no_fights_words` carries the argument in full and it is worth repeating
    /// here, because this variant is the answer to it. Its `NoFolder` arm says the Logs folder "is
    /// set in the main window's Settings, which opens from the gear at the foot of that window's
    /// rail". Every word of that is true and in `windows::ParserWindow` it is a next step with no
    /// next step: the pop-out has no rail, no gear and no Settings page, and it is the one window
    /// the owner keeps over the game while he plays. Naming the route was the best a screen could
    /// do while there was no ask for it. This is the ask.
    ///
    /// IT OPENS SETTINGS AND SETS NOTHING, WHICH IS THE WHOLE OF WHY IT IS SAFE TO RAISE FROM A
    /// POP-OUT. `no_fights_words` refuses a button that would set the folder from there, and it is
    /// right to: `windows::sync` adopts a tool window's settings WITHOUT reconfiguring the root
    /// window's ingest, so a control that repointed the pop-out would leave the main window reading
    /// the old folder. This one changes no setting at all. `Windows::show` hands a child's ask to
    /// the root and asks for the main window to come forward, so the reader ends up standing in
    /// front of the one editor there is, which is exactly the trip the words were describing.
    OpenSettings,
}

/// What a screen that reads the snapshot shows while the loader thread is still parsing it: the
/// WORKING square, the directory being read, and how long it has taken against the data module's
/// budget. Shared by the main window (every snapshot row) and the tool windows (their own copy of
/// the snapshot), so a load in progress is never drawn as an absence or a failure anywhere.
pub fn loading_notice(
    ui: &mut egui::Ui,
    what: &str,
    root: &std::path::Path,
    for_: std::time::Duration,
) {
    use crate::theme::{GOLD_HI, TEXT, TEXT_2, TEXT_3, WORKING};
    use egui::{FontId, RichText, Sense, Vec2};
    ui.label(
        RichText::new(what.to_ascii_uppercase())
            .font(crate::fonts::display(16.0))
            .color(GOLD_HI),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        let sq = egui::Rect::from_center_size(rect.center(), Vec2::splat(6.0));
        ui.painter().rect_filled(sq, 0.0, WORKING);
        ui.label(RichText::new("loading the snapshot").color(TEXT));
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new("from").color(TEXT_3));
        ui.label(
            RichText::new(root.display().to_string())
                .font(FontId::monospace(11.5))
                .color(TEXT_2),
        );
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(
            RichText::new(format!(
                "{:.1}s so far; the budget is {}s, and a load past it is reported here as a failure with the file it stopped on",
                for_.as_secs_f32(),
                crate::data::LOAD_BUDGET.as_secs()
            ))
            .color(TEXT_3),
        );
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

/* ------------------------------------------------------------------ the surface -- */

/// The smallest rectangle worth handing a webview surface, in egui points. Under this what is left
/// of the window would be a letterbox with no picture in it, so a screen says so instead of
/// starting a stream nobody can see.
///
/// IT IS THE FOLIO'S FLOOR FOR THE PICTURE TOO, AND ONE FLOOR IS THE POINT. The Watch folio is the
/// player's rectangle or the offline picture's, and they are the same rectangle, so a window too
/// small for one is too small for the other and both give way to the words. The pop-out's band has
/// a floor of its own (`watch::MIN_ART_H`) because its picture shares the window with the buttons
/// under it.
///
/// IT LIVES HERE RATHER THAN ON THE WATCH SCREEN BECAUSE TWO SCREENS NOW STAGE A SURFACE. Videos
/// takes the whole folio for a channel page exactly as Watch takes it for the player, and a floor
/// that only one of them consulted would be a floor with a hole in it. `watch` re-exports both
/// names so the arguments recorded against them there still resolve.
pub const MIN_STAGE: egui::Vec2 = egui::vec2(320.0, 180.0);

/// Is there room to host a surface here?
pub fn stage_fits(size: egui::Vec2) -> bool {
    size.x >= MIN_STAGE.x && size.y >= MIN_STAGE.y
}

/// RESERVE THE WHOLE FOLIO FOR A WEBVIEW SURFACE and ask the App to put one there.
///
/// SHARED BY BOTH SURFACE SCREENS AND NOT COPIED INTO EITHER. Watch stages the live player and
/// Videos stages the channel page, and everything between the two is identical: the same rectangle
/// taken whole, the same floor, the same black under it, the same occlusion rule, the same
/// `Cx::stage` hand-off and the same repaint tick. Only the [`crate::player::Feed`] differs, and
/// that is the argument. A second copy of this in a second screen is how the two would come apart,
/// and the way they would come apart is invisible: a fix to the occlusion rule applied to one
/// folio and not the other looks fine in both until something is opened over the wrong one.
///
/// THE RECTANGLE IS PAINTED BLACK FIRST. The surface is a separate OS window composited over this
/// one, so between the frame that reserves the rect and the frame WebView2 first paints into it
/// there is a gap, and the body's ink showing through that gap reads as a layout bug. Black also
/// stands in while the surface is hidden under a popup.
///
/// NOTHING MAY BE DRAWN OVER IT. A native child window owns its rectangle: egui paints into the GL
/// surface underneath and the OS composites the webview over it, so anything egui puts on the video
/// is not on top of it, it is under it and simply gone. `player::occluded_by_overlays` asks egui's
/// own layer list whether anything above the body crosses this rect, and the surface yields the
/// region for the frame rather than losing a fight silently.
pub fn stage_folio(ui: &mut egui::Ui, cx: &mut Cx, feed: crate::player::Feed, sound: bool) {
    use crate::theme::TEXT_3;
    use egui::{FontId, RichText};

    let rect = ui.available_rect_before_wrap();
    if !stage_fits(rect.size()) {
        ui.label(
            RichText::new(format!(
                "the window is too short to play here: this needs {} by {} points and there \
                 are {:.0} by {:.0}",
                MIN_STAGE.x,
                MIN_STAGE.y,
                rect.width().max(0.0),
                rect.height().max(0.0)
            ))
            .font(FontId::proportional(11.5))
            .color(TEXT_3),
        );
        return;
    }
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::BLACK);
    let occluded = crate::player::occluded_by_overlays(ui.ctx(), rect);
    if occluded {
        /* The surface has yielded the region for this frame, so there is no video to draw over
         * and this line is on black. Without it a reader whose stream vanished behind their own
         * search dropdown has no idea what happened. */
        ui.painter().text(
            rect.left_top() + egui::vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            "the player is hidden while something is open over it",
            FontId::proportional(11.5),
            TEXT_3,
        );
    }
    ui.allocate_rect(rect, egui::Sense::hover());
    /* THE SCALE TRAVELS WITH THE RECTANGLE AND IS NOT A SEPARATE ARGUMENT ANY MORE. A rect in the
     * root's points only means anything paired with the root's scale, and while these were two
     * loose values a caller could pair one window's rect with another window's scale. Inside
     * `Seat::Body` that pairing cannot be written down. */
    cx.stage = Some(crate::player::Stage {
        seat: crate::player::Seat::Body {
            rect,
            pixels_per_point: ui.ctx().pixels_per_point(),
            occluded,
        },
        feed,
        sound,
    });
    /* WebView2 paints on its own clock, but the rect only moves when egui lays out again, and
     * a window drag with no pointer events inside the body would otherwise leave the surface
     * behind. */
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

/// Lay a short stack of lines out in the middle of `rect`, each wrapped to a readable column.
///
/// SHARED BY THE TWO SURFACE SCREENS, because both of them have exactly one thing to say when they
/// have no surface to show and both of them have a whole folio to say it in. It was
/// `watch::WatchScreen::folio_words`' own loop; Videos needs the same one, and a second copy is how
/// a fix to the wrap or the centring lands on one folio and not the other.
///
/// WRAPPED TO A COLUMN AND NOT TO THE FOLIO. A sentence run across 1200 points is a line the eye
/// loses its place in, and a folio is as wide as the window.
pub fn centred_words(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    lines: Vec<(String, egui::FontId, egui::Color32)>,
) {
    let wrap = (rect.width() - 40.0).clamp(120.0, 560.0);
    let gap = 6.0;
    let laid: Vec<(std::sync::Arc<egui::Galley>, egui::Color32)> = lines
        .into_iter()
        .map(|(s, f, c)| (ui.painter().layout(s, f, c, wrap), c))
        .collect();
    let total: f32 = laid.iter().map(|(g, _)| g.size().y).sum::<f32>()
        + gap * (laid.len().saturating_sub(1)) as f32;
    let mut y = rect.center().y - total * 0.5;
    for (g, c) in laid {
        let size = g.size();
        ui.painter()
            .galley(egui::pos2(rect.center().x - size.x * 0.5, y), g, c);
        y += size.y + gap;
    }
}

/// The shared context every screen draws with. See the module note for why it borrows.
pub struct Cx<'a> {
    /// IS THE SHELL ALREADY OFFERING THIS SCREEN'S SECTIONS? Tier 3, asked of the frame rather
    /// than of the screen.
    ///
    /// A SCREEN THAT HAS SEVERAL VIEWS OWNS A SWITCHER BETWEEN THEM, and it has to, because the
    /// same screen is drawn in a pop-out tool window where there is no rail of any kind. In the
    /// main window the rail nests those same views under the destination, and a screen that drew
    /// its own row as well would put one choice on two controls, each able to contradict the other
    /// on screen. It did, for one build.
    ///
    /// SO THE SHELL SAYS, AND THE SCREEN OBEYS. True in the main window when `nav::SECTIONS` has
    /// a list for the screen showing; false in every tool window and for every screen with one
    /// view. `railed` and not `hide_tabs`, because the screen is being told a fact about the
    /// frame rather than given an instruction, and the next screen that wants to lay itself out
    /// differently beside a rail can read the same field.
    pub railed: bool,
    /// The loaded snapshot, or None when it could not be found or read (then `data_err` says why).
    pub data: Option<&'a crate::data::Snapshot>,
    /// Why `data` is None, in words a person can act on: the path looked at and the reason.
    pub data_err: Option<&'a str>,
    /// Last known live status of the channels, cheap clone from the watcher.
    pub live: &'a crate::watcher::Status,
    pub settings: &'a mut crate::settings::Settings,
    /// THE TWITCH CHAT READER, TO READ. Borrow its log with `with_log`; that takes the mutex the
    /// reader thread writes under and hands back a `&Log`, so a screen sees one consistent
    /// snapshot without copying two thousand messages once a frame.
    ///
    /// A SCREEN CANNOT CALL `start`, AND NOW THAT IS THE TYPE RATHER THAN A RULE. A `ChatHandle`
    /// has `with_log` and `reconnect_now` and nothing else; the thread owner is `App::chat` and
    /// never leaves it. That matters twice over: four tests in `main.rs` draw every screen of
    /// every destination, and `cargo test` must not open sockets to a live service; and the Chat
    /// screen is drawn in a tool window too, whose context is a CLONE of what the root holds, so
    /// a thread owner could not travel there even if it were allowed to.
    pub chat: crate::chat::ChatHandle,
    /// THE CHAT SCREEN ASKING FOR A CONNECTION, an out parameter exactly like `stage`.
    ///
    /// The screen sets it while it is on screen and wants lines; `App::ui` is what calls `start`,
    /// and `start` is idempotent, so setting this every frame dials once. In a test nothing reads
    /// it and the flag is inert, which is the whole reason it is a flag.
    pub chat_wanted: bool,
    /// WHETHER THIS APP MAY SPEAK, and never the thing that lets it.
    ///
    /// A view and not the `Auth`: the token stays behind `Auth::with_token` and no screen can
    /// reach it, so no screen can put it in a struct that derives `Debug`. See `twitch_auth`.
    pub auth: crate::twitch_auth::AuthView,
    /// The Chat screen asking to start a sign-in. Out parameter, exactly like `chat_wanted`:
    /// `App::ui` owns the `Auth` and is the only thing that may spawn its thread.
    pub auth_begin: bool,
    /// The reader asking to stop WATCHING a sign-in, without abandoning it.
    ///
    /// NOT A CANCEL OF THE FLOW, AND THE NAMING IS THE ONLY WEAK PART OF THIS. `Back to the
    /// player` means "put the video back", not "forget I asked": the polling thread keeps running,
    /// so somebody who steps away to fetch their phone and comes back finds the sign-in still
    /// live. What this stops is the PAGE occupying the folio.
    pub auth_cancel: bool,
    /// THE YOUTUBE HALF OF THE FEED, TO READ. See `ytchat::surface::YtHandle`: no `start`, so a
    /// screen cannot open a browser pane any more than it can open a socket.
    pub yt: crate::ytchat::surface::YtHandle,
    /// The Chat screen asking for the YouTube feed. Out parameter, like `chat_wanted`: `App::ui`
    /// owns the pane and is the only thing that may load a page into it.
    pub yt_wanted: bool,
    pub ingest: &'a mut crate::ingest::Ingest,
    /// The player surface, to READ, in primitives. What it is doing, whether it could be started at
    /// all, where its profile is, and what the BROWSER says about sound. A screen never drives it:
    /// it asks, through `stage` below. See `crate::player::PlayerView` for why it is a snapshot and
    /// not the `Player`.
    pub player: crate::player::PlayerView,
    /// Where the player surface goes this frame, if a screen wants one.
    ///
    /// WHY AN OUT PARAMETER AND NOT A CALL. The surface is attached to the ROOT window's handle,
    /// which only `App::ui` holds, and it has to be placed AFTER layout, because the rectangle is
    /// not known until the screen has drawn everything above it. So the screen states where the
    /// video goes and the App puts it there, exactly like `ask` states where the reader wants to go
    /// and the App routes it. Taken by the App each frame, so a stale rect cannot survive one.
    pub stage: Option<crate::player::Stage>,
    /// WHAT THE WATCH SCREEN WOULD PLAY, whether or not this window is the one playing it.
    ///
    /// THIS IS NOT A SECOND `stage` AND THE DIFFERENCE IS THE WHOLE POINT. `stage` means "the
    /// BODY is hosting, here is its rectangle". This means "there is something to play", and it
    /// is set in exactly the same branch, from exactly the same values, whichever window ends up
    /// hosting. One producer for the feed and the mute flag is what stops the two windows
    /// disagreeing about them for even one frame, and a disagreement there is a rebuild, and a
    /// rebuild is a stream that restarts at the default volume.
    pub demand: Option<(crate::player::Feed, bool)>,
    /// Navigation a screen wants. Screens set it; the App reads it and resets it to `Ask::None`.
    pub ask: Ask,
}

/// THE MOST WORDS A LOG PARSER PAGE MAY PAINT IN ONE STRING WHILE IT HAS DATA TO SHOW.
///
/// COUNTED IN ALPHABETIC WORDS AND NOT IN CHARACTERS, because the long strings a data page is
/// SUPPOSED to paint are long for reasons a character cap punishes: `Wed Jul 15 23:16:50 2026 to
/// Wed Jul 15 23:46:18 2026` is fifty two characters and five words, `Reclusive ghoul magus pet`
/// is a mob's name, and `eqlog_Reviir_freeport.txt` is a filename. Prose is long because it has
/// many WORDS in it, so that is what is counted.
#[cfg(test)]
pub const MAX_WORDS: usize = 12;

/// HOW MANY ALPHABETIC WORDS ARE IN `s`. Two letters or more, so units and initials do not count.
///
/// SPLIT ON WHITESPACE AND NOT ON EVERY NON-LETTER, which is what the first version did and is
/// wrong for the one string on these pages that is longest by right: a path. Splitting
/// `C:\Users\revii\AppData\Local\Temp\...\eqlog_Reviir_freeport.txt` on non-letters counts
/// fourteen "words" and reports the Logs page's most important fact as an essay. A sentence is
/// made of words separated by SPACES; a path is one token with punctuation in it. Trailing
/// punctuation is trimmed so `folder,` counts, and a token with a digit or a slash in it does not
/// count at all, which is how `23:16:50`, `40MB` and the path itself score zero.
#[cfg(test)]
pub fn words(s: &str) -> usize {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|w| w.len() >= 2 && w.chars().all(|c| c.is_ascii_alphabetic()))
        .count()
}

/// EVERY STRING A PAGE PAINTED THAT IS A SENTENCE RATHER THAN A FIGURE.
///
/// # THE RULE THIS ENFORCES, IN THE OWNER'S OWN WORDS
///
/// "The ONLY fucking words should be data." The main window answers "show me all the information
/// on that encounter" and an always-on-top overlay answers "show me what's shitting up and how
/// much". Neither of those questions is answered by a paragraph.
///
/// WHAT THIS CAUGHT WHEN IT WAS WRITTEN. Every tab of the Reports page painted the same two
/// hundred words above its tables: what population the tiles cover, what the rates divide by, and
/// that the fold is a snapshot. All true, all read once, all sitting on top of the numbers a
/// person opened the page for. The design sheets for both surfaces have NO SENTENCES ON THEM.
///
/// # WHY IT ONLY APPLIES WHEN THERE IS DATA
///
/// A page with nothing to draw has nothing BUT words, and the alternative to a sentence there is a
/// blank rectangle that cannot be told from a broken screen. That case is the one place this tree
/// has always insisted on plain English and it stays. So the caller drives the page over the
/// reference capture, where every panel has rows, and asserts the words are gone; the empty state
/// is tested separately and is allowed to speak.
///
/// EXPLANATION IS NOT BANNED, IT IS MOVED. `on_hover_text` costs nothing until somebody asks, and
/// the tiles, headings and scope buttons on these pages carry their caveats there. A hover is not
/// painted, so nothing it says reaches this function.
#[cfg(test)]
pub fn prose(said: &[String]) -> Vec<String> {
    said.iter()
        .filter(|s| words(s) > MAX_WORDS)
        .cloned()
        .collect()
}
