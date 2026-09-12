//! Viewport registry, per-tool windows, always-on-top. Decision D3.
//!
//! EVERY TOOL WINDOW IS A DEFERRED VIEWPORT, AND THAT SHAPES EVERYTHING BELOW.
//! A deferred viewport repaints on its own: when the game is in front and the Watch window is
//! pinned over it, the main window may be minimized or hidden and the Watch window still ticks.
//! An immediate viewport would repaint only when the main window did. That independence is what
//! D3 is for, and it has one cost: egui runs the deferred callback outside the root pass, so the
//! callback must be `Send + Sync + 'static`, which means it cannot borrow the `Cx` the root pass
//! is handed (that `Cx` borrows the App, which is not alive between passes). So the registry owns
//! a SECOND, shared context (`ChildCx`, behind an `Arc<Mutex>`) that the root pass refreshes from
//! its `Cx` on every call to `show`, and the tool windows draw from that. Concretely:
//!
//!   live status   cloned from the root each pass (the contract says `Status` is a cheap clone).
//!   settings      copied through serde each way (the root writes down, a tool window's edit is
//!                 written back up), because a screen such as LFG keeps its board in settings.
//!   ingest        a second `Ingest` with its own cursors, AND ONE BOOTSTRAP HISTORY BETWEEN THEM.
//!                 The sentence that used to be here read "two readers of one log file, each
//!                 reading everything: correct, and the file is never written". The second half
//!                 still holds: nothing in this app writes the log. The first half did not, because
//!                 a tail is not the whole of what an `Ingest` holds. `Ingest::fights` is the
//!                 BOOTSTRAP history: it is written once, by the scan `Ingest::new` starts, and the
//!                 live path never touches it again (see the note on `ingest::LIVE_LINES`, which is
//!                 the moving picture beside that photograph). The root builds its `Ingest` when the
//!                 app starts; this one is built when the FIRST tool window opens, which for the
//!                 owner is an hour of raiding later, and the Rescan control on either side refolds
//!                 only the ingest it was pressed in. So the two folds covered two different
//!                 stretches of one growing file: the pop-out's Fights table listed more fights than
//!                 the body's, the two printed different totals in the same sentence, and neither
//!                 said which one it was. Two readers each reading everything would be correct. Two
//!                 readers reading everything AT TWO DIFFERENT MOMENTS are two answers to one
//!                 question. WHAT PUTS THEM IN STEP IS [`Windows::sync`], which compares the two
//!                 bootstrap stamps on every root pass and hands the NEWER fold to the other side
//!                 through `Ingest::adopt_history`. The stamp travels with the rows, so the next
//!                 comparison says "in step" and neither side re-adopts; and because the comparison
//!                 is symmetric rather than root-to-child, a Rescan pressed in either window reaches
//!                 both. What stays per window, deliberately, is everything the LIVE fold owns: the
//!                 byte cursor, the tailed file, and the kill and loot streams. Those are facts
//!                 about one reader of one handle and could only be shared by sharing the handle.
//!   snapshot      a second `Snapshot`, loaded on a background thread the first time a window
//!                 that reads it (Sky) opens. 21MB parsed twice is the price of a Sky window
//!                 that lives without the main one. Watch, Parser and LFG never trigger it: the
//!                 Parser reads its roster through the ingest, not the snapshot.
//!
//! THE WATCH WINDOW IS A PICTURE IN PICTURE AND IT IS NOT A SCREEN. The owner, looking at what it
//! used to be: "this is supposed to be a MINIMAL CHROME always on top resizable window dude...
//! think like resizeable moveable picture in picture. from back in the day."
//!
//! WHAT IT WAS AND WHY THAT WAS BACKWARDS. It drew the `WatchScreen` in a second mode and let
//! that screen decide what a pop-out looks like, so when the main window's folio went full bleed this
//! window inherited everything the folio shed: a title strip, a status sentence, a last checked
//! line, Check now, two large browser buttons, a caption under them and a VIDEOS band with two
//! more buttons. The reasoning written down for that was "everything the body sheds is still
//! reachable in that window", which treats a pop-out as an OVERFLOW BIN. It is not one. It is a
//! small thing that floats over the game showing what is on, so it draws THE PICTURE and the
//! smallest chrome an undecorated window can be worked with, and it draws that chrome only while
//! the pointer is inside it. See [`pip`], which is the whole of its body.
//!
//! AND IT SHOWS A PICTURE RATHER THAN THE STREAM, WHICH IT SAYS OUT LOUD. `wry` attaches a child
//! surface to a `HasWindowHandle`, and the only one eframe hands out is the ROOT window's
//! (`eframe::Frame`); egui 0.36 gives a deferred viewport's callback a `(&mut Ui, ViewportClass)`
//! and no handle at all, so a surface asked for from this pass would be built over the MAIN
//! window's body. That is not a thing to work around, it is the shape of the API, and a person who
//! pops out a LIVE channel and gets a still picture has to be told where the stream is. That is
//! the one sentence this window carries: see [`IN_THE_MAIN_WINDOW`].
//!
//! THE OTHER THREE WINDOWS ARE UNCHANGED, and the title strip they draw is still right for them.
//! The Parser, Plane of Sky and LFG windows are ordinary tool windows: a screen in a frame, with a
//! bar naming which screen. Only the Watch slot is a picture (`Slot::is_pip`), and
//! `every_slot_but_watch_still_draws_a_title_strip` reads that back out of a real frame.
//!
//! THE PIN GLYPH IS MANDATORY. A window that is always on top and does not say so is a window
//! the user cannot explain. The three tool windows draw their own title strip (`titlebar::strip`)
//! with the pin glyph filled when pinned and hollow when not, and the hover on it says which; the
//! picture in picture window draws the SAME glyph (`titlebar::pin_glyph`) in its hover bar, so
//! that signal is one shape everywhere, as D3 words it. The main window's pin is mirrored from
//! `settings.always_on_top`.
//!
//! AND EVERY PIN IN THIS FILE SURVIVES A RELAUNCH, WHICH FOR FOUR OF THE FIVE IS NEW. The main
//! window's has always been written to settings; a tool window's was a fact about the SESSION, so
//! the owner re-pinned the parser over the game every launch until he stopped bothering. The five
//! are filed under `Slot::id` in `Settings::windows`, which existed for exactly this and had no
//! production caller at all until `Windows::show` grew the reconcile at the head of its window
//! loop. `crate::pin` still owns the mechanism and holds no opinion about any of this; the policy,
//! which window writes and when, is here.
//!
//! AND SO DOES WHETHER A WINDOW WAS OPEN, AND WHERE IT SAT. Same file, same map, same loop, and the
//! same defect until now: `WindowPrefs::open` and `WindowPrefs::rect` were built for this and had no
//! production caller at all. A pop-out is a window the owner arranges ONCE over the game and then
//! wants back; before this, every launch put the parser away and, when he opened it again, dropped
//! it at the default size in the corner beside the main window, however he had left it last night.
//!
//! THE FILE IS READ ONCE AND WRITTEN EVER AFTER, which is the one asymmetry with the pin, and it
//! falls out of who else can write. A pin has a second editing surface (the Settings screen), so it
//! is reconciled in both directions on every pass; nothing anywhere lets a person type "this window
//! is open" or "this window is 780 points wide" except the window itself. So the loop adopts the
//! saved answer on the FIRST pass of the process and the registry is the author from then on. The
//! restore only ever OPENS a window and never closes one, so a hotkey pressed before the first root
//! pass is not quietly undone by the file.
//!
//! THE RECTANGLE IS WRITTEN WHEN IT STOPS MOVING, AND THAT IS NOT TIDINESS. A drag reports a new
//! outer rect on every frame of the drag, and `Settings::save` serialises the whole file, writes a
//! sibling temp file and renames it. Done per frame that is tens of rewrites a second of the ONE
//! file whose corruption costs the owner everything in it (see `settings`'s own note on what an
//! unparsable file does to a load). So the window's own pass records the rectangle in memory and the
//! root writes it once the rectangle has held still for [`RECT_SETTLES_IN`]; `show` asks itself to
//! come back and check, so the settle does not lean on the hotkey poll happening to run.
//!
//! IT IS AN OUTER RECT GOING OUT AND AN INNER SIZE COMING BACK IN, AND FOR THESE WINDOWS THOSE ARE
//! ONE NUMBER. Every window this file builds is `with_decorations(false)`, and winit answers
//! `WM_NCCALCSIZE` for an undecorated window by leaving the client rectangle the same SIZE as the
//! window rectangle, so `GetClientRect` and `GetWindowRect` report one size, and the frame bits are
//! stripped from the style before `AdjustWindowRectEx` ever sees it. That is why a saved rectangle
//! can be handed straight back to `with_inner_size` without the window growing by a frame's width on
//! every launch, and it is the thing to re-measure if a decorated window is ever added here.
//!
//! TOGGLING THE MAIN WINDOW MINIMIZES, IT DOES NOT HIDE. `Ctrl+Alt+G` is the only way back from a
//! hidden window, and if that registration failed there would be no way back at all. A minimized
//! window keeps its taskbar button, so a failed hotkey is an inconvenience and not a stranding.
//! The real minimized state is read from the OS each pass rather than remembered, so a manual
//! restore from the taskbar does not desynchronise the toggle.

use crate::data::{DataError, Snapshot};
use crate::ingest::Ingest;
use crate::screens::lfg::LfgScreen;
use crate::screens::parser::ParserScreen;
use crate::screens::sky::SkyScreen;
use crate::screens::{Ask, Cx};
use crate::settings::Settings;
use crate::theme::*;
use crate::watcher::Status;
use egui::{
    Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/* ------------------------------------------------------------------------ the tools -- */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LfgMode {
    Generic,
    Raid,
    Motes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    /// The main window.
    Companion,
    Watch,
    Parser,
    Sky,
    /// The channel's chat, floating over the game. THE REASON THIS APP EXISTS, per the owner:
    /// watch the stream and talk to the room without leaving EverQuest.
    Chat,
    /// EVERY COMBAT OVERLAY AT ONCE, shown or hidden together. Decision D11.
    ///
    /// NOT ONE WINDOW, AND NOT A CHORD PER OVERLAY. The owner can have any number of these and
    /// they are made at runtime, so there is no fixed table for D4 to name them in. What he
    /// actually does with them is put them all up for a raid and take them all down to alt-tab,
    /// which is one press. Showing or hiding ONE is a click in the Parser list, where the list
    /// he built already is.
    Overlays,
    /// One window, three modes. Opening a different mode on an open LFG window switches it.
    Lfg(LfgMode),
}

impl Tool {
    /// The OS window title, AND the words the window's own title strip prints, which is why the
    /// list below is answerable against `nav::NAV` rather than to taste.
    ///
    /// "Broken Stoic" for Watch is what the goal doc checks for ("`Ctrl+Alt+W` produces a window
    /// titled Broken Stoic"), and it is the one entry here that is not a rail row: the picture in
    /// picture window draws no strip at all, so that name reaches a reader only through the
    /// taskbar, where the channel's name is the useful thing to read.
    ///
    /// # DEFECT: THE POP-OUT CALLED THE LOG PARSER SOMETHING THE RAIL NEVER CALLS IT
    ///
    /// This said "Parser". The rail row is `("Log Parser", ScreenId::Parser)` and the crumb over
    /// the main window's body is built from that same table (`nav::label`), so standing on
    /// CHRONICLE / Log Parser and pressing the picture in picture control opened a window whose
    /// strip and taskbar entry both said "Parser". One destination, two names, and a reader
    /// comparing the two windows has no way to know they are the same thing rather than two
    /// parsers. `the_pop_out_speaks_the_rails_words` is what holds the two together now.
    ///
    /// LFG IS THE ONE PLACE THIS DELIBERATELY DOES NOT COPY THE RAIL ROW, and it is not an
    /// oversight. The rail row is "Groups" and it opens ONE window with THREE modes; a window
    /// titled "Groups" three times over could not be told apart in a taskbar. The words used are
    /// `screens::lfg::Mode::heading`'s own, which is what the main window prints across the top of
    /// that very screen, so the two windows still agree on what the page is called.
    pub fn title(self) -> &'static str {
        match self {
            Tool::Companion => "EQL Grimoire",
            Tool::Watch => "Broken Stoic",
            Tool::Parser => "Log Parser",
            Tool::Sky => "Plane of Sky",
            Tool::Chat => "Chat",
            Tool::Overlays => "Overlays",
            Tool::Lfg(LfgMode::Generic) => "Looking for group",
            Tool::Lfg(LfgMode::Raid) => "Looking for raid",
            Tool::Lfg(LfgMode::Motes) => "Looking for motes",
        }
    }

    fn slot(self) -> Option<Slot> {
        match self {
            Tool::Companion => None,
            Tool::Watch => Some(Slot::Watch),
            Tool::Parser => Some(Slot::Parser),
            Tool::Sky => Some(Slot::Sky),
            Tool::Chat => Some(Slot::Chat),
            /* NO SLOT. An overlay is not one of the five built-in windows; this chord is
             * answered by `Windows::summon`, which flips every overlay's own flag. */
            Tool::Overlays => None,
            Tool::Lfg(_) => Some(Slot::Lfg),
        }
    }
}

/// THE FIVE BUILT-IN OS WINDOWS. `Tool::Lfg(_)` collapses onto one slot.
///
/// THE COMBAT OVERLAYS ARE NOT IN HERE AND THAT IS THE POINT OF D11 STAGE TWO. A slot is a
/// compile-time thing: a variant, an index, a screen. An overlay is made by the owner at runtime,
/// there can be any number of them, and they live in `Inner::overlays` keyed by their own id.
/// `Slot::Dps` was here for one build and was removed rather than kept alongside, because a
/// hardcoded DPS window and a user-made one would be two mechanisms for one idea, each with its
/// own pin, its own remembered size and its own way of being wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Watch,
    Parser,
    Sky,
    Chat,
    Lfg,
}

/// EVERY WINDOW THIS PROCESS CAN OPEN, AND THE ONE PLACE THAT SIZES THE REGISTRY. `Windows::new`
/// builds its `Vec<Window>` from this, so a slot added here gets its window with no second edit.
const SLOTS: [Slot; 5] = [Slot::Watch, Slot::Parser, Slot::Sky, Slot::Chat, Slot::Lfg];

impl Slot {
    fn index(self) -> usize {
        match self {
            Slot::Watch => 0,
            Slot::Parser => 1,
            Slot::Sky => 2,
            Slot::Chat => 3,
            Slot::Lfg => 4,
        }
    }

    fn viewport_id(self) -> ViewportId {
        ViewportId::from_hash_of(("grimoire.tool", self.index()))
    }

    /// THE KEY THIS WINDOW'S PREFERENCES ARE FILED UNDER IN `Settings::windows`.
    ///
    /// A STRING AND NOT `index()`, AND THE REASON IS THE FILE. `index()` is a position in `SLOTS`
    /// and positions move: inserting a slot ahead of Sky would hand Sky's saved pin to Chat on
    /// every machine that had ever set one, silently, with nothing on screen to say why. The
    /// settings file is the one place a name has to outlive a refactor, and `Settings::overlays`
    /// already learned this the same way (`sync_overlays` matches by id and not by position).
    ///
    /// THESE WORDS ARE THE HOTKEY TABLE'S, DELIBERATELY. `hotkeys::DEFAULTS` files the same five
    /// windows under `watch`, `parser`, `sky`, `chat` and `lfg`, and a settings file that spelled
    /// one window two ways in two maps would be a file nobody could read.
    fn id(self) -> &'static str {
        match self {
            Slot::Watch => "watch",
            Slot::Parser => "parser",
            Slot::Sky => "sky",
            Slot::Chat => "chat",
            Slot::Lfg => "lfg",
        }
    }

    /// Whether this window starts above the others when the owner has never said otherwise.
    ///
    /// NONE OF THEM DO, WHICH IS WHAT THE APP HAS ALWAYS DONE, and the value of writing it down is
    /// that `Settings::set_win_pinned` ERASES a key that agrees with it. A stored `false` and an
    /// absent key would otherwise be indistinguishable, and the stored one would pin today's
    /// default in place for ever: change this line in a later build and everybody who had ever
    /// toggled a pin twice would keep the old behaviour with no way to tell why.
    fn pin_default(self) -> bool {
        false
    }

    /// The smallest a BUILT-IN window may be dragged to.
    ///
    /// Every one of the five is a page of controls, and 320 by 200 is roughly the width of their
    /// narrowest column set and enough rows to be worth opening. An overlay uses
    /// [`OVERLAY_MIN`] instead, which is far smaller: a damage meter for a group of five is a
    /// header and five rows, about ninety points tall, and held to a 200 point floor it would sit
    /// over the game as a mostly empty box.
    fn min_size(self) -> Vec2 {
        Vec2::new(320.0, 200.0)
    }

    /// First-open size, in points, FOR A WINDOW THE OWNER HAS NEVER RESIZED.
    ///
    /// THE SECOND SENTENCE HERE HAS GROWN A CLAUSE AND THE FIRST HALF OF IT IS UNCHANGED. It read
    /// "the user's resize is kept after that (the builder passes the same size every pass, and egui
    /// only issues a resize when the builder's size changes)", which was true and was the whole of
    /// it: kept for the session, and gone at the next launch. `Windows::show` now hands the builder
    /// `Settings::win_rect`'s size when there is one and this only when there is not, so a resize is
    /// kept across a relaunch as well. The parenthesis is still exactly why that works: the saved
    /// size is written on a settle and is therefore a STABLE number, and a stable number issues no
    /// resize command and does not argue with the hand that is dragging the window.
    ///
    /// THE WATCH WINDOW OPENS 16 BY 9 AND THE OTHERS DO NOT, WHICH IS NOT A TIDY NUMBER. That
    /// window IS the picture: `pip` fits the artwork whole and letterboxes what is left over on
    /// the app's own ground. It used to open at 520 by 380, which is about 1.37 to 1, so the
    /// channel's 1920 by 1080 offline screen arrived with a black band about 44 points deep above
    /// and below it, on the first frame, before anybody had touched the window. At 480 by 270 the
    /// picture reaches all four edges and a person dragging the window into a shape of their own is
    /// choosing the letterbox rather than being handed one. The three tool windows are pages of
    /// controls and take the sizes their contents need.
    fn default_size(self) -> Vec2 {
        match self {
            Slot::Watch => Vec2::new(480.0, 270.0),
            Slot::Parser => Vec2::new(620.0, 460.0),
            Slot::Sky => Vec2::new(560.0, 680.0),
            /* TALL AND NARROW, WHICH IS THE SHAPE OF CHAT AND NOT A GUESS AT ONE. Every chat
             * client on the service is a column, because a message is a short line and what a
             * reader wants is MORE of them at once rather than wider ones. It also has to sit
             * beside a full screen game without covering it, which a wide window cannot do. */
            Slot::Chat => Vec2::new(380.0, 620.0),
            Slot::Lfg => Vec2::new(500.0, 420.0),
            /* WIDE AND SHORT, WHICH IS THE SHAPE OF A DAMAGE METER. Every row is a name, three
             * short figures and a bar, and the bar is the part that needs width: two dealers
             * within a few percent of each other are told apart by bar LENGTH long before anybody
             * reads the numbers. The height is four or five rows plus the header, because a group
             * is five and a reader glancing at this mid-pull is looking at the top of it. */
        }
    }

    /// Whether this window is the PICTURE IN PICTURE, which draws no screen, no title strip and no
    /// chrome at all until the pointer is inside it. The Watch slot alone; see [`pip`].
    ///
    /// A FUNCTION AND NOT A MATCH AT THE CALL SITE, so the one place that decides which of the two
    /// bodies a window has can be read on its own, per slot, by
    /// `only_the_watch_window_is_a_picture`. `draw_child` asks this and nothing else.
    fn is_pip(self) -> bool {
        matches!(self, Slot::Watch)
    }

    /// Which windows read the item snapshot: Sky alone. Watch reads live status, LFG reads
    /// settings, and the Parser reads its mob roster through the ingest (`Ingest::roster`), which
    /// the child context builds on its own; none of the three touches `Cx::data`, so none is
    /// worth a second 21MB parse. Flip an arm here if a screen grows a need.
    fn needs_snapshot(self) -> bool {
        matches!(self, Slot::Sky)
    }
}

/* --------------------------------------------------------------------- the screens -- */

/* Boxed, every one of them. `SkyScreen` alone is over 900 bytes and clippy's `large_enum_variant`
 * is an error under the goal's `-D warnings` gate. Boxing only the largest would move the lint
 * to the next screen that grows, so all three sit behind one pointer; there are three of these in
 * the whole process and they are built once.
 *
 * THERE IS NO WATCH ARM AND THAT IS THE CHANGE. The Watch slot used to hold a `WatchScreen` and
 * ask it for its second mode, which is how this window inherited a status page: a picture in
 * picture window is not a screen drawn differently, it is its own small thing, and the only part it
 * shares with the Watch screen is the ARTWORK (`screens::watch::paint_art`, called by `pip`).
 * `Pip` carries no state because it needs none: what it draws is the live tri-state, the artwork
 * and whether the pointer is inside the window, and all three are read fresh every pass. */
enum Screen {
    /// The Watch slot. NOT a screen: [`pip`] is the whole of that window's body.
    Pip,
    /// THE WHOLE LOG PARSER DESTINATION, and it used to be one fifth of it.
    ///
    /// See [`ParserWindow`].
    Parser(Box<ParserWindow>),
    Sky(Box<SkyScreen>),
    Chat(Box<crate::screens::chat::ChatScreen>),
    Lfg(Box<LfgScreen>),
}

impl Screen {
    fn new(slot: Slot) -> Screen {
        match slot {
            Slot::Watch => Screen::Pip,
            Slot::Parser => Screen::Parser(Box::default()),
            Slot::Sky => Screen::Sky(Box::default()),
            Slot::Chat => Screen::Chat(Box::default()),
            Slot::Lfg => Screen::Lfg(Box::default()),
        }
    }

    /// Draw the screen this window holds. The Watch slot never reaches here: `draw_child` asks
    /// `Slot::is_pip` first and takes the picture path, which draws no screen at all.
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        match self {
            Screen::Pip => debug_assert!(false, "the picture in picture window draws no screen"),
            Screen::Parser(s) => s.ui(ui, cx),
            Screen::Sky(s) => s.ui(ui, cx),
            Screen::Chat(s) => s.ui(ui, cx),
            Screen::Lfg(s) => s.ui(ui, cx),
            /* THE CONFIG COMES FROM THE WINDOW AND NOT FROM THE SCREEN. A `Screen::Dps` drawn
             * through this arm is the built-in slot, which renders the shipped overlay; a window
             * the owner created carries its own and does not come through here at all. */
        }
    }

    /// The tool this screen currently is. Only LFG varies, with the mode its chips have set.
    fn tool(&self) -> Tool {
        match self {
            Screen::Pip => Tool::Watch,
            Screen::Parser(_) => Tool::Parser,
            Screen::Sky(_) => Tool::Sky,
            Screen::Chat(_) => Tool::Chat,
            Screen::Lfg(s) => Tool::Lfg(s.mode),
        }
    }
}

/// THE LOG PARSER, IN ITS OWN WINDOW, WITH EVERY PAGE THE MAIN WINDOW HAS.
///
/// # DEFECT: FOUR OF THE FIVE SECTIONS HAD NO CODE PATH IN HERE AT ALL
///
/// The Parser tool window held a bare `ParserScreen`, whose views are Kills, Loot, Fights,
/// Analysis and Overlays. The main window's LOG PARSER destination has SECTIONS Dashboards,
/// Live, Fights, Reports and Logs, and four of those five are separate screens reached through
/// `main::draw_screen`, which the pop-out never calls.
///
/// SO POPPING THE PARSER OUT SILENTLY GAVE YOU A DIFFERENT APPLICATION. The control said `put
/// this in its own window`; what came up could not show the page you were looking at, and there
/// was no way from inside it to reach Dashboards, Live, Reports or Logs. The owner's whole
/// reason for a pop-out is to put the parser over the game while he plays, which is exactly
/// when the live page and the dashboard matter most.
///
/// # AND TWO PAGES WENT THE OTHER WAY
///
/// Analysis and Overlays were only ever views of `ParserScreen`, and `ParserScreen::show`'s own
/// doc admitted nothing constructs them. They are sections of the destination now, so both
/// windows have all nine pages and neither has a page the other lacks.
///
/// # WHY ONE STRUCT AND NOT FIVE SLOTS
///
/// A window is a place to stand, not a page. Five separate tool windows would mean five pins,
/// five positions and five things to close, for one destination the owner thinks of as `the
/// parser`. This is one window with the destination's own page row across the top, which is the
/// same row the rail draws in the main window and in the same order.
pub struct ParserWindow {
    /// Which page, as an index into [`ParserWindow::PAGES`].
    view: usize,
    /* THE PAGES. Boxed for the same reason the `Screen` arms are: `-D warnings` fails the
     * build on `large_enum_variant`, and these are built once per process. */
    parser: Box<ParserScreen>,
    dashboards: Box<crate::screens::dashboards::DashboardsScreen>,
    live: Box<crate::screens::live::LiveScreen>,
    reports: Box<crate::screens::reports::ReportsScreen>,
    logs: Box<crate::screens::logs::LogsScreen>,
}

impl Default for ParserWindow {
    fn default() -> Self {
        ParserWindow {
            view: 0,
            parser: Box::default(),
            dashboards: Box::default(),
            live: Box::default(),
            reports: Box::default(),
            logs: Box::new(crate::screens::logs::LogsScreen),
        }
    }
}

impl ParserWindow {
    /// EVERY PAGE OF THE LOG PARSER, IN THE RAIL'S OWN WORDS AND THE RAIL'S OWN ORDER.
    ///
    /// # DEFECT: THIS LIST INVENTED TWO NAMES AND SHUFFLED A THIRD PAGE
    ///
    /// The doc that stood here claimed "the first six are `nav::SECTIONS`' own words in
    /// `nav::SECTIONS`' own order". Both halves had stopped being true and the test under them
    /// could not tell, because it asked only whether every rail section appears SOMEWHERE in this
    /// array.
    ///
    /// THE ORDER. The rail runs Dashboards, Live, Fights, Reports, Logs, Analysis, Overlays. This
    /// list ran Dashboards, Live, Fights, ANALYSIS, Reports, Logs, so Analysis sat fourth here and
    /// sixth there: the same destination handed a reader its pages in two orders depending on
    /// which window he was in, and a hand that had learned "Reports is the fourth one along" was
    /// wrong in the other window.
    ///
    /// THE WORDS. The last two pages were called "Kills" and "Loot". Neither string appears
    /// anywhere in `nav::NAV`: the rail rows that show exactly those two pages (`ScreenId::
    /// KillTracker` and `ScreenId::Loot`, both drawn by `ParserScreen` in `View::Kills` and
    /// `View::Loot`, see `main::draw_screen`) are called "Hunt Journal" and "Loot Journal". So the
    /// pop-out offered two pages under names the main window never prints, which is the same
    /// defect as the window title above it, one level down.
    ///
    /// A HAND WRITTEN LIST rather than one derived from `SECTIONS`, and that is still right: a
    /// section added to the rail must be offered here DELIBERATELY, because "offer it" means
    /// writing the arm in [`ParserWindow::ui`] that draws it, and a list that grew on its own
    /// would silently add a page that fell through to Overlays.
    /// `the_pop_out_offers_every_page_the_rail_does` is what says it was not forgotten, and it
    /// asserts the order now as well as the membership.
    pub const PAGES: [&'static str; 9] = [
        "Dashboards",
        "Live",
        "Fights",
        "Reports",
        "Logs",
        "Analysis",
        "Overlays",
        /* THE TWO ROWS OF THE RAIL THAT ARE PAGES OF THIS DESTINATION RATHER THAN SECTIONS OF IT.
         * They sit under MY LEGEND in `nav::NAV` and not under CHRONICLE / Log Parser, but both
         * are `ParserScreen` in one of its views, `tool_of` sends both here, and a window is a
         * place to stand rather than a page. Their names are `nav::NAV`'s. */
        "Hunt Journal",
        "Loot Journal",
    ];

    /// Open this window on the page with this name. Unknown names leave it where it is, which
    /// is the honest answer: a pop-out that jumped to Dashboards because the caller misspelled
    /// a page would be worse than one that did not move.
    pub fn show_named(&mut self, name: &str) {
        if let Some(i) = Self::PAGES.iter().position(|p| *p == name) {
            self.view = i;
        }
    }

    /// Which page is showing, by name. What the window's own title strip reads.
    pub fn showing(&self) -> &'static str {
        Self::PAGES[self.view.min(Self::PAGES.len() - 1)]
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE PAGE ROW, WHICH THIS WINDOW OWNS BECAUSE IT HAS NO RAIL. `Cx::railed` is the flag
         * that says the shell is already offering a switcher; it is false in every tool window,
         * which is why this row is drawn here and why the inner screens must be told it is
         * TRUE for their own pass. Without that flip `ParserScreen` would draw a second row of
         * its five views six pixels under this one's nine, which is the exact defect the main
         * window's context bar was built to remove. */
        crate::chrome::context_bar(ui, &Self::PAGES, &mut self.view);
        ui.add_space(4.0);
        let railed = cx.railed;
        cx.railed = true;
        match Self::page_of(self.showing()) {
            Page::Dashboards => self.dashboards.ui(ui, cx),
            Page::Live => self.live.ui(ui, cx),
            Page::Reports => self.reports.ui(ui, cx),
            Page::Logs => self.logs.ui(ui, cx),
            Page::Parser(view) => {
                self.parser.show(view);
                self.parser.ui(ui, cx);
            }
        }
        cx.railed = railed;
    }

    /// WHICH BODY ONE PAGE NAME DRAWS.
    ///
    /// # BY NAME AND NOT BY INDEX, WHICH IS THE MAIN WINDOW'S OWN RULE
    ///
    /// This was `match self.view { 0 => .., 1 => .., .. }` against [`ParserWindow::PAGES`], and
    /// `main::on_section` had already written down why that is the wrong shape for this exact
    /// list: "the parser's section list has already moved once in this tree and has just grown by
    /// two, so its arms are looked up rather than counted". The list has moved again, to put
    /// Analysis where the rail puts it, and every numbered arm would have gone on pointing at the
    /// page that used to be there. A reordered array does not fail to compile and does not fail a
    /// test that reads names; it opens Reports on Analysis, in a window floating over a stream,
    /// for ever.
    ///
    /// # A FUNCTION AND NOT A MATCH INSIDE `ui`
    ///
    /// `ui` needs a `Ui`, a `Cx` and a real viewport, so a test of the arms written there could
    /// only read them off the screen. This can be driven with a string, which is what
    /// `every_page_the_pop_out_lists_draws_its_own_body` does to all nine of them.
    ///
    /// AN UNKNOWN NAME DRAWS THE FIGHTS TABLE. `PAGES` and this function are held together by that
    /// test, so the arm is unreachable in a build that passes, and it is here because the
    /// alternative is a panic in a window that floats over a live stream.
    fn page_of(name: &str) -> Page {
        use crate::screens::parser::View;
        match name {
            "Dashboards" => Page::Dashboards,
            "Live" => Page::Live,
            "Reports" => Page::Reports,
            "Logs" => Page::Logs,
            "Analysis" => Page::Parser(View::Analysis),
            "Overlays" => Page::Parser(View::Overlays),
            "Hunt Journal" => Page::Parser(View::Kills),
            "Loot Journal" => Page::Parser(View::Loot),
            _ => Page::Parser(View::Fights),
        }
    }
}

/// One page of the log parser pop-out: which screen draws it, and in which view when the screen is
/// the destination's own. See [`ParserWindow::page_of`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Dashboards,
    Live,
    Reports,
    Logs,
    /// One view of `ParserScreen`, which is the body the main window draws for Fights, Analysis,
    /// Overlays, the Hunt Journal and the Loot Journal alike.
    Parser(crate::screens::parser::View),
}

/* ------------------------------------------------------------- the shared state -- */

/// HOW LONG A WINDOW'S RECTANGLE MUST HOLD STILL BEFORE IT IS WRITTEN TO THE SETTINGS FILE.
///
/// A DEBOUNCE AND NOT A RATE LIMIT, and the difference is what lands in the file. A rate limit
/// writes the rectangle the drag happened to be passing through when the timer expired; this writes
/// the one the drag ENDED on, because the clock restarts every time the rectangle changes. One drag
/// is one save either way, and only one of the two saves the right numbers.
///
/// THREE QUARTERS OF A SECOND IS LONGER THAN A HAND PAUSES MID DRAG AND SHORTER THAN A HAND TAKES TO
/// REACH THE CLOSE BUTTON. Both ends matter: too short and a pause in the middle of a drag writes an
/// interim rectangle, too long and closing the app straight after a drag loses it. Nothing else
/// depends on the number, so it can move.
const RECT_SETTLES_IN: Duration = Duration::from_millis(750);

struct Window {
    open: bool,
    /// Whether this window keeps itself above the others, and what the OS has been told.
    ///
    /// TWO FIELDS BECAME ONE TYPE, and the type is `crate::pin::Pin`. They were `pinned` and
    /// `level_applied`, and the three lines that reconciled them stood in this file four times.
    /// The mechanism is the same for every window that will ever want this; the POLICY is not,
    /// which is why the pin holds no opinion about defaults, persistence or who may toggle it.
    pin: crate::pin::Pin,
    /// THIS WINDOW CHANGED ITS OWN PIN AND THE SETTINGS FILE HAS NOT HEARD YET.
    ///
    /// Exactly `OverlayWindow::dirty`, one field narrower, and for exactly the same reason: the
    /// pin glyph is clicked inside a deferred viewport's callback, which holds this registry's
    /// lock and cannot reach `Settings` at all. The root collects it on its next `show`, writes it
    /// through `Settings::set_win_pinned` and saves.
    ///
    /// `Option<bool>` AND NOT A BARE FLAG, so the value that is written is the one that was
    /// clicked. A flag would make the root read `pin.wants()` back, and between the click and the
    /// next root pass that field is also what `show` pushes settings INTO: the two would race and
    /// the loser would be the click.
    pin_out: Option<bool>,
    /// The first pass gives the window a position beside the main window; after that the user
    /// owns it.
    placed: bool,
    /// WHERE THE OS SAYS THIS WINDOW IS: `[x, y, w, h]` of its outer rect, in points, as its own
    /// pass last read it. `None` until that window has drawn at least once.
    ///
    /// RECORDED BY THE WINDOW AND WRITTEN BY THE ROOT, exactly like `pin_out` and for the same
    /// reason: `outer_rect` is only knowable from inside the deferred viewport's own pass, and that
    /// pass holds this registry's lock and cannot reach `Settings`.
    ///
    /// AND IT IS NOT CLEARED WHEN THE WINDOW CLOSES. The last place a window sat is the answer the
    /// next open wants; forgetting it on close would mean a window remembered its position only for
    /// as long as it was on screen, which is the state that needed no file in the first place.
    rect_out: Option<[f32; 4]>,
    /// WHEN `rect_out` LAST CHANGED, or `None` when what is in it has already been written down.
    ///
    /// THE CLOCK RESTARTS ON EVERY CHANGE, which is what makes [`RECT_SETTLES_IN`] a debounce rather
    /// than a rate limit: a drag in progress keeps pushing the deadline out and only the rectangle
    /// the hand let go of is saved. See the module doc for why a save per frame is not on offer.
    rect_since: Option<Instant>,
    /// Bring to front on the next root pass (the user summoned an already open window).
    focus: bool,
    /// HAS A ROOT PASS RUN SINCE THE LAST THING THIS WINDOW WAS ASKED FOR?
    ///
    /// A DEFERRED VIEWPORT ONLY EXISTS INSIDE A ROOT PASS. `show_viewport_deferred` is called from
    /// `Windows::show`, which runs inside `App::ui`, so a window that has been opened but whose
    /// registration has not been re-issued has no OS window at all and cannot get one until the
    /// root draws. That, and only that, is why `App::logic` is allowed to un-minimize the main
    /// window: the press would otherwise do nothing at all.
    ///
    /// THE FLAG EXISTS BECAUSE THE ALTERNATIVE WAS AN ASSUMPTION AND THE ASSUMPTION WAS FALSE.
    /// `tool_pending` used to answer `open`, and its note argued that inside `logic` open MUST
    /// mean open-and-unbuilt, because an open, visible tool window makes eframe's `show_ui` true
    /// and `logic` would not be running. Measured on 2026-09-05, with one pop-out open and the
    /// main window plainly visible on screen: 1,791 `logic` passes against about 1,740 `ui`
    /// passes in the same run, INTERLEAVED, and every one of those `logic` passes sent
    /// `Minimized(false)` to the root. So the main window could not be minimized at all while any
    /// pop-out was open: it was restored within milliseconds, every time, for ever.
    ///
    /// SET FALSE BY EVERY ASK (`open`, which also covers `summon` and a mode switch) AND TRUE BY
    /// THE PASS THAT SERVICES IT. Not by `Screen`, not by the OS: this is a fact about whether
    /// this registry has had a chance to act, which is exactly the question `logic` needs answered.
    serviced: bool,
    screen: Screen,
}

impl Window {
    fn new(slot: Slot) -> Window {
        Window {
            open: false,
            /* THE SAVED ANSWER IS NOT KNOWN HERE. `Windows::default` builds these before any
             * `Settings` is in reach, so the pin starts at the code's own default and the first
             * `show` pushes the owner's answer into it (see `Windows::show`). That is one pass of
             * "not pinned yet" on a window that has no OS window either, which is why it costs
             * nothing: `Pin::applied` is `None` until a window draws. */
            pin: crate::pin::Pin::new(slot.pin_default()),
            pin_out: None,
            placed: false,
            /* AND THE SAVED RECTANGLE IS NOT KNOWN HERE EITHER, for the reason above it: no
             * `Settings` is in reach. `Windows::show` reads the file into the builder when it
             * places the window, and this field is only ever what the OS has REPORTED, so a
             * value from the file must never be seeded into it or the first pass would write
             * the file's own answer straight back at it. */
            rect_out: None,
            rect_since: None,
            focus: false,
            serviced: false,
            screen: Screen::new(slot),
        }
    }
}

/// A snapshot load in flight: the channel it answers on, the root being parsed, and when the
/// parse started.
type Loading = (
    mpsc::Receiver<Result<Snapshot, DataError>>,
    PathBuf,
    Instant,
);

/// The tool windows' own context. See the module doc for why this exists.
struct ChildCx {
    live: Status,
    settings: Settings,
    ingest: Ingest,
    /// THE ROOT'S CHAT LOG, READ ONLY. Two `Arc` clones, not a second reader: `ChatHandle` has no
    /// `start`, so the pop-out reads the same messages the body reads and cannot open a socket of
    /// its own. What it CAN do is ask, through `Cx::chat_wanted`, which `draw_child` routes back
    /// up to the App below. See `chat::ChatHandle`.
    chat: crate::chat::ChatHandle,
    data: Option<Snapshot>,
    /// Why `data` is `None`, in words the screen can show. Only an ABSENT or FAILED snapshot is
    /// an error; a load in progress is `loading`, and the window draws the loading notice for it
    /// instead of the screen, exactly as the main window does.
    data_err: Option<String>,
    loading: Option<Loading>,
    /// The data root preference the last load attempt used, so a changed Settings value retries
    /// and an unchanged one does not retry every pass.
    attempted_for: Option<Option<PathBuf>>,
}

impl ChildCx {
    fn new(root: &Cx) -> ChildCx {
        /* A serde round trip is the clone the contract does not promise. If it fails (a field
         * that does not survive JSON), fall back to the file, which is what `load` reads. */
        let settings = settings_clone(root.settings).unwrap_or_else(|| {
            log::warn!("settings did not survive a serde round trip; the tool windows read the saved file instead");
            Settings::load()
        });
        let mut ingest = Ingest::new(&settings);
        /* THE TOOL WINDOWS OPEN THE FIGHT STORE TOO, AND THEY DID NOT.
         *
         * `Ingest::use_store` is opt in, deliberately: it was once called inside `Ingest::new`,
         * which pointed every `Ingest` a test ever built at the owner's real fights folder. So it
         * has to be asked for at each production construction site, and there are exactly two:
         * `main::App::new` and this one. This one was missed.
         *
         * WHAT THAT COST IS NOT COSMETIC. In the pop-out, `Ingest::stored` was permanently zero,
         * the hit point book behind `of ~N` was permanently empty, and no fight the owner finished
         * while the parser was popped out was ever written to disk. The pop-out is the window he
         * uses WHILE PLAYING, so it is the window in which fights actually finish.
         *
         * THE SAME FOLDER, AND THAT IS SAFE. `store::Store::append` is keyed on (character,
         * server, the fight's own start stamp) and skips a key it already holds, so both windows
         * writing the same fight writes it once. `Wrote::already` is what counts the skip.
         *
         * A TEST NEVER REACHES THIS. `ChildCx::new` is called from `Windows::draw_child`, which
         * needs a real viewport; the store tests build their own `Store::at(temp)`. */
        ingest.use_store(crate::store::Store::app_data());
        ChildCx {
            live: root.live.clone(),
            settings,
            ingest,
            chat: root.chat.clone(),
            data: None,
            data_err: None,
            loading: None,
            attempted_for: None,
        }
    }

    /// Start, or collect, the background snapshot load for a window that reads it. Never blocks.
    fn poll_snapshot(&mut self, slot: Slot) {
        if !slot.needs_snapshot() || self.data.is_some() {
            return;
        }
        if let Some((rx, root, _)) = &self.loading {
            match rx.try_recv() {
                Ok(Ok(snapshot)) => {
                    self.data = Some(snapshot);
                    self.data_err = None;
                    self.loading = None;
                }
                Ok(Err(e)) => {
                    /* Display, not Debug: the path and serde's reason, as the main window
                     * prints them, never a struct dump on screen. */
                    self.data_err = Some(format!(
                        "the snapshot at {} could not be read. {e}",
                        root.display()
                    ));
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.data_err = Some(format!(
                        "the snapshot loader thread for {} ended without answering",
                        root.display()
                    ));
                    self.loading = None;
                }
            }
            return;
        }
        let preference = self.settings.data_root.clone();
        if self.attempted_for.as_ref() == Some(&preference) {
            return;
        }
        self.attempted_for = Some(preference.clone());
        let Some(root) = preference.or_else(Snapshot::locate) else {
            /* The loader's own probe list, in its order, so these words cannot drift from what
             * `Snapshot::locate` actually tried; the same sentence the main window prints. */
            let tried: Vec<String> = crate::data::candidates()
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            self.data_err = Some(format!(
                "No snapshot found. Looked for {} in: {}. Put the data folder ({}, and the {}/ directory) at the first of those, or name its path on the Settings screen.",
                crate::data::GEAR_FILE,
                tried.join("; "),
                crate::data::FILES.join(", "),
                crate::data::ATLAS_DIR
            ));
            return;
        };
        let (tx, rx) = mpsc::channel();
        let path = root.clone();
        let spawned = std::thread::Builder::new()
            .name("grimoire-snapshot".to_owned())
            .spawn(move || {
                let _ = tx.send(Snapshot::load(&path));
            });
        match spawned {
            Ok(_) => {
                self.loading = Some((rx, root, Instant::now()));
                self.data_err = None;
            }
            Err(e) => {
                self.data_err = Some(format!("could not start the snapshot loader thread: {e}"));
            }
        }
    }

    /// The root being parsed and for how long, while a load is out.
    fn loading(&self) -> Option<(&std::path::Path, Duration)> {
        self.loading
            .as_ref()
            .map(|(_, root, since)| (root.as_path(), since.elapsed()))
    }
}

struct Inner {
    /// In `SLOTS` order.
    windows: Vec<Window>,
    /// THE OWNER'S COMBAT OVERLAYS, in the order his settings file lists them. Kept in step with
    /// `Settings::overlays` by [`sync_overlays`] on every pass.
    overlays: Vec<OverlayWindow>,
    cx: Option<ChildCx>,
    /// The leader hint, drawn in the corner of every tool window while a chord is armed.
    hint: Option<String>,
    /// Settings as JSON after a tool window's pass changed them; the root adopts it on its next
    /// `show`. Only ever set when the text actually differs from what the pass started with.
    settings_out: Option<String>,
    /// The last JSON that passed between root and child in either direction.
    settings_synced: String,
    /// A navigation request a tool window's screen made (`Cx::ask`). The App reads asks from the
    /// root `Cx` after its frame, so the root adopts this one on its next `show` and brings the
    /// main window forward, since that is where the asked-for screen lives.
    ask_out: Option<Ask>,

    /* ------ the three values the picture in picture window and the surface pass between them.
     *
     * TWO GO DOWN AND ONE COMES UP, and none of them is the surface itself. A tool window's pass
     * can neither reach the `Player` nor name a `Feed`, which is what keeps "which window is
     * playing" a question with one answer computed in one place (`player::choose_stage`) rather
     * than a negotiation between two passes that run at different times. */
    /// The main window's own handle, told to the registry by the App each frame.
    ///
    /// It is here for ONE reason: so the handle lookup can refuse to hand back the window the
    /// surface is already parented into. A `SetParent` into the root would succeed and would look
    /// exactly like everything working.
    root_hwnd: Option<isize>,
    /// Whether the surface reports that it is seated somewhere other than the body. Told, not
    /// assumed: a frame where the move failed has to draw the picture and not a black rectangle.
    hosting: bool,
    /// What the pop-out published this pass: where it is, and the shapes the video must withdraw
    /// from. Taken by the App, paired with the Watch screen's demand, and gone.
    pip_out: Option<crate::player::PipOffer>,
    /// A tool window's Chat screen asked for a connection this pass.
    ///
    /// SAME ROUTE AS `ask_out` AND FOR THE SAME REASON. `App::ui` owns the only `ChatReader` and
    /// is the only thing allowed to call `start`; a deferred viewport's pass never sees the root
    /// `Cx`. So the child writes here and `show` ors it into the root's `chat_wanted` before it
    /// returns, which is why `main.rs` runs `windows.show` BEFORE it reads that flag.
    ///
    /// A BOOL AND NOT AN `Option`, because two windows asking is one connection: `start` is
    /// idempotent and the flag is cleared by the root taking it every frame.
    chat_wanted_out: bool,
}

fn lock(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    /* A panic inside a screen must not take the registry with it. The data is plain state and is
     * fine to keep using. */
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn settings_json(s: &Settings) -> Option<String> {
    serde_json::to_string(s).ok()
}

fn settings_clone(s: &Settings) -> Option<Settings> {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
}

/// Bring a window to the front: out of hiding if it was hidden, out of the taskbar, and focused.
///
/// THREE LINES IN A ROW WERE THIS, TWICE, VERBATIM, with a two line version of the same idea a few
/// lines above them. Nothing was wrong with any of them; they were one act spelled out three
/// times, which is how the third comes to be missing a command the other two have. That had
/// already happened: the tool window path never sends `Visible` and the main window's path always
/// does, and nothing anywhere said whether that was a decision.
///
/// IT IS A DECISION, AND `unhide` IS WHERE IT IS NOW WRITTEN. A tool window is never hidden, only
/// minimised or closed, so asking to show one is a command with nothing to do. The main window CAN
/// be hidden, because the main window's toggle hides it, so raising that one has to undo that.
/// Making the caller say which is what stops the two drifting apart again.
///
/// THE ORDER IS NOT ARBITRARY: shown, then restored, then focused. A focus request aimed at a
/// window that is still minimised is one the OS may drop.
fn raise(ctx: &egui::Context, to: ViewportId, unhide: bool) {
    if unhide {
        ctx.send_viewport_cmd_to(to, ViewportCommand::Visible(true));
    }
    ctx.send_viewport_cmd_to(to, ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd_to(to, ViewportCommand::Focus);
}

fn settings_from_json(j: &str) -> Option<Settings> {
    serde_json::from_str(j).ok()
}

/* `level` LIVES IN `crate::pin` NOW, with the state it decides for. It was two lines here and
 * they were the only thing the four copies of the reconcile idiom shared. */
use crate::pin::level;

/* ----------------------------------------------------------------------- the registry -- */

/// What the main window has been asked to do, resolved in `show` against the real OS state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompanionReq {
    Show,
    Toggle,
}

pub struct Windows {
    inner: Arc<Mutex<Inner>>,
    companion_req: Option<CompanionReq>,
    companion_pin_req: Option<bool>,
    /// The level last pushed to the root window, mirrored from `settings.always_on_top`.
    /// The main window's own pin. Same type as every tool window's, and the only thing that
    /// differs is the policy around it: this one is read from and written to settings, so it
    /// survives a restart, and a tool window's does not.
    companion_pin: crate::pin::Pin,
    /// HAS THE SAVED "WHICH WINDOWS WERE OPEN" ANSWER BEEN ADOPTED YET?
    ///
    /// ON `Windows` AND NOT ON `Window`, because it is a fact about the PROCESS and not about any
    /// one window: the answer is read out of `Settings`, and `Settings` is not in reach until the
    /// first `show`. A flag per window would be five copies of one moment.
    ///
    /// ONE WAY ONLY, AND IT NEVER GOES BACK TO FALSE. Re-reading the file later would fight the
    /// registry for authorship of a value the registry is the only writer of; see the module doc.
    restored: bool,
}

impl Default for Windows {
    fn default() -> Self {
        Windows {
            inner: Arc::new(Mutex::new(Inner {
                windows: SLOTS.iter().map(|s| Window::new(*s)).collect(),
                overlays: Vec::new(),
                cx: None,
                hint: None,
                settings_out: None,
                settings_synced: String::new(),
                ask_out: None,
                root_hwnd: None,
                hosting: false,
                pip_out: None,
                chat_wanted_out: false,
            })),
            companion_req: None,
            companion_pin_req: None,
            companion_pin: crate::pin::Pin::new(false),
            restored: false,
        }
    }
}

impl Windows {
    /// What the App knows about the surface, told to the registry once a frame.
    ///
    /// TOLD RATHER THAN REACHED FOR. A tool window`s pass runs inside a deferred viewport`s
    /// callback, which cannot borrow the App and must not be able to: a pass that could reach the
    /// `Player` could move the video, and then two passes running at different times would both
    /// be deciding which window plays. These two values are the whole of what a pop-out is allowed
    /// to know, and neither of them can be acted on from there.
    pub fn tell_player(&mut self, root_hwnd: Option<isize>, hosted_elsewhere: bool) {
        let mut g = lock(&self.inner);
        g.root_hwnd = root_hwnd;
        g.hosting = hosted_elsewhere;
    }

    /// Where the picture in picture window says the video may live, while that window is open.
    ///
    /// READ AND NOT TAKEN, AND THE WORD THAT USED TO BE HERE WAS THE DEFECT. This was `take`, on
    /// the reasoning that an offer is about one frame and a stale one would send the surface
    /// into a dead handle. The first half of that was wrong in a way that broke the pop-out: the
    /// pop-out is a deferred viewport that publishes on ITS clock (100 ms while hosting), the
    /// root reads on ITS clock (at least 10 Hz for the hotkey drain, plus a frame per input
    /// event), and the two are not aligned. Every root frame that fell between two pop-out
    /// passes took nothing, `choose_stage` fell back to the body, the surface was moved or
    /// hidden, and `tell_player` told the pop-out it was no longer hosting, which slowed its
    /// clock to 1 s and made the next miss nine times likelier. The video was on screen about
    /// one root frame in ten. `the_pip_offer_survives_root_frames_between_pop_out_passes_and_
    /// dies_with_the_window` is the frame by frame account and fails on `take`.
    ///
    /// THE SECOND HALF, THE DEAD HANDLE, IS HANDLED WHERE IT BELONGS. An offer is a fact about a
    /// window that is OPEN, so the read is gated on the Watch slot's `open` flag, which is the
    /// registry's own truth about that and flips on every close path there is (the chip, the OS
    /// close, a hotkey toggle) BEFORE the root stops showing the viewport. A fresh open starts
    /// with no offer (see `open`), so the previous window's handle cannot leak into the new one.
    /// And `Surface::place_pip` asks the handle for its client size before pushing anything, so a
    /// handle that died in the microseconds between the flag and this read is a named placement
    /// problem, not a crash.
    pub fn pip_offer(&mut self) -> Option<crate::player::PipOffer> {
        let g = lock(&self.inner);
        if !g.windows[Slot::Watch.index()].open {
            return None;
        }
        g.pip_out.clone()
    }

    /// A CHAT POP-OUT WANTS THE SOCKET THE APP OWNS, and this is how the ask gets there.
    ///
    /// ORED, NEVER ASSIGNED. Both windows can draw the Chat screen on the same frame, and the
    /// body's ask is already in `cx.chat_wanted` when this runs. An assignment would let a frame
    /// in which no pop-out asked CANCEL the body's ask, which is the flicker the pop-out video
    /// seat had and the same mistake in a second place: a fact about one window used to overwrite
    /// a fact about another.
    ///
    /// TAKEN, so a window that has since closed stops asking on the very next frame rather than
    /// holding the socket open for a pop-out that is gone.
    fn adopt_chat_ask(&mut self, cx: &mut Cx) {
        cx.chat_wanted |= std::mem::take(&mut lock(&self.inner).chat_wanted_out);
    }

    /// Open a tool, or bring it to the front if it is open, ON WHATEVER PAGE IT IS ALREADY ON.
    ///
    /// This is [`Windows::open_at`] with no page, and it is the right door for a HOTKEY, which
    /// names a window and nothing else: `Ctrl+Alt+P` means "the parser, please", not "the parser,
    /// on the page I was looking at", because a hotkey pressed from inside the game is not
    /// pressed from a page at all.
    pub fn open(&mut self, tool: Tool) {
        self.open_at(tool, None);
    }

    /// Open a tool ON A NAMED PAGE, or bring it to the front and take it there if it is open.
    /// `Lfg(mode)` on an open LFG window switches its mode. `Companion` restores and focuses the
    /// main window.
    ///
    /// # DEFECT: POPPING OUT FROM ONE PAGE HANDED YOU A WINDOW SHOWING ANOTHER
    ///
    /// This function had an arm for `Tool::Lfg(mode)` that set the child screen's mode, and no
    /// equivalent for the Parser. `Tool::Lfg` CARRIES its mode and `Tool::Parser` carries nothing,
    /// so the LFG pop-out arrived on the page you left and the Parser pop-out arrived on
    /// `ParserWindow::default`, which is Dashboards. Standing on LOG PARSER / Fights, or on the
    /// Loot Journal, and pressing the picture in picture control, whose hover says "put THIS in
    /// its own window", put something else in a window. The owner's reason for a pop-out is to
    /// keep a page over the game while he plays, so the page is the entire point of the press.
    ///
    /// # WHY A NAME AND NOT A PAYLOAD ON `Tool`
    ///
    /// `Tool` is a hotkey's currency: every row of `hotkeys::DEFAULTS` is one, and `Settings`
    /// prints them in its rebinding list. A page on the variant would make every one of those rows
    /// name a page it has no opinion about, and `Ctrl+Alt+P` has none: it is pressed from inside
    /// the game. The page is a fact about the CALL, not about the tool, so it is an argument.
    ///
    /// AND A NAME AND NOT AN INDEX, for the reason `Ask::Open` gives at length and `main::
    /// on_section` follows: the parser's page list has moved twice in this tree, and an index that
    /// no longer names what it named opens the wrong page in silence. An unknown name leaves the
    /// window where it is (see [`ParserWindow::show_named`]).
    pub fn open_at(&mut self, tool: Tool, page: Option<&str>) {
        let Some(slot) = tool.slot() else {
            self.companion_req = Some(CompanionReq::Show);
            return;
        };
        let mut g = lock(&self.inner);
        /* A WINDOW THAT IS BEING OPENED HAS NOT PUBLISHED ANYTHING YET. Whatever is in the
         * offer slot belongs to the pop-out that was closed before this one, and its handle is
         * gone; this one publishes its own on its first pass, within 100 ms. See `pip_offer`. */
        if slot == Slot::Watch && !g.windows[slot.index()].open {
            g.pip_out = None;
        }
        let w = &mut g.windows[slot.index()];
        if let (Tool::Lfg(mode), Screen::Lfg(s)) = (tool, &mut w.screen) {
            s.mode = mode;
        }
        /* THE PARSER'S ARM, WHICH WAS MISSING. The LFG one above reads its page off the `Tool`
         * because that variant carries it; this one reads it off the argument, and both do the
         * same thing to the same field of the same window: put it on the page the caller is
         * standing on. */
        if let (Tool::Parser, Screen::Parser(p), Some(name)) = (tool, &mut w.screen, page) {
            p.show_named(name);
        }
        w.focus = w.open;
        w.open = true;
        /* ASKED FOR, NOT YET SERVICED. True whether this opened the window or only asked for an
         * open one to come forward: both need a root pass to be carried out, and the second is
         * the case that would leave `Ctrl+Alt+P` dead on a minimized app with a minimized
         * pop-out if this only tracked "never built". */
        w.serviced = false;
    }

    /// Open if closed, close if open. For `Lfg(mode)`: an open LFG window in the same mode
    /// closes, in a different mode switches. `Companion` minimizes or restores the main window.
    pub fn toggle(&mut self, tool: Tool) {
        let Some(slot) = tool.slot() else {
            self.companion_req = Some(CompanionReq::Toggle);
            return;
        };
        let same_mode = {
            let g = lock(&self.inner);
            let w = &g.windows[slot.index()];
            w.open && w.screen.tool() == tool
        };
        if same_mode {
            lock(&self.inner).windows[slot.index()].open = false;
        } else {
            self.open(tool);
        }
    }

    /// What a D4 hotkey does when it fires: `Ctrl+Alt+G` TOGGLES the main window, every other
    /// binding OPENS its tool (D4's table says "opens" for the tools and "toggled" for the whole
    /// companion). Encoded here, in the one place that knows which tool is the main window, so the
    /// integrator's loop is `for t in hotkeys.poll(ctx) { windows.summon(t) }` and cannot get the
    /// two verbs crossed.
    pub fn summon(&mut self, tool: Tool) {
        match tool {
            Tool::Companion => self.toggle(tool),
            /* NOT A WINDOW, A POPULATION. See `Tool::Overlays`. */
            Tool::Overlays => self.toggle_overlays(),
            other => self.open(other),
        }
    }

    /// Pin a window above every other window, or release it. The glyph in its title strip and the
    /// OS window level change together on the next pass of that window, and the answer is written
    /// to `Settings::windows` by the next root pass so it survives a relaunch.
    ///
    /// BOTH ARMS RECORD A REQUEST NOW, AND ONE OF THEM ALWAYS DID. The main window's pin has been
    /// persisted since `always_on_top` existed; a tool window's was per session, which
    /// `crate::pin`'s own module doc names as the thing that "is not a decision anybody took, it
    /// is what falls out of two implementations of one idea". There is one implementation, so
    /// there is one policy.
    pub fn pin(&mut self, tool: Tool, on: bool) {
        match tool.slot() {
            None => self.companion_pin_req = Some(on),
            Some(slot) => {
                let w = &mut lock(&self.inner).windows[slot.index()];
                w.pin.set(on);
                w.pin_out = Some(on);
            }
        }
    }

    /// For the main window this is the level last pushed to the OS, which mirrors
    /// `settings.always_on_top` once `show` has run at least once.
    pub fn is_pinned(&self, tool: Tool) -> bool {
        match tool.slot() {
            /* WHAT THE OS WAS TOLD, for the main window, and what the app WANTS for a tool
             * window. That asymmetry is older than this type and is preserved rather than
             * quietly fixed: the main window's answer is read back by the summon control on
             * the frame after a pin, and the tool windows' glyphs are drawn from it on the
             * same frame. `Pin` is what makes the two readings nameable instead of both
             * being a bare bool that happened to mean different things. */
            None => self.companion_pin.applied().unwrap_or(false),
            Some(slot) => lock(&self.inner).windows[slot.index()].pin.wants(),
        }
    }

    /// Whether a tool's window is open. The context bar's pop-out control reads it: "Pop out"
    /// when the row's tool has no window, "Focus window" when it does.
    pub fn is_open(&self, tool: Tool) -> bool {
        match tool.slot() {
            None => true,
            Some(slot) => {
                let g = lock(&self.inner);
                let w = &g.windows[slot.index()];
                w.open && (!matches!(tool, Tool::Lfg(_)) || w.screen.tool() == tool)
            }
        }
    }

    /// The leader hint every tool window draws in its corner. Pass `Hotkeys::leader_hint()`.
    pub fn set_hint(&mut self, hint: Option<String>) {
        lock(&self.inner).hint = hint;
    }

    /// Which page the Parser tool window is showing, by name. The only way to read the pop-out's
    /// own state without a viewport, which is what `popping_out_from_a_page_opens_the_window_
    /// there` needs: `Windows::open_at` is the door under test and the page it sets lives three
    /// types down, behind this registry's mutex.
    #[cfg(test)]
    fn parser_page(&self) -> &'static str {
        match &lock(&self.inner).windows[Slot::Parser.index()].screen {
            Screen::Parser(p) => p.showing(),
            _ => unreachable!("the Parser slot holds the Parser window"),
        }
    }

    /// The marks cell the Sky tool window's own screen holds. Read by the wiring check below,
    /// which is the only thing that can tell one shared cell from two identical looking copies.
    #[cfg(test)]
    fn sky_marks_cell(&self) -> crate::screens::sky::MarksCell {
        match &lock(&self.inner).windows[Slot::Sky.index()].screen {
            Screen::Sky(s) => s.marks_cell(),
            _ => unreachable!("the Sky slot holds the Sky screen"),
        }
    }

    /// WHAT THE OVERLAY WINDOWS CHANGED ABOUT THEMSELVES, for the App to write to settings.
    ///
    /// TAKEN, NOT PEEKED, so one edit is written once. A deferred viewport's callback cannot reach
    /// `Settings` (it holds this registry's lock and runs on its own clock), so a chip toggling a
    /// pin, an OS close flipping `open`, and a drag changing the remembered size all land here and
    /// are collected by the root on its next pass.
    /// # The edit carries `widgets` exactly as the registry holds it
    ///
    /// A window is seeded from `overlay::or_default`, so an overlay nobody has configured
    /// arrives here with `widgets: None` and leaves with it. That is what keeps a drag from
    /// being written down as a choice of contents: see `overlay::Overlay::widgets`.
    pub fn overlay_edits(&mut self) -> Vec<crate::overlay::Overlay> {
        let mut g = lock(&self.inner);
        let mut out = Vec::new();
        for o in g.overlays.iter_mut() {
            if std::mem::take(&mut o.dirty) {
                out.push(o.cfg.clone());
            }
        }
        out
    }

    /// Show or hide every overlay at once, which is what `Ctrl+Alt+D` does.
    ///
    /// SHOW UNLESS EVERY ONE IS ALREADY SHOWING. A press with two of three up means "I want them",
    /// not "hide the two I can see": the odd one out is the reason the press happened.
    pub fn toggle_overlays(&mut self) {
        let mut g = lock(&self.inner);
        let hide = !g.overlays.is_empty() && g.overlays.iter().all(|o| o.cfg.open);
        for o in g.overlays.iter_mut() {
            if o.cfg.open == hide {
                o.cfg.open = !hide;
                o.serviced = false;
                o.dirty = true;
            }
        }
    }

    /// Draw every open tool window and apply what the main window was asked. Call once per root
    /// pass, from the root viewport.
    pub fn show(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        /* A tool window asked for a screen that lives in the main window. Hand the ask to the
         * root `Cx` for the App to route after this frame, and bring the main window forward so
         * the answer is visible. A root ask already pending this frame wins; the tool window's
         * waits one pass. Call `show` BEFORE reading `cx.ask` for this to land the same frame. */
        if cx.ask == Ask::None {
            if let Some(ask) = lock(&self.inner).ask_out.take() {
                cx.ask = ask;
                self.companion_req = Some(CompanionReq::Show);
            }
        }
        self.adopt_chat_ask(cx);
        self.show_companion(ctx, cx);

        /* Everything the deferred callbacks need is prepared under the lock, then the lock is
         * released BEFORE `show_viewport_deferred` is called: on a backend that embeds viewports
         * egui runs the callback immediately, and that callback takes this same lock. */
        struct Plan {
            slot: Slot,
            title: &'static str,
            pinned: bool,
            place_at: Option<Pos2>,
            /// HOW BIG TO ASK FOR THIS WINDOW, in points: the size the owner left it, or the slot's
            /// own first-open size when he never moved it.
            ///
            /// PASSED EVERY PASS AND NOT ONLY ON THE FIRST ONE, which is what `Slot::default_size`
            /// already relied on: egui's `ViewportBuilder::patch` issues a resize only when the
            /// value it is handed DIFFERS from the one it was handed last, so a stable number is a
            /// window the user can drag. That is why the saved rectangle is written on a settle
            /// rather than per frame: a size that moved with the drag would be a builder arguing
            /// with the hand.
            size: Vec2,
            focus: bool,
        }
        /* THE OVERLAYS THE OWNER HAS, READ OFF SETTINGS BEFORE THE LOCK IS TAKEN, because
         * `Cx::settings` is the root's and the registry's lock guards the windows. */
        let want: Vec<crate::overlay::Overlay> = crate::overlay::or_default(&cx.settings.overlays);
        let mut plans: Vec<Plan> = Vec::new();
        let mut repaint_children = false;
        /* Whether a tool window's pin, open state or rectangle changed this pass and the file has to
         * be rewritten. Saved AFTER the registry's lock is dropped: a settings save reads the file
         * on disk, writes a sibling temp file and renames it, and doing that with this mutex held
         * would stall every pop-out's repaint behind the disk. */
        let mut save_prefs = false;
        /* A rectangle is moving right now and its deadline has not run out. The root has to come
         * back to write it, and nothing else will wake it: the window that reported the move asked
         * for one repaint when it changed, and if the hand has since stopped there will be no
         * second report. */
        let mut settling = false;
        /* THE OWNER'S SAVED "THESE WERE OPEN" ANSWER, ADOPTED ONCE. Read here rather than inside the
         * loop below so it lands BEFORE `any_open`, which is what decides whether the child context
         * is built this pass: doing it in the loop would leave every restored window drawing "this
         * window has no context yet" for one frame of every launch. */
        let restore = !self.restored;
        self.restored = true;
        {
            let mut g = lock(&self.inner);
            if restore {
                for (i, w) in g.windows.iter_mut().enumerate() {
                    /* ONLY EVER OPENS. A hotkey pressed before the first root pass has already set
                     * this flag, and a file that said `false` must not undo it; `false` is also what
                     * `Settings::win_open` answers for a window nobody has ever opened, so closing
                     * on it would be the file's silence overruling a person's press. */
                    if cx.settings.win_open(SLOTS[i].id()) && !w.open {
                        w.open = true;
                        w.serviced = false;
                    }
                }
            }
            let any_open = wants_child_cx(g.windows.iter().map(|w| w.open), &want);
            if g.cx.is_none() && any_open {
                let child = ChildCx::new(cx);
                g.settings_synced = settings_json(cx.settings).unwrap_or_default();
                g.cx = Some(child);
            }
            if g.cx.is_some() {
                repaint_children |= self.sync(&mut g, cx);
            }
            let root_outer = ctx.input(|i| i.viewport().outer_rect);
            for (i, w) in g.windows.iter_mut().enumerate() {
                /* ------------------------------------------------------------------ the pin --
                 *
                 * THE FILE IS THE OWNER OF THIS ANSWER, AND IT WAS NOBODY'S.
                 *
                 * `Settings::windows` and `WindowPrefs` were built for this on 2026-09-05 (D11)
                 * and then had ZERO production callers: `win_pinned`, `set_win_pinned` and their
                 * two chips-shaped siblings were reached only from `settings::tests`. So a pin was
                 * a fact about a SESSION. Pin the parser over the game, close the app, open it
                 * tomorrow, and the window is unpinned, which for the owner is a window he stops
                 * pinning. `crate::pin`'s module doc had already written down what caused it:
                 * always on top was an idiom copied four times, "the main window's pin persists to
                 * settings and a tool window's does not, which is not a decision anybody took".
                 *
                 * IN ONE DIRECTION PER PASS AND NEVER BOTH, which is the whole of the care needed
                 * here. A window that changed its own pin this frame (a chip inside its deferred
                 * callback, or `Windows::pin`) has an answer the file has not got, so it WRITES;
                 * anything else FOLLOWS the file, so the Settings screen and a second machine's
                 * file both reach an open window. Reading first and writing second in the same
                 * pass would overwrite the click with the value it was about to replace.
                 *
                 * OPEN OR NOT, AND THAT IS DELIBERATE: `is_pinned` is what the summon control
                 * reads to draw its glyph, and a window that is closed still has to answer with
                 * the pin it will come back with. */
                let slot = SLOTS[i];
                match w.pin_out.take() {
                    Some(on) => {
                        cx.settings
                            .set_win_pinned(slot.id(), slot.pin_default(), on);
                        save_prefs = true;
                    }
                    None => w
                        .pin
                        .set(cx.settings.win_pinned(slot.id(), slot.pin_default())),
                }

                /* ------------------------------------------------------- was it on screen --
                 *
                 * ONE DIRECTION, AND IT IS NOT THE PIN'S. The restore above this loop is the whole
                 * of the reading; from here on the registry is the only author, because there is no
                 * second surface anywhere in the app that can say a window is open. See the module
                 * doc. This covers every way `open` moves: a hotkey, the context bar's control, the
                 * OS close button, a chip, and the restore itself. */
                if cx.settings.win_open(slot.id()) != w.open {
                    cx.settings.set_win_open(slot.id(), w.open);
                    save_prefs = true;
                }

                /* ----------------------------------------------------------- and where it sat --
                 *
                 * WRITTEN ON A SETTLE, ABOVE THE `open` GUARD RATHER THAN BELOW IT. A window that
                 * was dragged and then closed in the same second still has an unwritten rectangle
                 * in hand, and it is the rectangle the next open wants; skipping closed windows
                 * here would drop exactly the move a person makes just before putting a window
                 * away. */
                if let (Some(r), Some(since)) = (w.rect_out, w.rect_since) {
                    if since.elapsed() >= RECT_SETTLES_IN {
                        w.rect_since = None;
                        if cx.settings.win_rect(slot.id()) != Some(r) {
                            cx.settings.set_win_rect(slot.id(), Some(r));
                            save_prefs = true;
                        }
                    } else {
                        settling = true;
                    }
                }

                if !w.open {
                    /* A closed window forgets that it has been PLACED, so the next open goes back
                     * through the placement below; what it does not forget is `rect_out`, which is
                     * where that placement now comes from. */
                    w.placed = false;
                    continue;
                }
                /* THE SIZE AND THE POSITION THE OWNER LEFT IT AT, WHICH THIS USED TO THROW AWAY.
                 *
                 * The comment under `placed` said "a closed window forgets its placement so it comes
                 * back beside the main window rather than wherever it was left, which may be on a
                 * monitor that is no longer there". The monitor half is real and is answered better
                 * by a saved rectangle than by forgetting one: a window is placed at the point the
                 * OS last reported it at, and if that monitor is gone the OS clamps the placement,
                 * which is what it does for every other application on the machine. The other half
                 * was throwing away the arrangement the pop-out exists to keep. */
                let saved = cx.settings.win_rect(slot.id());
                let place_at = if w.placed {
                    None
                } else {
                    w.placed = true;
                    Some(match saved {
                        Some([x, y, _, _]) => Pos2::new(x, y),
                        None => root_outer.map_or(Pos2::new(120.0, 120.0), |r| {
                            r.left_top() + Vec2::new(64.0 + 28.0 * i as f32, 64.0 + 28.0 * i as f32)
                        }),
                    })
                };
                let size = match saved {
                    Some([_, _, wide, high]) => Vec2::new(wide, high),
                    None => slot.default_size(),
                };
                /* THE PARAGRAPH THAT STOOD HERE DESCRIBED CODE THAT WAS NEVER WRITTEN. It said
                 * "SETTINGS IS THE PIN'S OWNER NOW, so the window follows it every pass rather
                 * than holding an opinion of its own ... a chip writes to settings and the
                 * settings value comes back round to here", and every clause of it was false:
                 * nothing in this file mentioned `Settings::windows`, the chip wrote to
                 * `Window::pin` and stopped there, and `Pin::toggle` had a caller thirty lines
                 * into `draw_child`. It was the intention of D11 written down as if it had
                 * happened, which is how a mechanism with no callers survives a reading. The loop
                 * head above is the code it describes, and `Pin::toggle` still has its caller,
                 * because a chip is a TOGGLE and reading the file to work out what to write back
                 * into the file would be a longer way to say the same thing. */
                /* SERVICED BY THIS PASS. The plan below ends in `show_viewport_deferred`, which
                 * is the whole of what a tool window needs from the root, so nothing this window
                 * has asked for is outstanding once this loop has seen it. Set here rather than
                 * after the plan loop so it cannot be missed by an early return added later. */
                w.serviced = true;
                plans.push(Plan {
                    slot,
                    title: w.screen.tool().title(),
                    pinned: w.pin.wants(),
                    place_at,
                    size,
                    focus: std::mem::take(&mut w.focus),
                });
            }

            /* AND THE CHILD IS HANDED THE NEW SETTINGS HERE RATHER THAN BY `sync` NEXT PASS, WHICH
             * IS NOT AN OPTIMISATION.
             *
             * `sync` answers ANY changed settings JSON by calling `Ingest::reconfigure`, which used
             * to re-read the log folder and start a fresh bootstrap unconditionally: a fold of the
             * last 40MB of the log, on a worker, every time. That guard exists now (see
             * `Ingest::reconfigure`), so the cost is no longer catastrophic, but the reason to keep
             * the two sides in step HERE is unchanged and is not about cost: a pin, an open flag and
             * a window rectangle are facts about a window, and routing them through the arm that
             * exists to answer a changed LOGS FOLDER means every one of them is one guard away from
             * a rescan while the owner is raiding.
             *
             * SO THE TWO SIDES ARE PUT IN STEP DIRECTLY: the child takes the same value and
             * `settings_synced` records it, which is `sync`'s own bookkeeping minus the rescan.
             * If the round trip fails the two are simply left apart and `sync` does it the
             * expensive way on the next pass, which is a slow correct answer rather than a fast
             * wrong one. */
            if save_prefs {
                match settings_json(cx.settings)
                    .and_then(|j| settings_from_json(&j).map(|s| (j, s)))
                {
                    Some((json, s)) => {
                        if let Some(child) = g.cx.as_mut() {
                            child.settings = s;
                        }
                        g.settings_synced = json;
                    }
                    None => log::warn!(
                        "a tool window's preferences could not be copied to the tool windows; they \
                         will take them on their next settings sync"
                    ),
                }
            }
        }

        /* WRITTEN DOWN. Outside the block above, so the lock is gone before the disk is touched, and
         * only on a pass where something actually moved: the setters erase a key that agrees with
         * the code's default, so a pin toggled on and off leaves the file byte for byte the one that
         * was never touched, and this saves that erasure too. */
        if save_prefs {
            if let Err(e) = cx.settings.save() {
                log::warn!("a tool window's preferences could not be saved: {e}");
            }
        }
        /* AND THE ROOT COMES BACK FOR A RECTANGLE THAT IS STILL MOVING. The window that reported the
         * move asked for one root repaint when it changed; if the hand has stopped there will be no
         * second report, so without this the settle would wait on whatever else happens to wake the
         * root. It asks for the deadline itself rather than a tick, so a still window costs nothing.
         */
        if settling {
            ctx.request_repaint_after(RECT_SETTLES_IN);
        }

        /* ------------------------------------------------------- the owner's overlays -- */
        /* A SECOND POPULATION BESIDE THE FIVE, AND NOT A SIXTH SLOT. A slot is a compile-time
         * thing; an overlay is made at runtime and there can be any number. Everything a window
         * needs that is not a `Slot` lives on `OverlayWindow`, and `draw_overlay` is its body. */
        let mut ov_plans: Vec<(usize, crate::overlay::Overlay, Option<Pos2>, bool)> = Vec::new();
        {
            let mut g = lock(&self.inner);
            let root_outer = ctx.input(|i| i.viewport().outer_rect);
            sync_overlays(&mut g.overlays, &want);
            for (i, o) in g.overlays.iter_mut().enumerate() {
                if !o.cfg.open {
                    o.placed = false;
                    continue;
                }
                let place_at = if o.placed {
                    None
                } else {
                    o.placed = true;
                    Some(place_overlay(o.cfg.at, root_outer, i))
                };
                o.serviced = true;
                o.pin.set(o.cfg.pinned);
                ov_plans.push((i, o.cfg.clone(), place_at, std::mem::take(&mut o.focus)));
            }
        }
        for (i, cfg, place_at, focus) in ov_plans {
            let id = ViewportId::from_hash_of(("grimoire.overlay", &cfg.id));
            let mut builder = overlay_builder(&cfg);
            if let Some(p) = place_at {
                builder = builder.with_position(p);
            }
            let inner = Arc::clone(&self.inner);
            ctx.show_viewport_deferred(id, builder, move |ui, class| {
                draw_overlay(&inner, i, ui, class);
            });
            if focus {
                raise(ctx, id, false);
            }
            if repaint_children {
                ctx.request_repaint_of(id);
            }
        }

        for plan in plans {
            let mut builder = ViewportBuilder::default()
                .with_title(plan.title)
                .with_inner_size(plan.size)
                .with_min_inner_size(plan.slot.min_size())
                .with_resizable(true)
                .with_decorations(false)
                .with_transparent(false)
                .with_window_level(level(plan.pinned));
            if let Some(p) = plan.place_at {
                builder = builder.with_position(p);
            }
            let inner = Arc::clone(&self.inner);
            let slot = plan.slot;
            ctx.show_viewport_deferred(slot.viewport_id(), builder, move |ui, class| {
                draw_child(&inner, slot, ui, class);
            });
            if plan.focus {
                raise(ctx, slot.viewport_id(), false);
            }
            if repaint_children {
                ctx.request_repaint_of(slot.viewport_id());
            }
        }
    }

    /// SHOW OR HIDE THE MAIN WINDOW, WITHOUT DRAWING ANYTHING.
    ///
    /// SPLIT OUT OF `show_companion` BECAUSE THE ONE STATE IT MATTERS IN IS THE ONE STATE THAT
    /// DOES NOT PAINT. eframe runs `App::ui` only while `show_ui` is true, and `show_ui` is
    /// `is_visible || is_viewport_or_descendant_visible` (glow_integration.rs:622); a minimized
    /// root with no tool window open makes both false, and eframe calls `App::logic` instead.
    /// So every line of the pass this used to live in was unreachable in exactly the situation
    /// `Ctrl+Alt+G` exists for: the app had minimized ITSELF (the Toggle arm below) and could
    /// not be asked back.
    ///
    /// IT TAKES THE REQUEST RATHER THAN PEEKING, so the two callers cannot both act on one
    /// press. `logic` runs when nothing paints and `show_companion` runs when something does;
    /// they never run in the same frame, but a peeking version would raise the window in one
    /// and then, seeing it no longer minimized, minimize it straight back in the other.
    pub fn wake(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        let root = ViewportId::ROOT;
        match self.companion_req.take() {
            Some(CompanionReq::Show) => {
                raise(ctx, root, true);
            }
            Some(CompanionReq::Toggle) => {
                if minimized {
                    raise(ctx, root, true);
                } else {
                    ctx.send_viewport_cmd_to(root, ViewportCommand::Minimized(true));
                }
            }
            None => {}
        }
    }

    /// IS A TOOL WINDOW WAITING FOR A ROOT PASS?
    ///
    /// Only ever asked from `App::logic`, which answers it by RESTORING THE MAIN WINDOW, so a
    /// wrong `true` here is a main window that cannot be put away.
    ///
    /// THIS USED TO ANSWER `open` AND THAT WAS THE BUG THE OWNER REPORTED: "why is it that if we
    /// have a popup open it refuses to minimize the main window?". The note that stood here
    /// argued that inside `logic`, open must mean open-and-unbuilt, because an open and visible
    /// tool window makes eframe's `show_ui` true (glow_integration.rs:622, via
    /// `is_viewport_or_descendant_visible`) and `logic` would not be running at all.
    ///
    /// EFRAME DOES NOT BEHAVE THAT WAY AND IT WAS MEASURED. With one pop-out open and the main
    /// window plainly visible, a run logged 1,791 `logic` passes INTERLEAVED with about 1,740
    /// `ui` passes, and `Minimized(false)` went to the root on every single one of them. The
    /// window was restored within one 200 ms poll of being minimized from outside the process,
    /// every time. See `Window::serviced`, which makes "waiting for a pass" a recorded fact
    /// instead of a deduction from a premise that was never true.
    pub fn tool_pending(&self) -> bool {
        let g = lock(&self.inner);
        g.windows.iter().any(|w| w.open && !w.serviced)
            || g.overlays.iter().any(|o| o.cfg.open && !o.serviced)
    }

    /// Apply the main window's requests, against the OS's own account of its state.
    fn show_companion(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        self.wake(ctx);
        if let Some(on) = self.companion_pin_req.take() {
            if cx.settings.always_on_top != on {
                cx.settings.always_on_top = on;
                if let Err(e) = cx.settings.save() {
                    log::warn!("the main window pin could not be saved: {e}");
                }
            }
        }
        /* THE POLICY IS HERE AND THE MECHANISM IS NOT: the answer comes off settings, so it
         * survives a restart, and telling the OS about it once is the pin's business. */
        self.companion_pin.set(cx.settings.always_on_top);
        self.companion_pin.reconcile(ctx, Some(ViewportId::ROOT));
    }

    /// Move live status and settings between the root `Cx` and the tool windows' context.
    /// Returns true when the tool windows should repaint because something they show changed.
    fn sync(&self, g: &mut Inner, cx: &mut Cx) -> bool {
        let mut changed = false;
        let Inner {
            cx: child,
            settings_out,
            settings_synced,
            ..
        } = g;
        let Some(child) = child.as_mut() else {
            return false;
        };

        if child.live != *cx.live {
            child.live = cx.live.clone();
            changed = true;
        }

        if let Some(out) = settings_out.take() {
            /* A tool window changed settings (LFG posted to its board). The tool window has
             * already saved; the root adopts the same value so its next save does not clobber
             * the change. A root edit made in the same 100ms is lost; the two edit surfaces are
             * both human and that coincidence is not worth a merge algorithm. */
            match settings_from_json(&out) {
                Some(s) => {
                    *cx.settings = s;
                    /* AND THE ROOT'S INGEST FOLLOWS THE SETTINGS UP, WHICH IT DID NOT. THE
                     * ASYMMETRY IS THE DEFECT, AND IT IS NOT A NUMBER ON SCREEN TODAY.
                     *
                     * The downward arm below has called `Ingest::reconfigure` since a changed Logs
                     * folder was found to leave every open pop-out tailing the old one for the life
                     * of the process. This arm, the one a TOOL WINDOW's edit comes UP through,
                     * assigned the settings and stopped.
                     *
                     * WHAT IT IS WORTH TODAY IS SMALL, AND SAYING SO IS THE POINT: a comment here
                     * claiming it fixes a visible disagreement would be this file's own recurring
                     * defect, an intention written down as if it had happened. The Settings screen
                     * owns the Logs folder and the data root and it is a MAIN WINDOW screen (a
                     * pop-out has no gear at all, which `screens::parser`'s roster empty state says
                     * in as many words), so nothing that comes up this arm can have moved a path and
                     * `reconfigure` returns at its own guard. What is left is the Kills view's three
                     * counting filters, which `reconfigure` copies BEFORE that guard and
                     * deliberately: the root's `Ingest` takes the pop-out's counting rule on THIS
                     * pass rather than whenever the main window next draws the Kills view, which is
                     * where `screens::parser` seeds its own copy. That is a frame, not a defect.
                     *
                     * WHAT IT IS WORTH THE DAY ANY POP-OUT SCREEN OFFERS A FOLDER CONTROL is the
                     * whole of what the downward arm exists for, pointing the other way: the pop-out
                     * reading the new folder, the main window reading the old one, both calling
                     * theirs "the log", until the process ends. The parser lane walked that and
                     * refused to add the control until this line existed, which is the right order.
                     *
                     * AND IT COSTS NOTHING ON THE PASSES THAT ARE NOT THAT. The guard returns before
                     * the rescan when neither resolved path moved, which is every edit that actually
                     * comes up this arm: an LFG post, a fight note, a pin. */
                    cx.ingest.reconfigure(cx.settings);
                    *settings_synced = out;
                }
                None => log::warn!(
                    "a tool window's settings change could not be read back into the main window"
                ),
            }
        } else if let Some(root_json) = settings_json(cx.settings) {
            if root_json != *settings_synced {
                match settings_from_json(&root_json) {
                    Some(s) => {
                        child.settings = s;
                        /* AND THE CHILD'S INGEST FOLLOWS THE SETTINGS DOWN, WHICH IT DID NOT.
                         *
                         * This copied the new `Settings` into the child and stopped there.
                         * `Ingest` reads the Logs folder and the data root ONCE, in
                         * `Ingest::new`, and `reconfigure` is the door that makes a running one
                         * follow a change; the main window calls it on every settings edit
                         * (`main.rs`, `self.ingest.reconfigure(&self.settings)`).
                         *
                         * SO CHANGING THE LOGS FOLDER LEFT EVERY OPEN TOOL WINDOW TAILING THE OLD
                         * ONE, for the life of the process, with no sign on screen: the pop-out
                         * went on printing fights, kills and loot out of a folder the owner had
                         * just told the app to stop reading. Two windows of one application
                         * reading two different logs and both saying they are reading `the log`.
                         *
                         * IT IS CHEAP NOW AND IT WAS NOT, AND THE NOTE HERE HAS BEEN WRONG IN BOTH
                         * DIRECTIONS. It first read "`Ingest::reconfigure` compares the resolved
                         * folder and returns without a rescan when it is the same", which described
                         * a guard nobody had written: that function copied the two paths, dropped
                         * the roster and called `rescan` unconditionally, so this arm cost one full
                         * 40MB fold per settings change of ANY kind. It was then corrected to say
                         * so. The guard exists now, in `reconfigure` itself and argued there, and it
                         * names THIS call site as the reason: the resolved log folder and data root
                         * are compared and an unchanged pair returns before the rescan.
                         *
                         * WHICH IS WHY THE PIN STILL DOES NOT COME THROUGH HERE, and the reason is
                         * no longer the price. `Windows::show` puts the child in step itself after
                         * it writes a pin, an open flag or a rectangle: those are facts about a
                         * WINDOW, and routing them through the arm whose job is a changed Logs
                         * folder would leave every one of them one guard away from a rescan while
                         * the owner is raiding. */
                        child.ingest.reconfigure(&child.settings);
                        *settings_synced = root_json;
                        changed = true;
                    }
                    None => log::warn!("settings could not be copied to the tool windows"),
                }
            }
        }

        /* ------------------------------------------------- one bootstrap history, not two --
         *
         * THE DEFECT: THE FIGHTS TABLE IN THE POP-OUT AND THE ONE IN THE BODY WERE TWO ANSWERS.
         *
         * `Ingest::fights` is a fold of the log's tail taken ONCE, when that `Ingest` was built or
         * last told to rescan; the live path never rewrites it (`ingest::LIVE_LINES`). The root's
         * is built by `App::new` at launch and this one by `ChildCx::new`, which runs when the FIRST
         * tool window opens. For the owner that is an hour of raiding apart, so the pop-out's Fights
         * table listed fights the body's did not, both printed a total in the same words, and
         * nothing on either screen said which fold it was counting. The Rescan control is a second
         * producer of the same split, and a worse one because it looks deliberate: it refolds the
         * ingest of whichever window it was pressed in and leaves the other where it was.
         *
         * THE STAMPS DECIDE, AND THE COMPARISON IS SYMMETRIC. Every fold stamps itself
         * (`Ingest::scanned_at`), so "which of these two is the later reading of the file" is a
         * question with an answer, and the later one is handed to the other side whole. Symmetric
         * rather than root-to-child is what makes Rescan work from either window: whichever side was
         * refolded now holds the newer stamp and is the source.
         *
         * `Option<DateTime<Utc>>` ORDERS THE WAY THIS NEEDS: `None` is less than every `Some`, so an
         * ingest whose bootstrap has not landed yet takes the one that has, and two `None`s compare
         * equal and nothing happens. Equal stamps mean the two are the same fold and there is
         * nothing to do, which is the state every pass after an adoption is in, because
         * `adopt_history` carries the stamp across with the rows.
         *
         * WHOLESALE, NOT MERGED, WHICH IS `Ingest::adopt`'s OWN RULE AND IT IS ARGUED THERE: rows
         * from two folds interleave by nothing at all and would read as one continuous history that
         * never happened. The denominator travels with them, because `fights_unreadable` counts the
         * lines THAT fold could not place and means nothing beside another fold's rows.
         *
         * AND NOTHING THE LIVE FOLD OWNS MOVES. The byte cursor, the tailed file, the kill and loot
         * streams and the live re-fold are facts about one reader of one handle; `adopt_history`
         * touches none of them, and neither ingest stops tailing its own file. */
        let root_at = cx.ingest.scanned_at();
        let child_at = child.ingest.scanned_at();
        if root_at > child_at {
            child.ingest.adopt_history(
                cx.ingest.fights().to_vec(),
                cx.ingest.fights_unreadable(),
                root_at,
            );
            changed = true;
        } else if child_at > root_at {
            cx.ingest.adopt_history(
                child.ingest.fights().to_vec(),
                child.ingest.fights_unreadable(),
                child_at,
            );
        }

        changed
    }
}

/* ------------------------------------------------------------------ one tool window -- */

/// The body of every deferred callback. Runs whenever the OS asks that window to repaint.
/// SHUT A TOOL WINDOW, AND WRITE OUT WHAT THE PAGE INSIDE IT WAS STILL HOLDING.
///
/// # THREE WAYS TO CLOSE ONE WINDOW WERE THREE COPIES OF THE SAME TWO LINES
///
/// `w.pin.forget(); w.open = false;` appeared at the OS close (the title bar's X, Alt+F4, the
/// taskbar), at the app's own chrome cross, and again in the pip window's chrome. Three copies of
/// a shutdown is three places for a shutdown to grow a step in two of them.
///
/// IT GREW ONE. `screens::analysis` keeps the fight note being typed in its own buffer and writes
/// it to `Settings::fight_notes` when the reader leaves the field or changes fight. Closing the
/// window is neither, and nothing draws a frame after it, so the page cannot notice its own close:
/// the last thing typed went in the bin. The main window answers the same case in
/// `eframe::App::on_exit`; this is the tool windows' half of it, and it has to be here because
/// each Parser window holds its OWN `ParserScreen` and therefore its own buffer.
///
/// THE PIN IS FORGOTTEN FIRST AND THE REASON IS UNCHANGED: reopening builds a NEW viewport at the
/// default level, so a pin that still remembered telling the old one would see no change, send
/// nothing, and leave a window at Normal with its glyph claiming it is pinned.
///
/// A WINDOW WITH NO CHILD CONTEXT STILL CLOSES. `cx` is `None` until the first child pass has run,
/// and a window closed before it ever drew is holding nothing to write out.
fn close_window(w: &mut Window, cx: &mut Option<ChildCx>) {
    if let (Screen::Parser(p), Some(c)) = (&mut w.screen, cx.as_mut()) {
        p.parser.flush_notes(&mut c.settings);
    }
    w.pin.forget();
    w.open = false;
}

fn draw_child(inner: &Arc<Mutex<Inner>>, slot: Slot, ui: &mut Ui, class: ViewportClass) {
    let ctx = ui.ctx().clone();
    let mut g = lock(inner);
    let Inner {
        windows,
        cx,
        hint,
        settings_out,
        ask_out,
        root_hwnd,
        hosting,
        pip_out,
        chat_wanted_out,
        ..
    } = &mut *g;
    let w = &mut windows[slot.index()];

    /* The OS close button, Alt+F4, or the taskbar's close. The registry is the truth about what is
     * open, so the flag flips here and the root pass stops showing the viewport, which is how egui
     * closes it. The root is asked to repaint so that happens now and not at its next tick. */
    if ctx.input(|i| i.viewport().close_requested()) {
        /* THE OS WINDOW IS GOING, so what it was told about its level goes with it. Showing this
         * viewport again builds a NEW window at the default level; a pin that still remembered
         * telling the old one would see no change, send nothing, and leave a window sitting at
         * Normal with its glyph saying it is pinned. */
        close_window(w, cx);
        ctx.request_repaint_of(ViewportId::ROOT);
        return;
    }

    /* The window level is applied on change, and re-asserted the first time this window draws so
     * a pin set before the window existed still lands. The builder carries the same level, so
     * egui issues the same command when it differs; sending it here as well is the explicit D3
     * mechanism and costs nothing. */
    w.pin.reconcile(&ctx, None);

    /* WHERE THIS WINDOW IS, RECORDED FOR THE ROOT TO WRITE DOWN.
     *
     * ONLY THIS PASS CAN KNOW IT. `outer_rect` is a fact about the viewport being drawn, and this
     * callback is the only code that runs inside one; the root's `show` reads the ROOT window's
     * rectangle from the same call and would get its own. So the reading happens here and the
     * writing happens there, exactly as `pin_out` does, and for the identical reason: this pass
     * holds the registry's lock and cannot reach `Settings`.
     *
     * ABOVE THE PICTURE IN PICTURE BRANCH, WHICH RETURNS. The Watch window is the one the owner
     * moves most, because it is the one that sits over the game.
     *
     * A CHANGE RESTARTS THE CLOCK AND ASKS THE ROOT TO COME BACK. A drag reports a new rectangle on
     * every frame of it, so the deadline keeps moving out and only the rectangle the hand let go of
     * is ever written; see `Window::rect_since` and [`RECT_SETTLES_IN`]. `None` on a backend with no
     * OS windows, and on the first pass before the window has been placed, and neither is a fault:
     * there is simply nothing to record yet. */
    if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
        let now = [r.min.x, r.min.y, r.width(), r.height()];
        if w.rect_out != Some(now) {
            w.rect_out = Some(now);
            w.rect_since = Some(Instant::now());
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    let title = w.screen.tool().title();

    let Some(cc) = cx.as_mut() else {
        /* Cannot happen through `show`, which builds the context before registering any
         * callback. Said plainly rather than drawn as an empty screen. */
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(INK)).show(ui, |ui| {
            ui.label(egui::RichText::new("This window has no context yet. The main window builds it on its next pass.").color(TEXT_2));
        });
        return;
    };

    /* THE PICTURE IN PICTURE WINDOW STOPS HERE. It draws no title strip, no screen and no margin:
     * `pip` takes the whole content rectangle and everything else about that window is in it. The
     * three things it can report are applied by the same hands as the strip's, because the
     * registry is the one owner of "on top" and of what is open. */
    if slot.is_pip() {
        let on = cc.settings.watch_on;
        let live = cc.live.on(on).live;
        /* THE ARTWORK IS FETCHED HERE AND HANDED IN, rather than reached for inside `pip`, so a
         * test can hand that function a texture it made itself and read the painted mesh back.
         * `channel_art` answers `None` in every test binary (its `FETCHING` switch is set on one
         * line of `main`), so a `pip` that went looking for its own picture would be a function no
         * test could ever drive past its empty case. */
        let art = crate::channel_art::artwork(&ctx, on);
        let pinned = w.pin.wants();
        let own_chrome = class != ViewportClass::EmbeddedWindow;

        /* WHERE THIS WINDOW IS, IN THE ONLY TERMS `SetParent` UNDERSTANDS.
         *
         * eframe hands a deferred viewport's callback no window handle, so the handle is looked
         * up: every window on THIS thread, narrowed by winit's class, by this window's own title,
         * and by its rectangle. The rectangle is what makes it an identification rather than a
         * guess, and `pick` refuses outright rather than breaking a tie, because a `SetParent`
         * into the wrong window SUCCEEDS and the video would go somewhere nobody can see with no
         * error raised anywhere.
         *
         * `outer_rect` is `GetWindowRect` divided by `pixels_per_point` on the way in, so this
         * multiplies by the same number on the way back out. */
        let ppp = ctx.pixels_per_point();
        let found = match ctx.input(|i| i.viewport().outer_rect) {
            Some(r) if own_chrome => crate::player::hwnd::pick(
                &crate::player::hwnd::candidates(),
                title,
                crate::player::hwnd::to_physical(r, ppp),
                *root_hwnd,
            ),
            /* On the first pass, or on a backend with no OS windows, there is nothing to find
             * and nothing has gone wrong. */
            _ => Err(crate::player::hwnd::PickErr::NoneMatched),
        };
        let my_hwnd = found.ok();
        if let Err(e) = found {
            if let Some(words) = e.words() {
                log::warn!("windows: {words}");
            }
        }

        /* IS THE VIDEO ACTUALLY IN THIS WINDOW RIGHT NOW? Not "should it be": the registry is
         * told what the surface reports, so a frame where the move failed draws the picture
         * rather than a black rectangle waiting for a video that never arrived. */
        let hosting = *hosting && my_hwnd.is_some();

        /* THE POINTER, FROM THE OS AND NOT FROM EGUI, while the video is here. A native child
         * window takes the pointer over every pixel it covers, so `rect_contains_pointer` answers
         * false over the whole window and the hover-gated chrome would never appear again. */
        let os_pointer = match my_hwnd {
            Some(h) if hosting => Some(crate::player::hwnd::pointer_over(h)),
            _ => None,
        };

        let mut hits = ChipHits::default();
        let mut inside = false;
        let mut body = Rect::NOTHING;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(INK))
            .show(ui, |ui| {
                let out = pip(
                    ui,
                    live,
                    art.as_ref(),
                    pinned,
                    own_chrome,
                    PipSeat {
                        hosting,
                        os_pointer,
                    },
                    &mut hits,
                );
                inside = out.0;
                body = out.1;
            });

        /* THE OFFER, PUBLISHED UPWARD. It says only "the video may live here, and these are the
         * shapes it must keep out of"; it carries no feed and no sound flag, so it cannot cause a
         * rebuild and cannot reset the volume. `player::choose_stage` is what pairs it with the
         * Watch screen's demand. */
        *pip_out = my_hwnd.map(|hwnd| crate::player::PipOffer {
            hwnd,
            carve_px: if inside && own_chrome {
                pip_holes(body, ppp)
            } else {
                Vec::new()
            },
        });
        if hits.pin {
            w.pin.toggle();
            w.pin.reconcile(&ctx, None);
            /* AND THE FILE HEARS ABOUT IT, WHICH IT DID NOT. This pass cannot reach `Settings`:
             * it runs inside a deferred viewport's callback holding this registry's lock, on its
             * own clock, with no `Cx`. So the answer is left here and the root writes it on its
             * next pass (`Windows::show`), which is asked for now rather than at its next tick so
             * a pin survives an app closed a second after it was clicked. */
            w.pin_out = Some(w.pin.wants());
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if hits.close {
            close_window(w, cx);
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }
        /* The grip follows the same hover rule as the rest of the chrome. It costs nothing: the
         * pointer has to be over the window to reach a grip in the first place. */
        if inside && own_chrome {
            resize_corner(&ctx, Id::new(("grimoire.resize", slot.index())));
        }
        if let Some(h) = hint.as_deref() {
            crate::hotkeys::draw_hint(&ctx, h);
        }
        /* THIS WINDOW STILL TICKS WITH THE MAIN ONE MINIMISED OR HIDDEN, which is the reason a
         * deferred viewport was chosen (see the module doc). The live state is what it draws and
         * a deferred viewport repaints only when it is asked to.
         *
         * IT TICKS FASTER WHILE IT HOLDS THE VIDEO, because the hover signal is a POLL now rather
         * than an event: nothing wakes this pass when the pointer crosses the window, so the
         * interval IS the latency of the chrome appearing. A tenth of a second is under the
         * threshold where a control feels unresponsive and is still a poll of three Win32 calls. */
        ctx.request_repaint_after(if hosting {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(1)
        });
        return;
    }

    /* The title strip is the app's one title bar (`crate::titlebar::strip`, decision D8): title in
     * Cinzel, the live pill, the pin glyph filled when pinned and hollow when not, minimise,
     * maximise, close, and an OS drag on the empty part. It paints the pin and reports the click;
     * the level itself is set HERE, because the registry is the one owner of "on top". Its close
     * also sends `ViewportCommand::Close`, which for a deferred viewport only raises
     * `close_requested`; the window actually goes when `open` flips and the root stops showing it.
     * An embedded viewport (a backend without OS windows) gets egui's own window chrome instead. */
    if class != ViewportClass::EmbeddedWindow {
        let mut hits = crate::titlebar::Hits::default();
        let pinned = w.pin.wants();
        let live = &cc.live;
        let watch_on = cc.settings.watch_on;
        egui::Panel::top(Id::new(("grimoire.strip", slot.index())))
            .exact_size(crate::titlebar::STRIP_H)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(PANEL))
            .show(ui, |ui| {
                crate::titlebar::strip(
                    ui,
                    crate::titlebar::Lead::Title(title),
                    live,
                    watch_on,
                    pinned,
                    &mut hits,
                );
            });
        /* THE PILL IN A TOOL WINDOW'S STRIP. The video cannot be hosted here: `eframe::Frame`
         * hands a deferred viewport the ROOT window's handle, so a surface asked for from this
         * pass would appear over the MAIN window's body, on top of whatever screen was showing
         * there. So the click travels to the root as an ask, exactly
         * like a screen's own ask does thirty lines below, and the stream comes up in the main
         * window where it belongs. The root is asked to repaint so it lands this frame. */
        if hits.watch {
            *ask_out = Some(Ask::WatchHere);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if hits.pin {
            w.pin.toggle();
            w.pin.reconcile(&ctx, None);
            /* AND THE FILE HEARS ABOUT IT, WHICH IT DID NOT. This pass cannot reach `Settings`:
             * it runs inside a deferred viewport's callback holding this registry's lock, on its
             * own clock, with no `Cx`. So the answer is left here and the root writes it on its
             * next pass (`Windows::show`), which is asked for now rather than at its next tick so
             * a pin survives an app closed a second after it was clicked. */
            w.pin_out = Some(w.pin.wants());
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if hits.close {
            close_window(w, cx);
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }
    }

    cc.poll_snapshot(slot);

    let before = settings_json(&cc.settings);
    let _used = egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(INK).inner_margin(egui::Margin::same(12)))
        .show(ui, |ui| {
            /* A snapshot still parsing is neither absent nor failed: this window draws the same
             * loading notice the main window draws for its snapshot rows, and the screen sees
             * nothing until the parse answers. */
            if let Some((root, for_)) = cc.loading() {
                crate::screens::loading_notice(ui, title, root, for_);
                return content_height(ui);
            }
            let data_err: Option<String> = match (&cc.data, &cc.data_err) {
                (Some(_), _) => None,
                (None, Some(e)) => Some(e.clone()),
                (None, None) => Some(
                    "This window does not read the item snapshot. Items, zones and drops are in the main window under FIND."
                        .to_owned(),
                ),
            };
            let mut child_cx = Cx {
                data: cc.data.as_ref(),
                railed: false,
                data_err: data_err.as_deref(),
                live: &cc.live,
                settings: &mut cc.settings,
                ingest: &mut cc.ingest,
                player: Default::default(),
                /* THE ROOT'S LOG, SO A POP-OUT SHOWS THE SAME CHAT THE BODY SHOWS. The note that
                 * stood here said no tool window hosts chat and lent an idle handle; there is a
                 * `Screen::Chat` arm now, and the `debug_assert` that guarded that claim has
                 * become the routing three lines below, which is what it asked for by name. */
                chat: cc.chat.clone(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            w.screen.ui(ui, &mut child_cx);
            /* THE POP-OUT'S ASK, ROUTED UP. `App::ui` owns the only reader and never sees this
             * context, so without this line a Chat pop-out opened while the body was on another
             * screen would say "Not connected yet" for as long as it stayed open. `show` ors this
             * into the root's flag; see `Inner::chat_wanted_out`. */
            if child_cx.chat_wanted {
                *chat_wanted_out = true;
            }
            let ask = std::mem::take(&mut child_cx.ask);
            if ask != Ask::None {
                *ask_out = Some(ask);
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            content_height(ui)
        })
        .inner;
    let after = settings_json(&cc.settings);
    if before != after {
        *settings_out = after;
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    if class != ViewportClass::EmbeddedWindow {
        resize_corner(&ctx, Id::new(("grimoire.resize", slot.index())));
    }

    if let Some(h) = hint.as_deref() {
        crate::hotkeys::draw_hint(&ctx, h);
    }

    /* Live status age and a fresh log tail need the clock to move even when nobody touches the
     * window. One second is the resolution the age text uses. */
    ctx.request_repaint_after(Duration::from_secs(1));
}

/* ---------------------------------------------------- the picture in picture -- */

/// The one sentence this window is allowed, and it is owed rather than offered.
///
/// A person who pops out a LIVE channel and gets a still picture has been handed a contradiction:
/// the pill says LIVE and the window shows a photograph. The reason is structural and is not
/// fixable from here (see the module doc: only the root window has a `HasWindowHandle`), so the
/// window says where the stream actually is instead of leaving a reader to work out that this one
/// is broken. It is drawn ONLY while the channel is live, because offline there is no stream to
/// point at and the word on the artwork has already said everything.
///
/// IT IS NOT CHROME AND SO IT IS NOT ON THE HOVER. Everything a hand can press in this window
/// appears when the pointer arrives and goes when it leaves; this is a caption on the picture,
/// like OFFLINE is, and a reader who has just been surprised is not going to move the mouse to
/// find out why.
pub const IN_THE_MAIN_WINDOW: &str = "The stream plays in the main window.";

/* THERE IS NO BAR HERE ANY MORE, AND THE REASON IS WHAT THE OWNER SAW.
 *
 * The first cut of this window put the chrome on a 26 point scrim spanning the full width,
 * pinned to the top edge, with a text button at the left end and two glyphs at the right. Every
 * word of that describes a TITLE BAR. Hiding it until the pointer arrives does not change what
 * it is, because the pointer is always there at the moment anyone looks at the thing: you reach
 * for the window, the bar appears, and what you are looking at is a small toolbar over a
 * picture. "Minimal chrome" was a claim about the mechanism and not about the result.
 *
 * What a picture in picture actually does, and has done since the ones this is named after, is
 * float its controls IN a corner rather than on a band ACROSS an edge: two small squares over
 * the picture, touching nothing else, leaving every other pixel of the window the picture. */

/// The side of a corner chip, and of the glyph inside it. One number, so a chip is square.
/// The margin a self-sizing window adds under its content: the body inner margin, twice, so the
/// bottom edge sits the same distance from the last row as the top edge sits from the header.
const BARE_PAD: f32 = 24.0;

const CHIP_W: f32 = 22.0;

/// The inset from the window's top and right edges.
const CHIP_EDGE: f32 = 8.0;

/// The gap between the two chips, so they read as two controls and not one bar in miniature.
const CHIP_GAP: f32 = 4.0;

/// The side of the two corner grips, resize and move. `resize_corner`'s own size.
const PIP_GRIP: f32 = 16.0;

/// The two chips, right to left from the top right corner: close outermost, then the pin.
///
/// EXTRACTED SO THAT THE HOLE AND THE CONTROL CANNOT DRIFT. While the video is in this window
/// these same rectangles are cut out of it (`pip_holes`), and a chip painted one place while the
/// video withdraws from another is a control drawn underneath a video, which is to say a control
/// that does not exist. One function answers where they are and both callers ask it.
fn chip_rects(rect: Rect) -> [Rect; 2] {
    let mut right = rect.right() - CHIP_EDGE;
    let mut next = || {
        let r = Rect::from_min_size(
            Pos2::new(right - CHIP_W, rect.top() + CHIP_EDGE),
            Vec2::splat(CHIP_W),
        );
        right -= CHIP_W + CHIP_GAP;
        r
    };
    let close = next();
    let pin = next();
    [close, pin]
}

/// The resize grip, bottom right. Mirrors `resize_corner`'s own placement.
fn pip_resize_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        rect.right_bottom() - Vec2::splat(PIP_GRIP),
        Vec2::splat(PIP_GRIP),
    )
}

/// The move grip, bottom LEFT, and it exists only while the video is in this window.
///
/// IT REPLACES SOMETHING BETTER AND IT IS NOT AN IMPROVEMENT. The rule this window was built on
/// is that the whole picture is the drag handle: you grab the picture and you move it. That
/// cannot survive a native child window, which takes the pointer over every pixel it covers, and
/// no arrangement of egui gets it back. A window that cannot be moved is not a picture in
/// picture at all, so a corner control is what is left: the same 16 points as the resize grip,
/// the same hover gate, the opposite corner, so the two read as a pair rather than as a leftover.
///
/// It is absent when the video is not here, because the whole picture is the drag handle again.
fn pip_move_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - PIP_GRIP),
        Vec2::splat(PIP_GRIP),
    )
}

/// Every shape the video must WITHDRAW from, in this window's client pixels.
///
/// THE ORDER MATTERS TO ONE CALLER ONLY: `Player::probe_notch` tests the first shape's centre to
/// find out whether a carved hole passes the pointer through at all, and close is the control
/// that must never become unreachable, so close is first.
fn pip_holes(rect: Rect, pixels_per_point: f32) -> Vec<(i32, i32, i32, i32)> {
    let [close, pin] = chip_rects(rect);
    [close, pin, pip_resize_rect(rect), pip_move_rect(rect)]
        .into_iter()
        .map(|r| crate::player::hwnd::to_physical(r, pixels_per_point))
        .collect()
}

/// What the picture in picture window was clicked on, reported to the registry that owns it.
///
/// THE SAME SHAPE AS `titlebar::Hits` AND FOR THE SAME REASON, which that struct's doc gives: no
/// `PartialEq` or `Eq` derive, because those read every field and switch OFF rustc's own dead
/// field warning, and this struct exists so that a control reporting a click nobody applies is a
/// compile warning rather than a quiet nothing.
#[derive(Default, Clone, Copy, Debug)]
struct ChipHits {
    /// The pin glyph was clicked. The registry flips the window's own flag and sets the level.
    pin: bool,
    /// The close glyph was clicked. There is no OS close button on an undecorated window.
    close: bool,
}

/// THE PICTURE IN PICTURE WINDOW'S WHOLE BODY. Returns whether the pointer is inside it.
///
/// THE PICTURE FILLS THE WINDOW. Edge to edge, aspect kept, letterboxed on the app's own ground,
/// which is what a picture in picture is: a small window that is the thing you are looking at,
/// with nothing around it. There is no margin, no strip and no band, and the frame `draw_child`
/// gives it has none either.
///
/// AND THE OFFLINE CASE IS THE MAIN WINDOW'S OWN FUNCTION, NOT A COPY OF IT.
/// `screens::watch::paint_art` paints the well, fits the picture into it centred, and lays OFFLINE
/// across it on a scrim with no caption. That is exactly the rule the main window's folio uses and
/// the owner asked for the same one here, so it is CALLED. The other two states cannot go through
/// it, because it always writes OFFLINE, so they paint the picture with `fit_into` (the same
/// aspect rule, also that module's) and put their own line on it through [`pip_line`].
///
/// THE HOVER RULE IS NOT ENOUGH ON ITS OWN, AND THE FIRST CUT OF THIS WINDOW PROVED IT. Chrome
/// that hides is still chrome the moment anybody looks at the window, because reaching for the
/// window is what makes the pointer arrive. A full width band across the top edge was therefore
/// a title bar with a delay on it. The hover rule stays, because a picture nobody is touching
/// should be only a picture, but WHAT appears on hover now is two square chips in the top right
/// corner (`corner_chips`) and the resize grip. Everything else in the window is the picture, at
/// every size and in every state.
///
/// THE WHOLE WINDOW IS THE DRAG HANDLE, which is what an undecorated window needs and what a
/// picture in picture has always done: you grab the picture and move it. The drag is registered
/// BEFORE the chrome so an exact hit on a control wins, because egui breaks a tie toward the last
/// widget registered; that is the same ordering `titlebar::strip` uses to put a link inside its
/// own drag surface, and reversing the two would make the buttons dead.
///
/// `own_chrome` IS FALSE ONLY ON A BACKEND WITHOUT REAL WINDOWS. `ViewportClass::EmbeddedWindow`
/// means egui has drawn its own frame around this viewport, with its own drag and its own close,
/// and a second set inside it would be two of each. The picture is still the picture.
/// WHERE THE VIDEO IS AND WHERE THE POINTER IS, which are one fact in two halves.
///
/// THEY TRAVEL TOGETHER BECAUSE THEY ARE ONLY EVER TRUE TOGETHER. `os_pointer` is `Some` exactly
/// when a native child window covers this rectangle, because that is the only state in which egui
/// cannot answer the question itself. Two loose booleans could say "not hosting, and here is the
/// OS`s pointer answer anyway", which is a sentence with no meaning.
#[derive(Clone, Copy, Debug, Default)]
struct PipSeat {
    /// The video is parented into this window right now.
    hosting: bool,
    /// What `WindowFromPoint` says, when egui cannot see the pointer itself.
    os_pointer: Option<bool>,
}

fn pip(
    ui: &mut Ui,
    live: Option<bool>,
    art: Option<&crate::channel_art::Art>,
    pinned: bool,
    own_chrome: bool,
    seat: PipSeat,
    hits: &mut ChipHits,
) -> (bool, Rect) {
    let rect = ui.available_rect_before_wrap();

    /* THE VIDEO IS ALREADY THERE, SO NOTHING IS PAINTED UNDER IT. A native child window fills
     * this rectangle and is composited over everything egui puts here, so a picture drawn now
     * would be invisible in the ordinary case and would show through the carved holes in the
     * worst one, which is a corner of an offline screen peeping out of a live stream. The holes
     * are meant to show the app's own ground and the chrome standing on it, and that is all. */
    if seat.hosting {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, INK);
        let inside = seat.os_pointer.unwrap_or(false);
        if own_chrome && inside {
            pip_move_grip(ui, rect, ui.id().with("pip"));
            corner_chips(ui, rect, ui.id().with("pip"), pinned, hits);
        }
        return (inside, rect);
    }

    match (art, live) {
        /* The main window's rule, called: picture, centred, OFFLINE across it, no caption. */
        (Some(a), Some(false)) => crate::screens::watch::paint_art(ui, a, rect.size()),
        (art, _) => {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, SUNK);
            if let Some(a) = art {
                let at = Rect::from_center_size(
                    rect.center(),
                    crate::screens::watch::fit_into(a.px, rect.size()),
                );
                ui.painter().image(
                    a.texture.id(),
                    at,
                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            /* A caption only when there IS something to caption. */
            let lay = if art.is_some() {
                Lay::Corner
            } else {
                Lay::Centred
            };
            pip_line(ui, rect, &pip_words(live), lay);
        }
    }

    let id = ui.id().with("pip");
    if own_chrome {
        let drag = ui.interact(rect, id.with("drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        /* The only affordance the picture itself carries. There is no bar to point at, so the
         * cursor is what says the window can be moved from anywhere on it. */
        drag.on_hover_cursor(egui::CursorIcon::Grab);
    }
    /* egui'S OWN ANSWER, WHICH IS ONLY AVAILABLE WHILE THERE IS NO VIDEO HERE. The moment a
     * child window covers this rectangle egui stops seeing the pointer over it at all, which is
     * why the hosting branch above takes the OS's answer instead. */
    let inside = seat
        .os_pointer
        .unwrap_or_else(|| ui.rect_contains_pointer(rect));
    if own_chrome && inside {
        corner_chips(ui, rect, id, pinned, hits);
    }
    (inside, rect)
}

/// The move grip: a corner you can drag the window by, drawn only while the video is here.
///
/// FOUR ARROWS AND NOT A DOT, because it has to say what it does with no words next to it and no
/// tooltip available (a tooltip is an egui layer outside every carved hole, so it would be drawn
/// under the video and simply vanish). Four arrows from a centre is the oldest move glyph there
/// is and it is what a cursor turns into anyway.
fn pip_move_grip(ui: &mut Ui, rect: Rect, id: Id) {
    let r = pip_move_rect(rect);
    let resp = ui.interact(r, id.with("move"), Sense::click_and_drag());
    let hot = resp.hovered() || resp.dragged();
    ui.painter().rect_filled(
        r,
        CornerRadius::ZERO,
        if hot {
            PANEL_2
        } else {
            INK.gamma_multiply(0.72)
        },
    );
    let col = if hot { GOLD_HI } else { TEXT_2 };
    let c = r.center();
    let arm = 4.5;
    let head = 2.0;
    let stroke = Stroke::new(1.2, col);
    for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
        let tip = Pos2::new(c.x + dx * arm, c.y + dy * arm);
        ui.painter().line_segment([c, tip], stroke);
        /* The two barbs of each arrowhead, square to the arm. */
        let (px, py) = (dy, dx);
        for sign in [-1.0_f32, 1.0] {
            ui.painter().line_segment(
                [
                    tip,
                    Pos2::new(
                        tip.x - dx * head + px * head * sign,
                        tip.y - dy * head + py * head * sign,
                    ),
                ],
                stroke,
            );
        }
    }
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    resp.on_hover_cursor(egui::CursorIcon::Move);
}

/// The line laid across the picture, in the two states `paint_art` does not cover.
///
/// THE STATE WORD IS THE PILL'S OWN. `titlebar::dot_of(..).word()` is what the pill in every other
/// window prints, so this window and the pill above the main window's body cannot print two words
/// for one fact. Offline reaches here only when there is NO artwork to lay OFFLINE on, and then
/// the word is all there is to draw.
fn pip_words(live: Option<bool>) -> String {
    let word = crate::titlebar::dot_of(live).word();
    match live {
        Some(true) => format!("{word}. {IN_THE_MAIN_WINDOW}"),
        _ => word.to_owned(),
    }
}

/// Where the one line sits, which depends on whether there is a picture under it.
///
/// THIS IS NOT A STYLE TOGGLE, IT IS TWO DIFFERENT JOBS. With artwork behind it the line is a
/// CAPTION on a picture, and a caption belongs out of the way; a banner across the middle of the
/// artwork is the second loudest thing in a window whose whole point is the first. With no
/// artwork the line is the ONLY thing in the window, and a word tucked into the bottom corner of
/// an otherwise empty rectangle reads as a window that failed to load rather than as an answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lay {
    /// No picture: the word is the window, so it goes in the middle of it.
    Centred,
    /// A picture: the line is a caption on it and sits in the bottom left, small.
    Corner,
}

/// One line on a scrim of the app's own ground, laid out by [`Lay`].
///
/// A SCRIM UNDER IT FOR THE SAME REASON `paint_art` HAS ONE: the artwork is the streamer's own and
/// we do not get to know what is behind the text, so the words sit on a band of INK at partial
/// alpha rather than on whatever the picture happens to be. The alpha and the shape are that
/// function's, so the two lines this window can draw look like one thing.
fn pip_line(ui: &Ui, rect: Rect, text: &str, lay: Lay) {
    let pad = Vec2::new(9.0, 5.0);
    let font = match lay {
        Lay::Centred => FontId::proportional(12.5),
        Lay::Corner => FontId::proportional(11.0),
    };
    /* Wrapped to the room the chosen corner actually leaves, so a narrow window shortens the
     * line instead of running it off the edge. */
    let wrap = (rect.width() - (CHIP_EDGE + pad.x) * 2.0).max(80.0);
    let galley = ui.painter().layout(text.to_owned(), font, TEXT, wrap);
    let size = galley.size();
    let at = match lay {
        Lay::Centred => rect.center() - size * 0.5,
        Lay::Corner => Pos2::new(
            rect.left() + CHIP_EDGE + pad.x,
            rect.bottom() - CHIP_EDGE - pad.y - size.y,
        ),
    };
    let scrim = Rect::from_min_size(at - pad, size + pad * 2.0).intersect(rect);
    ui.painter()
        .rect_filled(scrim, CornerRadius::ZERO, INK.gamma_multiply(0.72));
    ui.painter().galley(at, galley, TEXT);
}

/// The hover chrome: two square chips in the top right corner, over the picture, and nothing else.
///
/// WHY TWO AND NOT THREE.
///   the pin      D3 makes it mandatory: a window that is always on top must say so, and this is
///                the one window the owner will actually pin over a game. Same glyph as every
///                title strip (`titlebar::pin_glyph`), filled against hollow.
///   close        the window is undecorated, so there is no OS close button. Without this the only
///                way out is Alt+F4 or the hotkey, and a window with no visible way to shut it is
///                the one thing a floating window may not be.
///
/// CHECK NOW IS GONE FROM HERE, AND THE ARGUMENT THAT KEPT IT WAS FALSE.
/// It was kept on the ground that `Ask::CheckLive` had exactly one producer in the whole program
/// and this was it, so cutting the button would delete a capability rather than tidy a window.
/// That was true of REACHABLE code and only because the old pop-out page was still sitting in
/// `screens::watch` with its own button in it, unreachable since `Slot::is_pip` started returning
/// before any screen is drawn, and kept warm by its own tests so no dead-code warning ever fired.
/// The argument for the third control rested on code nothing could run. The page is deleted and
/// the poll now lives in the main window's Watch header, where the rest of this screen's controls
/// already are and where a hand that wants to operate the app already is;
/// `screens::watch::tests::check_now_is_in_the_header_and_asks_the_app_to_poll` holds it there.
///
/// WHAT ELSE IS CUT AND WHY. Minimise and maximise, which every title strip has: maximising a
/// picture in picture is the opposite of what it is for, and minimising it is closing it with
/// extra steps when the whole window is two hundred points tall. The live pill, because it is a
/// status readout and the picture already carries the state in the one line laid on it. The
/// status sentence, the last checked line, both browser buttons, their caption and the VIDEOS
/// band, which are the overflow bin this window was mistaken for. The window's TITLE, because the
/// taskbar entry and the OS window name still carry it (`Tool::title`) and a strip drawn to hold a
/// name is the strip this window is defined by not having.
///
/// EACH CHIP CARRIES ITS OWN GROUND RATHER THAN SHARING A BAND. That is the difference between a
/// control floating on a picture and a toolbar sitting across it, and it is the whole of the
/// change the owner asked for: everything between the two chips and every other edge of the
/// window is the picture, at every size.
///
/// HOVER TEXT ON BOTH, because both are drawings and a drawing has to be able to say what it is.
/// D3 asks for the pin's in so many words.
fn corner_chips(ui: &mut Ui, rect: Rect, id: Id, pinned: bool, hits: &mut ChipHits) {
    /* Right to left, so close is always in the corner: the title strip's own order. Asked for
     * rather than computed, because the video withdraws from these exact rectangles. */
    let [close_r, pin_r] = chip_rects(rect);

    /* THE CHIP IS PAINTED IN BOTH STATES, not only on hover. An unhovered chip still needs a ground
     * or the glyph is a thin stroke lost in whatever the artwork happens to be behind it, which is
     * the same reason the line above it has a scrim. Hover swaps the ground for the opaque panel
     * fill every other row in the app uses, so the feedback is the app's and not this window's. */
    let button = |ui: &Ui, r: Rect, name: &'static str, tip: &str| -> (Response, Color32) {
        let resp = ui.interact(r, id.with(name), Sense::click());
        let hot = resp.hovered();
        ui.painter().rect_filled(
            r,
            CornerRadius::ZERO,
            if hot {
                PANEL_2
            } else {
                INK.gamma_multiply(0.72)
            },
        );
        (resp.on_hover_text(tip), if hot { GOLD_HI } else { TEXT_2 })
    };

    let (pin, col) = button(
        ui,
        pin_r,
        "pin",
        if pinned {
            "unpin: let other windows cover this one"
        } else {
            "pin: keep this window on top"
        },
    );
    crate::titlebar::pin_glyph(
        ui.painter(),
        pin_r.center(),
        pinned,
        if pinned && col == TEXT_2 { GOLD } else { col },
    );
    if pin.clicked() {
        hits.pin = true;
    }

    let (close, col) = button(ui, close_r, "close", "close");
    crate::titlebar::close_glyph(ui.painter(), close_r.center(), col);
    if close.clicked() {
        hits.close = true;
    }
}

/// THE CHORD THAT MOVES A WINDOW WITH NO TITLE BAR. `Ctrl+Alt` held, then drag anywhere.
///
/// `Ctrl+Alt` AND NOT `Alt` ALONE, because it is the sentence this app already speaks: every
/// binding in D4 is `Ctrl+Alt` and something, so "Ctrl+Alt is how you talk to Grimoire" is a rule
/// a person learns once. `Alt+drag` is also the window manager's own gesture on other desktops,
/// which is a collision waiting for the first person who runs this under one.
pub const DRAG_CHORD: &str = "Ctrl+Alt+drag";

/// The one sentence a bare window offers, and only while the pointer is inside it.
const DRAG_HINT: &str = "Drag to move";

/// WHAT REPLACES A TITLE STRIP ON AN OVERLAY: a chord, and optionally two chips.
///
/// # The chord is read from raw input and not from a widget, deliberately
///
/// An invisible full-window drag surface would have to be registered either BEFORE the screen, in
/// which case every button in the body still wins its exact hit and the drag works everywhere
/// EXCEPT over the rows a reader is most likely to grab, or AFTER it, in which case the drag
/// swallows every click in the body. Neither is what "drag anywhere" means. The chord is a window
/// manager gesture rather than a widget interaction, so it is read as one: the press event plus
/// the modifiers, off this viewport's own input, before layout has an opinion.
///
/// ONCE `StartDrag` IS SENT THE OS OWNS THE DRAG, so letting go of the modifiers mid-move does not
/// drop the window, and no state has to be kept here between passes.
///
/// # It is called AFTER the body, and that is not a detail
///
/// The body is a `CentralPanel` with an opaque fill over the whole window. Called before it, every
/// chip this draws is painted and then buried, while its hit rect goes on working: an invisible
/// control that closes the window when a person clicks where they can see it is not. Called after,
/// the chips are on top and win their own hits, which is what a control floating over a body does.
///
/// # The hint appears on hover and the window is otherwise silent
///
/// A window with no chrome that cannot be moved unless you know a chord is a window that is stuck,
/// and the pointer is inside it at exactly the moment somebody is trying to move it. It is one dim
/// line at the bottom edge, inside the body's own margin, so it covers no row.
fn bare_chrome_at(
    ui: &mut Ui,
    at: usize,
    name: Option<&str>,
    pinned: bool,
    chips: bool,
    hits: &mut ChipHits,
) {
    let ctx = ui.ctx().clone();
    let (chord, inside) = ctx.input(|i| {
        let m = i.modifiers;
        (
            m.ctrl && m.alt && i.pointer.primary_pressed(),
            i.pointer.has_pointer(),
        )
    });
    if chord {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if !inside {
        return;
    }

    let rect = ui.available_rect_before_wrap();
    if chips {
        corner_chips(ui, rect, Id::new(("grimoire.chips", at)), pinned, hits);
    }
    let Some(name) = name else {
        return;
    };
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        /* THE WINDOW'S NAME FIRST, so the owner can call a window out by what it says on it. */
        format!("{name} \u{b7} {DRAG_HINT}"),
        egui::FontId::proportional(10.0),
        TEXT_3,
    );
}

/// HOW MUCH VERTICAL SPACE THE CONTENT OF THIS `Ui` ACTUALLY TOOK.
///
/// NOT `min_rect().height()`, AND THAT WAS A FIXED POINT RATHER THAN A NUMBER THAT WAS MERELY
/// WRONG. A panel's `Ui` is seeded with the WHOLE PANEL as its minimum, because filling its space
/// is what a panel is for, so `min_rect` starts at the full height and can only grow. Measured on
/// a 300 point window while the self-sizing code was live: used=276, pad=24, want=300, now=300.
/// Whatever height the window happened to be, it reported needing exactly that, the difference was
/// always zero, no resize was ever sent, and the window kept whatever it had been dragged to. The
/// feature looked implemented and did nothing.
///
/// THE CURSOR IS WHERE THE NEXT WIDGET WOULD GO, so the distance from the top of the content area
/// to it is the space the content took, and it does not depend on how much space there was. It
/// carries one trailing `item_spacing.y`, which is the gap the next row would have had.
///
/// A FUNCTION AND NOT A LINE, so `the_height_is_the_content_and_not_the_window` can drive it at two
/// different sizes and demand one answer. That is the whole of the defect and it is not visible
/// from reading either expression.
fn content_height(ui: &Ui) -> f32 {
    ui.cursor().top() - ui.max_rect().top()
}

/// THE SMALLEST AN OVERLAY MAY BE DRAGGED TO. Far under a page's floor: a group of five is a
/// header and five rows, about ninety points, and a 200 point floor would leave it mostly empty.
const OVERLAY_MIN: [f32; 2] = [240.0, 56.0];

/// ONE OF THE OWNER'S OVERLAY WINDOWS: his config, plus the per-session facts a window needs.
///
/// THE CONFIG IS COPIED RATHER THAN BORROWED, because the settings live on the root's `Cx` and
/// this lives behind the registry's mutex, which a deferred viewport's callback takes on its own
/// clock. `sync_overlays` is the one place the copy is refreshed.
struct OverlayWindow {
    cfg: crate::overlay::Overlay,
    pin: crate::pin::Pin,
    placed: bool,
    focus: bool,
    /// Has a root pass run since this window was last asked for something? See `Window::serviced`;
    /// the reason is identical and so is the bug it prevents.
    serviced: bool,
    /// THE WINDOW CHANGED ITS OWN CONFIG AND THE SETTINGS FILE HAS NOT HEARD YET.
    ///
    /// A chip toggles the pin, the OS close flips `open`, and a drag changes the remembered size,
    /// all inside a deferred viewport's callback which cannot reach `Settings`. The root collects
    /// these on its next pass; see `Windows::overlay_edits`.
    dirty: bool,
    /// WHERE THE WINDOW WAS SEEN AND SINCE WHEN, while it has not yet been remembered there. See
    /// [`remember_position`].
    moved: Option<([f32; 2], std::time::Instant)>,
}

/// BRING THE REGISTRY'S OVERLAY WINDOWS INTO LINE WITH THE OWNER'S SETTINGS.
///
/// MATCHED BY `id` AND NOT BY POSITION, which is the whole of the function. Reordering the list,
/// renaming an overlay or deleting one from the middle all move positions around; an id does not
/// move, so a window keeps its placement and its pin across every one of those. Matching by index
/// would hand window three's remembered position to whatever ended up third.
///
/// A WINDOW WHOSE OVERLAY IS GONE IS DROPPED, and the root simply stops showing that viewport,
/// which is how egui closes one.
/// DOES ANY WINDOW NEED THE CHILD CONTEXT: an open tool window, or an open overlay.
///
/// # DEFECT: AN OVERLAY OPENED ON ITS OWN WAS A BLANK WINDOW
///
/// The context an overlay draws from, with its own ingest, was built only when one of the five
/// tool windows was open. With nothing open but overlays it was never built, and draw_overlay
/// returns before painting when it is missing, so every overlay was an empty dark rectangle
/// that never sized itself to its content. Found by popping the meter, the coach and the pill
/// out on the owner's machine and capturing the three windows: all three were blank.
fn wants_child_cx(
    windows: impl IntoIterator<Item = bool>,
    overlays: &[crate::overlay::Overlay],
) -> bool {
    windows.into_iter().any(|open| open) || overlays.iter().any(|o| o.open)
}

/// WHERE AN OVERLAY OPENS: where the owner left it, or beside the main window, stepped by its
/// place in the list so two new ones do not open on top of each other.
fn place_overlay(at: Option<[f32; 2]>, root: Option<Rect>, i: usize) -> Pos2 {
    if let Some([x, y]) = at {
        return Pos2::new(x, y);
    }
    root.map_or(Pos2::new(160.0, 160.0), |r| {
        r.left_top() + Vec2::new(96.0 + 28.0 * i as f32, 96.0 + 28.0 * i as f32)
    })
}

/// HOW LONG A WINDOW HAS TO STAY PUT BEFORE ITS NEW PLACE IS WRITTEN DOWN.
///
/// A DRAG REPORTS A NEW POSITION EVERY FRAME, and writing the settings file on every one of them
/// would be sixty writes a second for as long as a hand is moving. The place is remembered once
/// the window has been still this long.
const OVERLAY_SETTLE: Duration = Duration::from_millis(400);

/// THE POSITION TO REMEMBER NOW, IF THERE IS ONE.
///
/// `here` is where the window is this frame. A position already remembered within a point of it
/// is nothing to do. A new one starts a clock, and is returned once the window has been at it for
/// [`OVERLAY_SETTLE`]; moving again restarts the clock.
fn remember_position(
    at: Option<[f32; 2]>,
    moved: &mut Option<([f32; 2], std::time::Instant)>,
    here: [f32; 2],
    now: std::time::Instant,
) -> Option<[f32; 2]> {
    let near = |a: [f32; 2]| (a[0] - here[0]).abs() <= 1.0 && (a[1] - here[1]).abs() <= 1.0;
    if at.is_some_and(near) {
        *moved = None;
        return None;
    }
    match *moved {
        Some((seen, since)) if near(seen) => {
            if now.duration_since(since) >= OVERLAY_SETTLE {
                *moved = None;
                Some(here)
            } else {
                None
            }
        }
        _ => {
            *moved = Some((here, now));
            None
        }
    }
}

/// A PLAIN DRAG ANYWHERE ON AN OVERLAY MOVES IT. Returns whether a move started this frame.
///
/// # WHY THIS REPLACED THE CHORD AS THE WAY TO MOVE ONE
///
/// The owner popped his overlays out and could not move them: the only way was `Ctrl+Alt+drag`,
/// and nothing a person does with a window by habit found it. The chord stays, and still works
/// over anything.
///
/// # WHAT STILL WINS A PRESS
///
/// This registers BEFORE the body and the chrome, so anything drawn after that takes a click or a
/// drag wins its own hit: the pin and close chips, the resize corner. The widgets take only hover,
/// and text on an overlay is not selectable, so a press on a figure or a name moves the window
/// rather than starting a text selection nobody wants over a game.
fn move_surface(ui: &mut Ui, at: usize) -> bool {
    ui.style_mut().interaction.selectable_labels = false;
    let grab = ui.interact(
        ui.max_rect(),
        Id::new(("grimoire.overlay.move", at)),
        egui::Sense::drag(),
    );
    if grab.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        return true;
    }
    false
}

/// IS THIS OVERLAY NOTHING BUT A PILL?
///
/// # WHAT THE OWNER ASKED FOR
///
/// The Pill popped out as a capsule drawn inside a dark box with rounded corners, and the owner
/// asked for one or the other. It was made see-through first, which this app's GL context cannot
/// be on Windows: the window came out black, Windows still rounded it, and eframe logged "Cannot
/// create transparent window: the GL config does not support it". So it is the box: a window of
/// one pill is a solid rounded box sized to the pill's one line, and the pill draws no capsule of
/// its own inside it (see `screens::dps::pill`). It is still called the Pill. Any other
/// overlay, a pill among other widgets included, keeps the ordinary body.
fn chromeless(cfg: &crate::overlay::Overlay) -> bool {
    matches!(cfg.panels().as_slice(), [crate::overlay::Widget::Pill(_)])
}

/// THE SMALLEST A PILL'S WINDOW MAY BE. Far under an overlay's floor, because a pill is one line.
const PILL_MIN: [f32; 2] = [40.0, 20.0];

/// THE MARGIN ROUND A PILL'S ONE LINE, inside its box: room either side of the line and a little
/// above and below, so the text is clear of the corners Windows rounds.
const PILL_PAD: egui::Margin = egui::Margin::symmetric(12, 6);

/// THE WINDOW AN OVERLAY OPENS IN. See [`chromeless`] for the Pill's.
///
/// NEVER SEE-THROUGH, the Pill's included: [`chromeless`] says what a see-through window
/// turned out to be here.
fn overlay_builder(cfg: &crate::overlay::Overlay) -> ViewportBuilder {
    let bare = chromeless(cfg);
    ViewportBuilder::default()
        .with_title(cfg.name.clone())
        .with_inner_size([cfg.w, cfg.h])
        .with_min_inner_size(if bare { PILL_MIN } else { OVERLAY_MIN })
        .with_resizable(!bare)
        .with_decorations(false)
        .with_transparent(false)
        .with_window_level(level(cfg.pinned))
}

/// THE FRAME AN OVERLAY'S BODY IS DRAWN IN: the page ground under both, a tight margin round a
/// pill's one line and the ordinary margin round anything else.
fn overlay_frame(bare: bool) -> egui::Frame {
    if bare {
        egui::Frame::NONE.fill(INK).inner_margin(PILL_PAD)
    } else {
        egui::Frame::NONE
            .fill(INK)
            .inner_margin(egui::Margin::same(12))
    }
}

/// DRAW AN OVERLAY'S BODY AND SAY HOW BIG WHAT IT DREW WAS: its height, and its width.
///
/// # THE WIDTH IS THE DRAWING'S AND NOT THE PANEL'S
///
/// This read the panel's own extent, and an egui central panel always spans its whole window, so
/// the Pill's window kept the 420 points it opened at while the pill in it was about 340: a
/// strip of window beside the pill that took the game's clicks. Found on the owner's screen, after
/// the pill itself had lost its box. The drawing is measured inside a scope of its own, which is as
/// wide as what was drawn and no wider.
fn overlay_body(ui: &mut Ui, bare: bool, draw: impl FnOnce(&mut Ui)) -> (f32, f32) {
    egui::CentralPanel::default()
        .frame(overlay_frame(bare))
        .show(ui, |ui| {
            let drawn = ui.scope(draw).response.rect;
            (content_height(ui), drawn.width())
        })
        .inner
}

/// THE SIZE AN OVERLAY'S WINDOW SHOULD BE, from what its body drew.
///
/// A PILL'S BOX IS ITS ONE LINE AND ITS MARGIN, rounded up, and no bigger: a box with room to spare
/// round one line is dead space over the game. Anything else keeps the width its owner dragged it
/// to and takes the height its content needs.
fn overlay_size(bare: bool, now: Vec2, used: f32, wide: f32) -> Vec2 {
    if bare {
        let pad = PILL_PAD.sum();
        Vec2::new(
            (wide + pad.x).ceil().max(PILL_MIN[0]),
            (used + pad.y).ceil().max(PILL_MIN[1]),
        )
    } else {
        Vec2::new(now.x, (used + BARE_PAD).max(OVERLAY_MIN[1]))
    }
}

fn sync_overlays(have: &mut Vec<OverlayWindow>, want: &[crate::overlay::Overlay]) {
    let mut next: Vec<OverlayWindow> = Vec::with_capacity(want.len());
    for cfg in want {
        match have.iter().position(|o| o.cfg.id == cfg.id) {
            Some(at) => {
                let mut keep = have.remove(at);
                /* THE ASK IS RE-RAISED WHEN THE OWNER OPENS ONE, because a window that was closed
                 * has no viewport and needs a root pass to get one. Same rule as `Window::open`. */
                /* A WINDOW'S OWN UNSAVED CHANGE WINS UNTIL IT HAS BEEN COLLECTED.
                 *
                 * DEFECT: AN OVERLAY FORGOT WHERE IT WAS PUT, AND ITS PIN AND CLOSE CHIPS UNDID
                 * THEMSELVES. The window writes its change onto its own config and marks itself
                 * dirty in its deferred pass. The root's next pass runs this sync first and the
                 * edit collection after it (main.rs), so the settings copy, which had not heard
                 * yet, was written over the change here, and what the collection then saved was
                 * the old config. Found on the owner's machine: three popped out overlays, left
                 * on screen, with no position in the settings file.
                 *
                 * WHAT THIS COSTS, SAID OUT LOUD: a change the Parser page makes to the same
                 * overlay in the same frame as the window's own is overwritten by the window's
                 * when it is collected. One frame, one overlay, and the page's next edit lands. */
                if !keep.dirty {
                    if cfg.open && !keep.cfg.open {
                        keep.serviced = false;
                    }
                    keep.cfg = cfg.clone();
                }
                next.push(keep);
            }
            None => next.push(OverlayWindow {
                cfg: cfg.clone(),
                pin: crate::pin::Pin::new(cfg.pinned),
                placed: false,
                focus: false,
                serviced: false,
                dirty: false,
                moved: None,
            }),
        }
    }
    *have = next;
}

/// ONE PASS OF ONE OVERLAY WINDOW.
///
/// MUCH SMALLER THAN `draw_child` AND DELIBERATELY SEPARATE. A built-in window carries a title
/// strip, a live pill, an item snapshot, a navigation ask and a picture-in-picture path; an overlay
/// carries none of that. Folding the two together would put five `if` statements around every one
/// of those for the benefit of sharing a `CentralPanel`.
fn draw_overlay(inner: &Arc<Mutex<Inner>>, at: usize, ui: &mut Ui, class: ViewportClass) {
    let ctx = ui.ctx().clone();
    let mut g = lock(inner);
    let Inner { overlays, cx, .. } = &mut *g;
    let Some(o) = overlays.get_mut(at) else {
        /* The owner deleted this overlay between the plan and the callback. Nothing to draw, and
         * the root will stop showing the viewport on its next pass. */
        return;
    };

    if ctx.input(|i| i.viewport().close_requested()) {
        o.pin.forget();
        o.cfg.open = false;
        o.dirty = true;
        ctx.request_repaint_of(ViewportId::ROOT);
        return;
    }
    o.pin.reconcile(&ctx, None);

    let Some(cc) = cx.as_mut() else {
        return;
    };

    let cfg = o.cfg.clone();
    let bare = chromeless(&cfg);
    move_surface(ui, at);
    let (used, wide) = overlay_body(ui, bare, |ui| {
        let mut child_cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &cc.live,
            settings: &mut cc.settings,
            ingest: &mut cc.ingest,
            player: Default::default(),
            chat: cc.chat.clone(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            stage: None,
            demand: None,
            ask: Ask::None,
        };
        crate::screens::dps::DpsScreen.ui(ui, &mut child_cx, &cfg);
    });

    /* THE CHROME, AFTER THE BODY. See `bare_chrome`: drawn first it is painted and then buried
     * under the panel's opaque fill while its hit rects go on working, which is an invisible
     * control that closes the window. */
    if class != ViewportClass::EmbeddedWindow {
        let mut hits = ChipHits::default();
        /* A WINDOW OF ONE WIDGET GOES BY THAT WIDGET'S NAME, and one of several by the name the
         * owner gave the overlay. See `Widget::codename`. */
        let name = match o.cfg.panels().as_slice() {
            [w] => w.codename().to_owned(),
            _ => o.cfg.name.clone(),
        };
        /* NO HINT LINE ON A PILL: the line would print across the pill itself. */
        let hint = (!bare).then_some(name.as_str());
        bare_chrome_at(ui, at, hint, o.cfg.pinned, o.cfg.chips, &mut hits);
        if hits.pin {
            o.cfg.pinned = !o.cfg.pinned;
            o.dirty = true;
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if hits.close {
            o.pin.forget();
            o.cfg.open = false;
            o.dirty = true;
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }

        /* The height follows the content; the width is the owner's. */
        if let Some(now) = ctx.input(|i| i.viewport().inner_rect) {
            let want = overlay_size(bare, now.size(), used, wide);
            if (want - now.size()).length() > 1.0 {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(want));
            }
            o.cfg.w = want.x;
            o.cfg.h = want.y;
        }
        /* AND WHERE IT IS, once it has stayed there. See `remember_position`. */
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            let here = [outer.left(), outer.top()];
            match remember_position(o.cfg.at, &mut o.moved, here, std::time::Instant::now()) {
                Some(p) => {
                    o.cfg.at = Some(p);
                    o.dirty = true;
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
                None if o.moved.is_some() => ctx.request_repaint_after(OVERLAY_SETTLE),
                None => {}
            }
        }
        /* A PILL IS AS BIG AS ITS WORDS, so it has nothing to drag out. */
        if !bare {
            resize_corner(&ctx, Id::new(("grimoire.overlay.resize", at)));
        }
    }

    ctx.request_repaint_after(Duration::from_millis(1000));
}

/// A drag handle in the bottom right corner, because the OS frame that would provide one is off.
/// Public because the main window is frameless too (D3, D8) and needs the same grip; `id` keeps
/// one Area per viewport.
pub fn resize_corner(ctx: &egui::Context, id: Id) {
    let screen = ctx.content_rect();
    let size = Vec2::splat(16.0);
    let pos = screen.right_bottom() - size;
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
            let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeNwSe);
            let col = if resp.hovered() || resp.dragged() {
                GOLD_DIM
            } else {
                RULE
            };
            let p = ui.painter();
            /* Three short diagonals, the conventional grip. */
            for k in 0..3 {
                let d = 4.0 * k as f32 + 3.0;
                p.line_segment(
                    [
                        Pos2::new(rect.right() - d, rect.bottom() - 1.0),
                        Pos2::new(rect.right() - 1.0, rect.bottom() - d),
                    ],
                    Stroke::new(1.0, col),
                );
            }
            if resp.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(
                    egui::ResizeDirection::SouthEast,
                ));
            }
        });
}

/* ---------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /// A PILL'S WINDOW IS AS WIDE AS THE PILL, MEASURED OFF A REAL PILL.
    ///
    /// The pure sizing rule was tested and the measurement handed to it was not, and the measurement
    /// was the panel's full width. This draws the real widget through the real body in a window of
    /// the width it opens at.
    ///
    /// WHAT MUTATION MAKES THIS RED: the body measuring the panel rather than the drawing.
    #[test]
    fn a_pill_windows_width_is_the_pill_and_not_the_window() {
        use crate::fights::{FightRow, Fighter, Who};
        use crate::overlay::{Pill, Widget};
        let fight = FightRow {
            secs: 10,
            headline: Some("a will sapper".to_owned()),
            fighters: vec![Fighter {
                who: Who::You,
                dealt: 900,
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 57.0))),
            ..Default::default()
        };
        let mut measured = (0.0, 0.0);
        let out = ctx.run_ui(input, |ui| {
            measured = overlay_body(ui, true, |ui| {
                crate::screens::dps::draw_widget(
                    ui,
                    &fight,
                    crate::fights::Pulse::Fighting,
                    &Widget::Pill(Pill::default()),
                );
            });
        });
        out.drop_without_applying_deltas();
        let (tall, wide) = measured;
        assert!(
            wide > 60.0 && wide < 400.0,
            "the pill measured {wide} points wide in a 420 point window, so its window keeps a \
             strip of empty box beside the pill's line"
        );
        assert!(
            tall > 10.0 && tall < 57.0,
            "the pill measured {tall} points tall"
        );
    }

    /// A PILL ON ITS OWN IS A SOLID BOX SIZED TO ITS LINE, AND NO WINDOW ASKS TO BE SEE-THROUGH.
    ///
    /// WHAT MUTATION MAKES THIS RED: a see-through pill window, which this GL context paints black;
    /// no ground under the pill or no margin round it; its box not sized to its line and margin; the
    /// app asking for a see-through context or clearing to nothing again.
    #[test]
    fn a_pill_on_its_own_is_a_solid_box_sized_to_its_line() {
        use crate::overlay::{Coach, Overlay, Pill, Widget};
        let pill = Overlay {
            widgets: Some(vec![Widget::Pill(Pill::default())]),
            ..Overlay::default()
        };
        let meter = Overlay::default();
        let both = Overlay {
            widgets: Some(vec![
                Widget::Pill(Pill::default()),
                Widget::Coach(Coach::default()),
            ]),
            ..Overlay::default()
        };
        assert!(
            chromeless(&pill),
            "a window of one pill is drawn as any other overlay"
        );
        assert!(
            !chromeless(&meter) && !chromeless(&both),
            "a window that is not only a pill is drawn as a pill"
        );

        assert_eq!(
            overlay_builder(&pill).transparent,
            Some(false),
            "the pill's window asks to be see-through, which this GL context paints black"
        );
        assert_eq!(overlay_builder(&meter).transparent, Some(false));
        let bare = overlay_frame(true);
        assert_eq!(bare.fill, INK, "the pill has no ground under it");
        assert_eq!(
            bare.inner_margin, PILL_PAD,
            "the pill has no margin round it"
        );
        assert!(
            bare.inner_margin.left > 0 && bare.inner_margin.top > 0,
            "the pill's line runs into the corners of its box"
        );
        assert_eq!(overlay_frame(false).fill, INK);

        assert_eq!(
            overlay_size(true, Vec2::new(420.0, 57.0), 18.4, 231.2),
            Vec2::new(256.0, 31.0),
            "the pill's box is not its line and its margin"
        );
        assert_eq!(
            overlay_size(false, Vec2::new(360.0, 91.0), 70.0, 300.0),
            Vec2::new(360.0, 94.0)
        );

        let main = include_str!("main.rs");
        let main = &main[..main.find("mod tests {").expect("the test module")];
        assert!(
            !main.contains(".with_transparent(true)"),
            "the app asks for a see-through context again"
        );
        assert!(
            !main.contains("fn clear_color"),
            "the app clears its windows to its own colour again"
        );
    }

    /// A WINDOW'S UNSAVED CHANGE SURVIVES THE ROOT PASS THAT RUNS BEFORE IT IS COLLECTED.
    ///
    /// The order in main.rs is the window sync, then the edit collection. The window moved and
    /// unpinned itself in its own pass, and the sync that follows is handed the settings copy,
    /// which has not heard yet. The change has to be there for the collection to save.
    ///
    /// WHAT MUTATION MAKES THIS RED: the sync copying the settings over a dirty window.
    #[test]
    fn an_overlays_own_change_survives_until_it_is_saved() {
        let stored = crate::overlay::Overlay {
            id: "pop-coach".to_owned(),
            open: true,
            ..crate::overlay::Overlay::default()
        };
        let mut have = Vec::new();
        sync_overlays(&mut have, std::slice::from_ref(&stored));

        /* THE WINDOW'S OWN PASS: it was dragged and its pin chip pressed. */
        have[0].cfg.at = Some([900.0, 40.0]);
        have[0].cfg.pinned = !stored.pinned;
        have[0].dirty = true;

        /* THE ROOT'S NEXT PASS SYNCS FROM SETTINGS THAT HAVE NOT HEARD. */
        sync_overlays(&mut have, std::slice::from_ref(&stored));
        assert_eq!(
            have[0].cfg.at,
            Some([900.0, 40.0]),
            "the sync wrote the settings copy over the window's new position before it was saved"
        );
        assert_eq!(
            have[0].cfg.pinned, !stored.pinned,
            "the pin chip undid itself"
        );
        assert!(
            have[0].dirty,
            "the change was dropped before it was collected"
        );

        /* ONCE COLLECTED AND SAVED, THE SETTINGS COPY IS TAKEN AGAIN. */
        have[0].dirty = false;
        let saved = crate::overlay::Overlay {
            at: Some([900.0, 40.0]),
            pinned: !stored.pinned,
            name: "renamed on the page".to_owned(),
            ..stored.clone()
        };
        sync_overlays(&mut have, std::slice::from_ref(&saved));
        assert_eq!(
            have[0].cfg, saved,
            "a window with nothing waiting ignored the settings"
        );
    }

    /// AN OVERLAY SAYS ITS NAME WHEN THE POINTER IS IN IT.
    ///
    /// WHAT MUTATION MAKES THIS RED: the hint dropping the name.
    #[test]
    fn an_overlay_says_its_name_under_the_pointer() {
        let ctx = egui::Context::default();
        let mut said = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(Pos2::new(40.0, 20.0))],
            Vec::new(),
        ] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 120.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let mut hits = ChipHits::default();
                bare_chrome_at(ui, 0, Some("Coach"), true, false, &mut hits);
            });
            for c in std::mem::take(&mut out.shapes) {
                if let egui::Shape::Text(t) = c.shape {
                    said.push(t.galley.text().to_owned());
                }
            }
            out.drop_without_applying_deltas();
        }
        assert!(
            said.iter().any(|s| s.starts_with("Coach")),
            "the overlay does not say which one it is: {said:?}"
        );
    }

    /// A PLAIN DRAG ON AN OVERLAY, OVER ITS WORDS, MOVES IT.
    ///
    /// Driven through real pointer events: a press over a label and a move past the drag
    /// threshold must send the OS a move. A label is drawn over the press on purpose, because a
    /// selectable label takes the drag for itself and the window would never move.
    ///
    /// WHAT MUTATION MAKES THIS RED: no move surface; text left selectable.
    #[test]
    fn a_plain_drag_over_an_overlays_words_moves_the_window() {
        let ctx = egui::Context::default();
        let mut started = false;
        let frame = |ctx: &egui::Context, events: Vec<egui::Event>, started: &mut bool| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 120.0))),
                events,
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| {
                move_surface(ui, 0);
                ui.label("1,204 dps and a long enough line to press on");
            });
            *started |= out.viewport_output.values().any(|v| {
                v.commands
                    .iter()
                    .any(|c| matches!(c, ViewportCommand::StartDrag))
            });
            out.drop_without_applying_deltas();
        };
        let at = Pos2::new(40.0, 10.0);
        frame(&ctx, vec![egui::Event::PointerMoved(at)], &mut started);
        frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut started,
        );
        for step in 1..=6 {
            frame(
                &ctx,
                vec![egui::Event::PointerMoved(
                    at + Vec2::new(8.0 * step as f32, 4.0 * step as f32),
                )],
                &mut started,
            );
        }
        assert!(started, "dragging an overlay by its words did not move it");
    }

    /// AN OVERLAY OPENS WHERE IT WAS LEFT, AND ITS PLACE IS WRITTEN ONCE, WHEN IT STOPS.
    ///
    /// WHAT MUTATION MAKES THIS RED: the saved place ignored on open; a place written on every
    /// frame of a drag; a place that never gets written.
    #[test]
    fn an_overlay_opens_where_it_was_left_and_remembers_it_once_it_stops() {
        let root = Some(Rect::from_min_size(
            Pos2::new(100.0, 100.0),
            Vec2::new(800.0, 600.0),
        ));
        assert_eq!(
            place_overlay(Some([1500.0, 40.0]), root, 2),
            Pos2::new(1500.0, 40.0)
        );
        assert_eq!(place_overlay(None, root, 0), Pos2::new(196.0, 196.0));

        let t0 = std::time::Instant::now();
        let mut moved = None;
        assert_eq!(
            remember_position(None, &mut moved, [500.0, 300.0], t0),
            None,
            "written mid drag"
        );
        let later = t0 + OVERLAY_SETTLE / 2;
        assert_eq!(
            remember_position(None, &mut moved, [520.0, 310.0], later),
            None,
            "written mid drag"
        );
        let still = later + OVERLAY_SETTLE / 2;
        assert_eq!(
            remember_position(None, &mut moved, [520.0, 310.0], still),
            None,
            "a window still moving a moment ago was written before it had settled"
        );
        let settled = later + OVERLAY_SETTLE;
        assert_eq!(
            remember_position(None, &mut moved, [520.0, 310.0], settled),
            Some([520.0, 310.0]),
            "a window that stopped was never remembered where it stopped"
        );
        assert_eq!(
            remember_position(
                Some([520.0, 310.0]),
                &mut moved,
                [520.5, 310.0],
                settled + OVERLAY_SETTLE
            ),
            None,
            "a window that has not moved is written again"
        );
    }

    /// AN OPEN OVERLAY ALONE BUILDS THE CONTEXT IT DRAWS FROM. See wants_child_cx.
    ///
    /// WHAT MUTATION MAKES THIS RED: wants_child_cx asking only the tool windows, or show
    /// deciding with its own inline test again.
    #[test]
    fn an_open_overlay_alone_builds_the_context_it_draws_from() {
        let mut ov = crate::overlay::Overlay::default_dps();
        ov.open = true;
        assert!(
            wants_child_cx([false; 5], std::slice::from_ref(&ov)),
            "an overlay open with no tool window open gets no context, so it draws nothing"
        );
        ov.open = false;
        assert!(!wants_child_cx([false; 5], std::slice::from_ref(&ov)));
        assert!(wants_child_cx([false, true, false, false, false], &[]));
        let src = include_str!("windows.rs");
        let src = &src[..src.find("mod tests {").expect("the test module")];
        assert!(
            src.contains("wants_child_cx(g.windows.iter().map(|w| w.open), &want)"),
            "the window pass decides whether to build the context without asking about overlays"
        );
    }

    /// THE POP-OUT KEEPS THE VIDEO BETWEEN ITS OWN TICKS.
    ///
    /// THE DEFECT, WALKED. The pop-out is a deferred viewport on its own clock: it repaints
    /// every 100 ms while it hosts the video, and each pass publishes the offer that says
    /// "the video may live here". The root is on ANOTHER clock, at least 10 Hz always
    /// (`hotkeys::POLL_EVERY`), plus a frame per input event. The root read the offer with
    /// `take`, so any root frame that landed between two pop-out passes found the slot empty,
    /// `choose_stage` fell back to the body seat, the surface was moved or hidden, and
    /// `tell_player` told the pop-out it was no longer hosting, which dropped its tick to 1 s.
    /// From there the offer was present on about one root frame in ten and the video was
    /// gone the other nine: the owner's "flashing at a million times a minute" and "broken in
    /// popout mode", both.
    ///
    /// THE RULE THIS HOLDS. An offer is a fact about a window that is OPEN, not about one frame,
    /// so it stays readable until the window closes, and every close path is covered because
    /// the read is gated on the one flag the registry already treats as the truth. A fresh open
    /// starts with no offer, so a handle from a previous window cannot leak into the new one.
    ///
    /// It writes `pip_out` directly, which is what the pop-out pass does at its publish line;
    /// the field is private and this module is inside it. The assertions read through the public
    /// function the root calls.
    #[test]
    fn the_pip_offer_survives_root_frames_between_pop_out_passes_and_dies_with_the_window() {
        let offer = crate::player::PipOffer {
            hwnd: 0x5150,
            carve_px: vec![(1, 2, 3, 4)],
        };
        let mut w = Windows::default();

        /* Nothing open, nothing published: nothing to read. */
        assert_eq!(w.pip_offer(), None, "an offer before any window exists");

        w.open(Tool::Watch);
        lock(&w.inner).pip_out = Some(offer.clone());

        /* THE ROOT FRAMES BETWEEN TWO POP-OUT PASSES. The first read is what shipped; the
         * second is the one that found the slot empty and moved the video. */
        assert_eq!(w.pip_offer(), Some(offer.clone()), "the first root frame");
        assert_eq!(
            w.pip_offer(),
            Some(offer.clone()),
            "a second root frame before the pop-out has run again: this is the frame that hid \
             the video. The offer is a fact about an open window, not about one pass"
        );
        assert_eq!(w.pip_offer(), Some(offer.clone()), "and a third");

        /* THE WINDOW CLOSES, BY THE SAME PATH A HOTKEY TAKES. The offer names a handle that is
         * about to stop existing, and it must not be readable for even one frame after. */
        w.toggle(Tool::Watch);
        assert_eq!(
            w.pip_offer(),
            None,
            "the offer outlived the window it names; the surface would be moved into a dead handle"
        );

        /* A NEW WINDOW STARTS EMPTY, even though nothing cleared the slot explicitly: the old
         * handle is the old window's, and the new one publishes its own on its first pass. */
        w.open(Tool::Watch);
        assert_eq!(
            w.pip_offer(),
            None,
            "a reopened window offered a handle from the window before it"
        );
    }

    #[test]
    fn titles_are_the_ones_the_goal_doc_names() {
        assert_eq!(Tool::Watch.title(), "Broken Stoic");
        /* "Parser" UNTIL THE POP-OUT WAS COMPARED WITH THE RAIL THAT OPENS IT. The goal doc pins
         * one title, Watch's, and this row was never in it; the rail row is "Log Parser" and the
         * crumb over the main window's body prints that, so a window strip saying "Parser" was one
         * destination under two names. `the_pop_out_speaks_the_rails_words` is what holds it. */
        assert_eq!(Tool::Parser.title(), "Log Parser");
        assert_eq!(Tool::Sky.title(), "Plane of Sky");
        assert_eq!(Tool::Lfg(LfgMode::Generic).title(), "Looking for group");
        assert_eq!(Tool::Lfg(LfgMode::Raid).title(), "Looking for raid");
        assert_eq!(Tool::Lfg(LfgMode::Motes).title(), "Looking for motes");
    }

    #[test]
    fn the_three_lfg_modes_share_one_window_and_the_others_have_their_own() {
        assert_eq!(Tool::Lfg(LfgMode::Generic).slot(), Some(Slot::Lfg));
        assert_eq!(Tool::Lfg(LfgMode::Raid).slot(), Some(Slot::Lfg));
        assert_eq!(Tool::Lfg(LfgMode::Motes).slot(), Some(Slot::Lfg));
        assert_eq!(Tool::Companion.slot(), None);
        let mut ids: Vec<ViewportId> = SLOTS.iter().map(|s| s.viewport_id()).collect();
        ids.sort_by_key(|id| id.0.value());
        ids.dedup();
        assert_eq!(ids.len(), SLOTS.len(), "every slot has its own viewport id");
        assert!(ids.iter().all(|id| *id != ViewportId::ROOT));
        for (i, s) in SLOTS.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn open_toggle_pin_state_machine() {
        let mut w = Windows::default();
        for t in [
            Tool::Watch,
            Tool::Parser,
            Tool::Sky,
            Tool::Lfg(LfgMode::Generic),
        ] {
            assert!(!w.is_open(t), "{t:?} starts closed");
        }
        assert!(!w.is_open(Tool::Watch));

        w.open(Tool::Watch);
        assert!(w.is_open(Tool::Watch));
        w.toggle(Tool::Watch);
        assert!(!w.is_open(Tool::Watch));
        w.toggle(Tool::Watch);
        assert!(w.is_open(Tool::Watch));

        /* Pin is independent of open. */
        assert!(!w.is_pinned(Tool::Watch));
        w.pin(Tool::Watch, true);
        assert!(w.is_pinned(Tool::Watch));
        w.toggle(Tool::Watch);
        assert!(w.is_pinned(Tool::Watch), "closing does not unpin");
        w.pin(Tool::Watch, false);
        assert!(!w.is_pinned(Tool::Watch));

        /* LFG: one window, the mode switches. */
        w.open(Tool::Lfg(LfgMode::Raid));
        assert!(w.is_open(Tool::Lfg(LfgMode::Raid)));
        assert!(!w.is_open(Tool::Lfg(LfgMode::Generic)));
        w.toggle(Tool::Lfg(LfgMode::Motes));
        assert!(
            w.is_open(Tool::Lfg(LfgMode::Motes)),
            "a different mode on an open window switches it"
        );
        assert!(!w.is_open(Tool::Lfg(LfgMode::Raid)));
        w.toggle(Tool::Lfg(LfgMode::Motes));
        assert!(
            !w.is_open(Tool::Lfg(LfgMode::Motes)),
            "the same mode on an open window closes it"
        );
        assert!(!w.is_open(Tool::Lfg(LfgMode::Generic)) && !w.is_open(Tool::Lfg(LfgMode::Raid)));
        w.pin(Tool::Lfg(LfgMode::Generic), true);
        assert!(
            w.is_pinned(Tool::Lfg(LfgMode::Motes)),
            "the pin belongs to the window, not the mode"
        );
    }

    #[test]
    fn companion_requests_are_recorded_for_show() {
        let mut w = Windows::default();
        assert!(w.is_open(Tool::Companion), "the main window is always open");
        w.toggle(Tool::Companion);
        assert_eq!(w.companion_req, Some(CompanionReq::Toggle));
        w.open(Tool::Companion);
        assert_eq!(
            w.companion_req,
            Some(CompanionReq::Show),
            "an explicit open overrides a pending toggle"
        );
        w.pin(Tool::Companion, true);
        assert_eq!(w.companion_pin_req, Some(true));
        assert!(
            !w.is_pinned(Tool::Companion),
            "the main window's pin is reported from what was applied, which needs a pass"
        );
    }

    #[test]
    fn summon_toggles_the_companion_and_opens_everything_else() {
        let mut w = Windows::default();
        w.summon(Tool::Companion);
        assert_eq!(
            w.companion_req,
            Some(CompanionReq::Toggle),
            "Ctrl+Alt+G is a toggle"
        );
        w.summon(Tool::Watch);
        assert!(w.is_open(Tool::Watch));
        w.summon(Tool::Watch);
        assert!(
            w.is_open(Tool::Watch),
            "a second Ctrl+Alt+W brings the window forward, it does not close it"
        );
        w.summon(Tool::Lfg(LfgMode::Raid));
        w.summon(Tool::Lfg(LfgMode::Motes));
        assert!(
            w.is_open(Tool::Lfg(LfgMode::Motes)),
            "a different LFG mode switches the one LFG window"
        );
    }

    /// THE POPPED-OUT SKY WINDOW IS A SECOND VIEW OF THE MARKS, NOT A SECOND COPY OF THEM.
    ///
    /// This registry builds a `SkyScreen` of its own for the Sky slot, beside the one the main
    /// window owns. While the two held their own `Marks` each, both read the marks file once and
    /// both wrote their whole copy back over it, so whichever saved second silently ate the
    /// other's mark: see `screens::sky::tests::two_sky_screens_do_not_eat_each_others_marks`,
    /// which is the same defect measured on the bytes.
    ///
    /// This is the reachability half of that fix, and it is the half that can rot: the marks
    /// behave correctly only if the two screens really are one cell, and nothing else in the
    /// program would notice if a change here quietly gave this one its own.
    #[test]
    fn the_popped_out_sky_window_shares_the_main_windows_marks() {
        let main = SkyScreen::default();
        let registry = Windows::default();
        assert!(
            main.marks_cell().is_same(&registry.sky_marks_cell()),
            "the Sky tool window holds its own copy of the marks, so the two windows will eat \
             each other's"
        );
    }

    /// DEFECT: POPPING THE PARSER OUT GAVE YOU A DIFFERENT APPLICATION.
    ///
    /// # WHAT WAS WRONG
    ///
    /// The Parser tool window held a bare `ParserScreen`, whose five views are Kills, Loot,
    /// Fights, Analysis and Overlays. The main window's LOG PARSER destination has five SECTIONS,
    /// and four of them (Dashboards, Live, Reports, Logs) are separate screens reached through
    /// `main::draw_screen`, which no tool window ever calls.
    ///
    /// So four fifths of the destination had NO CODE PATH in the pop-out. The control says `put
    /// this in its own window`; what came up could not show the page you were looking at and
    /// offered no way to reach it. The owner's whole reason for a pop-out is the parser over the
    /// game while he plays, which is exactly when Live and Dashboards matter most.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every section the rail draws under LOG PARSER is a page the pop-out offers, by name, and
    /// the pop-out opens on each of them. Compared against `nav::SECTIONS` rather than against a
    /// list of literals, because two lists of literals agree with each other while both are wrong.
    ///
    /// AND THE POP-OUT MAY OFFER MORE THAN THE RAIL, which is not a hole: the Hunt Journal and the
    /// Loot Journal reach the main window as rail ROWS under MY LEGEND rather than as sections of
    /// the parser, and both are `ParserScreen` in one of its views. They are pages of the
    /// destination that the rail happens to file elsewhere. What must never happen is the other
    /// direction, a rail section with no page in the window.
    ///
    /// AND THE ORDER IS ASSERTED NOW, WHICH IS THE HALF THAT WAS MISSING AND THE HALF THAT WAS
    /// WRONG. Membership was all this asked, so `PAGES` could run Dashboards, Live, Fights,
    /// ANALYSIS, Reports, Logs against a rail running Dashboards, Live, Fights, Reports, Logs,
    /// Analysis and stay green: the same destination handed a reader its pages in two orders
    /// depending on which window he was in. The rail's sections are required to be a PREFIX of
    /// `PAGES`, in the rail's own order, so the two switchers read the same left to right.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping a page from `ParserWindow::PAGES`, swapping any two
    /// of its first seven entries, renaming a section in `nav::SECTIONS` without renaming it here,
    /// or `show_named` refusing a real page.
    /// DEFECT: CLOSING THE PARSER WINDOW THREW AWAY THE NOTE THE READER HAD JUST TYPED.
    ///
    /// # WHY NO CALLER INSIDE THE PAGE COULD HAVE CAUGHT IT
    ///
    /// `screens::analysis` keeps the fight note being typed in its own buffer and writes it to
    /// `Settings::fight_notes` when the reader leaves the field or changes fight. Both of those
    /// need the page to draw AGAIN: `lost_focus` needs the field laid out to see the focus go, and
    /// the repointing runs from `ui`. Closing the window is neither, and nothing draws a frame
    /// after it, so the page cannot notice its own close.
    ///
    /// AND EACH PARSER WINDOW HOLDS ITS OWN `ParserScreen`, so this is not covered by the main
    /// window's `on_exit`: that flushes the App's copy of the page and knows nothing about a tool
    /// window's. The registry is the only thing that knows a tool window is closing.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// A note typed into a pop-out's Analysis page reaches that window's settings when the window
    /// is closed, and the window really does close. The second half is not padding: a `close_window`
    /// that flushed and then forgot to clear `open` would leave a window nothing can shut.
    ///
    /// AND THE PIN IS FORGOTTEN, because reopening builds a NEW viewport at the default level and a
    /// pin that still remembered telling the old one would leave a window at Normal with its glyph
    /// claiming it is pinned. That was the original reason these two lines travelled together, and
    /// a close that dropped it would be this fix breaking the thing it was folded into.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the flush from `close_window`, dropping the
    /// `w.open = false`, or dropping the `w.pin.forget()`.
    #[test]
    fn closing_the_parser_window_writes_out_the_note_being_typed() {
        const KEY: &str = "Wed Jul 15 23:16:50 2026";
        const TYPED: &str = "third pull, adds from the ramp";

        /* A REAL BUT EMPTY LOGS FOLDER, so `Ingest::new` resolves somewhere deterministic instead
         * of walking the usual places and finding the owner's own EverQuest install. */
        let dir = crate::fights::probe::logs_dir("close-window-note");
        let settings = Settings {
            log_dir: Some(dir.clone()),
            ..Settings::default()
        };
        let ingest = Ingest::new(&settings);
        let mut cx = Some(ChildCx {
            live: Status {
                twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
                youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
            },
            settings,
            ingest,
            chat: crate::chat::ChatHandle::idle(),
            data: None,
            data_err: None,
            loading: None,
            attempted_for: None,
        });

        let mut w = Window::new(Slot::Parser);
        w.open = true;
        let Screen::Parser(p) = &mut w.screen else {
            panic!("the parser slot does not hold a parser window");
        };
        /* THE STATE A READER LEAVES BEHIND: text in the buffer, pointed at the fight it is about,
         * and nothing in settings yet. Set through the page's own test door rather than by
         * reaching into private fields from another module. */
        p.parser.seed_note_for_test(KEY, TYPED);
        assert!(
            cx.as_ref()
                .is_some_and(|c| c.settings.fight_notes.is_empty()),
            "the fixture starts with a note already stored, so this would pass without a flush"
        );

        close_window(&mut w, &mut cx);

        assert_eq!(
            cx.as_ref()
                .and_then(|c| c.settings.fight_notes.get(KEY))
                .map(String::as_str),
            Some(TYPED),
            "the note the reader typed was thrown away by closing the window it was typed in"
        );
        assert!(!w.open, "the window did not actually close");
        assert!(
            w.pin.applied().is_none(),
            "the close stopped forgetting the pin, so reopening would show a window at Normal \
             with its glyph claiming it is pinned"
        );
    }

    #[test]
    fn the_pop_out_offers_every_page_the_rail_does() {
        let rail: Vec<&str> = crate::nav::sections_of(crate::nav::ScreenId::Parser)
            .iter()
            .map(|(name, _, _)| *name)
            .collect();
        assert!(
            rail.len() >= 5,
            "the log parser has only {} sections, so this is passing by not looking",
            rail.len()
        );
        assert!(
            ParserWindow::PAGES.len() >= rail.len(),
            "the pop-out lists fewer pages than the rail has sections"
        );
        assert_eq!(
            &ParserWindow::PAGES[..rail.len()],
            rail.as_slice(),
            "the rail's sections are not the pop-out's first pages in the rail's own order, so the \
             same destination lists its pages differently in the two windows and a hand that has \
             learned where one of them is is wrong in the other"
        );

        for name in &rail {
            assert!(
                ParserWindow::PAGES.contains(name),
                "LOG PARSER / {name} is on the rail and has no page in the pop-out, so popping \
                 the parser out loses it: pop-out has {:?}",
                ParserWindow::PAGES
            );
            /* AND IT ACTUALLY OPENS THERE. A name in a list the switcher cannot select is the
             * same hole one level down. */
            let mut w = ParserWindow::default();
            w.show_named(name);
            assert_eq!(
                w.showing(),
                *name,
                "the pop-out lists {name} and will not open on it"
            );
        }

        /* AND NO PAGE IS LISTED TWICE, which would be two rows of the switcher showing one page. */
        let mut ids = ParserWindow::PAGES.to_vec();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "the pop-out lists a page twice");

        /* AND AN UNKNOWN NAME LEAVES IT WHERE IT WAS rather than silently opening the first page,
         * because a pop-out that jumped to Dashboards on a misspelling would be worse than one
         * that did not move. */
        let mut w = ParserWindow::default();
        w.show_named("Logs");
        w.show_named("not a page");
        assert_eq!(w.showing(), "Logs");
    }

    /// DEFECT: THE POP-OUT NAMED PAGES WITH WORDS THE MAIN WINDOW NEVER PRINTS.
    ///
    /// A pop-out is a second view of the SAME destination, so the two windows have to be talkable
    /// about in one sentence: "the Fights page" must mean one thing whichever window a person is
    /// looking at. Two of the nine pages here were called "Kills" and "Loot", and neither string
    /// appears anywhere in `nav::NAV`; the rail rows that draw exactly those two pages are the
    /// Hunt Journal and the Loot Journal. The window's own strip said "Parser" while the crumb over
    /// the main window's body said "Log Parser". Nothing was broken, and a reader comparing the two
    /// windows had no way to tell the same page from a different one.
    ///
    /// IT READS THE WHOLE RAIL AND NOT A LIST OF LITERALS, because two lists of literals agree with
    /// each other while both are wrong. Every row of every heading and every section of every
    /// destination is what the main window is able to print; a page name that is not in that set is
    /// a word this app uses in one window only.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting "Kills" or "Loot" back in `ParserWindow::PAGES`, or
    /// `Tool::Parser::title` back to "Parser".
    #[test]
    fn the_pop_out_speaks_the_rails_words() {
        use crate::nav;
        let mut rail: Vec<&'static str> = Vec::new();
        for (_, rows) in nav::NAV.iter() {
            for (name, id) in rows.iter() {
                rail.push(name);
                for (section, _, _) in nav::sections_of(*id).iter() {
                    rail.push(section);
                }
            }
        }
        /* THE FENCE. A rail this failed to read would be an empty set, and an empty set makes
         * every assertion below vacuous in the direction that passes. */
        for must in ["Log Parser", "Hunt Journal", "Loot Journal", "Fights"] {
            assert!(
                rail.contains(&must),
                "the rail was not read: it does not even carry {must:?}"
            );
        }

        assert_eq!(
            Tool::Parser.title(),
            nav::label(nav::ScreenId::Parser),
            "the parser pop-out's title strip and taskbar entry say one thing and the rail row \
             that opens it says another"
        );

        for page in ParserWindow::PAGES {
            assert!(
                rail.contains(&page),
                "the pop-out offers a page it calls {page:?} and the main window's rail never \
                 prints that word anywhere, so the same page has two names depending on which \
                 window it is in"
            );
        }
    }

    /// EVERY PAGE THE SWITCHER LISTS DRAWS A DIFFERENT THING, AND THE NAMES ARE WHAT DECIDE IT.
    ///
    /// `ParserWindow::ui` used to match on the INDEX into `PAGES`, which is why reordering that
    /// array to the rail's order could not be done safely until this existed: nine numbered arms
    /// against a list whose order had just changed compile, pass every test that reads names, and
    /// open Reports on Analysis for ever. `page_of` is that decision on its own, so it can be
    /// driven with a string.
    ///
    /// THE FIVE PARSER VIEWS ARE SPELLED OUT AGAINST `main::on_section`'s OWN ARMS, so the same
    /// page name cannot mean Analysis in the rail and Overlays in the pop-out.
    ///
    /// WHAT MUTATION MAKES THIS RED: pointing two page names at one body in `page_of` (say
    /// "Reports" at `Page::Logs`), or swapping the `View::Kills` and `View::Loot` arms.
    #[test]
    fn every_page_the_pop_out_lists_draws_its_own_body() {
        use crate::screens::parser::View;
        let bodies: Vec<Page> = ParserWindow::PAGES
            .iter()
            .map(|p| ParserWindow::page_of(p))
            .collect();
        for (i, a) in bodies.iter().enumerate() {
            for (j, b) in bodies.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    a,
                    b,
                    "the pop-out lists {:?} and {:?} as two pages and both draw {a:?}, so one of \
                     the two switcher rows does nothing",
                    ParserWindow::PAGES[i],
                    ParserWindow::PAGES[j]
                );
            }
        }

        /* THE VIEWS, AGAINST THE MAIN WINDOW'S OWN ROUTING. */
        assert_eq!(ParserWindow::page_of("Fights"), Page::Parser(View::Fights));
        assert_eq!(
            ParserWindow::page_of("Analysis"),
            Page::Parser(View::Analysis)
        );
        assert_eq!(
            ParserWindow::page_of("Overlays"),
            Page::Parser(View::Overlays)
        );
        assert_eq!(
            ParserWindow::page_of("Hunt Journal"),
            Page::Parser(View::Kills)
        );
        assert_eq!(
            ParserWindow::page_of("Loot Journal"),
            Page::Parser(View::Loot)
        );

        /* AND THE FALLBACK IS FOR A NAME THAT IS NOT A PAGE, which cannot happen through the
         * switcher and can happen through `open_at`. */
        assert_eq!(
            ParserWindow::page_of("Kills"),
            Page::Parser(View::Fights),
            "a name this window no longer offers must land on the fights table and not on \
             whatever arm happens to be first"
        );
    }

    /// DEFECT: THE PICTURE IN PICTURE CONTROL SAID `put THIS in its own window` AND PUT SOMETHING
    /// ELSE IN A WINDOW.
    ///
    /// `Windows::open` had an arm that set the LFG child screen's mode, because `Tool::Lfg` carries
    /// its mode, and no arm at all for the Parser, because `Tool::Parser` carries nothing. So
    /// standing on LOG PARSER / Fights, or on the Loot Journal, and pressing the control opened a
    /// window on `ParserWindow::default`, which is Dashboards. The owner pops the parser out to
    /// keep ONE page over the game while he plays, so the page is the entire content of the press.
    ///
    /// THE HOTKEY IS THE OTHER HALF AND IT MUST NOT MOVE THE PAGE. `Ctrl+Alt+P` is pressed from
    /// inside the game, not from a page, so it names a window and nothing else; a hotkey that
    /// snapped the window back to Dashboards would be the same defect pointed the other way.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the `Tool::Parser` arm in `Windows::open_at`, or
    /// making `open` pass a page of its own.
    #[test]
    fn popping_out_from_a_page_opens_the_parser_window_there() {
        let mut w = Windows::default();
        assert_eq!(
            w.parser_page(),
            "Dashboards",
            "the pop-out's own first page, which is what every press used to get"
        );

        w.open_at(Tool::Parser, Some("Fights"));
        assert_eq!(
            w.parser_page(),
            "Fights",
            "the control was pressed from LOG PARSER / Fights and the window came up elsewhere"
        );

        /* AN ALREADY OPEN WINDOW IS TAKEN TO THE NEW PAGE, which is what the control's other
         * state has to mean: pressed from a different page it reads `Focus window`, and focusing a
         * window that is showing something else is the same broken promise one step on. */
        w.open_at(Tool::Parser, Some("Loot Journal"));
        assert_eq!(w.parser_page(), "Loot Journal");

        /* A HOTKEY NAMES A WINDOW AND NOT A PAGE. */
        w.open(Tool::Parser);
        assert_eq!(
            w.parser_page(),
            "Loot Journal",
            "Ctrl+Alt+P moved the page out from under the owner"
        );
        w.summon(Tool::Parser);
        assert_eq!(w.parser_page(), "Loot Journal");

        /* AND A NAME THAT IS NOT A PAGE LEAVES IT ALONE rather than snapping to the first one,
         * which is exactly what an index would have done silently. "Kills" was a page of this
         * window until it was renamed to the rail's own word. */
        w.open_at(Tool::Parser, Some("Kills"));
        assert_eq!(
            w.parser_page(),
            "Loot Journal",
            "a name this window no longer offers opened a different page in silence"
        );

        /* THE PAGE BELONGS TO THE PARSER WINDOW AND TO NO OTHER. */
        w.open_at(Tool::Sky, Some("Fights"));
        assert_eq!(w.parser_page(), "Loot Journal");
        assert!(w.is_open(Tool::Sky));
    }

    /// THE KEY A WINDOW'S PREFERENCES ARE FILED UNDER IS THE HOTKEY TABLE'S OWN WORD FOR IT.
    ///
    /// `Settings` now carries two maps keyed by window: `hotkeys`, filed under `hotkeys::DEFAULTS`
    /// row ids, and `windows`, filed under `Slot::id`. One file spelling one window two ways is a
    /// file a person cannot read and cannot hand-edit, and nothing else in the program would
    /// notice: both maps tolerate any key at all, because an unknown one is another build's.
    ///
    /// WHAT MUTATION MAKES THIS RED: renaming any arm of `Slot::id`, or giving two slots one key.
    #[test]
    fn every_windows_settings_key_is_the_hotkey_tables_own_word() {
        let mut ids: Vec<&str> = SLOTS.iter().map(|s| s.id()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            n,
            "two windows file their preferences under one settings key, so one of them wears the \
             other's pin"
        );
        for slot in SLOTS {
            let from_hotkeys = crate::hotkeys::DEFAULTS
                .iter()
                .find(|r| !r.alias && r.tool.slot() == Some(slot))
                .map(|r| r.id);
            assert_eq!(
                Some(slot.id()),
                from_hotkeys,
                "{slot:?} is `{}` in Settings::windows and `{from_hotkeys:?}` in Settings::hotkeys",
                slot.id()
            );
        }
    }

    #[test]
    fn only_sky_loads_the_snapshot() {
        assert!(Slot::Sky.needs_snapshot());
        assert!(
            !Slot::Parser.needs_snapshot(),
            "the parser's roster comes through the ingest, not Cx::data"
        );
        assert!(!Slot::Watch.needs_snapshot());
        assert!(!Slot::Lfg.needs_snapshot());
    }

    /// EVERY WINDOW IN THIS FILE USES THE ONE PIN, AND NOTHING HERE KEEPS ITS OWN COPY.
    ///
    /// THE TEST THAT STOOD HERE WENT WITH THE CODE IT WAS ABOUT. It asserted `level(true)` is
    /// `AlwaysOnTop`, and `level` is `crate::pin`'s now, where the same assertion lives as
    /// `pin::tests::on_top_is_always_on_top_and_off_is_normal`. Leaving a copy here would be a
    /// second test of somebody else's function.
    ///
    /// WHAT THIS FILE STILL OWNS is that it has not gone back to hand rolling the idiom. That is
    /// what nearly happened four times over: the reconcile triple was written out once per pin
    /// site, and a fifth window would have copied a fourth. So this reads the source, after
    /// cutting the tests off, and refuses to find a `WindowLevel` command built anywhere in it.
    ///
    /// IT IS FENCED, because a rule that matched nothing would pass forever: the pin type has to
    /// be reached, and reached more than once, or there is nothing here to have centralised.
    #[test]
    fn no_window_here_sets_its_own_level_and_they_all_go_through_the_pin() {
        let src = include_str!("windows.rs");
        let code = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
        let code: String = code
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !(t.starts_with("//") || t.starts_with("/*") || t.starts_with("*"))
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(
            code.matches("ViewportCommand::WindowLevel").count(),
            0,
            "a window in this file builds its own level command again; `pin::Pin::reconcile` is \
             where that lives, and the copies are what left the main window's pin persisted and \
             the tool windows' not"
        );
        let reconciles = code.matches(".reconcile(").count();
        assert!(
            reconciles >= 2,
            "only {reconciles} call(s) to the pin; if this file stopped using it there would be \
             nothing for the rule above to be about"
        );
    }

    #[test]
    fn no_dashes_in_titles_or_messages() {
        let mut words: Vec<String> = vec![IN_THE_MAIN_WINDOW.to_owned()];
        for live in [Some(true), Some(false), None] {
            words.push(pip_words(live));
        }
        for t in [
            Tool::Companion,
            Tool::Watch,
            Tool::Parser,
            Tool::Sky,
            Tool::Lfg(LfgMode::Generic),
            Tool::Lfg(LfgMode::Raid),
            Tool::Lfg(LfgMode::Motes),
        ] {
            words.push(t.title().to_owned());
        }
        for w in words {
            assert!(
                !w.contains('\u{2014}') && !w.contains('\u{2013}'),
                "a dash reached the screen: {w:?}"
            );
        }
    }

    /* ------------------------------------------------ the picture in picture -- */

    /// One 16 by 9 texture the test owns, so the mesh read back out of a frame can be identified
    /// by its id and its size is arithmetic rather than a fixture.
    ///
    /// IT IS 480 BY 270 AND NOT A THUMBNAIL, AND THAT IS NOT AN ARBITRARY NUMBER. `fit_into` will
    /// not blow a picture up past `MAX_ART_UPSCALE`, which is 2, because a 300 pixel avatar drawn
    /// four times its size is a smear rather than a picture. A small fixture would therefore never
    /// fill any window this size, and a test asserting that it did would be asserting against the
    /// app's own rule instead of against the window. The real artwork arrives downscaled to
    /// `channel_art::MAX_ART_W`, which is 960, so a picture wider than the window is the case that
    /// actually happens and this is a fixture of it.
    fn test_art(ctx: &egui::Context) -> crate::channel_art::Art {
        const W: usize = 480;
        const H: usize = 270;
        let image = egui::ColorImage::from_rgba_unmultiplied([W, H], &vec![255u8; W * H * 4]);
        crate::channel_art::Art {
            kind: crate::channel_art::Kind::TwitchOffline,
            texture: ctx.load_texture("pip-test-art", image, egui::TextureOptions::LINEAR),
            px: egui::vec2(W as f32, H as f32),
        }
    }

    fn themed() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx
    }

    /// Everything one pass of a window painted, flattened. `Shape::Vec` is nested by egui and a
    /// test that read only the top level would find nothing and pass, which is the shape of every
    /// reachability failure this tree has had.
    struct Painted {
        shapes: Vec<egui::Shape>,
        hits: ChipHits,
        inside: bool,
    }

    impl Painted {
        /// Every run of text on screen, in paint order.
        fn words(&self) -> Vec<String> {
            self.shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect()
        }

        /// The bounds of every mesh drawn with one texture: the picture, if it was painted.
        fn pictures(&self, id: egui::TextureId) -> Vec<Rect> {
            self.shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Mesh(m) if m.texture_id == id => Some(m.calc_bounds()),
                    _ => None,
                })
                .collect()
        }

        /// Every filled rectangle of one colour.
        fn fills(&self, col: Color32) -> Vec<Rect> {
            self.shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(r) if r.fill == col => Some(r.rect),
                    _ => None,
                })
                .collect()
        }

        /// The corner chips: the square grounds the two glyph buttons sit on, painted only while
        /// the pointer is inside the window. There is no bar any more, so these are the whole of
        /// this window's chrome ground.
        ///
        /// BOTH FILLS ARE READ because a chip under the pointer swaps its ground for `PANEL_2`,
        /// and a helper that knew only the resting colour would report one chip whenever the
        /// other one was hovered, which is every pass that clicks something.
        fn chips(&self) -> Vec<Rect> {
            let mut v: Vec<Rect> = self
                .fills(INK.gamma_multiply(0.72))
                .into_iter()
                .chain(self.fills(PANEL_2))
                .filter(|r| (r.width() - CHIP_W).abs() < 0.5 && (r.height() - CHIP_W).abs() < 0.5)
                .collect();
            v.sort_by(|a, b| a.left().total_cmp(&b.left()));
            v
        }

        /// Every ground this window's chrome and captions paint, whatever its shape. The anti-bar
        /// test measures WIDTH against this, so a band that came back in either scrim colour is
        /// caught rather than only one of them.
        /// The two corner grips: the 16 point square grounds, told from the 22 point chips by size.
        fn grips(&self) -> Vec<Rect> {
            let mut v: Vec<Rect> = self
                .fills(INK.gamma_multiply(0.72))
                .into_iter()
                .chain(self.fills(PANEL_2))
                .filter(|r| {
                    (r.width() - PIP_GRIP).abs() < 0.5 && (r.height() - PIP_GRIP).abs() < 0.5
                })
                .collect();
            v.sort_by(|a, b| a.left().total_cmp(&b.left()));
            v
        }

        fn scrims(&self) -> Vec<Rect> {
            self.fills(INK.gamma_multiply(0.72))
                .into_iter()
                .chain(self.fills(PANEL_2))
                .collect()
        }
    }

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

    /// One real headless pass of the picture in picture body, on a context the CALLER owns.
    ///
    /// THE CONTEXT IS A PARAMETER BECAUSE A CLICK NEEDS TWO PASSES: egui resolves interaction
    /// against the widget rects registered on the previous pass, so a press and release in the
    /// first frame a widget exists reaches nothing. Same reason `titlebar::tests::strip_pass`
    /// takes one.
    fn pip_pass(
        ctx: &egui::Context,
        size: Vec2,
        live: Option<bool>,
        art: Option<&crate::channel_art::Art>,
        pinned: bool,
        events: Vec<egui::Event>,
    ) -> Painted {
        pip_pass_seated(ctx, size, live, art, pinned, events, false, None)
    }

    /// The same pass, told whether the video is in this window and what the OS says about the
    /// pointer.
    ///
    /// THE POINTER IS A PARAMETER BECAUSE IT IS ONE IN THE REAL CODE TOO. While a native child
    /// window covers this rectangle egui sees no pointer over it at all, so the hover comes from
    /// `WindowFromPoint` and is handed in. A headless test has no windows, so handing it in here is
    /// the same substitution the app makes and not a hole cut for the test`s convenience.
    #[allow(clippy::too_many_arguments)]
    fn pip_pass_seated(
        ctx: &egui::Context,
        size: Vec2,
        live: Option<bool>,
        art: Option<&crate::channel_art::Art>,
        pinned: bool,
        events: Vec<egui::Event>,
        hosting: bool,
        os_pointer: Option<bool>,
    ) -> Painted {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let mut hits = ChipHits::default();
        let mut inside = false;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(INK))
                .show(ui, |ui| {
                    inside = pip(
                        ui,
                        live,
                        art,
                        pinned,
                        true,
                        PipSeat {
                            hosting,
                            os_pointer,
                        },
                        &mut hits,
                    )
                    .0;
                });
        });
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        Painted {
            shapes: flat,
            hits,
            inside,
        }
    }

    /// The centre of a corner chip, counted inward from the right edge: 0 is close, 1 is the pin.
    ///
    /// ONE PLACE DOES THIS ARITHMETIC. Four tests click these two spots, and a layout change that
    /// moved the chips would otherwise have to be chased through four copies of the same three
    /// constants, which is how a test ends up clicking empty picture and asserting nothing.
    fn chip_at(size: Vec2, from_right: f32) -> Pos2 {
        Pos2::new(
            size.x - CHIP_EDGE - (CHIP_W + CHIP_GAP) * from_right - CHIP_W * 0.5,
            CHIP_EDGE + CHIP_W * 0.5,
        )
    }

    fn click_at(at: Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    /// THE HOVER RULE, WHICH IS THE HEART OF WHAT A PICTURE IN PICTURE IS.
    ///
    /// Chrome that is always visible is not minimal chrome, it is a small toolbar, and a toolbar
    /// over a picture is exactly what this window used to be. With the pointer somewhere else,
    /// this window is the picture and its one line and NOTHING ELSE. The chips are the ground each
    /// control sits on, so their absence is the absence of both of them.
    ///
    /// THE FENCE MATTERS AS MUCH AS THE ABSENCE. A `pip` that painted nothing at all would satisfy
    /// "no bar", and that is the failure this tree keeps finding, so the picture is required to
    /// have been painted in the same pass.
    #[test]
    fn the_picture_carries_no_chrome_until_the_pointer_is_in_the_window() {
        let ctx = themed();
        let art = test_art(&ctx);
        let size = Vec2::new(480.0, 270.0);

        let cold = pip_pass(&ctx, size, Some(false), Some(&art), false, Vec::new());
        assert!(
            !cold.inside,
            "no pointer event was sent and the window still thinks the pointer is in it"
        );
        assert_eq!(
            cold.pictures(art.texture.id()).len(),
            1,
            "the picture is not painted at all, so the absences below prove nothing"
        );
        assert!(
            cold.chips().is_empty(),
            "the chrome is painted with the pointer outside the window: {:?}",
            cold.chips()
        );

        let warm = pip_pass(
            &ctx,
            size,
            Some(false),
            Some(&art),
            false,
            vec![egui::Event::PointerMoved(Pos2::new(240.0, 135.0))],
        );
        assert!(warm.inside, "the pointer is in the middle of the window");
        assert_eq!(
            warm.chips().len(),
            2,
            "the pointer is inside and the two chips did not both appear: {:?}",
            warm.chips()
        );
    }

    /// THE VIDEO WITHDRAWS FROM THE RECTANGLES THE CHROME IS ACTUALLY PAINTED IN.
    ///
    /// This is the whole of the notch, and the failure it guards has no symptom you could see in a
    /// screenshot of a healthy machine: a hole cut a few points away from the chip it is for leaves
    /// a control painted underneath a video, which is a control that does not exist.
    ///
    /// IT READS THE PAINTED SHAPES AND NOT THE FUNCTION THAT PLACES THEM, and the first cut of this
    /// test did the opposite. It built its expectation by calling `chip_rects` and
    /// `pip_move_rect`, the same functions `pip_holes` calls, so moving a control moved the hole AND
    /// the expectation together and the test stayed green: a mutation that shifted the move grip by
    /// seven points survived it. Comparing the holes against the grounds the chrome actually PAINTED
    /// is the only version of this assertion that can fail.
    ///
    /// CLOSE IS FIRST AND THAT ORDER IS LOAD BEARING. `Player::probe_notch` tests the first shape's
    /// centre to find out whether a carved hole passes the pointer through at all, and close is the
    /// one control that may never become unreachable, so close is the one that gets probed.
    #[test]
    fn the_video_withdraws_from_the_rectangles_the_chrome_is_painted_in() {
        let size = Vec2::new(480.0, 270.0);
        let rect = Rect::from_min_size(Pos2::ZERO, size);

        /* The chips as the window really paints them, hovered so they exist at all. */
        let ctx = themed();
        let art = test_art(&ctx);
        let drawn = pip_pass(
            &ctx,
            size,
            Some(false),
            Some(&art),
            false,
            vec![egui::Event::PointerMoved(Pos2::new(240.0, 135.0))],
        );
        let painted = drawn.chips();
        assert_eq!(painted.len(), 2, "the chips were not painted: {painted:?}");

        for ppp in [1.0_f32, 1.5, 2.0] {
            let holes = pip_holes(rect, ppp);
            assert_eq!(
                holes.len(),
                4,
                "at {ppp}x: two chips and two grips is four shapes"
            );

            /* EVERY PAINTED CHIP HAS A HOLE, at the same place, in physical pixels. */
            for c in &painted {
                let want = crate::player::hwnd::to_physical(*c, ppp);
                assert!(
                    holes.contains(&want),
                    "at {ppp}x a chip is painted at {c:?} ({want:?} physical) and the video does \
                     not withdraw from it, so that control is under the video: {holes:?}"
                );
            }

            /* CLOSE IS THE OUTERMOST CHIP AND THE FIRST SHAPE. `chips()` sorts left to right, so
             * the last of them is the one in the corner. */
            let close = crate::player::hwnd::to_physical(painted[painted.len() - 1], ppp);
            assert_eq!(
                holes[0], close,
                "at {ppp}x close is not the first carved shape, so the notch probe would test the \
                 wrong control and an unreachable close would go undetected"
            );

            /* AND EVERY SHAPE IS INSIDE THE CLIENT AREA. A hole outside it clips nothing and would
             * fail silently. */
            let (w, h) = (size.x * ppp, size.y * ppp);
            for (i, (l, t, r, b)) in holes.iter().enumerate() {
                assert!(
                    *l >= 0 && *t >= 0 && (*r as f32) <= w + 0.5 && (*b as f32) <= h + 0.5,
                    "at {ppp}x shape {i} is {:?}, outside a {w} by {h} client area",
                    (l, t, r, b)
                );
                assert!(*r > *l && *b > *t, "at {ppp}x shape {i} has no area");
            }
        }

        /* AND THE MOVE GRIP, WHICH ONLY EXISTS WHILE THE VIDEO IS HERE and so cannot be read from
         * the pass above. This is the assertion a mutation caught missing: shifting the grip by
         * seven points moved the hole and the expectation together while both came from the same
         * function, and it survived. Read from what is PAINTED, it cannot. */
        let ctx = themed();
        let art = test_art(&ctx);
        let hosted = pip_pass_seated(
            &ctx,
            size,
            Some(true),
            Some(&art),
            false,
            Vec::new(),
            true,
            Some(true),
        );
        let grips = hosted.grips();
        assert_eq!(
            grips.len(),
            1,
            "the move grip is the one grip this pass paints; the resize grip is an Area on the \
             context and is asserted by sharing `pip_resize_rect` with it: {grips:?}"
        );
        for ppp in [1.0_f32, 1.5, 2.0] {
            let want = crate::player::hwnd::to_physical(grips[0], ppp);
            assert!(
                pip_holes(rect, ppp).contains(&want),
                "at {ppp}x the move grip is painted at {:?} and the video does not withdraw from \
                 it, so the window cannot be moved while it is playing",
                grips[0]
            );
        }

        /* THE TWO GRIPS ARE THE OTHER TWO SHAPES, and they are opposite corners rather than a pair
         * of anything: resize bottom right, move bottom left. A version of this that only counted
         * four shapes would pass on two holes cut in the same place. */
        let holes = pip_holes(rect, 1.0);
        let corners: Vec<(i32, i32)> = holes.iter().map(|(l, t, _, _)| (*l, *t)).collect();
        assert_eq!(
            corners.len(),
            corners
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "two shapes are cut in the same place: {holes:?}"
        );
    }

    /// WHILE THE VIDEO IS HERE THIS WINDOW PAINTS NO PICTURE, AND ITS POINTER COMES FROM THE OS.
    ///
    /// TWO CLAIMS, AND BOTH ARE ABOUT THE SAME OS FACT. A native child window is composited over
    /// everything egui puts in its rectangle and takes the pointer over every pixel of it.
    ///
    /// SO NO PICTURE. Artwork drawn now would be invisible in the ordinary case and, worse, would
    /// show through the carved holes in the good one, which is a corner of an offline screen peeping
    /// out of a live stream. The holes are for the app's own ground and the chrome standing on it.
    ///
    /// AND SO THE HOVER IS HANDED IN. `rect_contains_pointer` answers false over the whole window
    /// once the video is there, so a chrome gated on it would never appear again and the close chip
    /// would be gone for good. The substitution is `WindowFromPoint`, and this drives both answers
    /// to prove the gate still works in both directions rather than being stuck open.
    #[test]
    fn hosting_paints_no_picture_and_takes_its_pointer_from_the_os() {
        let size = Vec2::new(480.0, 270.0);

        /* The fence first: the same fixture, NOT hosting, really does paint a picture, so the
         * absence below is a change and not an empty pass. */
        let ctx = themed();
        let art = test_art(&ctx);
        let docked = pip_pass(&ctx, size, Some(false), Some(&art), false, Vec::new());
        assert_eq!(
            docked.pictures(art.texture.id()).len(),
            1,
            "the docked window stopped painting the picture, so nothing below is measured"
        );

        for (name, pointer, chips) in [
            ("the hand is away", false, 0),
            ("the hand is on it", true, 2),
        ] {
            let ctx = themed();
            let art = test_art(&ctx);
            let drawn = pip_pass_seated(
                &ctx,
                size,
                Some(true),
                Some(&art),
                false,
                Vec::new(),
                true,
                Some(pointer),
            );
            assert!(
                drawn.pictures(art.texture.id()).is_empty(),
                "{name}: the picture is painted under the video, where it can only leak through the \
                 carved holes"
            );
            assert!(
                drawn.words().is_empty(),
                "{name}: a line is painted under the video: {:?}",
                drawn.words()
            );
            assert_eq!(
                drawn.inside, pointer,
                "{name}: the window did not take the OS's answer about the pointer"
            );
            assert_eq!(
                drawn.chips().len(),
                chips,
                "{name}: expected {chips} chips and the hover gate disagreed"
            );
        }
    }
    /// THE CHROME IS TWO CHIPS IN A CORNER AND NEVER A BAND ACROSS AN EDGE.
    ///
    /// This is the regression the owner caught by eye and no test held. The controls were on a 26
    /// point scrim spanning the full width of the window, pinned to the top; every test passed,
    /// because every test asked WHETHER the chrome appeared on hover and none asked what SHAPE it
    /// was. A full width band at an edge is a title bar whatever it is called, and this window is
    /// defined by not having one.
    ///
    /// CHROME IS DEFINED AS WHAT THE HOVER ADDS, WHICH IS THE ONLY HONEST WAY TO MEASURE IT. The
    /// first cut of this test read every dark ground in the window and demanded each be narrow.
    /// That is not a rule about chrome: the caption's own scrim wraps to 232 points in a 320 point
    /// window and tripped it, and a test rewritten to let that through by name would have been
    /// weakened to fit the failure rather than to fit the intent. So the resting window is painted
    /// first and the hovered one second, and the assertion is on the DIFFERENCE. Everything the
    /// window draws when nobody is touching it, the picture and its one line, is by definition not
    /// chrome and is not measured here; the two chips are the entire delta.
    ///
    /// THE DELTA IS ALSO ASSERTED NON-EMPTY, because a `pip` that had lost its chrome altogether
    /// would satisfy every width rule below and is the exact failure this tree keeps finding.
    #[test]
    fn the_chrome_is_never_a_band_across_the_window() {
        for size in [Vec2::new(480.0, 270.0), Vec2::new(320.0, 200.0)] {
            for live in [Some(false), Some(true), None] {
                /* A FRESH CONTEXT PER CASE. egui remembers where the pointer was: a context that
                 * hovered on the previous iteration still thinks the pointer is in the window on
                 * a pass carrying no events, so a shared one made every resting pass a hovered
                 * one and the delta below empty. */
                let ctx = themed();
                let art = test_art(&ctx);
                let case = format!("{size:?} {live:?}");
                let cold = pip_pass(&ctx, size, live, Some(&art), false, Vec::new());
                assert!(
                    !cold.inside,
                    "{case}: no pointer was sent and it thinks one is inside"
                );

                let at = Pos2::new(size.x * 0.5, size.y * 0.5);
                let warm = pip_pass(
                    &ctx,
                    size,
                    live,
                    Some(&art),
                    false,
                    vec![egui::Event::PointerMoved(at)],
                );
                assert!(
                    warm.inside,
                    "{case}: the pointer is in the middle of the window"
                );

                /* What hovering ADDED: every ground in the warm pass that the resting pass did not
                 * already paint. Matched by geometry, because the same caption is painted in both
                 * and is not this test's business. */
                let resting = cold.scrims();
                let added: Vec<Rect> = warm
                    .scrims()
                    .into_iter()
                    .filter(|w| {
                        !resting.iter().any(|c| {
                            (c.min - w.min).length() < 0.5 && (c.max - w.max).length() < 0.5
                        })
                    })
                    .collect();

                assert_eq!(
                    added.len(),
                    2,
                    "{case}: hovering added {} grounds and the chrome is two chips; a delta of \
                     zero would pass every width rule below with no chrome at all: {added:?}",
                    added.len()
                );
                for r in &added {
                    assert!(
                        r.width() <= size.x * 0.5,
                        "{case}: the hover added a ground {} wide across a {} wide window, which \
                         is the title bar this window may not have: {r:?}",
                        r.width(),
                        size.x
                    );
                    /* A NUMBER THIS TEST OWNS, not `CHIP_EDGE`. Written against the layout's own
                     * constant this could not fail: setting it to zero would move the chrome
                     * onto the edge and lower the bar it is measured against by the same
                     * amount. */
                    assert!(
                        r.top() >= 4.0 && r.right() <= size.x - 4.0,
                        "{case}: the hover added a ground flush to an edge, which is what a bar \
                         is: {r:?}"
                    );
                }
            }
        }
    }

    /// THE CHIPS SIT IN THE TOP RIGHT CORNER AND LEAVE THE REST OF THE WINDOW ALONE.
    ///
    /// Two squares, side by side, inset from the top and right edges, with close outermost so it
    /// is where a hand goes for it. The picture is what everything else is, so the chips are also
    /// required to occupy a genuinely small share of it: a control that grew to fill the corner
    /// would satisfy the width rule above and still be the chrome the owner refused.
    ///
    /// IT ASSERTS AN ABSOLUTE FLOOR AS WELL AS THE CONSTANT, BECAUSE THE CONSTANT ALONE PROVED
    /// NOTHING. Every inset assertion here was written as `(c.top() - CHIP_EDGE).abs() < 0.5`,
    /// which reads well and cannot fail: `CHIP_EDGE` is what the code lays the chip out with, so
    /// setting it to zero moves the chip AND the expectation together and the test stays green
    /// on chrome welded to the corner. A mutation run is what said so. The constant checks the
    /// LAYOUT is the one the module describes; the floor checks the DESIGN, which is that this
    /// chrome floats on the picture and never touches an edge, and that one is a number this
    /// test owns.
    #[test]
    fn the_chips_are_squares_in_the_top_right_corner() {
        let ctx = themed();
        let art = test_art(&ctx);
        let size = Vec2::new(480.0, 270.0);
        let drawn = pip_pass(
            &ctx,
            size,
            Some(false),
            Some(&art),
            false,
            vec![egui::Event::PointerMoved(Pos2::new(240.0, 135.0))],
        );
        let chips = drawn.chips();
        assert_eq!(chips.len(), 2, "there should be two chips: {chips:?}");
        let (pin, close) = (chips[0], chips[1]);
        assert!(
            (close.right() - (size.x - CHIP_EDGE)).abs() < 0.5,
            "close is not inset from the right edge by {CHIP_EDGE}: {close:?}"
        );
        /// The least a floating control may stand off an edge before it reads as attached to it.
        /// Owned by this test, not by the layout, which is the whole point of it.
        const FLOATS_BY: f32 = 4.0;
        for c in chips.iter() {
            assert!(
                (c.top() - CHIP_EDGE).abs() < 0.5,
                "a chip is not inset from the top edge by {CHIP_EDGE}: {c:?}"
            );
            assert!(
                c.top() >= FLOATS_BY && c.right() <= size.x - FLOATS_BY,
                "a chip is welded to the window edge; this chrome floats on the picture: {c:?}"
            );
        }
        assert!(
            (close.left() - pin.right() - CHIP_GAP).abs() < 0.5,
            "the chips are not {CHIP_GAP} apart, so they read as one bar: {pin:?} {close:?}"
        );
        let covered: f32 = chips.iter().map(|c| c.area()).sum();
        assert!(
            covered < size.x * size.y * 0.02,
            "the chrome covers {covered} of a {} point window, which is not a corner any more",
            size.x * size.y
        );
    }

    /// THE TWO CONTROLS ARE REACHABLE, AND THEY ARE THE ONLY TWO.
    ///
    /// Each is clicked where the previous pass painted it, and the flag the registry reads is the
    /// assertion. A control drawn in the wrong place, or one whose click is dropped on the floor,
    /// fails here; `draw_child` is what turns each flag into the window level and the open flag,
    /// and `the_pip_pin_and_close_reach_the_registry` holds that half.
    ///
    /// AND THE PICTURE IS NOT A CONTROL, WHICH IS MEASURED IN FOUR PLACES AND NOT ONE. The picture
    /// is the drag surface, so a press anywhere on it must report nothing at all. The middle is
    /// the obvious spot, but the corner where the chrome USED to live is the one that matters:
    /// the old top left held Check now, and a hit rect left behind there would be an invisible
    /// button on the artwork that nothing in this file draws.
    #[test]
    fn the_hover_chrome_is_a_pin_and_a_close_and_nothing_else() {
        let size = Vec2::new(480.0, 270.0);
        let spots: [(&str, Pos2, &[&str]); 6] = [
            ("close", chip_at(size, 0.0), &["close"]),
            ("pin", chip_at(size, 1.0), &["pin"]),
            ("the middle", Pos2::new(size.x / 2.0, size.y / 2.0), &[]),
            ("the old Check now spot", Pos2::new(30.0, 13.0), &[]),
            ("the top left corner", Pos2::new(4.0, 4.0), &[]),
            ("where a third chip would be", chip_at(size, 2.0), &[]),
        ];
        for (name, at, want) in spots {
            let ctx = themed();
            let art = test_art(&ctx);
            /* A press and a release in the first frame a widget exists reaches nothing, so the
             * first pass is what registers the rects and the second is what clicks them. */
            let first = pip_pass(
                &ctx,
                size,
                Some(false),
                Some(&art),
                false,
                vec![egui::Event::PointerMoved(at)],
            );
            assert!(first.inside, "{name}: the pointer is inside the window");
            let hit = pip_pass(&ctx, size, Some(false), Some(&art), false, click_at(at));
            let fired: Vec<&str> = [("pin", hit.hits.pin), ("close", hit.hits.close)]
                .into_iter()
                .filter(|(_, on)| *on)
                .map(|(n, _)| n)
                .collect();
            assert_eq!(
                fired, want,
                "a click on {name} at {at:?} reported {fired:?} and should have reported {want:?}"
            );
        }
    }

    /// THE PICTURE FILLS THE WINDOW, EDGE TO EDGE, WITH THE ASPECT KEPT.
    ///
    /// A picture in picture IS the picture: there is no margin, no strip and no band around it,
    /// and what is left over is letterboxed on the app's own ground rather than cropped, because
    /// cropping the streamer's own artwork cuts the streamer's own artwork. The 96 by 54 texture
    /// is 16 by 9, so in a 16 by 9 window it reaches all four edges exactly, and in a window that
    /// is not, it is centred and the bands are equal.
    ///
    /// IT IS DRIVEN OVER ALL THREE STATES BECAUSE THERE ARE TWO PAINT PATHS. Offline goes through
    /// `screens::watch::paint_art`, which is the main window's own function; live and unpolled
    /// cannot, because that function always writes OFFLINE, so they go through this module's own
    /// four lines. Measuring one would leave the other free to inset, stretch or lose the picture
    /// altogether, and the live case is the one a person is most likely to be looking at.
    #[test]
    fn the_picture_fills_the_window_and_keeps_its_aspect() {
        let ctx = themed();
        let art = test_art(&ctx);
        for live in [Some(false), Some(true), None] {
            for size in [Vec2::new(480.0, 270.0), Vec2::new(320.0, 200.0)] {
                let case = format!("live={live:?} {size:?}");
                let drawn = pip_pass(&ctx, size, live, Some(&art), false, Vec::new());
                let shown = drawn.pictures(art.texture.id());
                assert_eq!(shown.len(), 1, "{case}: the picture is painted once");
                let at = shown[0];
                let window = Rect::from_min_size(Pos2::ZERO, size);
                assert!(
                    (at.center() - window.center()).length() < 0.5,
                    "{case}: the picture is not centred: {at:?}"
                );
                assert!(
                    (at.width() / at.height() - 16.0 / 9.0).abs() < 0.01,
                    "{case}: the picture was stretched to {at:?}"
                );
                /* One of the two axes has to touch, or there is a border on every side and the
                 * picture is not filling anything. */
                let touches =
                    (at.width() - size.x).abs() < 0.5 || (at.height() - size.y).abs() < 0.5;
                assert!(touches, "{case}: the picture is inset at {at:?}");
                assert!(
                    at.width() <= size.x + 0.5 && at.height() <= size.y + 0.5,
                    "{case}: the picture runs off the window at {at:?}"
                );
            }
        }
    }

    /// OFFLINE IS LAID ON THE ARTWORK, CENTRED, WITH NO CAPTION, AND IT IS THE MAIN WINDOW'S OWN
    /// RULE RATHER THAN A SECOND ONE.
    ///
    /// `pip` calls `screens::watch::paint_art` for this case and nothing else, so the word, the
    /// scrim under it and the absence of a caption are that function's and cannot drift from what
    /// the main window's folio does. What this asserts is the WIRING: that the picture path
    /// really reaches it, and that the one word on screen is that one.
    #[test]
    fn offline_lays_the_word_on_the_artwork_and_says_nothing_else() {
        let ctx = themed();
        let art = test_art(&ctx);
        let size = Vec2::new(480.0, 270.0);
        let drawn = pip_pass(&ctx, size, Some(false), Some(&art), false, Vec::new());
        let words = drawn.words();
        assert_eq!(
            words.len(),
            1,
            "the offline picture carries one word and one only: {words:?}"
        );
        assert_eq!(
            words[0], "OFFLINE",
            "the word on the artwork is not the main window's: {words:?}"
        );
        assert!(
            !words
                .iter()
                .any(|w| *w == crate::channel_art::Kind::TwitchOffline.caption()),
            "a caption came back under the picture: {words:?}"
        );
    }

    /// A LIVE CHANNEL IS TOLD WHERE ITS STREAM IS, BECAUSE THIS WINDOW CANNOT HOLD ONE.
    ///
    /// `wry` needs a `HasWindowHandle` and only the root window has one, so a person who pops out
    /// a LIVE channel gets a still picture while the pill two windows away says LIVE. That is a
    /// contradiction the window has to resolve in words, and this is the one sentence it carries.
    ///
    /// AND THE WORD IS THE PILL'S OWN. `titlebar::dot_of(..).word()` is what every pill in the app
    /// prints, so this line and the pill cannot say two things about one channel.
    #[test]
    fn a_live_channel_is_told_the_stream_is_in_the_main_window() {
        let ctx = themed();
        let art = test_art(&ctx);
        let drawn = pip_pass(
            &ctx,
            Vec2::new(480.0, 270.0),
            Some(true),
            Some(&art),
            false,
            Vec::new(),
        );
        let words = drawn.words();
        let said = words.join(" ");
        assert!(
            said.contains(IN_THE_MAIN_WINDOW),
            "a live channel is not told where its stream is: {words:?}"
        );
        assert!(
            said.contains(crate::titlebar::dot_of(Some(true)).word()),
            "the live word is not the pill's: {words:?}"
        );
        assert!(
            !said.contains("OFFLINE"),
            "a live channel was called offline: {words:?}"
        );
        assert_eq!(
            drawn.pictures(art.texture.id()).len(),
            1,
            "the artwork is still the picture while he is live"
        );
    }

    /// AN UNPOLLED CHANNEL IS NEVER DRAWN AS AN OFFLINE ONE. Decision D2: offline is a fact about
    /// the channel and unknown is a fact about us, and a picture with nothing on it would read as
    /// the first. The word is the pill's again.
    #[test]
    fn an_unpolled_channel_says_unknown_and_not_offline() {
        let ctx = themed();
        let art = test_art(&ctx);
        let drawn = pip_pass(
            &ctx,
            Vec2::new(480.0, 270.0),
            None,
            Some(&art),
            false,
            Vec::new(),
        );
        let words = drawn.words();
        assert_eq!(
            words,
            vec![crate::titlebar::dot_of(None).word().to_owned()],
            "an unpolled channel does not say the pill's own word and nothing else"
        );
    }

    /// EVERYTHING THE POP-OUT USED TO DRAW IS GONE, IN EVERY STATE IT HAS.
    ///
    /// It drew a title strip, a status sentence naming the platform, a last checked line, Check
    /// now beside it, two large browser buttons, a caption under them and a VIDEOS band with two
    /// more buttons and a second dated line. Each of those came from a different condition, so a
    /// cut measured in one state would be green in the state the author happened to open.
    ///
    /// CHECK NOW HAS NO SURVIVORS EITHER, AND THAT IS THE CHANGE THIS TEST NOW CARRIES. It was
    /// kept in the hover chrome on the ground that `Ask::CheckLive` had no other producer in the
    /// program. It had one: the old pop-out page, still in `screens::watch`, unreachable, kept
    /// warm by its own tests. The page is deleted and the poll moved to the main window's Watch
    /// header, so the word is asserted gone from this window in every state INCLUDING the hovered
    /// one, which is the state it used to appear in and the state a resting-only test would miss.
    #[test]
    fn nothing_the_old_pop_out_page_drew_survives() {
        let ctx = themed();
        let art = test_art(&ctx);
        let gone = [
            "Open on Twitch",
            "Open chat",
            "Twitch videos",
            "YouTube channel",
            "VIDEOS",
            "last checked",
            "never checked",
            "Playback opens in your browser",
            "Broken Stoic",
            "Check now",
        ];
        let size = Vec2::new(480.0, 270.0);
        for live in [Some(true), Some(false), None] {
            for art in [Some(&art), None] {
                /* Resting AND hovered. The chrome only exists in the second, so a sweep that ran
                 * only the first could not see a control come back into it. */
                for (how, events) in [
                    ("resting", Vec::new()),
                    (
                        "hovered",
                        vec![egui::Event::PointerMoved(Pos2::new(
                            size.x / 2.0,
                            size.y / 2.0,
                        ))],
                    ),
                ] {
                    let drawn = pip_pass(&ctx, size, live, art, false, events);
                    let words = drawn.words();
                    for dead in gone {
                        assert!(
                            !words.iter().any(|w| w.contains(dead)),
                            "live={live:?} art={} {how}: the pop-out page is back, it says \
                             {dead:?}: {words:?}",
                            art.is_some(),
                        );
                    }
                    assert!(
                        words.len() <= 1,
                        "live={live:?} {how}: the window carries more than one line: {words:?}"
                    );
                }
            }
        }
    }

    /// IT IS USABLE AT THE 320 BY 200 MINIMUM THE BUILDER ALLOWS.
    ///
    /// `ViewportBuilder::with_min_inner_size([320.0, 200.0])` is what a hand can drag this window
    /// down to, and a control that runs off the edge there is a control that is gone at the size
    /// this window is most likely to be left at. The strip had exactly this defect once
    /// (`titlebar::tests::the_strip_survives_the_narrowest_window_it_allows`), so it is measured
    /// and not assumed: both controls are inside the window, they do not overlap, and both still
    /// report their click.
    ///
    /// THE LINE IS MEASURED IN BOTH LAYOUTS, because they run off the edge for different reasons.
    /// Centred wraps and can only overflow downward; the corner one is pushed off the BOTTOM by a
    /// wrap the caller did not expect, which a 320 point window is exactly where you would find.
    #[test]
    fn every_control_still_lands_at_the_narrowest_window_allowed() {
        /* The builder in `show`: `.with_min_inner_size([320.0, 200.0])`. */
        let size = Vec2::new(320.0, 200.0);
        let window = Rect::from_min_size(Pos2::ZERO, size);
        let close = chip_at(size, 0.0);
        let pin = chip_at(size, 1.0);
        for at in [close, pin] {
            assert!(window.contains(at), "{at:?} is outside a 320 by 200 window");
        }
        assert!(
            pin.x + CHIP_W * 0.5 < close.x - CHIP_W * 0.5,
            "the two chips overlap at the narrowest window"
        );
        for (name, at, want) in [("close", close, 1usize), ("pin", pin, 0)] {
            let ctx = themed();
            let art = test_art(&ctx);
            pip_pass(
                &ctx,
                size,
                Some(false),
                Some(&art),
                false,
                vec![egui::Event::PointerMoved(at)],
            );
            let hit = pip_pass(&ctx, size, Some(false), Some(&art), false, click_at(at));
            let flags = [hit.hits.pin, hit.hits.close];
            assert!(
                flags[want],
                "{name} does not land at 320 by 200: pin={} close={}",
                flags[0], flags[1]
            );
        }
        /* The line still fits, in the layout each case actually uses: no artwork puts the word in
         * the middle, artwork puts the sentence in the bottom left corner. */
        let ctx = themed();
        let art = test_art(&ctx);
        for (how, art) in [("centred", None), ("in the corner", Some(&art))] {
            let drawn = pip_pass(&ctx, size, Some(true), art, false, Vec::new());
            let runs: Vec<Rect> = drawn
                .shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(Rect::from_min_size(t.pos, t.galley.size())),
                    _ => None,
                })
                .collect();
            assert_eq!(
                runs.len(),
                1,
                "the live line is not painted {how} at 320 by 200"
            );
            assert!(
                window.contains_rect(runs[0]),
                "the live line runs off a 320 by 200 window {how}: {:?}",
                runs[0]
            );
        }
    }

    /// THE PIN GLYPH IS FILLED WHEN PINNED AND HOLLOW WHEN NOT, WHICH IS D3'S WHOLE SIGNAL.
    ///
    /// DEFECT: A SELF-SIZING WINDOW THAT REPORTS THE SIZE IT ALREADY HAS.
    ///
    /// The owner: "window should be auto resizing to meet the needs of window and NO MORE
    /// vertically". It was not, and the reason was not a missing feature: the resize was written,
    /// wired and running, and it measured `min_rect().height()`, which on a panel `Ui` is seeded to
    /// the whole panel. So the answer tracked the WINDOW instead of the CONTENT, `want` always came
    /// out equal to `now`, and the command was never sent.
    ///
    /// THIS DRIVES THE SAME CONTENT AT TWO WILDLY DIFFERENT SIZES AND DEMANDS ONE ANSWER, which is
    /// the property, and which the broken version fails by construction: it would answer about 76
    /// for the first and about 576 for the second.
    ///
    /// WHAT MUTATION MAKES THIS RED: `ui.min_rect().height()` in `content_height`.
    #[test]
    fn the_height_is_the_content_and_not_the_window() {
        let ctx = themed();
        let measure = |h: f32| -> f32 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, h))),
                ..Default::default()
            };
            /* THE ANSWER COMES OUT THROUGH A CELL, because `run_ui` hands back egui's
             * render output and not the value the closure returned. */
            let got = std::cell::Cell::new(0.0f32);
            let out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        /* Three plain rows. Whatever they measure, they measure the same in a
                         * short window and a tall one. */
                        for _ in 0..3 {
                            ui.label("row");
                        }
                        got.set(content_height(ui));
                    });
            });
            out.drop_without_applying_deltas();
            got.get()
        };

        let short = measure(100.0);
        let tall = measure(600.0);
        assert!(
            short > 0.0,
            "three rows took no space at all, so this measures nothing"
        );
        assert!(
            (short - tall).abs() < 0.5,
            "the content took {short} in a 100 point window and {tall} in a 600 point one, so this \
             is measuring the WINDOW. A self-sizing window fed that number asks for the size it \
             already has, every pass, and never resizes"
        );
        assert!(
            tall < 200.0,
            "three rows reported {tall} points in a 600 point window, which is the whole window \
             and not the rows"
        );
    }

    /// DEFECT: `sync_overlays` MATCHING BY POSITION INSTEAD OF BY ID.
    ///
    /// THE REGISTRY'S WINDOWS AND THE OWNER'S SETTINGS ARE TWO LISTS THAT MUST STAY PAIRED. He can
    /// reorder them, rename one, delete one from the middle, or add one, and every one of those
    /// moves positions around. An id does not move. Matching by index would hand the third
    /// window's remembered placement and pin to whatever ended up third.
    ///
    /// WHAT MUTATION MAKES THIS RED: pairing `have[i]` with `want[i]` in `sync_overlays`.
    #[test]
    fn overlay_windows_follow_their_id_and_not_their_position() {
        use crate::overlay::Overlay;

        let a = Overlay {
            id: "a".into(),
            name: "A".into(),
            open: true,
            ..Overlay::default()
        };
        let b = Overlay {
            id: "b".into(),
            name: "B".into(),
            open: true,
            ..Overlay::default()
        };
        let c = Overlay {
            id: "c".into(),
            name: "C".into(),
            open: true,
            ..Overlay::default()
        };

        let mut have: Vec<OverlayWindow> = Vec::new();
        sync_overlays(&mut have, &[a.clone(), b.clone(), c.clone()]);
        assert_eq!(have.len(), 3);

        /* Mark each window with something only IT knows, so the pairing is visible. */
        have[0].placed = true;
        have[1].serviced = true;
        have[2].focus = true;

        /* THE OWNER REORDERS AND DELETES THE MIDDLE ONE. */
        sync_overlays(&mut have, &[c.clone(), a.clone()]);
        assert_eq!(
            have.iter().map(|o| o.cfg.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "a"],
            "the windows must be in the owner's order"
        );
        assert!(have[0].focus, "c kept what belonged to c");
        assert!(have[1].placed, "a kept what belonged to a");
        assert!(
            !have[0].placed && !have[1].focus,
            "a mark crossed between windows"
        );

        /* A RENAME IS NOT A NEW WINDOW. The id is what identifies it, so the placement survives. */
        let a2 = Overlay {
            name: "Renamed".into(),
            ..a.clone()
        };
        sync_overlays(&mut have, &[a2]);
        assert_eq!(have.len(), 1);
        assert_eq!(have[0].cfg.name, "Renamed");
        assert!(have[0].placed, "renaming an overlay moved its window");
    }

    /// DEFECT: opening an overlay from the Parser list and getting no window, because nothing told
    /// the root a pass was owed.
    ///
    /// A deferred viewport only exists inside a root pass, so a window that has just been asked
    /// for must make `tool_pending` true or `App::logic` will not restore a minimised app to build
    /// it. Same rule as `Window::serviced`, same bug if it is missed.
    #[test]
    fn opening_an_overlay_asks_for_a_root_pass_and_stops_asking_after_one() {
        use crate::overlay::Overlay;
        let shut = Overlay {
            id: "a".into(),
            open: false,
            ..Overlay::default()
        };
        let open = Overlay {
            open: true,
            ..shut.clone()
        };

        let mut have: Vec<OverlayWindow> = Vec::new();
        sync_overlays(&mut have, std::slice::from_ref(&shut));
        have[0].serviced = true;

        sync_overlays(&mut have, std::slice::from_ref(&open));
        assert!(
            !have[0].serviced,
            "an overlay that was just opened has no viewport and only a root pass can give it one"
        );

        /* And a pass that changes nothing does not re-raise the ask. */
        have[0].serviced = true;
        sync_overlays(&mut have, &[open]);
        assert!(have[0].serviced);
    }

    /// DEFECT: two slots sharing a viewport id, so one window draws over another.
    ///
    /// The id is `from_hash_of(("grimoire.tool", index))`, so a duplicated index is a duplicated
    /// window. It would not fail to compile and no other test here would notice.
    #[test]
    fn every_slot_has_its_own_index() {
        let mut ix: Vec<usize> = SLOTS.iter().map(|s| s.index()).collect();
        let n = ix.len();
        ix.sort_unstable();
        ix.dedup();
        assert_eq!(ix.len(), n, "two slots share an index");
        assert_eq!(n, SLOTS.len());
    }

    /// It is `titlebar::pin_glyph` in both states, so this reads the difference the same way the
    /// glyph draws it: the pin's head is a filled rect when the window is on top and a stroked one
    /// when it is not. A window that is always on top and does not say so is a window the user
    /// cannot explain, and this window is the one the owner will actually pin over a game.
    #[test]
    fn the_pip_pin_says_whether_the_window_is_on_top() {
        let heads = |pinned: bool| -> usize {
            let ctx = themed();
            let art = test_art(&ctx);
            let drawn = pip_pass(
                &ctx,
                Vec2::new(480.0, 270.0),
                Some(false),
                Some(&art),
                pinned,
                vec![egui::Event::PointerMoved(Pos2::new(240.0, 135.0))],
            );
            /* The pin's head is a 6 by 4.5 box, and nothing else in this window is that size. */
            drawn
                .shapes
                .iter()
                .filter(|s| match s {
                    egui::Shape::Rect(r) => {
                        (r.rect.width() - 6.0).abs() < 0.1 && (r.rect.height() - 4.5).abs() < 0.1
                    }
                    _ => false,
                })
                .filter(|s| match s {
                    egui::Shape::Rect(r) => (r.fill != Color32::TRANSPARENT) == pinned,
                    _ => false,
                })
                .count()
        };
        assert_eq!(heads(true), 1, "a pinned window draws no filled pin head");
        assert_eq!(
            heads(false),
            1,
            "an unpinned window draws no hollow pin head"
        );
    }

    /* -------------------------------------------------- the registry's own wiring -- */

    /// A CHAT POP-OUT'S ASK REACHES THE APP, AND NEVER CANCELS THE BODY'S.
    ///
    /// `App::ui` owns the one `ChatReader` and is the only thing that may call `start`. A deferred
    /// viewport's pass never sees the root `Cx`, so a Chat pop-out opened while the body is on
    /// another screen can only connect if its ask is routed up. Without the routing the window
    /// would say "Not connected yet" for as long as it stayed open, which looks exactly like a
    /// channel that has gone quiet.
    ///
    /// THE THIRD CASE IS THE ONE THAT MATTERS AND IS WHY THIS IS AN OR. Both windows draw the Chat
    /// screen; the body's ask is already in the flag when this runs. An assignment would pass both
    /// of the first two cases and still hang up on the body every frame the pop-out was shut.
    #[test]
    fn a_pop_out_chat_ask_reaches_the_app_and_never_cancels_the_body_s() {
        let live = Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = Settings::default();
        let mut ingest = Ingest::new(&settings);
        /* A FUNCTION AND NOT A CLOSURE. A closure returning a `Cx` cannot name the lifetime that
         * ties the borrows it is handed to the value it returns, so rustc refuses it; an `fn` with
         * one named lifetime says exactly that and is the same four lines at every call. */
        fn cx<'a>(
            asked: bool,
            live: &'a Status,
            settings: &'a mut Settings,
            ingest: &'a mut Ingest,
        ) -> Cx<'a> {
            Cx {
                data: None,
                railed: false,
                data_err: None,
                live,
                settings,
                ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: asked,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
            }
        }

        /* 1. Nobody asked. */
        let mut w = Windows::default();
        let mut c = cx(false, &live, &mut settings, &mut ingest);
        w.adopt_chat_ask(&mut c);
        assert!(!c.chat_wanted, "an ask appeared from nowhere");

        /* 2. The POP-OUT asked and the body did not. This is the case the routing exists for. */
        lock(&w.inner).chat_wanted_out = true;
        let mut c = cx(false, &live, &mut settings, &mut ingest);
        w.adopt_chat_ask(&mut c);
        assert!(
            c.chat_wanted,
            "a Chat pop-out asked for the socket and the App never heard: the window would say \
             it was not connected for as long as it stayed open"
        );
        /* And it is TAKEN: the next frame starts clean, so a closed window stops asking. */
        let mut c = cx(false, &live, &mut settings, &mut ingest);
        w.adopt_chat_ask(&mut c);
        assert!(!c.chat_wanted, "the ask outlived the frame that made it");

        /* 3. The BODY asked and the pop-out did not. */
        let mut c = cx(true, &live, &mut settings, &mut ingest);
        w.adopt_chat_ask(&mut c);
        assert!(
            c.chat_wanted,
            "the body asked for the socket and a silent pop-out cancelled it; this is an OR, and \
             an assignment here would hang up on the body on every frame no pop-out was open"
        );
    }

    /// A registry with its child context already built, ready for `draw_child`.
    ///
    /// The data root is deliberately a path that does not exist: `Slot::Sky` is the one window
    /// that loads the snapshot, and a test that let `Snapshot::locate` find the real 21MB one
    /// would spawn a parse of it on every run for nothing.
    fn seeded(live: Status) -> Windows {
        let w = Windows::default();
        let mut settings = Settings {
            data_root: Some(PathBuf::from("no-such-data-root-for-tests")),
            ..Default::default()
        };
        let mut ingest = Ingest::new(&settings);
        let child = {
            let cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
            };
            ChildCx::new(&cx)
        };
        let mut g = lock(&w.inner);
        g.settings_synced = settings_json(&settings).unwrap_or_default();
        g.cx = Some(child);
        drop(g);
        w
    }

    fn status(live: Option<bool>) -> Status {
        let mut s = Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        s.twitch.live = live;
        s.youtube.live = live;
        s
    }

    /// ONE REAL ROOT PASS, through `Windows::show`, which is the only thing that registers a
    /// deferred viewport.
    ///
    /// A REAL `egui::Context::run` AND NOT A HAND CALL, because `show_viewport_deferred` is a
    /// context operation and a `Windows::show` called outside a pass would not exercise the thing
    /// under test. The `Cx` is built here rather than in `seeded` because it borrows a `Settings`
    /// and an `Ingest` that have to outlive the call.
    fn root_pass(w: &mut Windows, ctx: &egui::Context, live: &Status) {
        let mut settings = Settings::default();
        root_pass_with(w, ctx, live, &mut settings);
    }

    /// The same pass with the caller's own `Settings`, which is what the pin needs: `Windows::show`
    /// both READS that map and WRITES it, so a test that could not see the settings across two
    /// passes could not tell a pin that persists from one that is merely remembered in this
    /// process, which is the whole of the defect.
    ///
    /// NOTHING HERE TOUCHES THE OWNER'S FILE. `Settings::save` refuses outright under `cfg(test)`
    /// and hands back an `Err` the registry logs, so the write path is exercised and the bytes in
    /// `%APPDATA%` are not.
    fn root_pass_with(
        w: &mut Windows,
        ctx: &egui::Context,
        live: &Status,
        settings: &mut Settings,
    ) {
        let _ = root_pass_builders(w, ctx, live, settings);
    }

    /// THE SAME PASS, HANDING BACK THE WINDOW ATTRIBUTES EGUI WILL BUILD EACH TOOL WINDOW WITH.
    ///
    /// THE ONLY HONEST PLACE TO READ A PLACEMENT BACK. `Windows::show` builds one `ViewportBuilder`
    /// per open window, passes it straight to `show_viewport_deferred` and keeps none of them, so a
    /// test that asked the REGISTRY where it thinks a window is would be reading the wrong side of
    /// the door: it would pass just as happily with the builder still carrying `Slot::default_size`.
    /// `FullOutput::viewport_output` is the map eframe reads to size and place the real OS window,
    /// and it is what this returns.
    ///
    /// AND THE CONTEXT MUST HAVE `set_embed_viewports(false)`. egui's default is `true`, which is
    /// the right answer for a backend with no multi-window support: `show_viewport_deferred` then
    /// draws an `egui::Window` inside the root pass and registers no viewport at all. A caller that
    /// forgot would get an empty map and every assertion under it would be vacuous, which is why the
    /// two tests that use this say so at the top and this one says it here.
    fn root_pass_builders(
        w: &mut Windows,
        ctx: &egui::Context,
        live: &Status,
        settings: &mut Settings,
    ) -> Vec<(Slot, ViewportBuilder)> {
        let mut ingest = Ingest::new(settings);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live,
                settings,
                ingest: &mut ingest,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
            };
            w.show(ui.ctx(), &mut cx);
        });
        let built: Vec<(Slot, ViewportBuilder)> = SLOTS
            .iter()
            .filter_map(|s| {
                out.viewport_output
                    .get(&s.viewport_id())
                    .map(|v| (*s, v.builder.clone()))
            })
            .collect();
        out.drop_without_applying_deltas();
        built
    }

    /// DEFECT: THE MAIN WINDOW CANNOT BE MINIMIZED WHILE A POP-OUT IS OPEN.
    ///
    /// Reported by the owner: "why is it that if we have a popup open it refuses to minimize the
    /// main window?". `App::logic` restores the root whenever `tool_pending` is true, and that used
    /// to mean "any tool window is open", on the reasoning that `logic` cannot be running while a
    /// tool window is visible. Measured with a probe build on 2026-09-05: `logic` ran 1,791 times
    /// INTERLEAVED with about 1,740 `ui` passes, with the main window visible and unminimized the
    /// whole time, and sent `Minimized(false)` on every one. Minimizing the main window from
    /// outside the process was undone inside 200 ms, every time.
    ///
    /// SO THE QUESTION HAD TO NARROW: not "is a window open" but "is a window waiting for a root
    /// pass", which is the only state the restore exists to rescue.
    ///
    /// WHAT MUTATION MAKES THIS RED: `tool_pending` answering `w.open`, which is what it did.
    #[test]
    fn a_pop_out_stops_asking_for_the_main_window_once_it_has_had_its_pass() {
        let live = status(Some(false));
        let mut w = seeded(live.clone());
        assert!(
            !w.tool_pending(),
            "nothing is open, so nothing is waiting for a pass"
        );

        w.open(Tool::Sky);
        assert!(
            w.tool_pending(),
            "a window that has just been asked for has no OS window yet, and only a root pass can \
             give it one: this is the state `App::logic` restores the main window for"
        );

        let ctx = themed();
        root_pass(&mut w, &ctx, &live);
        assert!(
            !w.tool_pending(),
            "the pop-out has been registered, so the main window is owed nothing and must be free \
             to minimize. This is the owner's bug: answering `open` here left it true for as long \
             as the window stayed open, and `App::logic` restored the root on every pass"
        );

        /* AND A SECOND PASS DOES NOT BRING THE ASK BACK. A flag that were recomputed rather than
         * latched would flicker true again the moment anything else touched the window. */
        root_pass(&mut w, &ctx, &live);
        assert!(!w.tool_pending());

        /* SUMMONING AN ALREADY OPEN WINDOW ASKS AGAIN, and it has to: the raise it wants is
         * carried out by the same root pass, so on a minimized app with a minimized pop-out this
         * is the only thing that makes `Ctrl+Alt+P` work at all. */
        w.open(Tool::Sky);
        assert!(
            w.tool_pending(),
            "bringing an open window forward is also an ask that only a root pass can carry out"
        );
        root_pass(&mut w, &ctx, &live);
        assert!(!w.tool_pending());

        /* A CLOSED WINDOW ASKS FOR NOTHING. */
        w.toggle(Tool::Sky);
        assert!(!w.tool_pending(), "a closed window is owed no pass");
    }

    /// DEFECT: NOTHING ABOUT A TOOL WINDOW'S PIN SURVIVED A RELAUNCH, AND THE MECHANISM BUILT FOR
    /// IT HAD NO PRODUCTION CALLER AT ALL.
    ///
    /// `Settings::windows` and `WindowPrefs` landed on 2026-09-05 (D11) with `win_pinned`,
    /// `set_win_pinned` and the erasure rule that makes an absent key mean "follow the code". Every
    /// one of them was reached only from `settings::tests`. So the file could describe a pinned
    /// parser and no window ever read it, and a pin clicked in a window never reached the file: the
    /// owner pinned the parser over the game, closed the app, and opened it tomorrow to an
    /// unpinned window, which is a window he stops pinning. `crate::pin`'s own module doc had
    /// already named the cause in advance, from the shape of the code rather than from a report:
    /// always on top was an idiom copied four times, and "the main window's pin persists to
    /// settings and a tool window's does not, which is not a decision anybody took".
    ///
    /// THIS DRIVES THE WHOLE ROUND TRIP AND NOT ONE END OF IT. A test that only read the file into
    /// a window would pass with the write half missing, and a test that only wrote would pass with
    /// the read half missing; either way the pin still dies at the door.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting either arm of the `w.pin_out.take()` match at the
    /// head of `Windows::show`'s window loop. Deleting the write arm fails at step 2; deleting the
    /// read arm fails at step 1, which is the relaunch itself.
    #[test]
    fn a_tool_windows_pin_is_written_to_settings_and_read_back_from_it() {
        let live = status(Some(false));
        let ctx = themed();

        /* 1. THE RELAUNCH. A registry that has never been told anything, the owner's saved answer
         *    on disk, and one root pass between them. */
        let mut settings = Settings::default();
        settings.set_win_pinned(Slot::Parser.id(), Slot::Parser.pin_default(), true);
        let mut w = Windows::default();
        assert!(
            !w.is_pinned(Tool::Parser),
            "a fresh registry has not read anything yet, so this is measuring the pass below"
        );
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            w.is_pinned(Tool::Parser),
            "the settings file says the parser is pinned and the window came back unpinned"
        );
        assert!(
            !w.is_pinned(Tool::Sky),
            "a window the file says nothing about must follow the code's own default, not the \
             answer of whichever window was read before it"
        );

        /* 2. A PIN SET ON A WINDOW REACHES THE FILE. */
        w.pin(Tool::Sky, true);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            settings.win_pinned(Slot::Sky.id(), Slot::Sky.pin_default()),
            "the Sky window was pinned and the settings file never heard: {:?}",
            settings.windows
        );
        assert!(w.is_pinned(Tool::Sky), "and the window still wants it");

        /* 3. AND UNPINNING ERASES THE KEY rather than storing agreement with the default, which is
         *    `Settings::set_win_pinned`'s own rule: a stored `false` would pin today's default in
         *    place for ever for anybody who had ever toggled the glyph twice. */
        w.pin(Tool::Sky, false);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            !settings.windows.contains_key(Slot::Sky.id()),
            "unpinning left a key recording agreement with the default: {:?}",
            settings.windows
        );
        assert!(!w.is_pinned(Tool::Sky));

        /* 4. AND THE FILE WINS ON EVERY PASS THAT DID NOT COME FROM A CLICK, which is what lets an
         *    edit made anywhere else reach a window that is already open. */
        settings.set_win_pinned(Slot::Sky.id(), Slot::Sky.pin_default(), true);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            w.is_pinned(Tool::Sky),
            "the file was changed under an open window and the window did not follow it"
        );
    }

    /// DEFECT: WHICH POP-OUTS WERE ON SCREEN WAS A FACT ABOUT THE SESSION, AND THE MECHANISM BUILT
    /// FOR IT HAD NO PRODUCTION CALLER.
    ///
    /// `WindowPrefs::open` and `Settings::win_open` / `set_win_open` were reached only from
    /// `settings::tests`, exactly as the pin had been before it: the file could say the parser was
    /// open and no window ever read it, and a window opened in anger never reached the file. The
    /// owner's use of a pop-out is to arrange it over the game and leave it there, so a window that
    /// has to be summoned again every launch is a window he stops summoning. Same map, same key,
    /// same loop as the pin, and the same defect one field along.
    ///
    /// THIS DRIVES THE ROUND TRIP AND NOT ONE END OF IT. A test that only read the file into the
    /// registry would pass with the write half missing, and one that only wrote would pass with the
    /// read half missing; either way the arrangement still dies at the door.
    ///
    /// AND IT ASSERTS THE ONE-WAY RULE, which is the part that is not the pin's. There is no second
    /// surface in this app that can say a window is open, so the file is read ONCE and the registry
    /// is the author ever after; a loop that kept following the file would undo a close on the very
    /// next pass, because a close writes `false` and `false` is also what an absent key answers.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the restore block at the head of `Windows::show`
    /// (step 1), deleting the `set_win_open` write in its window loop (step 2), or making the
    /// restore run on every pass instead of the first (step 3, which reopens a window the person
    /// just shut).
    #[test]
    fn a_tool_window_that_was_open_comes_back_open() {
        let live = status(Some(false));
        let ctx = themed();

        /* 1. THE RELAUNCH. A registry that has been told nothing, the owner's saved answer on
         *    disk, and one root pass between them. */
        let mut settings = Settings::default();
        settings.set_win_open(Slot::Parser.id(), true);
        let mut w = Windows::default();
        assert!(
            !w.is_open(Tool::Parser),
            "a fresh registry has read nothing yet, so this is measuring the pass below"
        );
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            w.is_open(Tool::Parser),
            "the settings file says the parser was on screen and it came back shut"
        );
        assert!(
            !w.is_open(Tool::Sky),
            "a window the file says nothing about opened itself: there is no table in the code \
             that says a tool window wants to be on screen, which is why `win_open` takes no \
             fallback"
        );

        /* 2. A WINDOW OPENED IN THIS SESSION REACHES THE FILE, and closing it erases the key
         *    rather than storing agreement with the default. */
        w.open(Tool::Sky);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            settings.win_open(Slot::Sky.id()),
            "the Sky window was opened and the settings file never heard: {:?}",
            settings.windows
        );
        w.toggle(Tool::Sky);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            !settings.windows.contains_key(Slot::Sky.id()),
            "closing left a key recording agreement with the default: {:?}",
            settings.windows
        );

        /* 3. AND A CLOSE STAYS CLOSED. The parser's key still says `true` at this point only
         *    because nothing has closed it; close it, and the pass that writes `false` must not be
         *    followed by a pass that reads the file and opens it again. A restore that ran every
         *    pass would pass steps 1 and 2 and fail here, and on a real machine it would be a
         *    window that cannot be shut. */
        w.toggle(Tool::Parser);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(!w.is_open(Tool::Parser), "the close was undone by the file");
        assert!(!settings.win_open(Slot::Parser.id()));
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert!(
            !w.is_open(Tool::Parser),
            "and it stayed shut on the next pass"
        );
    }

    /// DEFECT: A POP-OUT'S SIZE AND POSITION DIED WITH THE SESSION, AND THE PLACEMENT CODE THREW
    /// THEM AWAY ON PURPOSE.
    ///
    /// `Windows::show` cleared `Window::placed` whenever a window was shut and built every builder
    /// from `Slot::default_size`, so the parser came back at 620 by 460 in the corner beside the
    /// main window no matter where the owner had dragged it. The note under `placed` argued for it
    /// ("a monitor that is no longer there"), and that half is answered better by a saved rectangle
    /// than by forgetting one: the OS clamps a placement onto a monitor that exists, for this app
    /// exactly as for every other one.
    ///
    /// # THE SETTLE IS THE HALF THAT IS EASY TO GET WRONG
    ///
    /// A drag reports a new rectangle on every frame of it, and `Settings::save` rewrites the whole
    /// file through a temp file and a rename. Written per report, one drag across a monitor is tens
    /// of rewrites a second of the one file whose corruption costs the owner everything in it. So
    /// the window records and the root writes on a deadline that RESTARTS on every change, which is
    /// what makes it a debounce: the rectangle that lands in the file is the one the hand let go of
    /// and not one the drag happened to be passing through.
    ///
    /// # AND IT READS THE BUILDER, NOT THE REGISTRY
    ///
    /// Asserting that `Settings` holds the right numbers proves half a round trip. The other half is
    /// that the OS is ASKED for them, and `Windows::show` keeps no copy of what it asked for: see
    /// `root_pass_builders`, which reads egui's own viewport output.
    ///
    /// WHAT MUTATION MAKES THIS RED: writing the rectangle without waiting for the deadline (step
    /// 2), never writing it at all (step 3), handing `Slot::default_size` to the builder again
    /// (step 4's size), or dropping the saved rectangle from `place_at` (step 4's position).
    #[test]
    fn a_windows_rectangle_reaches_the_file_only_once_it_has_stopped_moving() {
        let live = status(Some(false));
        let ctx = themed();
        /* THE VIEWPORTS HAVE TO BE REAL ONES. See `root_pass_builders`: egui's default embeds a
         * deferred viewport in the root pass and registers nothing, which would make step 4 read an
         * empty map and assert nothing at all. */
        ctx.set_embed_viewports(false);

        let mut settings = Settings::default();
        let mut w = Windows::default();
        w.open(Tool::Parser);
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert_eq!(
            settings.win_rect(Slot::Parser.id()),
            None,
            "a window nobody has moved must leave the file alone, so that changing a default size \
             in the code still reaches a machine that has one"
        );

        /* 1. THE HAND PICKS THE WINDOW UP. `draw_child` is what records this off `outer_rect`;
         *    writing the two fields here is the same act minus an OS window to drag, which is the
         *    one thing a unit test cannot have. */
        let dragged = [12.0, 34.0, 900.0, 640.0];
        {
            let mut g = lock(&w.inner);
            let win = &mut g.windows[Slot::Parser.index()];
            win.rect_out = Some(dragged);
            win.rect_since = Some(Instant::now());
        }

        /* 2. AND THE FILE HEARS NOTHING WHILE IT IS STILL MOVING. */
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert_eq!(
            settings.win_rect(Slot::Parser.id()),
            None,
            "the rectangle was written while the hand was still holding the window: one drag is \
             then one settings save per frame of it"
        );

        /* 3. THE HAND LETS GO, and the deadline runs out. */
        {
            let mut g = lock(&w.inner);
            g.windows[Slot::Parser.index()].rect_since = Some(
                Instant::now()
                    .checked_sub(RECT_SETTLES_IN)
                    .expect("this machine has been up longer than the settle"),
            );
        }
        root_pass_with(&mut w, &ctx, &live, &mut settings);
        assert_eq!(
            settings.win_rect(Slot::Parser.id()),
            Some(dragged),
            "the drag settled and the file never heard: {:?}",
            settings.windows
        );

        /* 4. THE RELAUNCH, AND THE OS IS ASKED FOR THAT EXACT WINDOW. A fresh registry, because
         *    `place_at` is only computed for a window that has not been placed yet, which is what a
         *    launch is. */
        let mut fresh = Windows::default();
        let built = root_pass_builders(&mut fresh, &ctx, &live, &mut settings);
        let parser = built
            .iter()
            .find(|(s, _)| *s == Slot::Parser)
            .map(|(_, b)| b.clone())
            .expect(
                "the parser was open in the file and no viewport was registered for it, so there \
                 is nothing here to place",
            );
        assert_eq!(
            parser.inner_size,
            Some(Vec2::new(900.0, 640.0)),
            "the window came back at the code's default size and not the owner's"
        );
        assert_eq!(
            parser.position,
            Some(Pos2::new(12.0, 34.0)),
            "the window came back beside the main window and not where it was left"
        );

        /* AND A WINDOW THE OWNER HAS NEVER MOVED STILL OPENS AT THE SIZE THIS FILE CHOOSES. */
        fresh.open(Tool::Sky);
        let built = root_pass_builders(&mut fresh, &ctx, &live, &mut settings);
        let sky = built
            .iter()
            .find(|(s, _)| *s == Slot::Sky)
            .map(|(_, b)| b.clone())
            .expect("the Sky window was opened and no viewport was registered for it");
        assert_eq!(
            sky.inner_size,
            Some(Slot::Sky.default_size()),
            "a window with no saved rectangle must take the slot's own first-open size"
        );
    }

    /// DEFECT: THE MAIN WINDOW AND THE POP-OUT PRINTED TWO TOTALS FOR ONE LOG.
    ///
    /// `Ingest::fights` is a fold of the log's tail taken ONCE, when that `Ingest` was built or last
    /// told to rescan; the live path never rewrites it. The root's is built by `App::new` at launch
    /// and the pop-out's by `ChildCx::new`, which runs when the FIRST tool window opens, and for the
    /// owner that is an hour of raiding later. So the two Fights tables listed different fights, the
    /// Dashboards headline and the Logs page printed different totals in the same words, and nothing
    /// on either screen said which fold it was counting. Neither number was wrong about the bytes it
    /// read; they were not the same bytes.
    ///
    /// # THE SECOND PRODUCER IS WORSE BECAUSE IT LOOKS DELIBERATE
    ///
    /// Rescan refolds the ingest of whichever window it was pressed in. A person who notices the two
    /// disagree and presses Rescan to fix it moves ONE of them, which is why the comparison here is
    /// symmetric rather than root-to-child: whichever side was refolded holds the newer stamp and is
    /// the source. A root-to-child version passes every assertion up to the fourth step and leaves
    /// the owner's obvious remedy still broken.
    ///
    /// # THE STAMP IS WHAT MAKES IT SETTLE
    ///
    /// `adopt_history` carries `scanned_at` across with the rows, so the pass after an adoption
    /// compares equal and does nothing. Without it the two sides would swap the same rows for ever,
    /// every frame, and the `changed` flag would ask both windows to repaint on every one.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting either arm of the stamp comparison at the foot of
    /// `Windows::sync` (steps 2 and 4), or passing anything but the source's own stamp as
    /// `adopt_history`'s third argument (steps 3 and 5).
    #[test]
    fn the_two_windows_settle_on_one_bootstrap_history() {
        use chrono::{DateTime, Utc};

        const SKELETON: &str = "\
[Wed Jul 15 21:00:00 2026] You slash a dry bone skeleton for 9 points of damage.
[Wed Jul 15 21:00:02 2026] You have slain a dry bone skeleton!
";
        const BEETLE: &str = "\
[Wed Jul 15 22:00:00 2026] You slash a fire beetle for 4 points of damage.
[Wed Jul 15 22:00:02 2026] You have slain a fire beetle!
";

        let quiet = crate::fights::quiet_window();
        let (roots_rows, _) = crate::fights::fold_text(SKELETON, quiet, None);
        let (childs_rows, _) = crate::fights::fold_text(BEETLE, quiet, None);
        assert_eq!(roots_rows.len(), 1, "the fixture has to fold to something");
        assert_ne!(
            roots_rows, childs_rows,
            "and the two must be tellable apart"
        );

        let live = status(Some(false));
        let w = seeded(live.clone());
        let mut settings = Settings {
            data_root: Some(PathBuf::from("no-such-data-root-for-tests")),
            ..Default::default()
        };
        /* THE SETTINGS ARMS OF `sync` ARE PUT TO SLEEP, so what this measures is the ingest arm on
         * its own: a settings difference would call `reconfigure` and return `changed` for a reason
         * that has nothing to do with the history. */
        lock(&w.inner).settings_synced = settings_json(&settings).expect("settings serialise");
        let mut ingest = Ingest::new(&settings);

        /* 1. NEITHER BOOTSTRAP HAS LANDED. Two `None` stamps compare equal and nothing moves. */
        assert!(
            !sync_pass(&w, &live, &mut settings, &mut ingest),
            "two ingests with no history between them found something to swap"
        );
        assert!(child_ingest(&w, |i| i.fights().is_empty()));

        /* 2. THE ROOT'S BOOTSTRAP LANDS FIRST, which is what launching the app and opening a
         *    pop-out an hour later actually looks like. `None` is less than every `Some`. */
        let first = DateTime::<Utc>::from_timestamp(1_752_600_000, 0);
        assert!(first.is_some(), "the constant is a real instant");
        ingest.adopt_history(roots_rows.clone(), 4, first);
        assert!(
            sync_pass(&w, &live, &mut settings, &mut ingest),
            "the child took a new history and nothing asked it to repaint"
        );
        assert!(
            child_ingest(&w, |i| i.fights() == roots_rows.as_slice()),
            "the pop-out kept a history the main window had already replaced"
        );
        assert!(
            child_ingest(&w, |i| i.fights_unreadable() == 4),
            "the denominator did not travel with the rows it qualifies, so the pop-out's Logs page \
             would date one fold's unreadable count against another fold's fights"
        );

        /* 3. AND THE PASS AFTER IT FINDS NOTHING TO DO. */
        assert!(
            !sync_pass(&w, &live, &mut settings, &mut ingest),
            "the two are in step and the pass adopted again: the stamp did not travel with the \
             rows, so this happens on every frame for ever"
        );

        /* 4. RESCAN, PRESSED IN THE POP-OUT. The newer fold is the child's now, and it has to
         *    reach the main window or the owner's own remedy for the disagreement makes it worse. */
        let later = DateTime::<Utc>::from_timestamp(1_752_603_600, 0);
        assert!(later > first, "the second stamp has to be the later one");
        {
            let mut g = lock(&w.inner);
            g.cx.as_mut()
                .expect("seeded built the child context")
                .ingest
                .adopt_history(childs_rows.clone(), 2, later);
        }
        sync_pass(&w, &live, &mut settings, &mut ingest);
        assert_eq!(
            ingest.fights(),
            childs_rows.as_slice(),
            "a Rescan pressed in the pop-out never reached the main window"
        );
        assert_eq!(ingest.fights_unreadable(), 2);
        assert_eq!(
            ingest.scanned_at(),
            later,
            "the main window took the rows and left its own stamp behind"
        );

        /* 5. AND THAT DIRECTION SETTLES TOO. */
        assert!(!sync_pass(&w, &live, &mut settings, &mut ingest));
    }

    /// A SETTINGS EDIT MADE IN A POP-OUT REACHED THE MAIN WINDOW'S `Settings` AND NEVER ITS
    /// `Ingest`.
    ///
    /// `Windows::sync` has two arms and only one of them was finished. The DOWNWARD arm, the one a
    /// main-window edit comes through, has called `Ingest::reconfigure` since a changed Logs folder
    /// was found to leave every open pop-out tailing the old one. The UPWARD arm assigned
    /// `*cx.settings` and stopped.
    ///
    /// # WHAT THIS IS WORTH TODAY, HONESTLY, BECAUSE THE FIRST DRAFT OF THIS DOC OVERSTATED IT
    ///
    /// It said the pop-out's Kills filters reached the main window's `Settings` and not its
    /// `Ingest`, so the two windows printed two completion percentages for one roster. That was the
    /// defect before `Settings::tracker` existed and it is not the state of the tree: `screens::
    /// parser::kills` opens by seeding `cx.ingest.tracker_mut().settings` from `cx.settings.tracker`
    /// whenever the two differ, and `summarize` and `credited` are the only readers of that copy in
    /// the crate, so a window that DRAWS the Kills view is already in step before it counts
    /// anything. Checked, not assumed: `grep` for those two functions answers with that view and
    /// with tests.
    ///
    /// SO WHAT THIS LINE BUYS TODAY IS A FRAME. The main window's `Ingest` takes the pop-out's
    /// counting rule on the pass the edit arrives rather than when the main window next reaches the
    /// Kills view. What it buys the day a pop-out screen offers a Logs folder control is the whole
    /// of the defect the downward arm exists for, pointing the other way, which is why the parser
    /// lane refused to add that control until this call existed.
    ///
    /// # AND THE TEST IS WORTH MORE THAN THE FRAME IS
    ///
    /// It is the only thing standing between this arm and a second silent divergence, and it fails
    /// on the code as it was. A test whose value is a guard rather than a repair is still a test;
    /// what would not be honest is a doc that called the guard a repair.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the `cx.ingest.reconfigure(cx.settings)` line from
    /// `Windows::sync`'s `settings_out` arm, which is what it looked like before.
    #[test]
    fn a_pop_outs_settings_edit_reaches_the_main_windows_ingest() {
        let live = status(Some(false));
        let w = seeded(live.clone());
        let mut settings = Settings {
            data_root: Some(PathBuf::from("no-such-data-root-for-tests")),
            ..Default::default()
        };
        lock(&w.inner).settings_synced = settings_json(&settings).expect("settings serialise");
        let mut ingest = Ingest::new(&settings);
        assert_eq!(
            ingest.tracker().settings,
            settings.tracker,
            "`Ingest::new` seeds the filters, so this fixture starts in step and what follows is \
             measuring the sync"
        );

        /* THE POP-OUT'S EDIT, IN THE SHAPE `draw_child` LEAVES IT IN: that pass compared the
         * settings JSON either side of the screen's own draw, found it different, and left the new
         * text here for the root to adopt on its next `show`. */
        let mut edited = settings.clone();
        edited.tracker.witnessed = !settings.tracker.witnessed;
        edited.tracker.ignore_cities = !settings.tracker.ignore_cities;
        lock(&w.inner).settings_out = settings_json(&edited);

        sync_pass(&w, &live, &mut settings, &mut ingest);

        assert_eq!(
            settings.tracker, edited.tracker,
            "the arm did not even adopt the settings, so the assertion below would be measuring \
             the wrong failure"
        );
        assert_eq!(
            ingest.tracker().settings,
            edited.tracker,
            "the main window's Settings took the pop-out's edit and its Ingest did not, so this \
             arm is the one place a settings change can move without the ingest under it moving \
             too. Today that costs a frame of the completion percentage; the day a pop-out can \
             name a Logs folder it costs the main window the whole evening's log"
        );
    }

    /// One call of `Windows::sync` over the caller's own root state. The `Cx` is fifteen fields and
    /// the borrows it takes have to END before an assertion can read the ingest back, which is what
    /// a function gives that an inline block does not.
    fn sync_pass(w: &Windows, live: &Status, settings: &mut Settings, ingest: &mut Ingest) -> bool {
        let mut g = lock(&w.inner);
        let mut cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live,
            settings,
            ingest,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: Ask::None,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
        };
        w.sync(&mut g, &mut cx)
    }

    /// Ask the tool windows' own `Ingest` a question. It lives behind the registry's mutex inside an
    /// `Option`, and every assertion about it would otherwise be four lines of unwrapping.
    fn child_ingest<T>(w: &Windows, ask: impl Fn(&Ingest) -> T) -> T {
        let g = lock(&w.inner);
        ask(&g
            .cx
            .as_ref()
            .expect("seeded built the child context")
            .ingest)
    }

    /// WHERE THE PIN GLYPH WAS PAINTED, FOUND RATHER THAN COMPUTED.
    ///
    /// `titlebar::strip` lays its buttons out with `BTN_W`, which is private, and counting columns
    /// in from the right in a test would be a second copy of that layout that goes wrong silently
    /// the day a button is added. `titlebar::pin_glyph` draws its head as a 6 by 4.5 rectangle and
    /// nothing else either window body paints is that size, which
    /// `the_pip_pin_says_whether_the_window_is_on_top` already relies on, so the glyph is found by
    /// its own shape and the centre is read back out of it.
    fn pin_head_centre(shapes: &[egui::Shape]) -> Pos2 {
        let heads: Vec<Rect> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r)
                    if (r.rect.width() - 6.0).abs() < 0.1
                        && (r.rect.height() - 4.5).abs() < 0.1 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            heads.len(),
            1,
            "the pin glyph was not painted exactly once, so this test is about to click \
             somewhere else: {heads:?}"
        );
        /* `pin_glyph` puts the head at (c.x - 3, c.y - 5), 6 by 4.5. */
        Pos2::new(heads[0].left() + 3.0, heads[0].top() + 5.0)
    }

    /// A PIN GLYPH CLICKED IN ANY WINDOW REACHES THE FILE, NOT JUST THIS SESSION.
    ///
    /// `the_pip_pin_and_close_reach_the_registry` holds the first hop: the glyph is painted,
    /// clicked, and applied to `Window::pin`. That was the whole journey, and it ended in memory.
    /// This is the second hop, which is the one the owner feels: the click lands inside a deferred
    /// viewport's callback, which holds this registry's lock and cannot reach `Settings` at all, so
    /// it leaves the answer on the window and the next root pass writes it down.
    ///
    /// BOTH BODIES, BECAUSE THERE ARE TWO AND THEY ARE DIFFERENT CODE. The Watch window is the
    /// picture in picture: no strip, a chip in the corner drawn only on hover, and it is the window
    /// the owner actually pins over a running game. The other four carry `titlebar::strip`. One
    /// handler covered and the other missed is this tree's commonest half-fix, so the same journey
    /// is driven through each.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `w.pin_out = Some(..)` line from EITHER
    /// `hits.pin` handler in `draw_child`. Every assertion in the pip tests stays green under both.
    #[test]
    fn a_pin_glyph_clicked_in_any_window_reaches_the_settings_file() {
        let live = status(Some(false));
        for slot in [Slot::Watch, Slot::Parser] {
            let size = if slot.is_pip() {
                Vec2::new(480.0, 270.0)
            } else {
                Vec2::new(620.0, 460.0)
            };
            let mut w = seeded(live.clone());
            {
                let mut g = lock(&w.inner);
                g.windows[slot.index()].open = true;
            }
            let tool = lock(&w.inner).windows[slot.index()].screen.tool();

            let child = themed();
            /* THE PICTURE IN PICTURE HIDES ITS CHROME UNTIL THE POINTER ARRIVES, so its chip
             * cannot be found in a resting pass and is read from the layout constants the whole
             * pip suite uses. The strip is always painted, so that one is found. */
            let at = if slot.is_pip() {
                chip_at(size, 1.0)
            } else {
                pin_head_centre(&child_pass(&w, slot, &child, size, Vec::new()))
            };
            child_pass(&w, slot, &child, size, vec![egui::Event::PointerMoved(at)]);
            child_pass(&w, slot, &child, size, click_at(at));
            assert!(
                w.is_pinned(tool),
                "{slot:?}: the pin glyph was clicked at {at:?} and the window is not even on top, \
                 so nothing below this is measured"
            );

            let mut settings = Settings::default();
            assert!(
                !settings.win_pinned(slot.id(), slot.pin_default()),
                "{slot:?}: the fixture starts unpinned"
            );
            let root = themed();
            root_pass_with(&mut w, &root, &live, &mut settings);
            assert!(
                settings.win_pinned(slot.id(), slot.pin_default()),
                "{slot:?}: the pin glyph was clicked and the settings file never heard, so the \
                 window comes back unpinned on the next launch: {:?}",
                settings.windows
            );

            /* AND THE PIN COST NOBODY A RESCAN. `sync` answers a root settings JSON that differs
             * from `settings_synced` by calling `Ingest::reconfigure`, which folds the last 40MB
             * of the log on a worker whatever it was that changed. The pass that writes the pin is
             * the pass that puts the two in step, so there is nothing left for that arm to do; if
             * it were left owing, every click of this glyph would re-scan the log, and the owner
             * clicks it mid raid. */
            assert_eq!(
                lock(&w.inner).settings_synced,
                settings_json(&settings).unwrap_or_default(),
                "{slot:?}: the root and the tool windows were left out of step over a pin, so the \
                 next pass answers it with a full log rescan"
            );
        }
    }

    /// One real pass of a whole tool window, through the callback egui runs for it.
    fn child_pass(
        w: &Windows,
        slot: Slot,
        ctx: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let inner = Arc::clone(&w.inner);
        let mut out = ctx.run_ui(input, |ui| {
            draw_child(&inner, slot, ui, ViewportClass::Deferred);
        });
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        flat
    }

    /// EVERY SLOT BUT WATCH STILL DRAWS ITS TITLE STRIP, AND WATCH DRAWS NO STRIP AT ALL.
    ///
    /// `Slot` holds more than Watch. The Parser, Plane of Sky and LFG windows are ordinary tool
    /// windows: a screen in a frame, with a bar naming which screen, and that bar is still right
    /// for them. Turning one of THEM into a picture would be the same mistake in the other
    /// direction, so this is read out of a real frame per slot rather than trusted to a match arm.
    ///
    /// THE TITLE IS THE ANCHOR because it is what a strip is for: `titlebar::strip` paints
    /// `Lead::Title(t)` in Cinzel at the left end, and the Watch window's name is the taskbar's
    /// now (`Tool::title`) and is drawn nowhere on its face.
    #[test]
    fn only_the_windows_that_have_a_strip_paint_their_title() {
        for slot in SLOTS {
            let w = seeded(status(Some(false)));
            {
                let mut g = lock(&w.inner);
                g.windows[slot.index()].open = true;
            }
            let ctx = themed();
            let shapes = child_pass(&w, slot, &ctx, Vec2::new(620.0, 460.0), Vec::new());
            let words: Vec<String> = shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            let title = SLOTS
                .iter()
                .find(|s| **s == slot)
                .map(|_| lock(&w.inner).windows[slot.index()].screen.tool().title())
                .expect("every slot has a tool");
            /* TWO BODIES: the picture and the title strip. The combat overlays were a third for
             * one build and are not slots any more (D11 stage two); `draw_overlay` is their body
             * and they never reach `draw_child`. */
            if slot.is_pip() {
                assert!(
                    !words.iter().any(|t| t == title),
                    "{slot:?} is the picture in picture and it painted its own title {title:?}: \
                     {words:?}"
                );
            } else {
                assert!(
                    words.iter().any(|t| t == title),
                    "{slot:?} lost the title strip that names it; it should still say {title:?}: \
                     {words:?}"
                );
            }
        }
    }

    /// ONLY THE WATCH SLOT IS A PICTURE, SAID ONE SLOT AT A TIME.
    ///
    /// `draw_child` asks `Slot::is_pip` and takes one of two whole bodies from the answer, so this
    /// is the fork the window's identity hangs on and it is worth an assertion of its own that a
    /// reader can check against the list.
    #[test]
    fn only_the_watch_window_is_a_picture() {
        assert!(Slot::Watch.is_pip());
        assert!(!Slot::Parser.is_pip());
        assert!(!Slot::Sky.is_pip());
        assert!(!Slot::Lfg.is_pip());
        assert_eq!(
            SLOTS.iter().filter(|s| s.is_pip()).count(),
            1,
            "a second window became a picture"
        );
    }

    /// THE WATCH WINDOW OPENS AT THE PICTURE'S OWN SHAPE.
    ///
    /// It is the picture, so a first-open size that is not 16 by 9 hands the reader a letterbox
    /// before they have touched anything: at the old 520 by 380 the channel's 1920 by 1080 offline
    /// screen arrived with a band about 44 points deep above and below it. The three tool windows
    /// are pages of controls and are deliberately not held to this.
    #[test]
    fn the_picture_window_opens_sixteen_by_nine() {
        let s = Slot::Watch.default_size();
        assert!(
            (s.x / s.y - 16.0 / 9.0).abs() < 0.01,
            "the Watch window opens at {s:?}, which is not the picture's shape"
        );
        assert!(
            s.x >= 320.0 && s.y >= 200.0,
            "the Watch window opens smaller than its own minimum: {s:?}"
        );
    }

    /// THE PIN AND THE CLOSE REACH THE REGISTRY, WHICH IS THE HALF `pip` CANNOT PROVE.
    ///
    /// `pip` reports a click in a `ChipHits`; the level, the open flag and the ask are the
    /// REGISTRY's, because the registry is the one owner of "on top" and of what is open. A window
    /// whose pin painted and reported perfectly while nothing applied it would pass every
    /// assertion in the pip tests above, which is the exact shape of defect this tree keeps
    /// finding, so this drives `draw_child` and reads the registry's own state back.
    ///
    /// AND CLOSING IS NOT UNPINNING. `open_toggle_pin_state_machine` holds that for the summon
    /// path; this holds it for the window's own close glyph, which is the only way out of an
    /// undecorated window that does not go through the keyboard.
    #[test]
    fn the_pip_pin_and_close_reach_the_registry() {
        let size = Vec2::new(480.0, 270.0);
        let pin_at = chip_at(size, 1.0);
        let close_at = chip_at(size, 0.0);

        let w = seeded(status(Some(false)));
        {
            let mut g = lock(&w.inner);
            g.windows[Slot::Watch.index()].open = true;
        }
        let ctx = themed();
        assert!(!w.is_pinned(Tool::Watch), "it starts unpinned");

        child_pass(
            &w,
            Slot::Watch,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(pin_at)],
        );
        child_pass(&w, Slot::Watch, &ctx, size, click_at(pin_at));
        assert!(
            w.is_pinned(Tool::Watch),
            "the pin chip was clicked and the window is not on top"
        );

        child_pass(
            &w,
            Slot::Watch,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(close_at)],
        );
        child_pass(&w, Slot::Watch, &ctx, size, click_at(close_at));
        assert!(
            !w.is_open(Tool::Watch),
            "the close glyph was clicked and the window is still open"
        );
        assert!(
            w.is_pinned(Tool::Watch),
            "closing the window unpinned it; the pin belongs to the window, not to this session"
        );
    }

    /// THE PICTURE WINDOW STILL TICKS WITH THE MAIN WINDOW MINIMISED OR HIDDEN.
    ///
    /// That independence is the whole reason these are DEFERRED viewports rather than immediate
    /// ones (module doc, decision D3), and a deferred viewport repaints only when it is asked to.
    /// The live state is what this window draws, so a pass that asked for no further repaint would
    /// leave a channel that went live showing a picture that says otherwise until somebody touched
    /// the window.
    ///
    /// THE FIRST PASSES ARE NOT THE MEASUREMENT, AND THE VERSION OF THIS TEST THAT READ THEM COULD
    /// NOT FAIL. A fresh context uploads its font atlas and lays everything out for the first
    /// time, and egui asks for an IMMEDIATE repaint on its own while that settles; a delay read
    /// from frame one is zero whatever the window asked for, which was measured: deleting the
    /// `request_repaint_after` left the test green. So the passes are run until nothing is
    /// settling, and both ends are asserted. Above zero says the frame has settled and this is the
    /// window's own request rather than egui's; at or under a second says the request is the tick.
    /// Without the call the settled delay is egui's "nothing to do", which is neither.
    #[test]
    fn the_picture_window_keeps_asking_to_be_repainted() {
        let w = seeded(status(Some(false)));
        {
            let mut g = lock(&w.inner);
            g.windows[Slot::Watch.index()].open = true;
        }
        let ctx = themed();
        let inner = Arc::clone(&w.inner);
        let mut delay = Duration::ZERO;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 270.0))),
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| {
                draw_child(&inner, Slot::Watch, ui, ViewportClass::Deferred);
            });
            delay = out
                .viewport_output
                .values()
                .map(|v| v.repaint_delay)
                .min()
                .expect("a pass produces viewport output");
            out.drop_without_applying_deltas();
        }
        assert!(
            delay > Duration::ZERO,
            "the frame is still settling after four passes, so this reads egui's own repaint and \
             not the window's, and would pass with the tick deleted"
        );
        assert!(
            delay <= Duration::from_secs(1),
            "the picture window asked for its next repaint in {delay:?}; the live state it draws \
             would go stale behind a minimised main window"
        );
    }
}
