//! Screen: DASHBOARDS. The page somebody LANDS on, and the way in to the rest.
//!
//! The owner said it in one line: "Dashboard should probably be the top of nav... cause dashboard."
//!
//! # A DASHBOARD IS A GRID OF WINDOWS ONTO THE LARGER PAGES, AND IT USED TO BE EIGHT ROLES
//!
//! This page shipped as `DPS / Healer / Tank / Pet / Solo / Group / Raid Leader / Custom`: eight
//! tabs, each a different ORDER of the same four or five panels. The owner's verdict on that, in
//! his words, was that roles go entirely, and he is right about why. A role is a claim about the
//! PERSON reading, and this app cannot make one: nothing in a log line says what anybody was
//! trying to do, so `Healer` was a tab that reordered a damage meter and then apologised in a
//! hover for having no healing breakdown. Eight tabs, two of which refused outright, over one
//! set of panels.
//!
//! WHAT REPLACED IT IS WHAT A DASHBOARD ACTUALLY IS: a grid of TILES, each one a window onto a
//! larger page of this application, which the reader arranges himself. See [`Tile`]. That is the
//! owner's own framing: "we should assume the dashboard is NOT live... a dashboard is just a
//! dashboard of larger pages in the application, and we should have a widget for each of the
//! larger pages."
//!
//! # THE LAYOUT IS THE READER'S AND IT IS SAVED
//!
//! [`Slot`] is one tile and how many columns it spans; `Settings::dashboard` is the list of them,
//! in his order. Unlocked, a tile's head drags to reorder and its right edge drags to resize;
//! locked, neither is drawn at all, because a page you read every night should not rearrange
//! itself under a stray click. The lock and the widget picker are on the context header, which is
//! where the owner asked for them.
//!
//! EVERY REARRANGEMENT IS A PURE FUNCTION AND THE POINTER ONLY CHOOSES ARGUMENTS. [`move_to`],
//! [`resize_at`] and [`drop_index`] hold all of it, so the rules a reader can break by dragging
//! are proven by tests rather than by looking at the screen.
//!
//! # A tile is an ORDER and a SELECTION, never a new renderer
//!
//! `overlay::Widget` already says what a combat panel IS and `screens::dps::draw_widget` is the
//! one thing that draws one. NOT ONE COMBAT NUMBER ON THIS PAGE IS COMPUTED HERE. That is not
//! tidiness: two implementations of one figure is the defect this tree is built to refuse, and it
//! is the defect that reaches the owner's stream, because the page and the overlay beside it would
//! be showing the same fight under the same word with two different numbers on them.
//!
//! # What this page refuses to draw, and why each refusal is a measurement
//!
//!   * NO PET TILE. See [`PET_IS_NOT_BUILDABLE`]: measured over the reference capture, not
//!     assumed. Nothing in a log line links a pet to an owner.
//!   * NO RAID CONCEPT. Nothing in a log line says a fight was a raid, so there is no roster tile
//!     and no attendance tile.
//!   * NO HEAL BREAKDOWN AND NO HEAL TIMELINE. `Fighter::abilities` is "every named source of
//!     DAMAGE" and `Fighter::series` is damage per second; neither keeps a heal. The healing tile
//!     is a ranked table of healing done and nothing more, because that is what the log carries.
//!   * NO CHART ACROSS A NIGHT FROM THE TAIL. `Ingest::fights` is written once, at the scan.
//!     [`Tile::Night`] counts what the STORE holds instead, which is every fight this character
//!     has finished and had written to disk, and it says which of the two it is counting.
use crate::chrome::State;
use crate::fights::FightRow;
use crate::ingest::{tail_cap_text, Ingest};
use crate::nav::ScreenId;
use crate::overlay::{Cols, Detail, Metric, Ranked, Subject, Widget};
use crate::screens::dashgrid::{self, Cell, Edge, Placement};
use crate::screens::night::{self, Filter, When};
use crate::screens::parser::{no_fights_words, why_no_fights, NoFights};
use crate::screens::{Ask, Cx};
use crate::theme::*;
use chrono::{DateTime, Utc};
use egui::{Align, Layout, Pos2, Rect, RichText, Sense, Ui, UiBuilder, Vec2};
use std::time::Duration;

/// WHY THERE IS NO PET DASHBOARD, MEASURED RATHER THAN ASSUMED.
///
/// A CONSTANT AND NOT A COMMENT, for the reason any recorded gap is one: a
/// reader looking for the gap finds it from the code that has it, a test can read the words, and
/// the day a log arrives that CAN support the tile, this goes with the change that builds it.
///
/// THE MEASUREMENT, over the reference capture (`web/fixtures/eqlog-tail-200k.txt`, 2,414 lines,
/// the same bytes `fights::probe::CAPTURE` folds):
///
///   * ONE entity in the whole file has "pet" in its name, `Reclusive ghoul magus pet`, and it is
///     an enemy: it punches the reader, the reader cleaves it, and the reader kills it. There is no
///     player pet in the capture at all.
///   * ZERO lines of the form `<somebody>'s pet`. Nothing anywhere links a pet to an owner, so
///     there is no line to read an ownership off.
///   * ZERO pet speech: no `Master.`, no `says, 'Attacking'`. The one line matching "Master" is a
///     player asking in chat what a key is for.
///   * ZERO charm events: the one line matching "charm" is an NPC reciting the Qeynos anthem.
///
/// SO A PET TILE COULD ONLY BE BUILT BY GUESSING WHICH MOB WAS SOMEBODY'S, and a guess here does
/// not come out as a blank panel, it comes out as somebody else's damage in the owner's own row
/// while he is streaming. What would settle it is a log with a pet in it: the lines the client
/// writes when a pet is summoned, ordered and dismissed are the grammar this needs.
///
/// IT IS NO LONGER A TAB THAT SAYS THIS, IT IS A TILE THAT DOES NOT EXIST, which is the more
/// honest shape: a control whose only behaviour is to refuse is still a control.
pub const PET_IS_NOT_BUILDABLE: &str =
    "Nothing in a log line links a pet to an owner. In the reference capture the only entity \
     named `pet` is an enemy, there is no line of the form `somebody's pet`, no pet speech and no \
     charm event. A tile here would have to guess which mob was yours, and a guess puts another \
     player's damage in your row. It arrives with a log that has a pet in it.";

/// HOW MANY ROWS A RANKED TILE DRAWS.
///
/// A group is five and this is the cap `screens::live` uses for the same tables, so a fight with a
/// few strays in it still fits without the page deciding for the reader which of them mattered.
const GROUP_CAP: usize = 12;

/// HOW MUCH OF A CARD'S HEAD BELONGS TO ITS BUTTONS AND NOT TO THE DRAG HANDLE.
///
/// Wide enough for `open`, `+`, `-` and `x` at the head's font with the theme's spacing, and it is
/// a constant because the handle and the buttons must be cut from one number: two numbers is how
/// a control ends up half draggable. See the handle in `DashboardsScreen::card`.
const HANDLE_KEEP: f32 = 110.0;

/// THE NARROWEST A HEAD GRIP MAY BE, in points, so a tile can always be picked up.
///
/// DEFECT: THE GRIP WAS `head.width() - HANDLE_KEEP` AND IT WENT TO ZERO. Any tile narrower than 110
/// points (a span-2 tile at the owner's width, every span-3 tile at the window's floor) had a
/// zero-width grip on its left edge, which egui's hit test never picked over the W and NW
/// resize zones that share those pixels. Such a tile could be resized and never moved again.
const GRIP_MIN: f32 = 24.0;

/// How thick an edge's resize strip is, and how big a corner's square is. Shared by the zones
/// and by the grip that has to stay clear of them; two spellings is how they drift apart.
const EDGE_T: f32 = 6.0;
const CORNER_K: f32 = 14.0;

/// WHERE A TILE'S DRAG HANDLE IS, given its head. Pure, so it is tested at every width.
///
/// CUT FROM THE HEAD'S INTERIOR: in from the corners by [`CORNER_K`] and down from the top by
/// [`EDGE_T`], so it never shares a pixel with a resize zone; and never narrower than [`GRIP_MIN`],
/// so it never vanishes. What it gives up on a narrow head is the room the buttons had, which
/// is the right trade: a card you cannot move is a card you cannot fix.
pub(crate) fn grip_rect(head: Rect) -> Rect {
    let inner_l = head.left() + CORNER_K;
    let inner_r = head.right() - CORNER_K;
    let lo = inner_l + GRIP_MIN;
    let hi = inner_r.max(lo);
    let right = (head.right() - HANDLE_KEEP).clamp(lo, hi);
    Rect::from_min_max(
        Pos2::new(inner_l, head.top() + EDGE_T),
        Pos2::new(right, head.bottom()),
    )
}

/* ------------------------------------------------------------------------ the tiles -- */

/// A TILE: ONE WIDGET ON THE DASHBOARD, AND A WINDOW ONTO ONE LARGER PAGE.
///
/// # WHY THE LIST IS HAND WRITTEN
///
/// [`Tile::ALL`] is typed out rather than derived, so a variant added to this enum has to be
/// offered deliberately. A tile that exists and is not in `ALL` is a panel nobody can ever add,
/// which is this tree's signature defect; `every_tile_is_offered_and_placed` is what says so.
///
/// # EVERY TILE IS BACKED BY SOMETHING THE LOG ACTUALLY STATES
///
/// There is no tile here for a number this app cannot read. That is the house rule and it is the
/// reason the list is shorter than the design's: no threat, no roster, no attendance, no pet, no
/// mob health percentage that was not measured off finished kills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tile {
    /// The mark, the headline, the clock and what is being hit right now.
    Live,
    /// Everyone in the current fight, ranked by damage dealt.
    Damage,
    /// Everyone in the current fight, ranked by healing done.
    Healing,
    /// Everyone in the current fight, ranked by damage taken.
    Taken,
    /// Damage second by second within one fight, a line per fighter, with its events flagged.
    Timeline,
    /// Your damage per second, one point per fight across the scope.
    Progression,
    /// What the reader's own damage was made of: three readings behind tabs in the card's head.
    Yours,
    /// The fights the bootstrap scan found, newest first.
    Fights,
    /// What has been written to disk for this character: fights, minutes, span.
    Night,
    /// What a mob has been measured to absorb, off kills that finished.
    Mobs,
    /// The named kills the tracker has seen.
    Kills,
    /// What dropped.
    Loot,
    /// The overlays the owner built himself, drawn at page density.
    Overlays,
}

impl Tile {
    /// Every tile, in the order a fresh install meets them. See the type note.
    pub const ALL: [Tile; 13] = [
        Tile::Live,
        Tile::Damage,
        Tile::Healing,
        Tile::Taken,
        Tile::Timeline,
        Tile::Progression,
        Tile::Yours,
        Tile::Fights,
        Tile::Night,
        Tile::Mobs,
        Tile::Kills,
        Tile::Loot,
        Tile::Overlays,
    ];

    /// THE NAME THIS TILE IS SAVED UNDER, and it is a string on purpose.
    ///
    /// `Settings::dashboard` stores these rather than the enum, so a layout written by a NEWER
    /// build loads on an older one: [`Tile::of`] returns `None` for an id it does not know and
    /// [`layout`] drops that slot. The alternative is a serde enum whose unknown variant fails the
    /// whole settings file, taking the reader's hotkeys and log folder down with a tile.
    ///
    /// SHORT, LOWER CASE AND NEVER THE LABEL. A label is what the page calls a tile today; an id is
    /// what a file on the owner's disk calls it forever.
    pub fn id(self) -> &'static str {
        match self {
            Tile::Live => "live",
            Tile::Damage => "damage",
            Tile::Healing => "healing",
            Tile::Taken => "taken",
            Tile::Timeline => "timeline",
            Tile::Progression => "progression",
            Tile::Yours => "yours",
            Tile::Fights => "fights",
            Tile::Night => "night",
            Tile::Mobs => "mobs",
            Tile::Kills => "kills",
            Tile::Loot => "loot",
            Tile::Overlays => "overlays",
        }
    }

    /// The tile with this id, or `None` for one this build does not have. See [`Tile::id`].
    pub fn of(id: &str) -> Option<Tile> {
        Tile::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The heading on the card, and the words in the picker. One list, so the two cannot drift.
    pub fn label(self) -> &'static str {
        match self {
            Tile::Live => "Live fight",
            Tile::Damage => "Damage",
            Tile::Healing => "Healing",
            Tile::Taken => "Damage taken",
            Tile::Timeline => "Timeline",
            Tile::Progression => "Progression",
            Tile::Yours => "Your damage",
            Tile::Fights => "Recent fights",
            Tile::Night => "Written to disk",
            Tile::Mobs => "Mob health",
            Tile::Kills => "Named kills",
            Tile::Loot => "Loot",
            Tile::Overlays => "Your overlays",
        }
    }

    /// THE MOCK'S OWN GLYPH FOR THIS WIDGET, out of its `.widget-card-icon` set.
    ///
    /// THREE ARE THE MOCK'S EXACTLY, because the mock ships the same three widgets: Named kills is
    /// its crown, Motes rate its star, Item drops its diamond. The rest are chosen in the same
    /// idiom, which is one heavy geometric or pictographic character that reads at eleven points.
    ///
    /// NOTHING HERE IS OUTSIDE THE BASIC MULTILINGUAL PLANE and nothing is an emoji. An emoji is
    /// rendered from a colour font this app does not ship, so on the owner's machine it would come
    /// out as an empty box; these are all glyphs the bundled face has.
    pub fn glyph(self) -> &'static str {
        match self {
            Tile::Live => "\u{25c9}",
            Tile::Damage => "\u{2694}",
            Tile::Healing => "\u{271a}",
            Tile::Taken => "\u{25c8}",
            Tile::Timeline => "\u{223f}",
            Tile::Progression => "\u{2197}",
            Tile::Yours => "\u{25b3}",
            Tile::Fights => "\u{25a4}",
            Tile::Night => "\u{25a3}",
            Tile::Mobs => "\u{2665}",
            Tile::Kills => "\u{265b}",
            Tile::Loot => "\u{25c6}",
            Tile::Overlays => "\u{25f0}",
        }
    }

    /// THE `.panel-sub` BESIDE THE HEADING: what this card is OVER, in the mock's own idiom.
    ///
    /// The mock heads its roster `Performance at playhead` and then, in `12px --muted` beside it,
    /// `4 combatants \u{b7} Damage done \u{b7} playhead`. That second line is not decoration: a
    /// dashboard is a page of cards and the one thing a reader cannot tell from a card's title is
    /// WHICH FIGHT, WHICH SCOPE or WHICH SOURCE it is over.
    ///
    /// `None` WHERE THE TITLE IS ALREADY THE WHOLE ANSWER. `The log` is over the log; there is no
    /// second thing to say and a sub there would be filler.
    pub fn sub(self) -> Option<&'static str> {
        match self {
            Tile::Live => Some("live \u{b7} the fight going now"),
            /* THE TAB NAMES THE READING NOW, so the sub only says the scope. It read
             * `damage done \u{b7} in scope` beside a lit `DEALT` tab, which is the same fact
             * twice in one head that had run out of room. */
            Tile::Damage | Tile::Taken | Tile::Healing => Some("in scope"),
            Tile::Timeline => Some("per second \u{b7} in scope"),
            Tile::Progression => Some("your dps, fight by fight"),
            Tile::Yours => Some("you \u{b7} in scope"),
            Tile::Fights => Some("in scope, newest first"),
            /* NO SUB. `Written to disk` says it; `this app's own store` said it again. */
            Tile::Night => None,
            /* NO SUB. `finished kills` was the card telling a reader about its own
             * method before it had told him the number, and `Mob health` already says what
             * it is. The
             * hover on the figure carries the method, which is where a method belongs. */
            Tile::Mobs => None,
            Tile::Kills => Some("in scope"),
            Tile::Loot => Some("as the client printed it"),
            Tile::Overlays => Some("read only"),
        }
    }

    /// WHAT THIS TILE IS A WINDOW ONTO: the destination, and the section of it by NAME.
    ///
    /// BY NAME AND NOT BY INDEX, for the reason the last literal section index in this tree went:
    /// the parser's section order has already moved once, and an index would quietly open the
    /// wrong page rather than fail. `main::App::answer` resolves it against `nav::SECTIONS` and
    /// `every_tile_opens_a_section_that_exists` is what keeps every one of these a real place.
    ///
    /// `None` FOR THE SECTION WHERE THE DESTINATION HAS NO SECTIONS, which is a real answer: the
    /// kill tracker and the loot list are top level rows.
    pub fn page(self) -> (ScreenId, Option<&'static str>) {
        match self {
            Tile::Live
            | Tile::Damage
            | Tile::Healing
            | Tile::Taken
            | Tile::Timeline
            | Tile::Progression => (ScreenId::Parser, Some("Live")),
            Tile::Yours | Tile::Fights | Tile::Mobs => (ScreenId::Parser, Some("Fights")),
            Tile::Night | Tile::Overlays => (ScreenId::Parser, Some("Reports")),
            Tile::Kills => (ScreenId::KillTracker, None),
            Tile::Loot => (ScreenId::Loot, None),
        }
    }

    /// THE READINGS THIS TILE KEEPS BEHIND TABS IN ITS HEAD, or none for a tile with one.
    ///
    /// # WHEN A CARD IS TABBED AND WHEN IT IS THREE CARDS
    ///
    /// TABS WHEN THE READINGS ARE ALTERNATIVES; separate cards when a reader wants them at once.
    /// `Your damage` is three answers to one question, what your damage was made of, and nobody
    /// needs all three in front of them simultaneously: that is a tabbed card. Damage, Healing
    /// and Damage taken are different PEOPLE's contributions and a raid leader genuinely wants
    /// them side by side: those stay three cards.
    ///
    /// THIS IS THE MOCK'S OWN CONTROL. Section 7 gives `.panel-actions` a row of `.metric-tab`s,
    /// and `theme::metric_tab` is that control; `theme::panel_head`'s `right` closure is that
    /// slot. A stacked card without them was three panels in one box, which could not obey the
    /// rule the rest of the page obeys (draw what fits, say what you left out) because a fit
    /// rule cannot sensibly cut across a stack.
    pub fn tabs(self) -> &'static [&'static str] {
        match self {
            Tile::Yours => &["Abilities", "Targets", "Hit results"],
            /* EVERY ROSTER OFFERS ALL FOUR READINGS, AND THE OWNER IS RIGHT THAT IT SHOULD.
             *
             * These four are one table with a different column summed: `Metric` is a FIELD
             * SELECTOR (see `overlay::Metric`) and nothing else about the card changes between
             * them. A reader who wants damage and healing side by side puts two of these cards
             * on the page and points them at different readings; a reader with one card can
             * still reach all four. Making them tabs costs the side by side nothing and buys
             * the switch.
             *
             * SO THE THREE TILES ARE THREE DEFAULTS AND NOT THREE CAPABILITIES. `Tile::metric`
             * is the tab each one opens on, and after that they are the same card. */
            /* ONE WORD EACH, because four of these share the head with the card's own name and
             * its controls. `Damage done / Damage taken / Healing done / Healing taken` was
             * right in a sentence and pushed the column heads off a span-4 card. These are the
             * words `Metric::unit` already uses for the same four readings. */
            Tile::Damage | Tile::Taken | Tile::Healing => &["Dealt", "Taken", "Healed", "Received"],
            _ => &[],
        }
    }

    /// WHICH READING THIS TILE OPENS ON, as an index into [`Tile::tabs`].
    ///
    /// The three roster tiles differ in this and in nothing else. See [`Tile::tabs`].
    pub fn opens_on(self) -> usize {
        match self {
            Tile::Taken => 1,
            Tile::Healing => 2,
            _ => 0,
        }
    }

    /// THE READING A ROSTER TAB NAMES. The one place a tab index becomes a metric.
    pub fn roster_metric(tab: usize) -> Metric {
        match tab {
            1 => Metric::Taken,
            2 => Metric::Healed,
            3 => Metric::Received,
            _ => Metric::Dealt,
        }
    }

    /// WHERE THIS TILE SITS BEFORE THE READER HAS AN OPINION: column, span, row, rows.
    ///
    /// # THE MOCK'S OWN PLACEMENT, ON THE MOCK'S OWN GRID
    ///
    /// Section 6 of the spec places nine tiles by `col / span / row / rows` on a twelve column
    /// grid of ten point rows. Those nine are mapped onto the tiles this build has and the five
    /// the mock does not place are packed under them, so the shipped dashboard has no hole in it
    /// and nothing overlaps. `the_shipped_dashboard_fits_the_grid_with_nothing_on_top_of_anything`
    /// is what holds that.
    ///
    /// THIS IS THE ONLY PLACE A DEFAULT POSITION IS DECIDED. A tile added later cannot arrive at
    /// a position chosen by whichever branch of the layout code ran first; it arrives here or it
    /// does not compile.
    pub fn place(self) -> Cell {
        /* FOUR BANDS, EACH EXACTLY TWELVE COLUMNS WIDE, EACH ONE HEIGHT.
         *
         * The owner's rule: the defaults have to make sense horizontally AND vertically. So
         * every band fills the width with no gap, every tile in a band is the band's height,
         * and the bands stack with no gap. Rows 1..31 is the mock's own performance panel
         * height; the right column of that band is the live card over the progression chart,
         * stacked to the same 31 rows.
         *
         *   rows  1..30   Timeline (12), which pays for its three flag lanes
         *   rows 31..61   Damage (7)             | Live (5, 12 rows) over Progression (5, 19)
         *   rows 62..83   Taken (4) | Healing (4) | Fights (4)
         *   rows 84..104  Named kills (5) | Loot (6) | Mob health (1, 10 rows)
         *                                          | Written to disk (1, 11 rows)
         *   rows 105..126 Your damage (6) | Your overlays (6)
         *
         * A roster needs four columns to hold its five column head at the owner's width, which
         * is why the second band is thirds of four and not quarters of three. */
        match self {
            /* THE TIMELINE OPENS THE PAGE, directly under the scope strip.
             *
             * The owner put it there and the reason holds up: it is the only card that answers
             * `what HAPPENED`, and every other card on the page answers `how much`. A reader
             * looking back at a night wants the shape of it before he wants anybody's total, and
             * the flags on it (a kill, a death, a mez) are what he navigates by. */
            Tile::Timeline => Cell::new(1, 12, 1, 30),
            Tile::Damage => Cell::new(1, 7, 31, 31),
            Tile::Live => Cell::new(8, 5, 31, 12),
            Tile::Progression => Cell::new(8, 5, 43, 19),
            /* THE TIMELINE TAKES A BAND OF ITS OWN, full width. It is the mock's own biggest
             * chart and the one that earns the room: five lines, a clock and its flags. */
            Tile::Taken => Cell::new(1, 4, 62, 22),
            Tile::Healing => Cell::new(5, 4, 62, 22),
            Tile::Fights => Cell::new(9, 4, 62, 22),
            /* THE TWO STAT CARDS SHARE ONE NARROW STRIP AND THE TWO LISTS TAKE THE REST.
             *
             * `Mob health` and `Written to disk` hold one figure each. Given a third of a band
             * apiece they were a short line of text in the top left corner of a wide empty box,
             * which is what the owner was looking at when he called it cut off and far too wide.
             * Two columns is as narrow as the longest line in either of them, and seven and nine
             * rows are a head and the figures with nothing spare.
             *
             * STACKED RATHER THAN SHRUNK WHERE THEY WERE, because a short card in the middle of
             * a band leaves a hole under it and this page's own guard walks every square of the
             * shipped layout and refuses one. Together they fill the strip exactly.
             *
             * AND THE ROOM THEY GAVE UP GOES WHERE THE WORDS ARE: `Loot` prints item names the
             * client wrote (`Fine Steel Rapier +4`) and `Named kills` prints mob names, and both
             * were eliding. They take five columns each and the band is five rows deeper.
             *
             * EIGHT AND NINE ROWS AND NOT SEVEN AND NINE, and a guard is why:
             * `no_card_ships_shorter_than_the_thing_it_draws` measures what each of these two
             * paints against the box it drew for itself, and it caught `Mob health` overflowing
             * its card by three points the first time this strip was cut. Neither figure here is
             * a guess. */
            Tile::Kills => Cell::new(1, 5, 84, 21),
            Tile::Loot => Cell::new(6, 6, 84, 21),
            Tile::Mobs => Cell::new(12, 1, 84, 10),
            Tile::Night => Cell::new(12, 1, 94, 11),
            Tile::Yours => Cell::new(1, 6, 105, 22),
            /* THE LAST BAND IS HALVES SINCE `Who is here` WENT. It was a count of casters with
             * their names under it, which is a fact about the LOG rather than about the raid,
             * and the owner cut it. Its columns went to the overlays card, which is drawing real
             * meters and wanted the width. */
            Tile::Overlays => Cell::new(7, 6, 105, 22),
        }
    }

    /// WHERE THIS TILE SITS IN AN ARRANGEMENT, or `None` when that arrangement has no room for
    /// it. See [`Size`] for why there are three of them and how one is chosen.
    ///
    /// # THE SHORT TABLES ARE NOT THE LONG ONE WITH ROWS DELETED
    ///
    /// Dropping nine cards out of the full page and leaving the other four where they were
    /// would leave four cards and a screenful of holes, and this page's own rule for a default
    /// is that every square from the top row to the bottom of the lowest tile is covered by
    /// exactly one tile. So each arrangement is its own table, and
    /// [`tests::a_dashboard_of_any_size_fits_the_grid_with_nothing_on_top_of_anything`] walks
    /// every square of all three.
    pub fn place_in(self, size: Size) -> Option<Cell> {
        match size {
            Size::Large => Some(self.place()),
            /* THE OWNER'S OWN SHORT LIST, ASKED AND ANSWERED: `Timeline + Damage + Live +
             * Recent fights`. The timeline still opens the page and still takes a band of its
             * own; damage takes the left seven columns under it and the live fight and the
             * fight list share the right five. That is the full page's own first band with the
             * fight list standing where progression stands there. */
            Size::Small => match self {
                Tile::Timeline => Some(Cell::new(1, 12, 1, 22)),
                Tile::Damage => Some(Cell::new(1, 7, 23, 26)),
                Tile::Live => Some(Cell::new(8, 5, 23, 11)),
                Tile::Fights => Some(Cell::new(8, 5, 34, 15)),
                _ => None,
            },
            /* AND THEN THE TWO ROSTERS AND THE CHART THE SHORT LIST GAVE UP. Healing and damage
             * taken are the pair a reader reads against damage dealt, so they arrive together,
             * in a band of thirds with the fight list, which is the full page's third band
             * exactly. Progression takes back the corner the fight list was borrowing. */
            Size::Medium => match self {
                Tile::Timeline => Some(Cell::new(1, 12, 1, 24)),
                Tile::Damage => Some(Cell::new(1, 7, 25, 30)),
                Tile::Live => Some(Cell::new(8, 5, 25, 12)),
                Tile::Progression => Some(Cell::new(8, 5, 37, 18)),
                Tile::Taken => Some(Cell::new(1, 4, 55, 22)),
                Tile::Healing => Some(Cell::new(5, 4, 55, 22)),
                Tile::Fights => Some(Cell::new(9, 4, 55, 22)),
                _ => None,
            },
        }
    }

    /// THE SHORTEST THIS TILE MAY BE MADE, in rows.
    ///
    /// `dashgrid::MIN_ROWS` is a head plus one line, which is honest for a list or a stat and a
    /// lie for a roster: a roster tile carries a 34 point foot under its scroll, so at six rows
    /// its body was `(70 - 40 - 34).max(0)` = 0 and it showed no rows at all. Thirteen rows is
    /// head, spacing, one combatant row and the foot.
    pub fn min_rows(self) -> u16 {
        match self {
            Tile::Damage | Tile::Healing | Tile::Taken => 14,
            /* MEASURED AND NOT GUESSED, by `no_card_slices_a_row_at_any_height_a_reader_can_drag_it_to`,
             * which draws each tile at every height from its floor to its shipped cell and refuses
             * a row that starts inside the card and finishes outside it. These four were sitting
             * on `dashgrid::MIN_ROWS`, which is a head and one line: true of a list and untrue of a
             * card that draws a fixed block. A floor that lies is how a card ends up cut off. */
            Tile::Live => 11,
            Tile::Mobs => 10,
            Tile::Kills => 10,
            Tile::Night => 11,
            /* A CHART NEEDS ITS AXES AND ITS KEY, which is head plus 44 points of y scale
             * plus 16 of clock plus something to draw in. */
            Tile::Timeline | Tile::Progression => 16,
            _ => dashgrid::MIN_ROWS,
        }
    }

    /// THE NARROWEST THIS TILE MAY BE MADE, in columns.
    ///
    /// THE COMPANION TO [`Tile::min_rows`], and it arrived for the same reason: a floor that
    /// belongs to the widget and not to the geometry. `dashgrid::MIN_SPAN` was two columns for
    /// everything, which is a sixth of the page, and it meant a card holding one number could
    /// not be made smaller than a card holding a five column roster. The owner asked three
    /// times for one of them to be narrower and the grid quietly widened it back every time.
    ///
    /// TWO IS STILL THE ANSWER FOR ALMOST EVERYTHING. A roster has a five column head, a chart
    /// has axes and a key, and a list has names in it; none of them mean anything at a sixth of
    /// their width. What changed is that the two cards which hold ONE FIGURE EACH are allowed to
    /// be the size of one figure.
    pub fn min_span(self) -> u8 {
        match self {
            /* A NUMBER AND THE WORDS FOR IT, WHICH IS ALL THESE HOLD. `stat_pair` writes its
             * caption a word to a line precisely so it survives being this narrow. */
            Tile::Night | Tile::Mobs => 1,
            _ => 2,
        }
    }

    /// WHAT THIS TILE READS, FOR ITS OWN HOVER.
    ///
    /// EVERY ONE OF THESE NAMES THE SOURCE and never the intention. A reader who does not trust a
    /// number on a dashboard wants to know where it came from, and "read from kills that finished"
    /// is an answer where "shows mob health" is a claim.
    pub fn blurb(self) -> &'static str {
        match self {
            Tile::Live => {
                "The fold's own quiet window over the end of the log: whether combat is still \
                 going, what is being hit, and for how long."
            }
            Tile::Damage => {
                "Damage dealt across the fights in scope, per second, off the same fold the \
                 Reports page uses."
            }
            Tile::Healing => {
                "Healing done across the fights in scope. There is no per-spell breakdown in \
                 this app: `Fighter::abilities` keeps every named source of DAMAGE and nothing \
                 keeps a heal."
            }
            Tile::Taken => "Damage taken across the fights in scope, per second, off the fold.",
            Tile::Timeline => {
                "One fight, second by second, a line per fighter, with engage, kills and player \
                 deaths flagged. A second by second chart is one fight's own clock, so this is \
                 the most recent fight in the scope; the filter picks another."
            }
            Tile::Progression => {
                "Your damage per second, one point per finished fight in scope, in time order. \
                 No line is fitted and no trend is claimed: a trend would be a statement about \
                 two fights being comparable, and nothing in a log line says they are."
            }
            Tile::Yours => {
                "Your damage across the fights in scope: what you used, what you hit, and how \
                 your swings ended, behind tabs in the card's head. `You` resolves through the \
                 character name in the log's FILE name, not through a line."
            }
            Tile::Fights => {
                "Every finished fight in the scope, newest first, off this app's own store. A \
                 fight still going is not one of them."
            }
            Tile::Night => {
                "What has been written to this app's own store for this character: every fight \
                 that finished, kept after the log's tail has moved past it."
            }
            Tile::Mobs => {
                "What a mob has been measured to absorb, from kills that finished. The log never \
                 states a mob's health; this is a median over agreeing kills and it refuses to \
                 answer until enough of them agree."
            }
            Tile::Kills => {
                "What died in the scope, most killed first, off each fight's own death marks."
            }
            Tile::Loot => "What dropped, newest first, as the client printed it.",
            Tile::Overlays => {
                "The overlays you built, drawn at page density. Read only: made and named where \
                 overlays are made."
            }
        }
    }
}

/* ------------------------------------------------------------------- the layout -- */

/// ONE TILE ON THE DASHBOARD AND HOW WIDE IT IS. `Settings::dashboard` is a list of these.
///
/// THE TILE IS A STRING, WHICH IS DELIBERATE AND IS EXPLAINED ON [`Tile::id`]: a settings file
/// written by a build with more tiles in it than this one has must still load.
/// HOW MUCH DASHBOARD THE WINDOW HAS ROOM FOR.
///
/// # THE OWNER'S QUESTION
///
/// "we either need to NOT allow scrolling and allow x y space depending on how the person has
/// it sized (thinking possibly allowing quarter 4k what we have right now half screen and full
/// screen there without resize? is that just like really bad idear or could be interesting".
///
/// It is the interesting one, and it is the other half of the fix that made a row a share of the
/// viewport. Rows dividing the height means the page always FITS. It does not mean thirteen
/// cards are worth looking at on a quarter of a 4K screen, where every one of them would be a
/// title and two lines of nothing. So the NUMBER of cards comes off the window too.
///
/// # THE THREE ARRANGEMENTS, AND WHO CHOSE THEM
///
/// The owner picked the small one himself, asked in as many words: the timeline, damage, the
/// live fight and recent fights. Medium adds the two rosters that short list gave up and the
/// progression chart. Large is the mock's own full page, which is what this app has shipped all
/// along and what a 4K screen still gets.
///
/// # HOW ONE IS PICKED, AND WHY IT IS NOT A TABLE OF SCREEN SIZES
///
/// A screen size is a guess about a window; the window is right there to be measured. An
/// arrangement fits when its rows each get [`ROOMY`] points, and the biggest one that fits is the
/// one the reader gets. Nothing here has to know what a quarter of a 4K screen is, and nothing
/// breaks on the next monitor.
///
/// # IT ONLY EVER APPLIES TO A DASHBOARD NOBODY HAS ARRANGED
///
/// `Settings::dashboard` is `None` until the reader moves something and `Some` for ever after.
/// Once it is his, the window does not get a vote: see [`layout`]. `Reset layout` in the widget
/// sheet sets it back to `None`, which is how a reader gets the automatic one back.
/// THE ROW PITCH AN ARRANGEMENT HAS TO CLEAR, or the window takes a shorter one instead.
///
/// # IT IS NOT THE PITCH AT WHICH A CARD STILL WORKS, AND THAT IS THE POINT
///
/// That one is [`dashgrid::ROW_UNIT`]: every card's [`Tile::min_rows`] was measured against
/// twelve point rows, by the guard that draws every tile at every height a reader can drag it
/// to, so an arrangement whose rows clear twelve points is legal, draws nothing sliced, and is
/// what a purely mechanical rule would pick.
///
/// THIS IS A TASTE AND IT IS THE OWNER’S. Asked what a quarter of a 4K screen should carry he
/// named four cards; twelve point rows would have given that window seven, every one of them
/// legal and every one of them smaller than he asked for. Fourteen is the pitch at which the
/// window he was looking at takes the four he named, so the number is his answer written down
/// rather than a threshold somebody guessed.
const ROOMY: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Size {
    /// THE OWNER'S FOUR: timeline, damage, the live fight, recent fights.
    Small,
    /// AND HEALING, DAMAGE TAKEN AND PROGRESSION.
    Medium,
    /// THE WHOLE MOCK. The default, so a page that has not measured itself yet asks for
    /// everything rather than quietly hiding nine cards on the first frame.
    #[default]
    Large,
}

impl Size {
    /// BIGGEST FIRST, which is the order [`Size::fitting`] reads them in.
    pub const ALL: [Size; 3] = [Size::Large, Size::Medium, Size::Small];

    /// THE ARRANGEMENT A GRID OF THIS MUCH HEIGHT GETS.
    ///
    /// `avail` IS THE GRID'S HEIGHT AND NOT THE WINDOW'S. The encounter head, the summary strip
    /// and the version line are not the dashboard's to spend, and the page knows what is left
    /// after them without anybody writing the number down.
    ///
    /// THE SMALL ONE IS THE FLOOR, because there has to be a page even in a window nothing fits
    /// in. Below it the pitch bottoms out at [`dashgrid::MIN_PITCH`] and the page scrolls, which
    /// is the honest answer for a window too short for four cards.
    pub fn fitting(avail: f32) -> Size {
        Size::ALL
            .into_iter()
            .find(|s| avail >= f32::from(s.rows()) * ROOMY)
            .unwrap_or(Size::Small)
    }

    /// THE ARRANGEMENT ITSELF, in [`Tile::ALL`]'s order so a tile's saved index is stable.
    pub fn shipped(self) -> Vec<Placement> {
        Tile::ALL
            .iter()
            .filter_map(|t| t.place_in(self).map(|c| Placement::new(t.id(), c)))
            .collect()
    }

    /// HOW MANY ROWS DEEP IT IS, asked of its own cells rather than written down a second time.
    pub fn rows(self) -> u16 {
        self.shipped()
            .iter()
            .map(|p| p.cell().row_end())
            .max()
            .unwrap_or(1)
            .saturating_sub(1)
    }

    /// FOR A MESSAGE. Nothing on screen names these: a reader sees cards, not a size.
    pub fn label(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }
}

/// THE DASHBOARD A FRESH INSTALL GETS ON A SCREEN THAT HOLDS IT: every tile, at the mock's
/// placement. [`Size::Large`]'s arrangement, and the one the other two are cut down from.
///
/// EVERY TILE AND NOT A CHOSEN FEW, because the picker is how a reader takes one away and there is
/// nothing on this page to tell him a tile exists until he has seen it once.
pub fn shipped() -> Vec<Placement> {
    Tile::ALL
        .iter()
        .map(|t| Placement::new(t.id(), t.place()))
        .collect()
}

/// THE LAYOUT TO DRAW, resolved against the tiles this build actually has and repaired into a
/// grid that can be drawn. See `dashgrid::resolve` for the three repairs.
///
/// AN EMPTY LIST TAKES [`shipped`]. A fresh install and a deliberately emptied list look identical
/// in JSON; both get the shipped dashboard back rather than a blank page.
///
/// `None` IS A DASHBOARD THAT WAS NEVER ARRANGED and takes the arrangement the window has room
/// for; `Some(empty)` is one the reader emptied and stays empty. As a bare list the two were one,
/// and taking the last card off put all thirteen back.
///
/// AND `None` IS THE ONLY CASE `size` IS CONSULTED IN, which is the whole contract: an
/// arrangement a reader made is his at every window size, and one nobody made follows the room.
///
/// EACH ENTRY CARRIES ITS SAVED INDEX, which is what every write back must use: the drawn order
/// skips ids this build does not know, the saved list does not.
pub fn layout(saved: Option<&[Placement]>, size: Size) -> Vec<(usize, Tile, Cell)> {
    let src: Vec<Placement> = match saved {
        None => size.shipped(),
        Some(s) => s.to_vec(),
    };
    dashgrid::resolve(&src, |id| Tile::of(id).map(Tile::place))
        .into_iter()
        .filter_map(|(at, id, c)| Tile::of(&id).map(|t| (at, t, c)))
        .collect()
}

/// The saved list as it stands, with the shipped dashboard standing in for an empty one, so an
/// edit is applied to what the reader sees rather than to nothing.
fn current(cx: &Cx, size: Size) -> Vec<Placement> {
    cx.settings
        .dashboard
        .clone()
        .unwrap_or_else(|| size.shipped())
}

/// THE SAVED LIST WITH EVERY KNOWN TILE'S CELL REPAIRED AND EVERY UNKNOWN ENTRY UNTOUCHED.
///
/// PURE, so [`tests::an_edit_repairs_what_it_knows_and_keeps_what_it_does_not`] can drive it with no
/// settings file anywhere near it. See [`store`] for why it must not rebuild the list.
fn merged(mut list: Vec<Placement>) -> Vec<Placement> {
    for (at, _, c) in dashgrid::resolve(&list, |id| Tile::of(id).map(Tile::place)) {
        if let Some(p) = list.get_mut(at) {
            p.set(c);
        }
    }
    list
}

/// THE ONE PLACE THE ARRANGEMENT IS WRITTEN: in memory, and to disk.
///
/// # TWO DEFECTS LIVED AT THE WRITE BACK AND BOTH WERE INVISIBLE UNTIL THE APP RESTARTED
///
/// NOTHING SAVED IT. The two write sites assigned `cx.settings.dashboard` and stopped there.
/// Every other mutating screen in this app calls `Settings::save` (analysis, gear, lfg, parser,
/// valet, windows all do), and this page did not, so a move, a resize, a removal or a picker
/// change survived exactly as long as the process. The reader arranges his dashboard, quits, and
/// gets the shipped one back.
///
/// AND IT DESTROYED WHAT IT COULD NOT READ. The list was REBUILT from `resolve`'s output, and
/// `resolve` drops every placement whose tile id this build does not know. So the first edit on
/// an older build deleted a newer build's tiles from the file for good, which is precisely the
/// forward compatibility the string id and the saved index exist to protect: the id survives
/// being unreadable, and then the first drag threw it away.
///
/// SO THE REPAIRED CELLS ARE MERGED BACK BY SAVED INDEX and every entry `resolve` did not return
/// is left exactly as it was found.
/// THE CELL A RELEASE COMMITS, or `None` when the release commits nothing.
///
/// # DEFECT: A CLICK ON A CARD'S HEADING SAVED THE WHOLE DASHBOARD AS AN ARRANGEMENT
///
/// The owner's settings file carried a dashboard of all thirteen cards at exactly the shipped
/// cells, in exactly `Tile::ALL`'s order, written at 00:01 on the night it appeared. Nobody
/// arranges thirteen cards into the shipped layout by hand. What does it is a heading pressed
/// and let go without the card moving: egui reads a press with a little travel as a drag, the
/// ghost lands on the card's own cell, the release commits, and `store` writes the whole
/// arrangement the page was drawing. From then on the dashboard was `Some`, the window got no
/// vote (see `Size`), and a quarter screen drew the full page crushed with its bottom cut off.
///
/// SO A RELEASE COMMITS A CHANGE AND NOTHING ELSE: a gold ghost, over the tile that was
/// picked up, on a cell that is not the one the tile is already in.
fn committed(cells: &[Cell], landing: Option<(usize, Cell, bool)>, i: usize) -> Option<Cell> {
    let (tile, want, ok) = landing?;
    (ok && tile == i && cells.get(i) != Some(&want)).then_some(want)
}

/// A SAVED ARRANGEMENT THAT IS ONE OF THE SHIPPED ONES, HANDED BACK TO THE WINDOW.
///
/// # WHY THE SETTINGS FILE IS REPAIRED ON LOAD, AND NOT ONLY THE CLICK THAT BROKE IT
///
/// `committed` stops the next one. The one already on disk stays `Some` for ever, and the
/// owner's quarter screen goes on drawing the full page, unless something recognises it for
/// what it is. A saved list that is exactly one of [`Size::ALL`]'s arrangements, the same
/// tiles in the same cells in any order, is indistinguishable from nobody having chosen, and
/// following the window with it loses nothing: it IS the arrangement the window would pick
/// when there is room for it.
///
/// THE SAME RULE AS `overlay::forget_unchosen`, which repaired the same kind of file for the
/// overlays. Anything that differs from every shipped arrangement by one cell is a choice and
/// is not touched.
pub fn forget_unchosen(list: &mut Option<Vec<Placement>>) {
    let Some(saved) = list.as_deref() else {
        return;
    };
    let same = |a: &Placement, b: &Placement| {
        a.tile == b.tile && a.col == b.col && a.span == b.span && a.row == b.row && a.rows == b.rows
    };
    let shipped = |size: Size| {
        let ship = size.shipped();
        ship.len() == saved.len()
            && saved.iter().all(|p| ship.iter().any(|s| same(p, s)))
            && ship.iter().all(|s| saved.iter().any(|p| same(p, s)))
    };
    if Size::ALL.into_iter().any(shipped) {
        *list = None;
    }
}

fn store(cx: &mut Cx, list: Vec<Placement>) {
    cx.settings.dashboard = Some(merged(list));
    /* A DISK THAT REFUSED THE WRITE IS LOGGED AND NOT SHOUTED ABOUT. The arrangement is already
     * correct on screen; what is lost is the next launch, and a modal over a dashboard because a
     * card moved would be worse than the loss. */
    if let Err(e) = cx.settings.save() {
        log::warn!("the dashboard arrangement was not saved: {e}");
    }
}

/// THE OTHER PLACE THE ARRANGEMENT IS WRITTEN: FORGETTING IT.
///
/// `None` AND NOT `Some(the shipped list)`, and the difference is the whole point. Writing
/// today's shipped cells down would freeze them exactly as the first drag did; `None` means
/// `follow whatever this build ships`, so a card the next build makes shorter arrives on its
/// own. See the control in [`DashboardsScreen::picker`] for what this is undoing.
///
/// A SECOND WRITER AND NOT A BRANCH INSIDE [`store`], because they are opposite operations:
/// `store` takes a list and must repair and merge it, and this takes nothing and must not. They
/// are two functions and the guard in
/// [`an_edit_repairs_what_it_knows_and_keeps_what_it_does_not`] counts both, so a THIRD write
/// site anywhere in this file is still a failing test.
fn forget(cx: &mut Cx) {
    cx.settings.dashboard = None;
    if let Err(e) = cx.settings.save() {
        log::warn!("the dashboard reset was not saved: {e}");
    }
}

/* ------------------------------------------------------------------- the widgets -- */

/// A ranked table as this page asks for one.
///
/// NO HEADLINE, EXACTLY AS `screens::live` DOES IT. The overlay's big number exists because that
/// window is glanced at from across a room with one number on it; here the reader is looking at the
/// page on purpose and the card's own heading already says which metric it is.
fn table(metric: Metric, cap: usize) -> Widget {
    Widget::Ranked(Ranked {
        foot: false,
        metric,
        rate: true,
        cols: Cols {
            rank: true,
            value: true,
            share: true,
            bar: true,
            head: true,
        },
        cap,
        headline: false,
        /* NOT FITTED: this is the roster renderer, which elides by its own count. See `overlay::Detail::fit`. */
        fit: false,
    })
}

/// A detail panel about the reader.
///
/// `Subject::You` RESOLVES THROUGH `Who::You` AND NOT A NAME: `Fights::with_owner` folds the
/// character name out of the log's file name into that variant before the aggregator sees a line.
/// A fight the reader only watched has no such row and `screens::widgets` says so in words rather
/// than falling back to somebody else.
fn yours(cap: usize) -> Detail {
    Detail {
        who: Subject::You,
        cap,
        /* THE CARD'S HEAD ALREADY NAMES IT. See `overlay::Detail::head`. */
        head: false,
        /* AND THE CARD'S HEIGHT IS THE GRID'S AND NOT THE READER'S, so this panel draws what
         * fits and the card says what it left out. See `overlay::Detail::fit`. */
        fit: true,
    }
}

/// THE COMBAT PANELS A TILE DRAWS, or empty for a tile that reads something other than a fight.
///
/// EMPTY IS A REAL ANSWER HERE AND NOT A GAP. Six of the fourteen tiles are not about the current
/// fight at all: what is on disk, what a mob absorbs, who is casting, which file is being read.
/// Those draw their own few lines and have no `Widget` to check, which is why the vocabulary test
/// asserts over this function's output rather than over the tile list.
pub fn widgets(t: Tile) -> Vec<Widget> {
    match t {
        Tile::Damage => vec![table(Metric::Dealt, GROUP_CAP)],
        Tile::Healing => vec![table(Metric::Healed, GROUP_CAP)],
        Tile::Taken => vec![table(Metric::Taken, GROUP_CAP)],
        /* THE TWO CHARTS DRAW THEMSELVES, for the reason the rosters do: the overlay's
         * renderer is built for a window glanced at from across a room and carries no axes,
         * no clock and no key, which is exactly what the owner asked this page to have. Both
         * still take every figure from `screens::dps`, so no number can differ. */
        /* THE READER'S OWN FIGHT, ONE LEVEL DOWN. Three panels rather than one, because "what was
         * my damage made of" is not answered by any one of them: what you used, what you hit, and
         * what happened to the swings you threw. */
        /* EVERY READING THIS TILE CAN SHOW, in tab order. The card draws ONE of them, the one
         * its tab is on; this returns all three so the vocabulary guard checks every config the
         * page can put on screen and not merely the one that happens to be selected. */
        Tile::Yours => vec![
            Widget::Abilities(yours(12)),
            Widget::Targets(yours(12)),
            Widget::Outcomes(yours(12)),
        ],
        Tile::Live
        | Tile::Timeline
        | Tile::Progression
        | Tile::Fights
        | Tile::Night
        | Tile::Mobs
        | Tile::Kills
        | Tile::Loot
        | Tile::Overlays => Vec::new(),
    }
}

/// EVERY WIDGET THIS PAGE CAN DRAW, FOR A TEST IN ANOTHER MODULE.
///
/// See `screens::dps`'s unit guard: the invariant is asserted over every shipped config and has to
/// be able to reach this one.
#[cfg(test)]
pub fn widgets_for_test() -> Vec<(Tile, Widget)> {
    Tile::ALL
        .into_iter()
        .flat_map(|t| widgets(t).into_iter().map(move |w| (t, w)))
        .collect()
}

/// HOW MANY PLAYERS ARE NAMED IN THIS FIGHT.
///
/// COUNTED OFF `Who::player`, WHICH IS A MEASURED RULE AND NOT A GUESS: an EverQuest character name
/// is a single word and everything the world spawns breaks that, measured over every entity that
/// dealt damage in the reference capture (six players, none with a space; twenty-five others, all
/// with one). Its stated failure is a player with a surname, which no combat line in the capture
/// has ever printed.
///
/// NOT `FightRow::participants`, AND THE DIFFERENCE IS THE WHOLE POINT. A participant is anything
/// that was HIT, so the capture's first fight has twenty-two of them and four are people.
///
/// `FightRow::players` AND NOT A COPY OF ITS FILTER, because that filter is `FightRow::ours` now:
/// a fight whose group the log proved counts the reader and his group, and the roster under this
/// line draws the same people.
fn players_in(f: &FightRow) -> usize {
    f.players()
}

/// THE `.panel-sub` A CARD IS HEADED WITH, and for a card that draws the roster's people it says
/// whose those are.
///
/// # THE SUB IS WHERE `in scope` ALREADY SAID WHAT THE CARD IS OVER
///
/// [`Tile::sub`] says which scope; a roster over that scope now draws the reader's group when
/// every fight in it proved one and every player when any did not (`reports::roll`), and those
/// two lists look the same. So every card whose figures are the roster's population carries
/// [`crate::screens::dps::whose`] after its scope, in the sub that already exists:
///
///   * THE THREE ROSTERS, over the scope's fold.
///   * THE TIMELINE, over the same fold: its lines are the roster's damage second by second
///     (`group_seconds`, `one_fight`), and a chart with the strangers taken out of it looks exactly
///     like a chart they were never in.
///   * THE LIVE CARD, over the FIGHT GOING NOW and not the scope, because its count is that fight's
///     roster. It printed `0 players named` over the capture's last fight with nothing saying the
///     count was of a solo reader's roster, while the log names `Losumyda` in it.
///
/// A kill list, the progression chart and the store have no roster for the words to be about.
///
/// NO FIGHT, NO CLAIM. With nothing in scope, or nothing being fought, the card says so in its body,
/// and a caption about the group of no fights would be a sentence about nothing.
fn card_sub(tile: Tile, fold: Option<&FightRow>, live: Option<&FightRow>) -> Option<String> {
    let scope = tile.sub()?;
    let over = match tile {
        Tile::Damage | Tile::Healing | Tile::Taken | Tile::Timeline => fold,
        Tile::Live => live,
        _ => None,
    };
    Some(match over {
        Some(f) => format!("{scope} \u{b7} {}", crate::screens::dps::whose(f)),
        None => scope.to_owned(),
    })
}

/* --------------------------------------------------------------------- the facts -- */

/// EVERY FACT THE LANDING STRIP STATES, COPIED OFF [`Ingest`] IN ONE PLACE AND NOWHERE ELSE.
///
/// A STRUCT AND NOT A PILE OF CALLS INSIDE THE PAINT, so that the sentences this page puts on
/// screen are a pure function of what the ingest answered and can be driven in a test without a
/// window, a log or a clock. Nothing is derived here: every field is one accessor's answer.
struct Facts {
    /// `Ingest::scanning`: the bootstrap is still on its worker thread.
    scanning: bool,
    /// `Ingest::log_dir().dir.is_some()`.
    folder: bool,
    /// `Ingest::log_dir_problem`.
    folder_problem: Option<String>,
    /// `Ingest::active_log()`, by file name.
    log: Option<String>,
    /// `Ingest::active_problem`.
    log_problem: Option<String>,
    /// `Ingest::active_character`, which lives in the log's FILE NAME and in no line.
    character: Option<String>,
    /// `Ingest::last_read`.
    read: Option<DateTime<Utc>>,
    /// `Ingest::scanned_at`.
    scanned: Option<DateTime<Utc>>,
    /// `Ingest::tail_start() > 0`: only the last cap of a bigger file was read.
    clipped: bool,
    /// `Ingest::fights().len()`.
    fights: usize,
    /// `Ingest::fights_unreadable`.
    unreadable: u32,
}

impl Facts {
    /// One accessor per field and no arithmetic. See the struct note.
    fn of(ig: &Ingest) -> Facts {
        Facts {
            scanning: ig.scanning(),
            folder: ig.log_dir().dir.is_some(),
            folder_problem: ig.log_dir_problem().map(str::to_owned),
            log: ig.active_log().map(crate::ingest::LogFile::name),
            log_problem: ig.active_problem().map(str::to_owned),
            character: ig.active_character().map(str::to_owned),
            read: ig.last_read(),
            scanned: ig.scanned_at(),
            clipped: ig.tail_start().is_some_and(|s| s > 0),
            fights: ig.fights().len(),
            unreadable: ig.fights_unreadable(),
        }
    }

    /// THE ONE LINE ABOUT WHAT IS BEING READ, and the state square that goes with it.
    ///
    /// READING IS ASKED FIRST AND THE ORDER IS THE WHOLE FUNCTION, which is `why_no_fights`' own
    /// argument and the reason this asks that function rather than writing a fourth ladder. For the
    /// second or so before the first scan lands, a perfectly healthy machine with a perfectly good
    /// Logs folder has no folder and no active log, and a strip that opened with "No Logs folder is
    /// set" would be an accusation about the reader's configuration made by a page that has not
    /// looked yet.
    ///
    /// THE THREE EMPTY CASES BORROW THE APP'S OWN WORDS. `no_fights_words` is what the Fights list
    /// and the Live page say for the same three causes, and each of those sentences already names
    /// the fix (a path in Settings, `/log on`, waiting). A fourth wording of the same three states
    /// is how two screens come to disagree about what is wrong with somebody's machine.
    fn watching(&self) -> (State, String) {
        let why = why_no_fights(self.scanning, self.folder, self.log.is_some());
        if matches!(why, NoFights::Reading) {
            return (State::Working, no_fights_words(why).to_owned());
        }
        /* A PROBLEM OUTRANKS THE LADDER ONCE THE SCAN IS DONE, because it is the more specific
         * answer: `log_dir_problem` names the paths that were tried and `active_problem` names the
         * file that would not open, where the ladder can only say the category. */
        if let Some(p) = &self.folder_problem {
            return (State::Wrong, p.clone());
        }
        if let Some(p) = &self.log_problem {
            return (State::Wrong, p.clone());
        }
        match why {
            NoFights::NoCombat => {
                let name = self.log.clone().unwrap_or_default();
                let who = match &self.character {
                    Some(c) => format!(" ({c})"),
                    None => String::new(),
                };
                (
                    State::Settled,
                    format!("Reading {name}{who}, last read {}.", since(self.read)),
                )
            }
            /* Reading is handled above and cannot arrive here; the other two are the ladder's. */
            other => (State::Idle, no_fights_words(other).to_owned()),
        }
    }

    /// WHAT IS KNOWN ABOUT THE FIGHTS BEHIND THIS PAGE, in as many lines as it takes.
    ///
    /// EMPTY WHEN THERE IS NOTHING TO COUNT, because the line above has already said why in words
    /// that name the fix, and a count of zero under it would be the page saying the same thing
    /// twice with a number in it.
    ///
    /// THE STALENESS CLAIM IS NOT OPTIONAL AND IT IS THE POINT OF THIS BLOCK. `Ingest::fights`
    /// is written ONCE, by the bootstrap scan, and never again while the app runs; the panels below
    /// come from `Ingest::current_fight`, which is re-folded from the end of the log on every poll
    /// that brought new lines. Those are two different ages of the same log on one page, and a
    /// reader who is not told will read the count as "tonight".
    fn history(&self) -> Vec<(String, String)> {
        if self.fights == 0 {
            return Vec::new();
        }
        let s = if self.fights == 1 { "" } else { "s" };
        /* THE AGE OF THE READ, AND NOT A SECOND FIGHTS COUNT.
         *
         * DEFECT: this said `42 fights, scanned 12m ago` in the encounter head's hover, beside a
         * meta item stating the number of fights in scope. Two different counts under one word,
         * in one head, and the bigger one was the bootstrap's fold of the log's TAIL, which this
         * page does not read at all any more. What is still worth saying is how long ago the log
         * was looked at, because that is how a reader tells a quiet night from a stalled app. */
        let _ = s;
        let mut out = vec![(
            format!("log scanned {}", since(self.scanned)),
            String::from(
                "When this app last read the log folder. The cards on this page count what is in \
                 this app's own store, which survives the log's tail moving on, so they do not \
                 change when that scan does.",
            ),
        )];
        if self.unreadable > 0 {
            out.push((
                format!(
                    "{} line{} not placed",
                    self.unreadable,
                    if self.unreadable == 1 { "" } else { "s" }
                ),
                String::from(
                    "Those lines carried a stamp this build could not place, so every total on \
                     this page rests on the lines it could.",
                ),
            ));
        }
        if self.clipped {
            out.push((
                format!("last {} read", tail_cap_text()),
                String::from(
                    "The oldest fight in the file opened before the part that was read, so its \
                     numbers are a floor.",
                ),
            ));
        }
        out
    }
}

/// HOW LONG AGO, IN WORDS, AND NEVER A CLOCK TIME.
///
/// A DURATION MAKES NO TIMEZONE CLAIM AND A STAMP WOULD. These two instants are the app's own
/// `Utc::now()` and printing one as `23:18:04` would be right for a reader in London and an hour
/// out for the owner, which is the same class of mistake `FightRow::start` refuses by keeping the
/// log's stamps as text.
///
/// THE SHAPE IS `screens::parser::age`'s AND THAT IS A DUPLICATE I CANNOT REMOVE HERE: that
/// function is private to a module this file may not edit. `None` is the case it does not have,
/// and it is a real one: a folder can be resolved before anything in it has been read.
fn since(t: Option<DateTime<Utc>>) -> String {
    let Some(t) = t else {
        return String::from("never");
    };
    let s = (Utc::now() - t).num_seconds().max(0);
    if s < 5 {
        String::from("just now")
    } else if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h {}m ago", s / 3600, (s % 3600) / 60)
    }
}

/// `266` becomes `04:26`.
pub(crate) fn clock(secs: i64) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// `20016` becomes `20,016`. The same grouping `screens::live` prints.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/* --------------------------------------------------------------------- the screen -- */

/// THE DASHBOARDS PAGE.
///
/// NOTHING HERE IS A COPY OF THE FIGHT. Everything drawn is read fresh from `Cx` on the pass that
/// draws it, which is the same guarantee `screens::dps` states: a cached row would be a third copy
/// of the fight, able to sit on screen saying something the log no longer says. The fields are the
/// three pieces of INTERACTION state that have to survive between frames, and no data at all.
pub struct DashboardsScreen {
    /// ARE THE TILES PINNED? Locked draws no handles at all. Locked on a fresh install.
    locked: bool,
    /// IS THE WIDGETS SHEET OPEN?
    picking: bool,
    /// WHAT THE PAGE IS LOOKING BACK AT. `None` until the reader picks: the latest night.
    filter: Option<Filter>,
    /// IS THE FILTER SHEET OPEN?
    filtering: bool,
    /// A TILE IN THE AIR: which, and where the pointer took hold of it.
    drag: Option<Drag>,
    /// A TILE BEING RESIZED FROM ITS CORNER: which, and the size the pointer has it at.
    resize: Option<Grab>,
    /// ONE FIGHT OUT OF THE SCOPE, by its own start stamp, or `None` for all of them.
    ///
    /// # THE START STAMP AND NOT AN INDEX
    ///
    /// It is the fight's natural key: `store` writes rows under `(character, server, start)` and
    /// dedupes on it. An index into the scope would name a different fight the moment the store
    /// grew a row or the filter moved, and this page re-folds on both.
    ///
    /// A PICK THE SCOPE NO LONGER HOLDS IS DROPPED, in the fold below, so narrowing the filter
    /// past the chosen fight cannot leave the page reading a fight that is not in it.
    pick: Option<String>,
    /// WHICH MONTH THE FILTER SHEET'S CALENDAR IS SHOWING, as its first day.
    ///
    /// `None` UNTIL THE SHEET IS OPENED, and then the month holding the newest night with a
    /// fight in it, which is the month a reader is looking back from. Not saved: which month a
    /// calendar was scrolled to is not a thing anybody wants remembered a week later.
    cal: Option<chrono::NaiveDate>,
    /// WHICH READING EACH TABBED CARD IS ON, by tile id. See [`Tile::tabs`].
    ///
    /// NOT SAVED, DELIBERATELY. Where a card sits and how big it is is an arrangement the reader
    /// made and expects to find again; which of three readings he glanced at last is a thing he
    /// did a second ago, and a dashboard that opened on `Hit results` because that is where he
    /// left it a week ago would be remembering the wrong kind of thing.
    tabs: std::collections::BTreeMap<&'static str, usize>,
    /// WHICH OF THE THREE ARRANGEMENTS THE WINDOW HAS ROOM FOR. See [`Size`].
    ///
    /// MEASURED IN [`DashboardsScreen::grid`] AND READ BY THE WIDGET SHEET, which is drawn after
    /// the grid on the same pass, so the sheet is never a frame behind the page under it.
    /// Not saved: it is a fact about the window this second, not a thing the reader chose.
    size: Size,
    /// THE SCOPE'S READING, KEPT BETWEEN FRAMES. See [`Folded`].
    folded: Option<Folded>,
}

/// WHAT THE SCOPE FOLDS TO, HELD BETWEEN FRAMES BEHIND A KEY THAT CANNOT GO STALE.
///
/// # WHY THIS PAGE HAS A CACHE AT ALL, WHEN ITS OWN RULE IS THAT NOTHING IS CACHED
///
/// `screens::dps` states the rule and it is a good one: a cached row is a copy of the fight that
/// can sit on screen saying something the log no longer says. It was written about the LIVE fold,
/// which changes on every poll. This page reads the STORE, which is append only, and it re-cloned
/// every fight in scope and re-folded them through `reports::roll` on every egui pass. With the
/// ALL chip on a character with a season of nights that is the whole history copied and merged
/// once per frame, while the pointer moves, before a single tile is painted.
///
/// # THE KEY IS COMPLETE, WHICH IS THE ONLY THING THAT MAKES A CACHE HONEST
///
/// `Ingest::history` is written in exactly two places and both are in `ingest.rs`: `keep_fights`
/// REPLACES it (or clears it, when the owner goes away) and `keep_live_fights` PUSHES to it. So
/// every change moves the length, the last start stamp, or both:
///
///   * a fight finishing pushes one row: the length moves.
///   * a rescan replaces the list: the length moves, or the last stamp does, or the list is
///     identical and nothing needed recomputing.
///   * a character change clears it: the length moves to zero.
///
/// AND THE SCOPE IS IN THE KEY, because the same history folds differently under a different
/// chip. Recomputed from the ingest on every frame and compared, so a stale reading cannot
/// outlive the thing it was read from by even one pass.
struct Folded {
    /* THE PICKED FIGHT IS IN THE KEY. Choosing one fight changes what every tile reads, so a
     * key without it would hand the page the whole scope's fold under one fight's name. */
    /* AND THE HISTORY'S GENERATION. A refill rewrites stored rows without moving the length or
     * the newest stamp, so without it the page served the fold from before the store learned
     * each fight's group. See `Ingest::history_gen`. */
    key: (Filter, usize, String, Option<String>, u64),
    fights: Vec<FightRow>,
    fold: Option<FightRow>,
}

/// A DRAG IN PROGRESS. The offset keeps the tile under the pointer where it was picked up rather
/// than snapping its corner to the cursor, which is what makes a drag feel like holding a thing.
#[derive(Clone, Copy)]
struct Drag {
    tile: usize,
    offset: Vec2,
}

/// A RESIZE IN PROGRESS: which tile, which edge of it, where the pointer took hold, and the
/// cell the pointer currently has it at.
///
/// THE ANCHOR IS WHERE THE DRAG BEGAN AND NOT THE TILE'S CORNER, so a resize is measured as how
/// far the pointer has MOVED, in whole cells, and applied through `dashgrid::resize_from`. That
/// is what lets the left and top edges work: they move the origin and keep the far edge, which a
/// size measured from the corner cannot express.
#[derive(Clone, Copy)]
struct Grab {
    tile: usize,
    edge: Edge,
    anchor: Pos2,
    cell: Cell,
}

impl Default for DashboardsScreen {
    fn default() -> Self {
        DashboardsScreen {
            locked: true,
            picking: false,
            filter: None,
            filtering: false,
            drag: None,
            resize: None,
            pick: None,
            cal: None,
            tabs: std::collections::BTreeMap::new(),
            size: Size::default(),
            folded: None,
        }
    }
}

impl DashboardsScreen {
    /// ARE THE TILES PINNED? The context header's lock reads this to choose its glyph.
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// PIN OR UNPIN THE TILES. The context header's lock is the one caller.
    ///
    /// CLOSING THE SHEET ON LOCK IS NOT TIDINESS. The sheet's rows add and remove tiles, which is
    /// a rearrangement; leaving it open over a locked grid would be a control that still worked
    /// under a lock that says nothing moves.
    pub fn toggle_lock(&mut self) {
        self.locked = !self.locked;
        if self.locked {
            self.picking = false;
            self.drag = None;
            self.resize = None;
        }
    }

    /// IS THE FILTER SHEET OPEN? The context header's Filter button reads this to light itself.
    pub fn filtering(&self) -> bool {
        self.filtering
    }

    /// OPEN OR CLOSE THE FILTER SHEET. The context header's Filter button is the one caller.
    ///
    /// IT DOES NOT UNLOCK, unlike the widget sheet: choosing what to look at is not rearranging
    /// the page, and a reader who has pinned his layout should be able to change the night
    /// without unpinning it.
    pub fn toggle_filter(&mut self) {
        self.filtering = !self.filtering;
    }

    /// What the page is looking back at, once it has something to look at.
    pub fn filter(&self) -> Option<&Filter> {
        self.filter.as_ref()
    }

    /// Look back at this instead. The sheet is the one caller.
    pub fn look_at(&mut self, filter: Filter) {
        self.filter = Some(filter);
    }

    /// IS THE WIDGETS SHEET OPEN? The context header's `Widgets` button reads this to light itself.
    pub fn picking(&self) -> bool {
        self.picking
    }

    /// OPEN OR CLOSE THE WIDGETS SHEET.
    ///
    /// OPENING IT UNLOCKS, because adding a tile IS a rearrangement and a sheet whose every row
    /// was inert would be a button that opens a list of things you cannot do.
    pub fn toggle_picker(&mut self) {
        self.picking = !self.picking;
        if self.picking {
            self.locked = false;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE PUMP, as every parser page does it, so the store keeps filling while this page
         * is open. Rate limited inside. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        /* THE HISTORY, AND NOT THE LIVE FOLD. This page looks back: the owner's rule is that
         * nobody reads a dashboard mid pull, they read it after, over a night or a week. So
         * everything below comes off `Ingest::history`, which is every finished fight on disk,
         * and the one tile that is still live says so on its own head. */
        let facts = Facts::of(&*cx.ingest);
        let filter: Filter = {
            let history = cx.ingest.history();
            /* A HELD NIGHT THE HISTORY NO LONGER HAS IS DROPPED, which is what a character
             * change looks like from here: the store under the page is another person's. The
             * zone and the mob are dropped with it, because they were chosen against that
             * night's fights and mean nothing against somebody else's. */
            if let Some(f) = &self.filter {
                if let When::Night(d) = f.when {
                    if !history.iter().any(|r| night::night_of_row(r) == Some(d)) {
                        self.filter = None;
                    }
                }
            }
            self.filter
                .clone()
                .unwrap_or_else(|| Filter::opening(history))
        }; /* THE SCOPE'S READING, RECOMPUTED ONLY WHEN THE STORE OR THE SCOPE HAS MOVED. See
            * `Folded` for why this page keeps one and why the key cannot go stale. */
        let key = {
            let history = cx.ingest.history();
            (
                filter.clone(),
                history.len(),
                history.last().map(|f| f.start.clone()).unwrap_or_default(),
                self.pick.clone(),
                cx.ingest.history_gen(),
            )
        };
        if self.folded.as_ref().map(|f| &f.key) != Some(&key) {
            let history = cx.ingest.history();
            let mut fights: Vec<FightRow> = night::in_filter(history, &filter)
                .into_iter()
                .cloned()
                .collect();
            /* A PICK THE SCOPE NO LONGER HOLDS IS DROPPED. Narrowing the filter past the chosen
             * fight must not leave the page reading a fight the filter says is not in it. */
            if self
                .pick
                .as_deref()
                .is_some_and(|s| !fights.iter().any(|f| f.start == s))
            {
                self.pick = None;
            }
            /* ONE FIGHT PICKED IS THAT FIGHT ITSELF, NOT A FOLD OF ONE.
             *
             * `roll` sums figures and drops what cannot be summed across fights: `moments` and
             * every fighter's `series` are stamped from their OWN fight's start, so rolling two
             * fights cannot keep them. Rolling ONE would throw them away for nothing, and they
             * are exactly what the DPS timeline draws. So a pick takes the row whole. */
            let fold: Option<FightRow> = match self.pick.as_deref() {
                Some(s) => {
                    let one = fights.iter().find(|f| f.start == s).cloned();
                    if one.is_some() {
                        fights.retain(|f| f.start == s);
                    }
                    one
                }
                /* ONE FOLD, THE SAME FOLD REPORTS USES. `roll` turns a scope into one row, and
                 * two pages folding a night two ways is two answers to one question. */
                None if fights.is_empty() => None,
                None => {
                    let refs: Vec<&FightRow> = fights.iter().collect();
                    let mut r = crate::screens::reports::roll(&refs);
                    /* THE SCOPE'S OWN HEADLINE. `roll` folds figures, never names the fight. */
                    r.headline = scope_headline(&r);
                    Some(r)
                }
            };
            self.folded = Some(Folded { key, fights, fold });
        }
        /* TAKEN OUT FOR THE PASS AND PUT BACK AT THE END, which is how the page draws from it
         * while `self` is borrowed mutably by `grid`, without copying it. There is no early
         * return between here and the restore below, and there must never be one. */
        let held = self.folded.take().expect("the fold was just filled");
        let Folded {
            ref fights,
            ref fold,
            ..
        } = held;
        let fold = fold.as_ref();
        let why = why_no_fights(facts.scanning, facts.folder, facts.log.is_some());

        /* THE MOCK'S OWN OPENING, IN ITS OWN ORDER: the encounter head with the scope chips,
         * the summary strip, then the grid. */
        /* THE WHOLE SCOPE AND NOT THE NARROWED LIST, because the menu has to offer the fights a
         * reader can switch TO, and `fights` is already down to the one he is on. */
        let choosable: Vec<FightRow> = night::in_filter(cx.ingest.history(), &filter)
            .into_iter()
            .cloned()
            .collect();
        let chose = encounter_head(
            ui,
            &facts,
            &filter,
            fold,
            fights,
            &choosable,
            self.pick.as_deref(),
        );
        ui.add_space(12.0);
        summary_strip(ui, &filter, fold, fights);
        ui.add_space(12.0);

        /* THE BAR IS THERE WHEN THERE IS SOMETHING UNDER THE FOLD, AND THAT IS THE FIX FOR
         * `THE CARDS ARE CUT OFF`.
         *
         * egui's default bar is FLOATING: it fades in when the pointer is over the area and is
         * invisible otherwise. On a page whose last band sits below the fold, that means a
         * reader sees a row of cards sliced off by the window edge and NOTHING anywhere saying
         * there is more underneath. The owner read it as cards being cut off, twice, and he was
         * reading exactly what was on the screen: a cut card and no scrollbar is a broken card.
         *
         * THE PAGE COULD ALWAYS REACH ITS OWN BOTTOM; what was missing was any sign that
         * reaching was a thing to do.
         *
         * AND ON AN ARRANGEMENT NOBODY HAS TOUCHED THERE IS NOTHING UNDER THE FOLD ANY MORE,
         * because the page picks an arrangement the window holds and the rows divide the height
         * it has. A permanent rail on a page that cannot scroll is a control that does nothing,
         * which is the same lie in the other direction, so it shows WHEN NEEDED: a reader who
         * arranges more cards than his window holds still gets it, and nobody else sees it. The
         * grid stops short of its width either way (see `grid`), so the page does not reflow
         * when it appears.
         *
         * A DASHBOARD IS A DOCUMENT AND NOT A LIST INSIDE A CARD. The floating bar suits the
         * small scrolled areas inside tiles, where a permanent rail would eat the width; it does
         * not suit the page. And the grid already stops short of the bar's width (see `grid`),
         * so making it permanent costs no room and takes no handle away.
         */
        ui.scope(|ui| {
            /* AND IT HAS TO BE A COLOUR YOU CAN SEE, which is the second half of the same
             * defect. `theme::install` paints every inactive widget in `PANEL`, which is right
             * for a button sitting on the page ground and leaves a scrollbar handle at
             * `0x121820` against a `0x0B0E13` page: present, permanent, and invisible from more
             * than a foot away. A rail nobody can see says exactly as much as no rail. */
            let tiles = ui.visuals().widgets.clone();
            let vis = ui.visuals_mut();
            vis.widgets.inactive.bg_fill = TEXT_3;
            vis.widgets.inactive.weak_bg_fill = TEXT_3;
            vis.widgets.hovered.bg_fill = GOLD_DIM;
            vis.widgets.hovered.weak_bg_fill = GOLD_DIM;
            vis.widgets.active.bg_fill = GOLD;
            vis.widgets.active.weak_bg_fill = GOLD;
            egui::ScrollArea::vertical()
                .id_salt("dashboard_grid")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    /* THE TILES GET THE REAL THEME BACK. Only the rail wanted recolouring, and
                     * a card drawn with a gold hover fill would be a different page. */
                    ui.visuals_mut().widgets = tiles.clone();
                    self.grid(ui, cx, fold, fights, why);
                });
        });

        if self.picking {
            self.picker(ui, cx);
        }
        if self.filtering {
            self.sheet(ui, cx, &filter);
        }
        self.folded = Some(held);
        /* AFTER THE FOLD IS PUT BACK, so the next pass sees the new pick and re-folds against
         * it. Writing it before would be writing through a `self` this pass has lent out. */
        if let Some(p) = chose {
            self.pick = p;
        }
    }

    /// THE FILTER SHEET: when, where, against what, and how many.
    ///
    /// # THE OWNER ASKED FOR A KEY IN THE CORNER AND A MODAL BEHIND IT
    ///
    /// The head carried a chip per night, which does not survive a month of play and cannot say
    /// `Plane of Sky` or `the last two Nagafen fights` at all. His words: a filter key that pops
    /// a modal with date selection, zone selection, mob selection and a range, "with interesting
    /// ways of putting the data together so that people could see, oh hey I was doing this dps in
    /// Plane of Sky but now I am doing this".
    ///
    /// EVERY LIST NARROWS WITH THE OTHERS. Choose Plane of Sky and the nights become the nights
    /// you were there; choose Nagafen and the zones become the zones you fought him in. A list
    /// that offered everything regardless would send a reader to a combination that comes back
    /// empty, which reads as a broken filter rather than as a night he did not raid. See
    /// `night::nights_for`.
    ///
    /// AND EVERY ROW CARRIES ITS COUNT, so the reader is choosing between readings he can see the
    /// size of rather than guessing.
    fn sheet(&mut self, ui: &mut Ui, cx: &mut Cx, current: &Filter) {
        let history: Vec<FightRow> = cx.ingest.history().to_vec();
        let mut next = current.clone();
        let mut changed = false;
        let out = egui::Modal::new(egui::Id::new("grimoire.filter"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, LINE))
                    .corner_radius(RADIUS)
                    .inner_margin(egui::Margin::symmetric(18, 16)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_max_width(640.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("FILTER")
                            .font(crate::fonts::display(15.0))
                            .color(GOLD_HI),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small(ui, "close").clicked() {
                            ui.close();
                        }
                        if small(ui, "clear")
                            .on_hover_text("Back to the most recent night, with nothing narrowed")
                            .clicked()
                        {
                            next = Filter::opening(&history);
                            changed = true;
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(RichText::new(next.label()).color(TEXT_2).size(11.5));
                ui.add_space(12.0);

                egui::ScrollArea::vertical()
                    .id_salt("filter-sheet")
                    .max_height(430.0)
                    .show(ui, |ui| {
                        /* ---- WHEN ---- */
                        section(ui, "When");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let latest = night::latest(&history).map(When::Night);
                            let mut quick: Vec<(String, When)> = Vec::new();
                            if let Some(w) = latest {
                                quick.push((String::from("Latest night"), w));
                            }
                            quick.push((String::from("Last 3 nights"), When::LastNights(3)));
                            quick.push((String::from("Last 7 nights"), When::LastNights(7)));
                            quick.push((String::from("Everything"), When::All));
                            for (word, w) in quick {
                                if crate::theme::metric_tab(ui, &word, next.when == w).clicked() {
                                    next.when = w;
                                    changed = true;
                                }
                            }
                        });
                        ui.add_space(6.0);

                        /* THE NIGHTS THEMSELVES, each with what it was and how big it was. */
                        let nights = night::nights_for(&history, &next);
                        if nights.is_empty() {
                            ui.label(
                                RichText::new("No night matches the rest of this filter.")
                                    .color(TEXT_3)
                                    .size(11.0),
                            );
                        }

                        /* THE CALENDAR, WHICH IS WHERE ONE OR MANY DAYS ARE PICKED. */
                        if !nights.is_empty() {
                            ui.add_space(4.0);
                            if let Some(w) = calendar(ui, &mut self.cal, &nights, &next.when) {
                                next.when = w;
                                changed = true;
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("or one night at a time")
                                    .color(TEXT_3)
                                    .size(11.0),
                            );
                            ui.add_space(2.0);
                        }
                        for (d, n) in nights.iter().take(30) {
                            let on = next.when == When::Night(*d);
                            let zone = night::zone_of_night(&history, *d);
                            let words = match &zone {
                                Some(z) => format!(
                                    "{}  \u{b7}  {n} fight{}  \u{b7}  {z}",
                                    d.format("%a %b %-d"),
                                    if *n == 1 { "" } else { "s" }
                                ),
                                None => format!(
                                    "{}  \u{b7}  {n} fight{}",
                                    d.format("%a %b %-d"),
                                    if *n == 1 { "" } else { "s" }
                                ),
                            };
                            if row(ui, &words, on).clicked() {
                                next.when = When::Night(*d);
                                changed = true;
                            }
                        }

                        /* A RANGE, as two ends off the same list. */
                        if nights.len() > 1 {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new("or a range").color(TEXT_3).size(11.0));
                                let (mut from, mut to) = match next.when {
                                    When::Range(a, b) => (a, b),
                                    _ => (
                                        nights.last().map(|(d, _)| *d).unwrap_or_default(),
                                        nights.first().map(|(d, _)| *d).unwrap_or_default(),
                                    ),
                                };
                                let mut touched = false;
                                for (label, slot) in
                                    [("from", &mut from), ("to", &mut to)].into_iter()
                                {
                                    egui::ComboBox::from_id_salt(("filter-range", label))
                                        .selected_text(
                                            RichText::new(slot.format("%b %-d").to_string())
                                                .size(11.5)
                                                .color(TEXT),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (d, _) in &nights {
                                                if ui
                                                    .selectable_label(
                                                        *slot == *d,
                                                        d.format("%a %b %-d, %Y").to_string(),
                                                    )
                                                    .clicked()
                                                {
                                                    *slot = *d;
                                                    touched = true;
                                                }
                                            }
                                        });
                                }
                                if touched {
                                    next.when = When::Range(from, to);
                                    changed = true;
                                }
                            });
                        }

                        /* ---- WHERE ---- */
                        let zones = night::zones_for(&history, &next);
                        if !zones.is_empty() {
                            ui.add_space(12.0);
                            section(ui, "Where");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if crate::theme::metric_tab(ui, "Any zone", next.zone.is_none())
                                    .clicked()
                                {
                                    next.zone = None;
                                    changed = true;
                                }
                                for (z, n) in zones.iter().take(24) {
                                    let on = next.zone.as_deref() == Some(z.as_str());
                                    if crate::theme::metric_tab(ui, &format!("{z}  {n}"), on)
                                        .on_hover_text(format!(
                                            "{n} fight{} in {z}",
                                            if *n == 1 { "" } else { "s" }
                                        ))
                                        .clicked()
                                    {
                                        next.zone = if on { None } else { Some(z.clone()) };
                                        changed = true;
                                    }
                                }
                            });
                        }

                        /* ---- AGAINST WHAT ---- */
                        let mobs = night::mobs_for(&history, &next);
                        if !mobs.is_empty() {
                            ui.add_space(12.0);
                            section(ui, "Against");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if crate::theme::metric_tab(ui, "Anything", next.mob.is_none())
                                    .clicked()
                                {
                                    next.mob = None;
                                    changed = true;
                                }
                                for (m, n) in mobs.iter().take(24) {
                                    let on = next.mob.as_deref() == Some(m.as_str());
                                    if crate::theme::metric_tab(ui, &format!("{m}  {n}"), on)
                                        .on_hover_text(format!(
                                            "{n} fight{} against {m}",
                                            if *n == 1 { "" } else { "s" }
                                        ))
                                        .clicked()
                                    {
                                        next.mob = if on { None } else { Some(m.clone()) };
                                        changed = true;
                                    }
                                }
                            });
                        }

                        /* ---- HOW MANY ---- */
                        ui.add_space(12.0);
                        section(ui, "How many");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if crate::theme::metric_tab(ui, "All of them", next.last.is_none())
                                .clicked()
                            {
                                next.last = None;
                                changed = true;
                            }
                            for n in [1usize, 2, 3, 5, 10] {
                                let on = next.last == Some(n);
                                if crate::theme::metric_tab(ui, &format!("Last {n}"), on)
                                    .on_hover_text(format!(
                                        "The {n} most recent fight{} of whatever the rest of this \
                                         filter leaves",
                                        if n == 1 { "" } else { "s" }
                                    ))
                                    .clicked()
                                {
                                    next.last = if on { None } else { Some(n) };
                                    changed = true;
                                }
                            }
                        });
                    });

                ui.add_space(10.0);
                /* WHAT THIS FILTER WOULD SHOW, counted before he commits to it. */
                let n = night::in_filter(&history, &next).len();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "A raid night is a night and a zone: nothing in a log line \
                                       says a fight was a raid.",
                        )
                        .color(TEXT_3)
                        .size(10.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{n} fight{}", if n == 1 { "" } else { "s" }))
                                .color(if n == 0 { WRONG } else { TEXT_2 })
                                .size(11.0)
                                .strong(),
                        );
                    });
                });
            });

        if changed {
            self.look_at(next);
        }
        if out.should_close() {
            self.filtering = false;
        }
    }

    /// THE WIDGETS SHEET: every tile this build has, with the ones on the page ticked.
    ///
    /// A TOGGLE PER TILE AND NOT AN `Add` MENU, because the same control has to be able to take one
    /// away. A picker that only adds leaves the reader looking for the remove button on the tile,
    /// which is a second place to learn.
    fn picker(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let mut slots = current(cx, self.size);
        let mut changed = false;
        /* The reader asked for the shipped arrangement back. See where it is set. */
        let mut reset = false;
        let out = egui::Modal::new(egui::Id::new("grimoire.widgets"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, LINE))
                    .corner_radius(RADIUS)
                    .inner_margin(egui::Margin::symmetric(18, 16)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_max_width(560.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("WIDGETS")
                            .font(crate::fonts::display(15.0))
                            .color(GOLD_HI),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small(ui, "close").clicked() {
                            ui.close();
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Select a card to put it on the dashboard. Drag a card's heading to move \
                         it, or any edge or corner to resize it. The lock on the header pins all \
                         of it.",
                    )
                    .color(TEXT_3)
                    .size(11.0),
                );
                ui.add_space(12.0);

                /* CARDS AND NOT CHECKBOXES, which is what the mock's `.widget-grid` is and what
                 * this list needs to be. A checkbox is a control for a thing you already know the
                 * name of; this is a catalogue of fourteen panels most of which the reader has
                 * never seen, so each entry is a GLYPH, the name, the sentence saying what it
                 * reads, and its state in words (`On the dashboard` / `Add`). Straight off
                 * section 11 of the spec. */
                let cols = 2;
                let gap = 10.0;
                let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .id_salt("widget-picker")
                    .show(ui, |ui| {
                        let all = Tile::ALL;
                        for chunk in all.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for t in chunk {
                                    let on = slots.iter().any(|s| s.tile == t.id());
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(w, 0.0),
                                        Layout::top_down(Align::Min),
                                        |ui| {
                                            ui.set_width(w);
                                            if widget_card(ui, *t, on).clicked() {
                                                changed = true;
                                                if on {
                                                    slots.retain(|s| s.tile != t.id());
                                                } else {
                                                    /* UNDER EVERYTHING, at its own size. A
                                                     * new card must not land in a gap the
                                                     * reader left on purpose. */
                                                    /* THE CELLS AS DRAWN, not as saved: a file
                                                     * from the span-only build saves rows of
                                                     * zero, and a bottom computed from those is
                                                     * row 1, which is the top of the page. */
                                                    let cells: Vec<Cell> =
                                                        dashgrid::resolve(&slots, |id| {
                                                            Tile::of(id).map(Tile::place)
                                                        })
                                                        .into_iter()
                                                        .map(|(_, _, c)| c)
                                                        .collect();
                                                    let mut c = t.place();
                                                    c.col = 1;
                                                    c.row = dashgrid::next_free_row(
                                                        &cells, 1, c.span, c.rows,
                                                    );
                                                    slots.push(Placement::new(t.id(), c));
                                                }
                                            }
                                        },
                                    );
                                }
                            });
                            ui.add_space(gap);
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "Select a card to add it or take it off. Drag a card's heading on \
                             the dashboard to move it.",
                        )
                        .color(TEXT_3)
                        .size(10.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} on the dashboard", slots.len()))
                                .color(TEXT_2)
                                .size(11.0)
                                .strong(),
                        );
                        ui.add_space(10.0);
                        /* THE WAY BACK TO THE SHIPPED ARRANGEMENT, AND THERE WAS NONE.
                         *
                         * `Settings::dashboard` is `None` until the first drag and `Some` for
                         * ever after, and `Tile::place` is only consulted for a tile the saved
                         * list does not name. So one drag froze the whole layout: every later
                         * improvement to a shipped cell (a card made shorter because its
                         * contents shrank, a band re-cut to give a list more width) was
                         * invisible to anybody who had ever touched a card, with nothing on
                         * screen to say so and no control to undo it.
                         *
                         * SAME SHAPE AS THE OVERLAY DEFECT ON `overlay::Overlay::widgets`, with
                         * one difference that matters: this arrangement really was made by a
                         * person, so it is his and nothing may drop it behind his back. What
                         * was missing is the door, not a migration.
                         *
                         * `None` AND NOT `Some(shipped)`, so the layout goes on following the
                         * shipped one as it changes rather than freezing today's copy of it. */
                        if cx.settings.dashboard.is_some()
                            && ui
                                .button(RichText::new("Reset layout").size(11.0).color(GOLD))
                                .on_hover_text(
                                    "Forget where you have put things and follow the \
                                     arrangement this build ships with, including any card it \
                                     has resized or moved since. Your cards are not changed.",
                                )
                                .clicked()
                        {
                            forget(cx);
                            reset = true;
                        }
                    });
                });
            });

        /* THE RESET WINS OVER THE SAME PASS'S EDITS, because `store` would write the list this
         * modal was drawn from straight back over the `None` it just set. */
        if changed && !reset {
            store(cx, slots);
        }
        /* THE BACKDROP AND ESCAPE BOTH CLOSE IT, which is what `should_close` folds together, and
         * it is why this is a modal rather than an `Area`: a picker you can only dismiss by
         * finding the button you opened it with is a picker that stays open. */
        if out.should_close() {
            self.picking = false;
        }
    }

    /// THE GRID ITSELF: twelve columns, ten point rows, every tile at its own cell.
    ///
    /// # THIS IS THE MOCK'S GRID AND NOT A PACKER
    ///
    /// The first build of this page packed tiles into rows by span and left holes wherever a
    /// tall tile sat beside a short one. This reserves the grid's whole height once, then paints
    /// every tile into the rect its cell names (`dashgrid::cell_rect`). Nothing flows; a tile is
    /// where the reader put it and exactly the size he made it.
    ///
    /// # A TILE IN THE AIR FOLLOWS THE POINTER, AND THE GHOST SAYS WHERE IT WILL LAND
    ///
    /// While a head is being dragged the tile paints under the pointer (offset by where it was
    /// picked up), painted LAST so it is over the others, and a ghost rect paints at the cell it
    /// would snap to: gold when that cell is free, red when it is on another tile or off the
    /// grid. On release the move is committed only if the ghost was gold; otherwise the tile goes
    /// back where it was. No tile is ever pushed aside, because a dashboard that rearranges
    /// itself when you drop one card on another has rearranged something you did not touch.
    ///
    /// A RESIZE IS THE SAME SHAPE from the bottom right corner: the tile grows under the pointer
    /// in whole cells, the ghost says whether that size fits, and release commits or reverts.
    fn grid(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        fold: Option<&FightRow>,
        fights: &[FightRow],
        why: NoFights,
    ) {
        /* WHAT THIS WINDOW HAS ROOM FOR, MEASURED BEFORE ANYTHING IS PLACED. `available_height`
         * here is the scroll viewport's, which is the page less its head and its version line:
         * exactly the height the grid gets to spend. See [`Size`]. */
        let size = Size::fitting(ui.available_height());
        self.size = size;
        let tiles = layout(cx.settings.dashboard.as_deref(), size);
        if tiles.is_empty() {
            ui.label(RichText::new("No widgets on this dashboard.").color(TEXT_2));
            return;
        }
        let cells: Vec<Cell> = tiles.iter().map(|(_, _, c)| *c).collect();
        /* THE GRID STOPS SHORT OF THE PAGE'S OWN SCROLLBAR, for the reason the tile body stops
         * short of the E strip: the dashboard is inside a `ScrollArea`, whose floating bar sits
         * in the rightmost points of the viewport and is registered after everything in it. A
         * tile whose right edge reached that far had its E handle swallowed, which on a fresh
         * install is the five tiles that end at column 12. */
        let full = (ui.available_width() - ui.spacing().scroll.bar_width - 1.0).max(120.0);
        let colw = dashgrid::col_width(full);

        /* THE ROWS DIVIDE THE HEIGHT THE PAGE ACTUALLY HAS, WHICH IS THE WHOLE POINT.
         *
         * A column has always been a twelfth of the width, so this grid has always filled the
         * window across. A row was a fixed twelve points, so the page was a fixed height in any
         * window: too tall for a quarter screen, which is what the scrollbar was, and too short
         * for a full one, which is what the band of dead page under the last widget was. Every
         * complaint the owner has made about the bottom of this page is that one asymmetry.
         *
         * SO THE LAYOUT'S OWN ROW COUNT IS DIVIDED INTO THE VIEWPORT and the page fits exactly,
         * at any size, with nothing under it and nothing past the fold.
         *
         * UNDER `dashgrid::MIN_PITCH` IT SCROLLS INSTEAD, and that is deliberate: a window too
         * short for its layout would otherwise crush every card below the height of its own
         * title, and a scroll is a better answer than a page of unreadable stubs.
         */
        let rows = cells.iter().map(|c| c.row_end()).max().unwrap_or(1);
        let rowh = dashgrid::row_height(ui.available_height(), rows.saturating_sub(1));
        let height = dashgrid::grid_height(&cells, rowh);
        let (grid_rect, _) = ui.allocate_exact_size(Vec2::new(full, height), Sense::hover());
        let origin = grid_rect.min;
        let pointer = ui.ctx().pointer_interact_pos();

        /* THE DRAGGED TILE PAINTS LAST, so it is over the rest. */
        let mut order: Vec<usize> = (0..tiles.len()).collect();
        if let Some(d) = self.drag {
            order.retain(|i| *i != d.tile);
            order.push(d.tile);
        }

        let mut acts: Vec<(usize, Act)> = Vec::with_capacity(tiles.len());
        for i in order {
            let (_, tile, cell) = tiles[i];
            let mut rect = dashgrid::cell_rect(origin, colw, rowh, cell);
            if let (Some(d), Some(p)) = (self.drag, pointer) {
                if d.tile == i {
                    rect = Rect::from_min_size(p - d.offset, rect.size());
                }
            }
            if let Some(g) = self.resize {
                if g.tile == i {
                    rect = dashgrid::cell_rect(origin, colw, rowh, g.cell);
                }
            }
            /* CLAMPED AGAINST THE TILE'S OWN LIST, so a tab index cannot outlive a build that
             * offers fewer readings than the one that stored it. */
            /* UNTOUCHED, IT OPENS ON ITS OWN READING. `Damage`, `Damage taken` and `Healing`
             * are the same card pointed at three of the four metrics: see `Tile::tabs`. */
            let tab = self
                .tabs
                .get(tile.id())
                .copied()
                .unwrap_or_else(|| tile.opens_on())
                .min(tile.tabs().len().saturating_sub(1));
            let act = self.card(ui, cx, tile, i, rect, fold, fights, why, tab);
            acts.push((i, act));
        }

        /* THE GHOST, for a drag or a resize: the mock's `.grid-drop-preview`. */
        let mut landing: Option<(usize, Cell, bool)> = None;
        if let (Some(d), Some(p)) = (self.drag, pointer) {
            let (col, row) = dashgrid::cell_at(origin, colw, rowh, p - d.offset);
            let want = Cell::new(col, cells[d.tile].span, row, cells[d.tile].rows).clamped();
            let ok = dashgrid::can_place(&cells, Some(d.tile), want);
            landing = Some((d.tile, want, ok));
        }
        if let Some(g) = self.resize {
            let want = g.cell.clamped();
            let ok = dashgrid::can_place(&cells, Some(g.tile), want);
            landing = Some((g.tile, want, ok));
        }
        if let Some((_, want, ok)) = landing {
            let ghost = dashgrid::cell_rect(origin, colw, rowh, want);
            let (fill, line) = if ok {
                (GOLD.gamma_multiply(0.10), GOLD)
            } else {
                (WRONG.gamma_multiply(0.10), WRONG)
            };
            ui.painter().rect(
                ghost,
                crate::theme::RADIUS as f32,
                fill,
                egui::Stroke::new(1.5, line),
                egui::StrokeKind::Inside,
            );
        }

        /* ---- and now, once, what the pointer asked for ---- */
        let mut saved: Option<Vec<Placement>> = None;
        let mut open: Option<Tile> = None;
        for (i, act) in acts {
            if let Some(offset) = act.grab {
                self.drag = Some(Drag { tile: i, offset });
            }
            /* THE WRITE BACK ADDRESSES THE SAVED LIST BY THE SAVED INDEX, never by `i`. `i` is
             * the drawn order, which skips every id this build does not know. */
            let sidx = tiles[i].0;
            if act.released {
                /* A RELEASE COMMITS; AN ESCAPE DOES NOT. egui ends an aborted drag with the
                 * same `drag_stopped`, so `commit` is what tells the two apart, and the clear
                 * below is unconditional so no ghost is ever stranded. */
                if act.commit {
                    if let Some(want) = committed(&cells, landing, i) {
                        let mut s = current(cx, self.size);
                        if let Some(p) = s.get_mut(sidx) {
                            p.set(want);
                        }
                        saved = Some(s);
                    }
                }
                self.drag = None;
            }
            if let Some((edge, at)) = act.resize_start {
                self.resize = Some(Grab {
                    tile: i,
                    edge,
                    anchor: at,
                    cell: cells[i],
                });
            }
            if let (Some(p), Some(g)) = (act.resize_to, self.resize) {
                if g.tile == i {
                    /* HOW FAR THE POINTER HAS COME, IN CELLS, applied to the tile as it was when
                     * the drag began and not to the preview, so a wobble does not accumulate. */
                    let (dc, dr) = dashgrid::delta_cells(colw, rowh, g.anchor, p);
                    let mut cell = dashgrid::resize_from(cells[i], g.edge, dc, dr);
                    /* A ROSTER HAS A FOOT AND CANNOT GO AS SHORT AS A LIST: see `Tile::min_rows`.
                     * The top edge keeps the bottom where it is, as `resize_from` does. */
                    let floor = tiles[i].1.min_rows();
                    if cell.rows < floor {
                        if g.edge.dy() < 0 {
                            cell.row = cell.row_end().saturating_sub(floor).max(1);
                        }
                        cell.rows = floor;
                    }
                    /* AND THE SAME ON THE X AXIS. `dashgrid` floors at one column, which is its
                     * own unit; what THIS widget needs is `Tile::min_span`, and the west edge
                     * keeps the east one where it is exactly as the north edge does. */
                    let narrow = tiles[i].1.min_span();
                    if cell.span < narrow {
                        if g.edge.dx() < 0 {
                            cell.col =
                                cell.col_end().saturating_sub(u16::from(narrow)).max(1) as u8;
                        }
                        cell.span = narrow;
                    }
                    self.resize = Some(Grab { cell, ..g });
                }
            }
            if act.resize_stop {
                if act.commit {
                    if let Some(want) = committed(&cells, landing, i) {
                        let mut s = current(cx, self.size);
                        if let Some(p) = s.get_mut(sidx) {
                            p.set(want);
                        }
                        saved = Some(s);
                    }
                }
                self.resize = None;
            }
            if let Some(t) = act.tab {
                self.tabs.insert(tiles[i].1.id(), t);
            }
            if act.open {
                open = Some(tiles[i].1);
            }
            if act.remove {
                let mut s = current(cx, self.size);
                if sidx < s.len() {
                    s.remove(sidx);
                }
                saved = Some(s);
            }
        }
        /* WRITTEN BACK REPAIRED AND SAVED, both through `store`: what is on disk is what was
         * drawn, and it is on disk at all. */
        if let Some(s) = saved {
            store(cx, s);
        }
        if let Some(t) = open {
            let (id, section) = t.page();
            cx.ask = Ask::Open(id, section);
        }
        if self.drag.is_some() || self.resize.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// ONE CARD, PAINTED INTO EXACTLY THIS RECT. Returns what the pointer did to it.
    ///
    /// # THE CARD IS THE RECT AND THE BODY SCROLLS INSIDE IT
    ///
    /// The mock's `.panel` is `overflow: hidden`: a tile is the size its cell says and its
    /// contents fit inside or scroll. The first build let every card be as tall as its body,
    /// which is what turned a grid into a stack. Here the frame is forced to the cell's height,
    /// the body is a scroll area capped at what is left under the head, and nothing paints
    /// outside the cell.
    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        tile: Tile,
        slot: usize,
        rect: Rect,
        fold: Option<&FightRow>,
        fights: &[FightRow],
        why: NoFights,
        tab: usize,
    ) -> Act {
        let mut act = Act::default();
        let in_air = self.drag.is_some_and(|d| d.tile == slot);
        let locked = self.locked;
        let clip = rect.intersect(ui.clip_rect());
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .id_salt(("tile", tile.id()))
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(clip);
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                crate::theme::panel(ui, |ui| {
                    /* THE CONTENT IS THE CELL LESS ITS TWO BORDERS, AND THAT IS THE WHOLE FIX.
                     *
                     * DEFECT: EVERY CARD ON THE PAGE WAS MISSING ITS BOTTOM BORDER. An
                     * `egui::Frame` draws its stroke OUTSIDE the rect it lays content in, so
                     * content pinned to the cell's full height made a frame two points taller
                     * than the cell (measured: cell `156..274`, frame `156..276`). The card is
                     * clipped to its cell, so those two points, which are exactly where the top
                     * and bottom strokes live, were cut off. The TOP one survived because the
                     * clip and the frame share a top edge and the stroke fell inside it; the
                     * bottom had nowhere to go.
                     *
                     * SO THE CONTENT IS THE CELL MINUS BOTH STROKES and the frame lands exactly
                     * on the cell. See `theme::PANEL_STROKE`. */
                    let inner = (rect.height() - crate::theme::PANEL_STROKE * 2.0).max(0.0);
                    ui.set_min_height(inner);
                    ui.set_max_height(inner);
                    let sub = card_sub(tile, fold, cx.ingest.current_fight());
                    crate::theme::panel_head(ui, tile.label(), sub.as_deref(), |ui| {
                        if !locked
                            && small(ui, "\u{d7}")
                                .on_hover_text("Take this off the dashboard")
                                .clicked()
                        {
                            act.remove = true;
                        }
                        let (page, section) = tile.page();
                        let there = match (crate::nav::name_of(page), section) {
                            (Some(n), Some(s)) => format!("Open {n} \u{203a} {s}"),
                            (Some(n), None) => format!("Open {n}"),
                            (None, _) => String::from("Open the page this reads"),
                        };
                        if small(ui, "\u{203a}").on_hover_text(there).clicked() {
                            act.open = true;
                        }
                        /* THE READINGS THIS CARD KEEPS, as the mock's `.panel-actions` row of
                         * `.metric-tab`s. Drawn after the icons because this closure is laid out
                         * right to left, so the tabs end up to the LEFT of them, which is where
                         * the mock puts them. */
                        let names = tile.tabs();
                        if !names.is_empty() {
                            ui.add_space(4.0);
                            for (i, name) in names.iter().enumerate().rev() {
                                if crate::theme::metric_tab(ui, name, i == tab).clicked() {
                                    act.tab = Some(i);
                                }
                            }
                        }
                    });
                    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 40.0));
                    if !locked {
                        /* THE HANDLE IS CUT FROM THE HEAD'S INTERIOR AND NEVER VANISHES: see
                         * `grip_rect`. Clear of the resize zones on every side and of the
                         * buttons on the right where there is room for both. */
                        let handle = grip_rect(head);
                        let grip = ui.interact(
                            handle,
                            ui.id().with(("tile-grip", tile.id())),
                            Sense::drag(),
                        );
                        if grip.drag_started() {
                            if let Some(p) = ui.ctx().pointer_interact_pos() {
                                act.grab = Some(p - rect.min);
                            }
                        }
                        if grip.drag_stopped() {
                            act.released = true;
                            act.commit = ui.ctx().input(|i| i.pointer.any_released());
                        }
                        if grip.hovered() || grip.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                    /* THE ROSTER FOOT IS PINNED UNDER THE SCROLL, the mock's own `.roster-foot`.
                     *
                     * DEFECT: IT WAS AN `egui::Panel::bottom` AND IT ESCAPED THE TILE. On the
                     * owner's screen the foot (`55 named / Group total 564,867`) was painted inside
                     * the summary strip's first cell, well above the grid. A Panel keeps its rect
                     * in context memory by Id and lays out against the ui it is shown in; inside a
                     * scrolled, clipped, per-tile child ui that is a mechanism with more state than
                     * the job needs, and it went wrong in a way a two-frame headless probe did not
                     * reproduce. So it is arithmetic now: the body is allocated at exactly the
                     * height left under the head and above the foot, and the foot follows it in
                     * plain flow. Nothing is remembered between frames and nothing can land
                     * anywhere but under this body. */
                    /* NO STRIP RESERVED FOR A FOOT THE CARD NO LONGER DRAWS.
                     *
                     * The roster draws its own foot inside the body, because the foot says how
                     * many rows were left out and only the body knows that. Keeping the strip
                     * here as well would take those points off the rows for nothing and leave a
                     * band of dead card under them, which is the blank space the owner rang.
                     */
                    let sp = ui.spacing().item_spacing.y;
                    let foot_h = 0.0;
                    /* OFF THE FRAME'S INNER HEIGHT AND NOT THE CELL'S, or the body claims the two
                     * points the borders live in and the frame grows back to overflowing. */
                    let body_h = (inner - 40.0 - sp - foot_h).max(0.0);
                    /* THE BODY STOPS SHORT OF THE E RESIZE STRIP, and it has to.
                     *
                     * DEFECT: A TILE'S SCROLLBAR COULD NOT BE PRESSED. The body was allocated at
                     * the card's full width, so egui put its floating scrollbar in the rightmost
                     * ten points; the E strip is the rightmost six and is registered AFTER the
                     * body, and egui's hit test breaks a distance-zero tie in favour of the last
                     * widget registered. A press on those six points resized the tile instead of
                     * scrolling it, on every tile whose body overflows. Cutting `EDGE_T` off the
                     * body is the same clearance `grip_rect` already gives the head. */
                    let body_w = (rect.width() - EDGE_T).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(body_w, body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_min_height(body_h);
                            ui.set_max_height(body_h);
                            ui.set_max_width(body_w);
                            /* `min_scrolled_height` AS WELL AS `max_height`: egui's default of
                             * 64 silently overrides a smaller cap, so a short tile's body was
                             * 64 tall however small the cell and the foot fell out of it. */
                            egui::ScrollArea::vertical()
                                .id_salt(("tile-body", tile.id()))
                                .max_height(body_h)
                                .min_scrolled_height(body_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    body(ui, cx, tile, fold, fights, why, tab);
                                });
                        },
                    );
                });
            },
        );
        /* THE RESIZE HANDLES: EVERY EDGE AND EVERY CORNER, drawn only when unlocked.
         *
         * # ONE GRIP WAS NOT A RESIZE
         *
         * This was a single grip at the bottom right, and the owner's answer was that you should
         * be able to grab the bottom, or the side, and pull in any direction. He is describing a
         * window, and a tile that grows from one corner only has to be moved before it can be
         * made bigger on the left.
         *
         * THE ZONES ARE A THIN STRIP ALONG EACH EDGE AND A SQUARE AT EACH CORNER, and the corners
         * are registered LAST so they win the pixels they share with the two edges beside them
         * (egui gives a later interact the hover). They are all registered after the head's own
         * drag handle for the same reason: the top strip overlaps the head, and a press there is
         * a resize, not a move. The cursor names the axis before anything is pressed. */
        if !locked {
            let t = EDGE_T;
            let k = CORNER_K;
            let zones: [(Edge, Rect); 8] = [
                (
                    Edge::N,
                    Rect::from_min_max(
                        Pos2::new(rect.left() + k, rect.top()),
                        Pos2::new(rect.right() - k, rect.top() + t),
                    ),
                ),
                (
                    Edge::S,
                    Rect::from_min_max(
                        Pos2::new(rect.left() + k, rect.bottom() - t),
                        Pos2::new(rect.right() - k, rect.bottom()),
                    ),
                ),
                (
                    Edge::W,
                    Rect::from_min_max(
                        Pos2::new(rect.left(), rect.top() + k),
                        Pos2::new(rect.left() + t, rect.bottom() - k),
                    ),
                ),
                (
                    Edge::E,
                    Rect::from_min_max(
                        Pos2::new(rect.right() - t, rect.top() + k),
                        Pos2::new(rect.right(), rect.bottom() - k),
                    ),
                ),
                (Edge::NW, Rect::from_min_size(rect.min, Vec2::splat(k))),
                (
                    Edge::NE,
                    Rect::from_min_max(
                        Pos2::new(rect.right() - k, rect.top()),
                        Pos2::new(rect.right(), rect.top() + k),
                    ),
                ),
                (
                    Edge::SW,
                    Rect::from_min_max(
                        Pos2::new(rect.left(), rect.bottom() - k),
                        Pos2::new(rect.left() + k, rect.bottom()),
                    ),
                ),
                (
                    Edge::SE,
                    Rect::from_min_max(rect.max - Vec2::splat(k), rect.max),
                ),
            ];
            let mut lit: Option<Edge> = None;
            for (edge, zone) in zones {
                let grip = ui.interact(
                    zone,
                    ui.id().with(("tile-edge", tile.id(), edge as u8)),
                    Sense::drag(),
                );
                if grip.drag_started() {
                    if let Some(pp) = ui.ctx().pointer_interact_pos() {
                        act.resize_start = Some((edge, pp));
                    }
                }
                if grip.dragged() {
                    act.resize_to = ui.ctx().pointer_interact_pos();
                }
                if grip.drag_stopped() {
                    act.resize_stop = true;
                    act.commit = ui.ctx().input(|i| i.pointer.any_released());
                }
                if grip.hovered() || grip.dragged() {
                    lit = Some(edge);
                    ui.ctx().set_cursor_icon(match edge {
                        Edge::N | Edge::S => egui::CursorIcon::ResizeVertical,
                        Edge::E | Edge::W => egui::CursorIcon::ResizeHorizontal,
                        Edge::NE | Edge::SW => egui::CursorIcon::ResizeNeSw,
                        Edge::NW | Edge::SE => egui::CursorIcon::ResizeNwSe,
                    });
                }
            }
            /* THE EDGE UNDER THE POINTER LIGHTS, so the reader sees which one he has hold of
             * before he pulls. The corner ticks stay as the standing hint that a tile resizes
             * at all; they are the mock's own affordance. */
            let p = ui.painter();
            if let Some(edge) = lit {
                let st = egui::Stroke::new(2.0, GOLD);
                let (l, r, tp, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
                if edge.dy() < 0 {
                    p.line_segment([Pos2::new(l, tp + 1.0), Pos2::new(r, tp + 1.0)], st);
                }
                if edge.dy() > 0 {
                    p.line_segment([Pos2::new(l, b - 1.0), Pos2::new(r, b - 1.0)], st);
                }
                if edge.dx() < 0 {
                    p.line_segment([Pos2::new(l + 1.0, tp), Pos2::new(l + 1.0, b)], st);
                }
                if edge.dx() > 0 {
                    p.line_segment([Pos2::new(r - 1.0, tp), Pos2::new(r - 1.0, b)], st);
                }
            }
            let c = if lit.is_some() { GOLD } else { TEXT_3 };
            for n in 0..3 {
                let d = 4.0 + 4.0 * n as f32;
                p.line_segment(
                    [
                        Pos2::new(rect.max.x - d, rect.max.y - 3.0),
                        Pos2::new(rect.max.x - 3.0, rect.max.y - d),
                    ],
                    egui::Stroke::new(1.0, c),
                );
            }
        }
        if in_air {
            ui.painter().rect_stroke(
                rect,
                crate::theme::RADIUS as f32,
                egui::Stroke::new(1.5, GOLD),
                egui::StrokeKind::Inside,
            );
        }
        act
    }
}

/// WHAT ONE CARD'S POINTER PASS PRODUCED. Every field is a fact about THIS frame.
#[derive(Default)]
struct Act {
    /// The head was grabbed, at this offset from the tile's corner.
    grab: Option<Vec2>,
    released: bool,
    /// An edge was taken hold of, at this pointer position.
    resize_start: Option<(Edge, Pos2)>,
    /// An edge is being dragged and the pointer is here.
    resize_to: Option<Pos2>,
    resize_stop: bool,
    /// The gesture ended with the pointer actually released, as opposed to Escape.
    commit: bool,
    /// A tab in the head was pressed.
    tab: Option<usize>,
    open: bool,
    remove: bool,
}

/// THE LIBRARY CARD, section 11 of the mock: glyph, name, what it reads, and its state in words.
fn widget_card(ui: &mut Ui, t: Tile, on: bool) -> egui::Response {
    let r = crate::theme::card_tinted(ui, if on { PANEL_2 } else { PANEL }, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            let (glyph, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter().rect_filled(
                glyph,
                5.0,
                if on {
                    egui::Color32::from_rgb(0x24, 0x1E, 0x10)
                } else {
                    egui::Color32::from_rgb(0x18, 0x20, 0x2B)
                },
            );
            ui.painter().text(
                glyph.center(),
                egui::Align2::CENTER_CENTER,
                t.glyph(),
                egui::FontId::proportional(12.0),
                if on { GOLD_HI } else { TEXT_2 },
            );
            ui.vertical(|ui| {
                ui.label(RichText::new(t.label()).size(12.0).strong().color(if on {
                    TEXT
                } else {
                    TEXT_2
                }));
                ui.label(RichText::new(t.blurb()).size(10.0).color(TEXT_3));
                ui.add_space(2.0);
                ui.label(
                    RichText::new(if on { "On the dashboard" } else { "Add" })
                        .size(9.5)
                        .color(if on { GOLD } else { TEXT_3 }),
                );
            });
        });
    })
    .response;
    let hit = ui.interact(
        r.rect,
        ui.id().with(("widget-card", t.id())),
        Sense::click(),
    );
    if on || hit.hovered() {
        ui.painter().rect_stroke(
            r.rect,
            crate::theme::RADIUS as f32,
            egui::Stroke::new(1.0, if on { GOLD_DIM } else { LINE }),
            egui::StrokeKind::Inside,
        );
    }
    hit
}

/// A HEADING INSIDE A SHEET: small, tracked, muted, with room above it.
fn section(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(egui::FontId::proportional(10.0))
            .color(TEXT_3),
    );
    ui.add_space(5.0);
}

/// ONE PICKABLE LINE IN A SHEET: full width, lit when it is the one chosen.
///
/// A ROW AND NOT A CHIP, because a night carries three facts (when, how many, where) and a chip
/// row of those is unreadable past about four. The chips above it are for the answers that are
/// one word.
fn row(ui: &mut Ui, text: &str, on: bool) -> egui::Response {
    let full = ui.available_width();
    let r = ui.add_sized(
        Vec2::new(full, 24.0),
        egui::Button::new(
            RichText::new(text)
                .font(egui::FontId::proportional(11.5))
                .color(if on { GOLD_HI } else { TEXT }),
        )
        .fill(if on {
            egui::Color32::from_rgba_unmultiplied(0xF2, 0xB9, 0x4F, 20)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(egui::Stroke::NONE),
    );
    if on {
        ui.painter().rect_stroke(
            r.rect,
            4.0,
            egui::Stroke::new(1.0, GOLD_DIM),
            egui::StrokeKind::Inside,
        );
    }
    r
}

/// HOW MUCH ROOM THE `+N more` LINE NEEDS AT THE FOOT OF A LIST.
const MORE_H: f32 = 20.0;

/// THE ROOM A BODY KEEPS BACK FOR A LINE IT DRAWS AFTER ITS ROWS, over and above [`MORE_H`].
///
/// A BODY THAT ENDS WITH A SUMMARY HAS TWO LINES TO PAY FOR, not one. See `kills_body`.
const TAIL_H: f32 = 17.0;

/// IS THERE ROOM FOR ANOTHER ROW OF ROUGHLY THIS HEIGHT, AND STILL FOR THE LINE THAT SAYS WHAT
/// WAS LEFT OUT?
///
/// # "IF YOU HAVE TO SCROLL A WIDGET, IS IT EVEN REALLY A WIDGET"
///
/// The owner is right and the answer is no. A widget is a glance; a thing you scroll is a page in
/// a small box. Every list on this dashboard was either UNBOUNDED (the fights list drew a whole
/// night, seventy-six rows in a tile that holds ten) or capped at a number that had nothing to do
/// with the tile it was in (`GROUP_CAP` of twelve roster rows in a card that fits six, six loot
/// lines in a card that fits four).
///
/// SO A LIST DRAWS WHAT FITS AND SAYS WHAT IT DID NOT, and the saying is a control: it opens the
/// larger page that has the rest. Asked of the ui rather than computed from a row count, because
/// the reader can drag a tile to any height and only the ui knows what is left.
fn room_for(ui: &Ui, row_h: f32) -> bool {
    ui.available_height() >= row_h + MORE_H
}

/// THE LINE THAT SAYS WHAT DID NOT FIT, and opens the page that has the rest.
///
/// A CONTROL AND NOT A CAPTION. `+64 more fights` with nowhere to go is a widget telling a reader
/// he cannot see his own data; the same words that open LOG PARSER / Fights are a widget doing its
/// job, which is to be a window onto a larger page.
fn more_line(ui: &mut Ui, cx: &mut Cx, tile: Tile, hidden: usize, noun: &str) {
    if hidden == 0 {
        return;
    }
    let (page, section) = tile.page();
    let there = crate::nav::name_of(page).unwrap_or("the page this reads");
    let r = ui.add(
        egui::Button::new(
            /* ONE OF A THING IS NOT A PLURAL. Every caller passes a plural noun, because that
             * is what it is nearly always counting; stripping the s at one is cheaper and reads
             * better than making thirteen callers pass two nouns each. */
            RichText::new(format!(
                "+{hidden} more {}",
                if hidden == 1 {
                    noun.strip_suffix('s').unwrap_or(noun)
                } else {
                    noun
                }
            ))
            .font(egui::FontId::proportional(10.5))
            .color(GOLD),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    );
    if r.on_hover_text(format!(
        "A widget shows what fits in it. The other {hidden} are on {there}; make the card \
             taller or open the page."
    ))
    .clicked()
    {
        cx.ask = Ask::Open(page, section);
    }
}

/// ONE FIGURE AND WHAT IT COUNTS, SIDE BY SIDE AND THE SAME HEIGHT.
///
/// # THE SHAPE, AND WHY IT IS THIS ONE
///
/// A big numeral, and to the RIGHT of it the words that say what it counts, one word per line,
/// stacked so the block of words is exactly as tall as the numeral. The owner asked for it in
/// those terms and it earns its place: a stat written as a number over a caption spends three
/// lines of card on two facts, and `Written to disk` was spending a whole twelve row tile on
/// two numbers and a sub-heading repeating what the card was already called.
///
/// ONE WORD PER LINE AND NOT A WRAPPED PARAGRAPH. Wrapping puts the break wherever the card
/// happens to be wide, so two of these side by side would break in different places and stop
/// looking like one thing. A word per line is the same shape at every width this card has.
///
/// # IT IS A CONTROL
///
/// Returned as a clickable [`egui::Response`] covering BOTH halves, because the figure and its
/// words are one thing to point at. What a click does is the caller's: this knows nothing about
/// pages.
fn stat_pair(
    ui: &mut Ui,
    n: impl std::fmt::Display,
    words: &str,
    tint: egui::Color32,
) -> egui::Response {
    let list: Vec<&str> = words.split_whitespace().collect();
    /* THE WORDS SET THE SIZE AND THE NUMERAL FOLLOWS, WHICH IS THE ONLY ORDER THAT WORKS.
     *
     * Sized the other way round, three caption lines have to divide whatever height a 22 point
     * numeral happens to ink, which came out at about five points a line: too small to read.
     * A caption line has a floor a person can read at, the block is as many of those as there
     * are words, and the figure is then grown to match it. So `= the same amount of space as
     * the number` holds by construction at any number of words, and the figure is as big as the
     * caption beside it earns.
     */
    let band = WORD_H * list.len().max(1) as f32;

    /* HOW MUCH INK THIS FACE PUTS ON THE PAGE PER POINT OF FONT SIZE, MEASURED AND NOT ASSUMED.
     *
     * `Galley::size` is the LINE BOX, which for the display face is close to twice the height of
     * a digit because it is leaving room for accents and descenders that `0` and `207` do not
     * have. Scaling against it would leave the caption block visibly overhanging the numeral, so
     * this lays the digits out once at a reference size, reads `mesh_bounds` (the glyphs that
     * were actually produced), and scales from that. A face swap cannot silently break it.
     */
    let probe = ui
        .painter()
        .layout_no_wrap(n.to_string(), crate::fonts::display(REF), tint);
    let per_pt = (probe.mesh_bounds.height() / REF).max(0.05);
    let size = (band / per_pt).clamp(FIG_MIN, FIG_MAX);
    let num = ui
        .painter()
        .layout_no_wrap(n.to_string(), crate::fonts::display(size), tint);
    let ink = num.mesh_bounds;

    let (rect, r) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), band.max(ink.height())),
        egui::Sense::click(),
    );
    let p = ui.painter().with_clip_rect(rect);
    /* PLACED BY ITS INK AND NOT BY ITS BOX, so the digits start flush at the card's left edge
     * and their top lands on the first caption line whatever the face's bearings are. */
    let at = egui::pos2(rect.left() - ink.left(), rect.top() - ink.top());
    p.galley(at, num, tint);

    let x = rect.left() + ink.width() + WORD_GAP;
    for (i, w) in list.iter().enumerate() {
        p.text(
            egui::pos2(x, rect.top() + WORD_H * (i as f32 + 0.5)),
            egui::Align2::LEFT_CENTER,
            *w,
            egui::FontId::proportional(WORD_PT),
            TEXT_2,
        );
    }
    r
}

/// ONE QUIET FIGURE ON ONE LINE, as a control.
///
/// THE SECOND STAT ON A CARD IS NOT THE FIRST ONE AGAIN. [`stat_pair`] is a headline: it is what
/// the card is about and it is sized to be read from across the desk. A card whose second figure
/// were built the same way would be two headlines arguing, and the owner said so about this one
/// in as many words: the fights already on disk are context for the fights written tonight, not a
/// rival for them. So it is one small line, and the difference in weight IS the ranking.
fn stat_line(ui: &mut Ui, text: String, tint: egui::Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(11.0).color(tint))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE),
    )
}

/// The gap between a [`stat_pair`]'s figure and its words.
const WORD_GAP: f32 = 7.0;

/// ONE CAPTION LINE IN A [`stat_pair`], and so one unit of the figure's height.
const WORD_H: f32 = 9.0;

/// How big a [`stat_pair`]'s caption words are. Under [`WORD_H`], so the lines do not touch.
const WORD_PT: f32 = 7.5;

/// The size the ink-per-point probe in [`stat_pair`] is measured at. Arbitrary and cancels out.
const REF: f32 = 100.0;

/// The smallest and largest a [`stat_pair`]'s figure may be grown to.
///
/// A FLOOR AND A CEILING AND NOT A SIZE, because the caption is what picks it. These only stop a
/// one word caption from shrinking the headline to nothing and a five word one from turning it
/// into a banner.
const FIG_MIN: f32 = 18.0;
const FIG_MAX: f32 = 40.0;

/// A MONTH OF NIGHTS, PICKED ONE OR MANY AT A TIME.
///
/// # WHY A CALENDAR AND NOT A LONGER LIST
///
/// The sheet already lists nights newest first, which answers `last night` and `the night
/// before` and stops being useful at about a fortnight. The owner asked for a calendar because
/// the questions past that are shaped like dates: the raid on the 3rd, those two Tuesdays, the
/// week either side of a patch. A month grid says where the gaps are, which a list cannot.
///
/// # ONLY NIGHTS WITH FIGHTS IN THEM CAN BE PICKED
///
/// A day with nothing stored is drawn but dead: no hover, no click, no outline. A calendar that
/// let a reader pick an empty Tuesday would hand him a page with nothing on it and no way to
/// tell a filter that found nothing from an app that broke.
///
/// AND THE DAYS ARE NIGHTS. A cell is the NIGHT OF that date (see [`night::night_of`]), which
/// rolls at six in the morning, so a raid that ran to one o'clock is on the day it started and
/// not split across two cells. The header says so; the numbers would be wrong in a way nobody
/// could see if it did not.
///
/// Returns the new set of days when a cell was pressed, and `None` when nothing was.
fn calendar(
    ui: &mut Ui,
    month: &mut Option<chrono::NaiveDate>,
    nights: &[(chrono::NaiveDate, usize)],
    when: &When,
) -> Option<When> {
    use chrono::Datelike;
    let first_of = |d: chrono::NaiveDate| d.with_day(1).unwrap_or(d);
    /* OPENS ON THE NEWEST NIGHT'S MONTH, which is where a reader is looking back from. */
    let shown =
        *month.get_or_insert_with(|| first_of(nights.first().map(|(d, _)| *d).unwrap_or_default()));
    let picked = when.days();
    let mut out: Option<When> = None;

    ui.horizontal(|ui| {
        if small(ui, "\u{2039}")
            .on_hover_text("The month before")
            .clicked()
        {
            *month = Some(first_of(shown - chrono::Duration::days(1)));
        }
        ui.label(
            RichText::new(shown.format("%B %Y").to_string())
                .size(12.0)
                .color(TEXT)
                .strong(),
        );
        if small(ui, "\u{203a}")
            .on_hover_text("The month after")
            .clicked()
        {
            let next_month = first_of(shown + chrono::Duration::days(32));
            *month = Some(next_month);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("a day is the night that started on it")
                    .size(10.0)
                    .color(TEXT_3),
            )
            .on_hover_text(format!(
                "A night rolls over at {} in the morning, so a raid that ran past midnight \
                 belongs to the day it started on and is not split in two.",
                night::NIGHT_ROLLS_AT
            ));
        });
    });
    ui.add_space(3.0);

    /* MONDAY FIRST, and the letters are the week's own. */
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CAL_GAP;
        for d in ["M", "T", "W", "T", "F", "S", "S"] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(CAL_CELL, 13.0), egui::Sense::hover());
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                d,
                egui::FontId::proportional(9.5),
                TEXT_3,
            );
        }
    });

    let lead = shown.weekday().num_days_from_monday() as usize;
    let days = days_in(shown);
    let mut cell = 0usize;
    while cell < lead + days {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = CAL_GAP;
            for _ in 0..7 {
                let (rect, r) =
                    ui.allocate_exact_size(egui::vec2(CAL_CELL, CAL_CELL), egui::Sense::click());
                if cell >= lead && cell < lead + days {
                    let day = shown.with_day((cell - lead + 1) as u32).unwrap_or(shown);
                    let count = nights.iter().find(|(d, _)| *d == day).map(|(_, n)| *n);
                    let on = picked.contains(&day);
                    draw_day(ui, rect, day.day(), count, on);
                    if let Some(n) = count {
                        let r = r.on_hover_text(format!(
                            "{}  \u{b7}  {n} fight{}",
                            day.format("%a %b %-d, %Y"),
                            if n == 1 { "" } else { "s" }
                        ));
                        if r.clicked() {
                            /* A CLICK TOGGLES, which is what makes many days as easy as one.
                             * `When::toggled` is where that means something, and it is a
                             * function so the one behaviour only a pointer can reach here is
                             * still provable without one. */
                            out = Some(when.toggled(day));
                        }
                    }
                }
                cell += 1;
            }
        });
    }
    out
}

/// ONE DAY CELL: the number, whether anything was fought, and whether it is picked.
fn draw_day(ui: &Ui, rect: Rect, day: u32, count: Option<usize>, on: bool) {
    let p = ui.painter();
    match (count, on) {
        /* PICKED: the app's own gold, as everywhere else a choice is showing. */
        (Some(_), true) => {
            p.rect_filled(rect, 3.0, GOLD_DEEP);
            p.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, GOLD_HI),
                egui::StrokeKind::Inside,
            );
        }
        /* PLAYED BUT NOT PICKED: a well, so the month's shape reads at a glance. */
        (Some(_), false) => {
            p.rect_filled(rect, 3.0, PANEL_2);
        }
        /* NOTHING STORED: nothing drawn but the number, quietly. */
        (None, _) => {}
    }
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("{day}"),
        egui::FontId::proportional(11.0),
        match (count, on) {
            (Some(_), true) => INK,
            (Some(_), false) => TEXT,
            (None, _) => TEXT_3.gamma_multiply(0.55),
        },
    );
    /* HOW BUSY THE NIGHT WAS, as a bar under the number rather than a second numeral. A count
     * in a 26 point cell is unreadable; a bar says more and less, which is what a month view is
     * for. Capped, because one long night must not make every other night look empty. */
    if let Some(n) = count {
        let full = (n as f32 / f32::from(CAL_BUSY)).clamp(0.15, 1.0);
        let w = (rect.width() - 8.0) * full;
        let bar = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
            Vec2::new(w, 2.0),
        );
        p.rect_filled(bar, 1.0, if on { INK } else { GOLD_DIM });
    }
}

/// HOW MANY DAYS THE MONTH HOLDING THIS DATE HAS.
fn days_in(d: chrono::NaiveDate) -> usize {
    use chrono::Datelike;
    let (y, m) = (d.year(), d.month());
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let first = chrono::NaiveDate::from_ymd_opt(y, m, 1);
    let next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1);
    match (first, next) {
        (Some(a), Some(b)) => (b - a).num_days() as usize,
        _ => 30,
    }
}

/// How wide and tall one calendar day is, in points.
const CAL_CELL: f32 = 26.0;

/// The gap between calendar days.
const CAL_GAP: f32 = 3.0;

/// THE FIGHT COUNT A DAY'S BUSY BAR IS FULL AT.
///
/// A CAP AND NOT A MAXIMUM. Scaling each month to its own busiest night would make a quiet week
/// look like a raid week; a fixed cap means the bars mean the same thing in every month.
const CAL_BUSY: u16 = 25;

/// THE LITTLE TRIANGLE THAT SAYS A MENU IS BEHIND SOMETHING.
///
/// # PAINTED AND NOT TYPED, WHICH IS THIS APP'S OWN RULE
///
/// The first version of this was `\u{25be}` in a `Label` and it came out as a hollow box: the
/// display face has no such glyph and neither does the fallback. `chrome::chevron` has drawn the
/// rail's markers as three points since the beginning for the same reason, so this does too. A
/// shape cannot be missing from a font.
fn caret(ui: &mut Ui) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(CARET_SPAN, CARET_SPAN), Sense::click());
    let c = rect.center();
    let s = CARET_SPAN * 0.5;
    let d = CARET_SPAN * 0.3;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x - s, c.y - d),
            Pos2::new(c.x + s, c.y - d),
            Pos2::new(c.x, c.y + d),
        ],
        GOLD,
        egui::Stroke::NONE,
    ));
    r
}

/// Arm to arm across the headline's caret, in points.
const CARET_SPAN: f32 = 9.0;

/// THE FIGHTS IN THIS SCOPE, WITH ALL AT THE TOP.
///
/// # ONE FIGHT IS A SCOPE OF ONE AND THE WHOLE PAGE FOLLOWS IT
///
/// Every tile reads the same fold, so choosing a fight makes the page that fight: its damage,
/// its healing, its timeline, its loot. That is what the word scope means here and it is why
/// this control sits on the scope's own name rather than inside a tile.
///
/// WHAT IT COSTS is the two charts that are drawn FIGHT BY FIGHT: `Progression` over one fight
/// is one point. That is honest rather than wrong, and marking the chosen fight on a chart of
/// the whole scope is the better answer whenever it is worth building.
///
/// NEWEST FIRST, because a look-back page is read backwards: the pull you just did, then the one
/// before it. `fights` arrives in time order (`Store::all` sorts it) so this walks it in
/// reverse rather than sorting a second time and risking a second answer.
fn fight_menu(
    head: &egui::Response,
    fights: &[FightRow],
    pick: Option<&str>,
) -> Option<Option<String>> {
    let mut out: Option<Option<String>> = None;
    egui::Popup::menu(head).gap(4.0).show(|ui| {
        ui.set_min_width(320.0);
        if ui
            .selectable_label(
                pick.is_none(),
                RichText::new(format!(
                    "All {} fight{}",
                    fights.len(),
                    if fights.len() == 1 { "" } else { "s" }
                ))
                .size(12.0)
                .color(GOLD),
            )
            .clicked()
        {
            out = Some(None);
            ui.close();
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                for f in fights.iter().rev() {
                    let on = pick == Some(f.start.as_str());
                    let label = format!(
                        "{}   {}",
                        f.headline.as_deref().unwrap_or(UNNAMED),
                        clock(f.secs)
                    );
                    let r = ui.selectable_label(
                        on,
                        RichText::new(label)
                            .size(11.5)
                            .color(if on { GOLD_HI } else { TEXT }),
                    );
                    /* THE STAMP IS IN THE HOVER AND NOT THE ROW. Two pulls on one mob in one
                     * night are the same words; the stamp is what tells them apart, and it
                     * is too long to put in a menu forty rows deep. */
                    if r.on_hover_text(f.start.clone()).clicked() {
                        out = Some(Some(f.start.clone()));
                        ui.close();
                    }
                }
            });
    });
    out
}

/// A SMALL FLAT CONTROL ON A CARD'S HEAD. One place, so they cannot drift apart.
fn small(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .font(egui::FontId::proportional(10.5))
                .color(TEXT_3),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    )
}

/* ---------------------------------------------------------------- the tile bodies -- */

/// WHAT A TILE DRAWS INSIDE ITS CARD.
///
/// EVERY ARM EITHER DRAWS SOMETHING THE LOG STATES OR SAYS WHY IT CANNOT, and there is no third
/// kind of thing here. The match is exhaustive, so a tile added to the enum is a compile error in
/// this function rather than a card that quietly draws nothing.
/// WHAT A TILE DRAWS INSIDE ITS CARD, off the SCOPE'S fold and the scope's fights.
///
/// THE ONE EXCEPTION IS THE LIVE TILE, which reads the live fold and says so on its own head.
/// The owner's rule is that the dashboard looks back; that one card is the window onto the page
/// that does not, kept because a reader glancing at his dashboard between pulls still wants to
/// know whether combat is going.
fn body(
    ui: &mut Ui,
    cx: &mut Cx,
    tile: Tile,
    fold: Option<&FightRow>,
    fights: &[FightRow],
    why: NoFights,
    tab: usize,
) {
    match tile {
        Tile::Live => {
            let f = cx.ingest.current_fight().cloned();
            let live = cx.ingest.fight_is_live();
            crate::theme::panel_body(ui, |ui| live_body(ui, cx, f.as_ref(), live)).inner
        }
        Tile::Damage | Tile::Healing | Tile::Taken => {
            let Some(f) = fold else {
                crate::theme::panel_body(ui, |ui| {
                    ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
                });
                return;
            };
            roster(ui, cx, tile, f, Tile::roster_metric(tab), false);
        }
        Tile::Timeline => crate::theme::panel_body(ui, |ui| timeline_body(ui, fights, why)).inner,
        Tile::Progression => {
            crate::theme::panel_body(ui, |ui| progression_body(ui, fights, why)).inner
        }
        /* ONE PANEL EACH, HEADED BY ITS CARD. They were one tile stacking three panels, which
         * could not obey the rule the rest of the page obeys: a widget draws what fits and says
         * what it left out, and a fit rule cannot sensibly cut across three stacked panels. The
         * owner saw the result, a Targets heading clipped at the bottom edge with nothing saying
         * so. Three tiles the reader can place, size and remove one at a time. */
        Tile::Yours => {
            let Some(f) = fold else {
                crate::theme::panel_body(ui, |ui| {
                    ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
                });
                return;
            };
            /* THE ONE ITS TAB IS ON. `tab` is clamped by the caller against `Tile::tabs`. */
            let all = widgets(tile);
            /* DRAWN DIRECTLY AND NOT THROUGH `draw_widget`, WHICH IS THE POINT.
             *
             * `draw_widget` is the OVERLAY's dispatch and it throws the row count away, because
             * an overlay clips on purpose: see `overlay::Detail::fit`. A card cannot. This tile
             * was drawing a fixed twelve rows into whatever height the grid gave it, so the last
             * row it could reach was sliced in half by the card's own bottom edge, and nothing
             * said a thing. That is the defect the owner reported as the bottom of the cards
             * being cut off.
             *
             * SO THE COUNT COMES BACK AND THE CARD SAYS WHAT IT LEFT OUT, which is the rule every
             * other body on this page already keeps.
             *
             * AND THE LINE GOES INSIDE THE BODY, where every other tile puts it. Drawn after
             * `panel_body` it lands in whatever the card has left over, and a card with nothing
             * left over drew it straight through its own bottom edge. */
            crate::theme::panel_body(ui, |ui| {
                let drew = match all.get(tab) {
                    Some(Widget::Abilities(d)) => crate::screens::widgets::abilities(ui, f, d),
                    Some(Widget::Targets(d)) => crate::screens::widgets::targets(ui, f, d),
                    Some(w) => {
                        crate::screens::dps::draw_widget(ui, f, crate::fights::Pulse::Closed, w);
                        crate::screens::widgets::Drew { shown: 0, total: 0 }
                    }
                    None => crate::screens::widgets::Drew { shown: 0, total: 0 },
                };
                more_line(ui, cx, tile, drew.hidden(), "rows");
            });
        }
        Tile::Fights => crate::theme::panel_body(ui, |ui| fights_body(ui, cx, fights, why)).inner,
        Tile::Night => crate::theme::panel_body(ui, |ui| night_body(ui, cx)).inner,
        Tile::Mobs => crate::theme::panel_body(ui, |ui| mobs_body(ui, cx)).inner,
        Tile::Kills => crate::theme::panel_body(ui, |ui| kills_body(ui, cx, fights, why)).inner,
        Tile::Loot => crate::theme::panel_body(ui, |ui| loot_body(ui, cx)).inner,
        Tile::Overlays => {
            crate::theme::panel_body(ui, |ui| overlays_body(ui, cx, fold, false, why)).inner
        }
    }
}

/// THE NAMED THING THAT TOOK THE MOST DAMAGE ACROSS THE SCOPE, which is the same rule the engine
/// uses for one fight (`Fight::headline`: taken first, dealt as the fallback), applied to the fold.
/// WHAT THE HEAD SAYS WHEN NO SINGLE FIGHT IS PICKED. See `encounter_head`.
const ALL_FIGHTS: &str = "All fights";

fn scope_headline(f: &FightRow) -> Option<String> {
    /* NAMED, AND NOT MERELY `NOT A PLAYER`. `Who::Unknown` is neither: it is falling damage and
     * `You hurt yourself`, which the engine keeps rather than inventing an attacker for. Filtered
     * on `!player()` alone it reached this, and `Who::text` spells it `(nobody named)`, so a
     * fight where the only thing that took damage was the reader put NOBODY NAMED in the head at
     * 23 points. `Fight::headline` requires a name and so does this. */
    let mobs = f
        .fighters
        .iter()
        .filter(|x| matches!(x.who, crate::fights::Who::Named(_)));
    mobs.clone()
        .max_by_key(|x| x.taken)
        .filter(|x| x.taken > 0)
        .or_else(|| mobs.max_by_key(|x| x.dealt).filter(|x| x.dealt > 0))
        .map(|x| x.who.text().to_owned())
}

/// WHAT AN EMPTY SCOPE SAYS. The log's own three empty causes first, because a page with no
/// folder to read must not say `no fights stored` as if the store were the problem.
fn nothing_in_scope(why: NoFights) -> String {
    match why {
        NoFights::NoCombat => String::from("No finished fights in this scope."),
        other => no_fights_words(other).to_owned(),
    }
}

/// WHAT A CARD OR A PAGE NAMES AS THE SUBJECT OF A FIGHT, decided once so two surfaces cannot
/// answer it differently about the same row.
///
/// # THIS EXISTS BECAUSE THE TWO SURFACES DID ANSWER IT DIFFERENTLY
///
/// `screens::live` puts `FightRow::current_target` in its big gold slot, with that target's own
/// clock beside it, and its own tests argue the case at length: a fight is a run of combat with no
/// quiet gap in it, so on a raid night an eight minute chain of pulls is ONE fight, and
/// `FightRow::headline` is the biggest damage sponge of the whole chain, which can be something
/// that died six minutes ago. This card was drawing `headline` in ITS big slot, with the CHAIN's
/// clock beside it, under the same `IN COMBAT` mark, off the same `FightRow`. Both numbers were
/// correct and the two windows named different mobs as the thing being fought, side by side, on a
/// live stream.
///
/// THE LIVE PAGE'S ANSWER IS THE ONE THAT WINS, and not because it came first: the owner asked for
/// that page by name and its rule is the one with the reasoning and the fixtures behind it. See
/// `screens::live`'s `the_header_names_what_is_being_fought_now_and_not_the_biggest_thing_in_the_chain`.
///
/// THREE ARMS AND NOT TWO, because "nothing alive is being fought" is two different real states. A
/// fight stays open for a quiet window after the last blow, so for half a minute after a kill there
/// is a live fight with nothing alive in it; falling straight through to the label would print the
/// corpse in the same gold with nothing to say it was one.
///
/// `LiveSubject` AND NOT `Subject`, because `overlay::Subject` is already in this file's imports
/// and means something else entirely (which PERSON a detail panel is about). One of the two had to
/// carry the qualifier and it is this one, since the overlay type is spelled the same way in eight
/// other modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiveSubject<'a> {
    /// Something named is under fire, and this is how long IT has been under fire. Never the
    /// chain's own duration: a clock next to a name has to be the clock FOR that name.
    Fighting { who: &'a str, secs: i64 },
    /// Nothing alive is being fought and this is the last thing the run killed.
    Slain { who: &'a str },
    /// Nothing named has been hit or killed at all, so the fight's own label is all there is, and
    /// it is drawn as a label rather than as a claim about what is in front of you.
    Label { text: &'a str },
}

/// The words a fight with no named entity in it at all is given. `FightRow::headline` is `None`
/// for that, which is a real state (a run that is nothing but the reader hurting himself) and not
/// an error, so it gets words rather than an empty slot.
pub(crate) const UNNAMED: &str = "an unnamed fight";

/// WHAT THE DAMAGE ON THIS PAGE MAY BE COMPARED AGAINST, and how much history stands behind it.
///
/// `(figure, measured from, kills seen)`, or `None` where no comparison is earned. Both surfaces
/// this file draws (the live tile and the summary strip's `Target health`) ask this one function,
/// because they had the same two defects in two spellings.
///
/// # THE FIGURE AND ITS COUNT CAME OFF DIFFERENT BRANCHES
///
/// The tile read the figure from `hp::Reading::expect` and the count from `Reading::modal`, and
/// those are not the same group whenever the samples are SETTLED: `expect` is then the median of
/// EVERY sample while `modal` is the densest agreeing window inside them, which can be a subset.
/// Six kills printed `5 kills of 6 agreed on ~111` where the 111 was measured off all six. Two
/// statistics wearing one sentence, on the line whose only job is to say how far the figure beside
/// it can be trusted. `Reading::expect_with_count` hands both halves out of one branch and that is
/// what is asked here; the branch is not spelled out again, because a second spelling agrees with
/// the first until somebody edits one of them.
///
/// # AND A BURIED MOB MAKES THE WHOLE COMPARISON UNEARNED
///
/// The `buried` count is [`FightRow::deaths_of`]. The fold keys a fighter by NAME, so a repeat pull
/// shares the first mob's row and the damage beside the name counts more than one mob;
/// `hp::fold_into` goes the other way and throws away any fight where a name died twice, exactly so
/// a reading is ONE mob's worth. Comparing them is this page inventing a number by division, and no
/// better numerator can be recovered: the fold keeps totals and two stamps, not a damage timeline,
/// so how much went into the mob standing there now is not in this process's memory at all.
///
/// THE REFUSAL IS THEREFORE THE FIX AND NOT A FALLBACK, which is why it lives in the rule rather
/// than in each paint: a caller cannot reach a figure it is not allowed to use.
fn measured_against(r: Option<&crate::hp::Reading>, buried: usize) -> Option<(u64, usize, usize)> {
    if buried > 0 {
        return None;
    }
    let r = r?;
    r.expect_with_count().map(|(v, of)| (v, of, r.n()))
}

/// Read the subject off one fight. Pure, so the rule can be tested without a frame.
pub(crate) fn live_subject(f: &FightRow) -> LiveSubject<'_> {
    if let Some((who, secs)) = f.current_target() {
        return LiveSubject::Fighting {
            who,
            secs: i64::from(secs),
        };
    }
    if let Some((who, _)) = f.last_slain() {
        return LiveSubject::Slain { who };
    }
    LiveSubject::Label {
        text: f.headline.as_deref().unwrap_or(UNNAMED),
    }
}

/// THE LIVE TILE: the mark, what is being hit, and for how long.
///
/// # IT NAMED THE WRONG MOB, AND ITS OWN DOC SAID SO
///
/// The paragraph that used to sit here read "THE TARGET IS `FightRow::current_target` AND NOT THE
/// HEADLINE", and argued it correctly. The code under it did the opposite: `f.headline` in the big
/// gold slot with `f.secs`, the CHAIN's clock, beside it, and the current target underneath in
/// small text. So on a long pull the Live page named what the reader was hitting and this card
/// named the first thing he hit that night, in bigger type, under the same `IN COMBAT` mark.
///
/// THE SUBJECT IS NOW ASKED OF [`live_subject`], which is the Live page's rule written once. Both
/// surfaces put the same name in the same slot because there is one function to disagree with.
///
/// AND THE CHAIN'S CLOCK IS STILL DRAWN, because it is a real number: it moved to its own line and
/// took the words `in combat for` with it. Beside a mob's name it read as that mob's age, which is
/// the second half of the same defect and is exactly what the Live page moved it for.
///
/// # AND A NAME IS NOT A MOB, WHICH IS WHAT EVERY FIGURE UNDER THE NAME DEPENDS ON
///
/// The fold keys a fighter by NAME because nothing in an EverQuest log tells two `a thunder spirit
/// princess` apart, so a second pull of one name lands in the FIRST one's row: `taken` becomes both
/// mobs' damage and `first_taken_at`..`last_taken_at` spans both engagements with the looting in
/// the middle. [`FightRow::current_target`] deliberately lets that row through -- its rule is that
/// a hit strictly after the last death mark means one of them is up again -- so this is a state the
/// tile is reached in, not a corner case.
///
/// SO THE CLOCK AND THE COMPARISON ARE BOTH GATED ON [`FightRow::deaths_of`], and this card had
/// neither gate while the Live header had both. The clock claimed to be "how long this target has
/// been under fire" while measuring two mobs and a gap; the `of ~N` divided a numerator counting
/// every mob of the name by a denominator `hp::read` measured on exactly one (it throws away any
/// fight where a name died twice, for precisely this reason). An app that never invents a number
/// can still print one by division.
///
/// THE NAME ITSELF IS STILL RIGHT and stays where it is: it IS what is being fought. What is
/// refused is every claim that treats the row as one instance, and what is offered instead is the
/// fact the log does state, which is how many of them have gone down.
fn live_body(ui: &mut Ui, cx: &mut Cx, fight: Option<&FightRow>, live: bool) {
    let Some(f) = fight else {
        ui.label(RichText::new("Nothing is being fought.").color(TEXT_2));
        return;
    };
    let subject = live_subject(f);
    /* HOW MANY MOBS OF THIS NAME THIS RUN HAS ALREADY BURIED, read once and used twice: it decides
     * the clock beside the name and it decides whether the damage below can be compared to
     * anything. `FightRow::deaths_of` is the rule, on the row, so this card and the Live header
     * ask one function rather than agreeing by hand. Zero for the arms that are not fighting
     * anything, where the question does not arise. */
    let buried = match subject {
        LiveSubject::Fighting { who, .. } => f.deaths_of(who),
        LiveSubject::Slain { .. } | LiveSubject::Label { .. } => 0,
    };
    ui.horizontal(|ui| {
        let (word, tint) = if live {
            ("IN COMBAT", SETTLED)
        } else {
            ("LAST FIGHT", TEXT_3)
        };
        ui.label(RichText::new(word).color(tint).strong());
        match subject {
            LiveSubject::Fighting { who, secs } => {
                ui.label(
                    RichText::new(who)
                        .font(crate::fonts::display(15.0))
                        .color(GOLD_HI),
                );
                if buried == 0 {
                    ui.label(RichText::new(clock(secs)).color(TEXT_2).monospace())
                        .on_hover_text(
                            "How long this target has been under fire, from the first time it was \
                             hit to the last. The whole run of combat is on the line below.",
                        );
                } else {
                    /* WHAT IS LEFT THAT THE LOG ACTUALLY STATES, in the Live header's own words.
                     * The clock is gone because it would run from the first hit on a mob that is
                     * already looted, through the gap, to now; the kill count in its place is read
                     * straight off the death marks and is the fact that explains the absence. */
                    ui.label(
                        RichText::new(format!("{buried} killed"))
                            .color(TEXT_3)
                            .monospace(),
                    )
                    .on_hover_text(format!(
                        "This run has already killed {buried} of these. Nothing in the log tells \
                         two mobs of one name apart, so they share one row: there is no clock for \
                         the one standing there now, and no way to say how much of this damage \
                         went into it."
                    ));
                }
            }
            /* THE KILL IS NAMED AS A KILL, in the quieter tint this page gives a finished thing.
             * The reader's question in the gap after a kill is not "what am I fighting", it is
             * "did it go down". */
            LiveSubject::Slain { who } => {
                ui.label(RichText::new("slain").color(TEXT_3));
                ui.label(
                    RichText::new(who)
                        .font(crate::fonts::display(15.0))
                        .color(TEXT_2),
                )
                .on_hover_text(
                    "The last thing this run killed. Nothing is being fought right now; the fight \
                     stays open until combat has been quiet for a while.",
                );
            }
            LiveSubject::Label { text } => {
                ui.label(
                    RichText::new(text)
                        .font(crate::fonts::display(15.0))
                        .color(GOLD_HI),
                )
                .on_hover_text(
                    "Nothing named has been hit in this run, so this is the fight's own label: \
                     the biggest thing in it.",
                );
            }
        }
    });
    /* WHAT WENT IN, AND WHAT ITS KIND HAS BEEN MEASURED TO SURVIVE.
     *
     * THE DENOMINATOR IS MEASURED AND NOT INVENTED: `hp::Reading` is the median of kills that
     * FINISHED and it refuses to answer on a first meeting, on two kills, and on a mob whose kills
     * disagree. The log states no mob health and this page never makes one up.
     *
     * WHETHER THERE IS ANYTHING TO COMPARE TO AT ALL IS [`measured_against`], which carries both
     * refusals (the count that must come off the figure's own branch, and the buried mob that
     * makes the whole division meaningless) and is asked by the summary strip as well.
     *
     * THE ROW IS FOUND WITH `Who::player` REFUSED, which is the same filter `current_target` and
     * `FightRow::deaths_of` use. Finding it any other way would let this line read its damage off
     * one slot while the kill count was read off another. */
    if let LiveSubject::Fighting { who, .. } = subject {
        let into = f
            .fighters
            .iter()
            .find(|x| !x.who.player() && x.who.text() == who)
            .map(|x| x.taken)
            .unwrap_or(0);
        let counted = measured_against(cx.ingest.hp_of(who), buried);
        if into > 0 {
            ui.horizontal(|ui| match counted {
                Some((expect, of, n)) => {
                    ui.label(
                        RichText::new(format!("{} of ~{}", thousands(into), thousands(expect)))
                            .color(TEXT_2)
                            .monospace(),
                    )
                    .on_hover_text(format!(
                        "Damage into this target, against what this kind of mob has been \
                         measured to absorb: {} kills of {} agreed on ~{}. The log never \
                         states a mob's health.",
                        of,
                        n,
                        thousands(expect)
                    ));
                }
                /* NO CLAIM ABOUT A READING EITHER WAY. This arm is taken whether or not the book
                 * has a figure for this mob, so the sentence says why a figure could not be USED
                 * rather than whether one exists: a measured health is what ONE mob absorbs and
                 * this numerator is more than one of them added together. */
                None if buried > 0 => {
                    ui.label(RichText::new(thousands(into)).color(TEXT_3).monospace())
                        .on_hover_text(format!(
                            "Damage into every {who} in this run, {} of them, because the log \
                             gives two mobs of one name one row. No health figure beside it: a \
                             measured one is what ONE of them absorbs, and this counts more than \
                             one.",
                            buried + 1
                        ));
                }
                None => {
                    ui.label(RichText::new(thousands(into)).color(TEXT_3).monospace())
                        .on_hover_text(
                            "Damage into this target. No health figure: the log never states \
                             one, and this mob has not been killed enough times for its \
                             previous kills to agree on what it absorbs.",
                        );
                }
            });
        }
    }
    ui.horizontal(|ui| {
        /* THE WHOLE RUN, AND THE LABEL HAS TO SAY SO, in the Live page's own words so the two
         * surfaces call the same measurement the same thing. */
        ui.label(RichText::new("in combat for").color(TEXT_3).size(11.0));
        ui.label(RichText::new(clock(f.secs)).color(TEXT_2).monospace())
            .on_hover_text(
                "Every second of combat since the last quiet gap. A fight is a run of combat, so \
                 a chain of pulls with no lull in it is one fight and this is its whole length.",
            );
        ui.label(RichText::new(roster_count(f)).color(TEXT_3));
    });
}

/// THE LIVE CARD'S HEAD COUNT, IN WORDS THAT SAY WHICH HEADS.
///
/// `players_in` is the roster's count, which is every player when the group is not known and the
/// reader, his group and his pets when it is. `N players named` is a claim about the LOG, and on
/// a filtered roster the log names more: over the capture's last fight, solo after the removal at
/// 23:21:54, it printed `0 players named` while `Losumyda` dealt damage in it. So a filtered count
/// says it is the roster's.
fn roster_count(f: &FightRow) -> String {
    let n = players_in(f);
    match f.group {
        None => format!("{n} player{} named", if n == 1 { "" } else { "s" }),
        Some(_) => format!("{n} on your roster"),
    }
}

/* ------------------------------------------------------------------- the charts -- */

/// HOW MUCH OF A CHART GOES TO ITS AXES AND ITS LEGEND.
///
/// THE MOCK'S CHART HAS ALL THREE and the first version of this page had none of them: bare lines
/// on a flat panel with no scale, no clock and no key. A chart without a y axis is a shape, not a
/// measurement; the reader cannot tell 500 from 5,000.
const Y_AXIS: f32 = 44.0;
const X_AXIS: f32 = 16.0;
const KEY_W: f32 = 104.0;
/// The horizontal rules behind a chart, the mock's own count: 0, a quarter, a half, and so on.
const Y_TICKS: usize = 4;

/// THE PLOT AREA AND THE KEY AREA, with the y scale drawn and the frame ruled.
///
/// # ONE CHROME FOR EVERY CHART ON THE PAGE
///
/// Both charts here (the per-second timeline and the fight-by-fight progression) get the same
/// gridlines, the same tick labels and the same key, because two charts on one page drawn two ways
/// is two things a reader has to learn. Returns the rect the data goes in and the rect the key
/// goes in; the caller draws its own x labels, because seconds and fights are not the same axis.
///
/// `top` IS THE VALUE AT THE TOP RULE and is chosen by the caller off its own data, so the scale
/// is the data's and never a rounded invention.
fn chart_frame(ui: &mut Ui, height: f32, top: u64, key: bool, lanes: usize) -> (Rect, Rect) {
    let full = ui.available_width().max(80.0);
    let (all, _) = ui.allocate_exact_size(Vec2::new(full, height.max(60.0)), Sense::hover());
    let key_rect = if key {
        Rect::from_min_max(
            Pos2::new(all.right() - KEY_W, all.top()),
            Pos2::new(all.right(), all.bottom() - X_AXIS),
        )
    } else {
        Rect::NOTHING
    };
    /* THE LANES LIVE INSIDE THE CHART'S OWN ALLOCATION. The flags used to be painted at
     * `plot.top() - 15`, which is ABOVE the rect the chart was given, so they sat on whatever
     * the card had put there. Stacking them made that worse the higher they went. */
    let plot = Rect::from_min_max(
        Pos2::new(
            all.left() + Y_AXIS,
            all.top() + 4.0 + lanes as f32 * LANE_STEP,
        ),
        Pos2::new(
            if key {
                key_rect.left() - 10.0
            } else {
                all.right()
            },
            all.bottom() - X_AXIS,
        ),
    );
    let p = ui.painter();
    for i in 0..=Y_TICKS {
        let t = i as f32 / Y_TICKS as f32;
        let y = plot.bottom() - plot.height() * t;
        p.hline(
            plot.x_range(),
            y,
            egui::Stroke::new(1.0, if i == 0 { LINE } else { LINE_SOFT }),
        );
        let value = (top as f64 * t as f64).round() as u64;
        p.text(
            Pos2::new(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            crate::screens::dps::thousands(value),
            egui::FontId::proportional(9.5),
            TEXT_3,
        );
    }
    (plot, key_rect)
}

/// ONE LABEL UNDER THE X AXIS, centred on its point and kept inside the plot.
fn x_label(ui: &Ui, plot: Rect, x: f32, text: &str) {
    let x = x.clamp(plot.left() + 14.0, plot.right() - 14.0);
    ui.painter().text(
        Pos2::new(x, plot.bottom() + 4.0),
        egui::Align2::CENTER_TOP,
        text,
        egui::FontId::proportional(9.5),
        TEXT_3,
    );
}

/// THE KEY: a swatch and a name per line, down the right hand side, as the mock has it.
fn chart_key(ui: &Ui, rect: Rect, rows: &[(String, egui::Color32)]) {
    if rect == Rect::NOTHING {
        return;
    }
    let p = ui.painter();
    for (i, (name, tint)) in rows.iter().enumerate() {
        let y = rect.top() + 8.0 + i as f32 * 17.0;
        if y > rect.bottom() {
            break;
        }
        p.rect_filled(
            Rect::from_min_size(Pos2::new(rect.left(), y - 4.0), Vec2::splat(9.0)),
            2.0,
            *tint,
        );
        p.text(
            Pos2::new(rect.left() + 15.0, y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(10.5),
            TEXT_2,
        );
    }
}

/// THE WIDTH THE TAB FOR THIS LABEL WILL TAKE, asked before anything is drawn.
///
/// [`flag`]'S OWN ARITHMETIC, so the two cannot drift: the word, its padding, and the gap the
/// next tab has to start after.
fn flag_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::proportional(FLAG_PT), TEXT)
        .size()
        .x
        + FLAG_PAD
        + FLAG_APART
}

/// FLAGS THAT WOULD OVERPRINT EACH OTHER, FOLDED INTO ONE.
///
/// # DEFECT: FIVE DEATHS IN ONE PULL CAME OUT AS `YoYouYeYuYiXited died`
///
/// [`flag`] centres a tab on the second it marks and nothing stopped two tabs sharing a pixel.
/// On a fight's own clock the deaths in it are spread across the whole width; on eight hours of
/// wall clock they are the same three pixels, and five labels printed on top of each other are
/// not any of the five words.
///
/// # MEASURED, NOT SPACED
///
/// A fixed minimum gap is a guess about how wide a label is, and these are not one width:
/// `Kill` is a third of `Tanefilo died`. So each label is laid out and asked how much room it
/// wants, and a flag is folded into the one before it when its tab would start left of where
/// that one ends. Folding CHANGES the label, and so its width, so the running edge is
/// recomputed from the folded text rather than from what it was before.
///
/// NOTHING IS DROPPED, WHICH IS THE DIFFERENCE FROM A CAP. A fold keeps the first name and says
/// how many more went with it (`Tanefilo died +4`), so the chart never quietly loses a death.
fn fold_flags(
    ui: &Ui,
    mut marks: Vec<(f32, egui::Color32, String)>,
) -> Vec<(f32, egui::Color32, String, usize)> {
    marks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    /* (x, tint, first name, how many are folded into it) */
    let mut groups: Vec<(f32, egui::Color32, String, usize)> = marks
        .into_iter()
        .map(|(x, tint, name)| (x, tint, name, 1))
        .collect();

    loop {
        /* LANES FIRST, LEFT TO RIGHT, LOWEST FREE LANE WINS.
         *
         * A tab goes in the lowest lane whose last tab has already ended before this one
         * starts. That is what puts neighbours at different heights instead of folding them
         * into one label, which is what the owner asked for and is also strictly more
         * information: three lanes hold three times the flags before anything has to be folded.
         */
        let mut edge = [f32::MIN; FLAG_LANES];
        let mut lane_of: Vec<usize> = Vec::with_capacity(groups.len());
        let mut stuck: Option<usize> = None;
        for (i, g) in groups.iter().enumerate() {
            let w = flag_width(ui, &fold_label(&g.2, g.3));
            match (0..FLAG_LANES).find(|l| g.0 - w / 2.0 >= edge[*l]) {
                Some(l) => {
                    edge[l] = g.0 + w / 2.0;
                    lane_of.push(l);
                }
                /* EVERY LANE IS BUSY UNDER THIS TAB. Fold, and lay them all out again. */
                None => {
                    stuck = Some(i);
                    break;
                }
            }
        }
        let Some(i) = stuck else {
            return groups
                .into_iter()
                .zip(lane_of)
                .map(|((x, tint, first, n), lane)| (x, tint, fold_label(&first, n), lane))
                .collect();
        };

        /* THE CLOSEST PAIR IS THE ONE THAT GOES, and not the one that happened to be stuck.
         *
         * Folding the stuck flag into whatever precedes it throws away the flag furthest from
         * its neighbours as readily as the one on top of them. Merging the tightest pair on the
         * chart loses the least: it is the pair a reader could least tell apart anyway.
         *
         * EVERY PASS MERGES EXACTLY ONE PAIR, so this cannot spin. At worst every mark ends up
         * in one group, which still says how many there were.
         */
        let _ = i;
        let mut tightest = 0usize;
        let mut gap = f32::MAX;
        for (j, pair) in groups.windows(2).enumerate() {
            let d = pair[1].0 - pair[0].0;
            if d < gap {
                gap = d;
                tightest = j;
            }
        }
        if groups.len() < 2 {
            /* ONE FLAG THAT STILL WILL NOT FIT is drawn anyway: `flag` clamps it into the plot,
             * and a chart too narrow for one label is a chart nobody is reading labels off. */
            return groups
                .into_iter()
                .map(|(x, tint, first, n)| (x, tint, fold_label(&first, n), 0))
                .collect();
        }
        let folded = groups.remove(tightest + 1);
        groups[tightest].3 += folded.3;
    }
}

/// HOW MANY HEIGHTS A FLAG MAY BE DRAWN AT, stacked upward off the top of the plot.
///
/// THREE, BECAUSE THAT IS WHAT THE CHART CAN SPARE. Every lane is room taken off the plot
/// itself (see `chart_frame`), so this is a trade between how many flags can be read and how
/// much chart is left to read them against.
const FLAG_LANES: usize = 3;

/// How tall one flag's tab is, and how far apart two lanes sit.
const TAB_H: f32 = 17.0;
const LANE_STEP: f32 = TAB_H + 3.0;

/// WHAT A FOLDED FLAG SAYS: the first name, and how many went with it.
///
/// THE NAME SURVIVES THE FOLD. `+4` on its own would say a number happened; `Tanefilo died +4`
/// still names somebody and says there were four more, which is what the marks actually are.
fn fold_label(first: &str, n: usize) -> String {
    if n <= 1 {
        first.to_owned()
    } else {
        format!("{first} +{}", n - 1)
    }
}

/// The type a flag's tab is lettered in, and the padding around it.
const FLAG_PT: f32 = 11.5;
const FLAG_PAD: f32 = 12.0;

/// The clear air between two tabs, so a fold is not two labels touching.
const FLAG_APART: f32 = 6.0;

/// A FLAG ON THE TIMELINE: a vertical rule with a labelled tab at the top, the mock's own marker.
fn flag(ui: &Ui, plot: Rect, x: f32, text: &str, tint: egui::Color32, lane: usize) {
    let p = ui.painter();
    /* THE RULE STARTS AT THE TAB, not at the plot, so a flag in the second lane is joined to
     * its own second rather than floating over the chart with a gap under it. */
    let top = plot.top() - (lane as f32 + 1.0) * LANE_STEP;
    p.line_segment(
        [Pos2::new(x, top + TAB_H), Pos2::new(x, plot.bottom())],
        egui::Stroke::new(1.0, tint),
    );
    let galley = p.layout_no_wrap(text.to_owned(), egui::FontId::proportional(FLAG_PT), TEXT);
    let w = galley.size().x + FLAG_PAD;
    let left = (x - w / 2.0).clamp(plot.left(), plot.right() - w);
    let tab = Rect::from_min_size(Pos2::new(left, top), Vec2::new(w, TAB_H));
    p.rect_filled(tab, 3.0, tint.gamma_multiply(0.30));
    p.rect_stroke(
        tab,
        3.0,
        egui::Stroke::new(1.0, tint),
        egui::StrokeKind::Inside,
    );
    /* AT `FLAG_PT`, WHICH IS WHAT THE TAB ABOVE WAS MEASURED FOR. The words were drawn at 9.5
     * points inside a tab sized for 11.5, which is the illegible pink the owner pointed at with
     * room to spare on both sides of it. */
    p.text(
        tab.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(FLAG_PT),
        TEXT,
    );
}

/// THE DPS TIMELINE: one line per fighter, second by second, with the fight's own events flagged.
///
/// # THIS IS ONE FIGHT AND IT HAS TO BE
///
/// `Fighter::series` is stamped from its OWN fight's start, and `reports::roll` drops both the
/// series and the moments when it folds a scope for exactly that reason: two fights' seconds
/// cannot be laid on one axis without inventing a gap between them. So this draws the most recent
/// fight in the scope and names it, and the filter is how a reader picks a different one.
///
/// # WHICH EVENTS ARE FLAGGED, AND WHICH THE MOCK HAS THAT THIS DOES NOT
///
/// ENGAGE, every player DEATH by name, and every KILL. All three come off `FightRow::moments`,
/// which the fold builds from lines the log printed.
///
/// THE MOCK ALSO FLAGS `Slow` AND `Proc` AND THIS DOES NOT, which is section 15's own ruling: both
/// need effect lines this build does not classify, and a flag on a chart is a claim about a moment
/// in a fight. Abilities and crits are real and are NOT flagged either, for a different reason:
/// there are hundreds of them in a four minute fight and a chart under that many rules is a
/// chart nobody can read.
fn timeline_body(ui: &mut Ui, fights: &[FightRow], why: NoFights) {
    if fights.is_empty() {
        ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
        return;
    }
    /* ONE FIGHT IS A FIGHT'S OWN CLOCK; MANY ARE THE PERIOD'S.
     *
     * DEFECT: THIS DREW ONE FIGHT WHATEVER THE SCOPE WAS. It picked `the newest fight with a
     * reading` out of the scope and said so in the corner, which the owner called what it was:
     * a card that ignores the filter above it. A reader who has picked seven days is asking
     * about seven days.
     *
     * AND THE TWO CASES WANT DIFFERENT CHARTS. Over a period the question is the SHAPE of it,
     * where the pulls were and where the quiet was, and six named lines across a week is a
     * thicket nobody can read; over one fight the question is who was doing what, and the names
     * are the whole point. So the period draws one line and no key, and a fight draws its
     * players and names them. That is also why the key vanishes on a period: there is nothing
     * left for it to name. */
    match fights {
        [one] => one_fight(ui, one),
        many => whole_scope(ui, many),
    }
}

/// THE WHOLE SCOPE ON ONE CLOCK: what the group was doing, second by second, across every fight.
///
/// # THE AXIS IS WALL CLOCK AND THE GAPS ARE REAL
///
/// From the first fight's start to the last one's end, so the idle stretches between pulls take
/// up the room they actually took. Each fight is its OWN polyline: a single line through every
/// point would join the end of one pull to the start of the next and draw damage across a gap
/// where the reader was selling loot.
///
/// ONE LINE, WHICH IS THE GROUP'S. Every player's second-by-second damage summed at each second.
/// Six named lines over a week is a thicket, and the question a period asks is where the work
/// was, not whose it was: that is what picking a fight is for.
fn whole_scope(ui: &mut Ui, fights: &[FightRow]) {
    use chrono::Timelike;
    /* THE SCOPE'S OWN CLOCK. A fight whose stamp this build cannot read has no place on a wall
     * clock axis and is left off rather than filed under a guessed time. */
    let mut spans: Vec<(i64, &FightRow)> = Vec::new();
    let mut first: Option<chrono::NaiveDateTime> = None;
    for f in fights {
        let Some(t) = crate::screens::night::started_at(&f.start) else {
            continue;
        };
        first = Some(first.map_or(t, |a: chrono::NaiveDateTime| a.min(t)));
    }
    let Some(origin) = first else {
        ui.label(RichText::new("No fight in this scope carries a readable time.").color(TEXT_2));
        return;
    };
    for f in fights {
        let Some(t) = crate::screens::night::started_at(&f.start) else {
            continue;
        };
        spans.push(((t - origin).num_seconds(), f));
    }
    let span = spans
        .iter()
        .map(|(at, f)| at + f.secs.max(1))
        .max()
        .unwrap_or(1)
        .max(1);

    /* THE GROUP'S OWN SERIES, PER FIGHT. Summed across the players in that fight at each second
     * of it, which is the same arithmetic the group total under a roster does, per second.
     *
     * THE SCOPE'S RULE, `reports::scope_roster`, which is what the roster and the Group DPS cell
     * on this page merge by (`reports::roll`). See `group_seconds`. */
    let known = crate::screens::reports::rolled_group(fights).is_some();
    let mut lines: Vec<Vec<(i64, u64)>> = Vec::new();
    let mut peak = 1u64;
    for (at, f) in &spans {
        let by_sec = group_seconds(known, f);
        if by_sec.is_empty() {
            continue;
        }
        peak = peak.max(by_sec.values().copied().max().unwrap_or(1));
        /* WALKED SECOND BY SECOND OVER THE FIGHT so a lull inside it reads as zero rather than
         * as a straight line between two pulls, which is what `one_fight` does and for the same
         * reason. */
        let mut pts = Vec::with_capacity(f.secs.max(1) as usize + 1);
        for s in 0..=u32::try_from(f.secs.max(1)).unwrap_or(1) {
            pts.push((at + i64::from(s), by_sec.get(&s).copied().unwrap_or(0)));
        }
        lines.push(pts);
    }
    if lines.is_empty() {
        ui.label(
            RichText::new("No fight in this scope has a second by second reading.").color(TEXT_2),
        )
        .on_hover_text(no_timeline_why(known));
        return;
    }

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} fights", fights.len()))
                .color(TEXT)
                .size(11.5)
                .strong(),
        );
        ui.label(
            RichText::new(format!(
                "{} to {}",
                origin.format("%b %-d %H:%M"),
                (origin + chrono::Duration::seconds(span)).format("%b %-d %H:%M")
            ))
            .color(TEXT_3)
            .size(11.0),
        );
    });
    ui.add_space(14.0);

    let height = (ui.available_height() - 6.0).clamp(90.0, 320.0);
    /* NO KEY: one line has nothing to name. See the note on this function. */
    let (plot, _) = chart_frame(ui, height, peak, false, FLAG_LANES);
    let x_of = |sec: i64| plot.left() + plot.width() * (sec as f32 / span as f32).clamp(0.0, 1.0);
    let y_of = |a: u64| plot.bottom() - plot.height() * (a as f32 / peak as f32).clamp(0.0, 1.0);

    /* THE CLOCK ALONG THE BOTTOM IS THE WALL CLOCK, and it carries the day when the scope
     * crosses one: `21:14` five times over a week says nothing about which day. */
    let days = span > 20 * 60 * 60;
    for i in 0..=4 {
        let sec = span * i64::from(i) / 4;
        let at = origin + chrono::Duration::seconds(sec);
        let text = if days {
            at.format("%b %-d").to_string()
        } else {
            format!("{:02}:{:02}", at.hour(), at.minute())
        };
        x_label(ui, plot, x_of(sec), &text);
    }

    for pts in &lines {
        let line: Vec<Pos2> = pts
            .iter()
            .map(|(s, a)| Pos2::new(x_of(*s), y_of(*a)))
            .collect();
        ui.painter()
            .add(egui::Shape::line(line, egui::Stroke::new(1.5, GOLD)));
    }

    /* THE DEATHS ACROSS THE WHOLE PERIOD, capped, newest last. */
    let mut flagged: Vec<(i64, String, egui::Color32)> = Vec::new();
    for (at, f) in &spans {
        for m in &f.moments {
            if let crate::fights::Mark::Death { victim, .. } = &m.what {
                let Some(x) = f.fighters.get(*victim) else {
                    continue;
                };
                /* A DEATH ON THE GROUP'S OWN LINE, so the same scope rule as the line. */
                if crate::screens::reports::scope_roster(known, f, &x.who) {
                    flagged.push((
                        at + i64::from(m.at),
                        format!("{} died", x.who.text()),
                        death_tint(&x.who),
                    ));
                }
            }
        }
    }
    /* DEATHS ONLY, AND NOT KILLS. Over one fight a kill is the shape of the fight; over a night
     * there are thirty of them and they would be a picket fence across the top of the chart. */
    /* FOLDED SO NOTHING OVERPRINTS, and nothing is dropped to make that true: see `fold_flags`. */
    let marks: Vec<(f32, egui::Color32, String)> = flagged
        .into_iter()
        .map(|(at, text, tint)| (x_of(at), tint, text))
        .collect();
    for (x, tint, text, lane) in fold_flags(ui, marks) {
        flag(ui, plot, x, &text, tint, lane);
    }
}

/// WHY THE SCOPE TIMELINE HAS NO LINE, by the scope's rule.
///
/// `A timeline needs a player who dealt damage ... the log recorded none` was true while every
/// player was on the line. Over a known scope the line is the roster's, a stranger may well have
/// dealt damage in every fight, and the log did record it: the sentence has to name the roster.
fn no_timeline_why(known: bool) -> &'static str {
    if known {
        "A timeline needs somebody on your roster who dealt damage: you, your pets, or whoever was \
         in your group in that fight. None of them did in any fight in scope. A player outside \
         your group may have, and is not drawn."
    } else {
        "A timeline needs a player who dealt damage. Every fight in scope is one where the log \
         recorded none, which is what a wipe looks like from the fold's side."
    }
}

/// ONE FIGHT'S LINE ON THE SCOPE TIMELINE: the scope's roster's damage in that fight, summed at each
/// second of it.
///
/// THE SCOPE'S RULE AND NOT A COPY OF IT (`reports::scope_roster`). The roster card and the Group
/// DPS cell beside this chart are the scope's fold, and `reports::roll` merges by that same rule,
/// so a second on this line is damage a row beside it counts. `known` is whether the scope's group
/// is known: every player's series when it is not, this fight's own roster's when it is.
fn group_seconds(known: bool, f: &FightRow) -> std::collections::BTreeMap<u32, u64> {
    let mut by_sec: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
    for x in f
        .fighters
        .iter()
        .filter(|x| crate::screens::reports::scope_roster(known, f, &x.who))
    {
        for (s, a) in &x.series {
            *by_sec.entry(*s).or_default() += *a;
        }
    }
    by_sec
}

/// WHAT A DEATH ON ONE FIGHT'S CHART IS FLAGGED AS, or `None` for a death the chart says nothing
/// about.
///
/// THREE CASES AND NOT TWO. Somebody on the roster died; a mob is a kill; and a player the log
/// proved was NOT in the reader's group is neither. He has no line on this chart, and calling his
/// death a `Kill` would say the group killed a person.
/// THE COLOUR OF A DEATH ON A TIMELINE, by whose it was.
///
/// THREE COLOURS, BECAUSE THE OWNER READS THREE DIFFERENT EVENTS. The reader dying is red, the
/// alarm colour this app uses for the reader's own trouble; somebody else on the roster dying is
/// orange, still bad news and plainly not his; a mob going down is gold, the colour the chart
/// already uses for the good kind of mark. All three were one red, so a night with the reader's
/// death in it looked the same as a night with a groupmate's.
fn death_tint(who: &crate::fights::Who) -> egui::Color32 {
    if matches!(who, crate::fights::Who::You) {
        WRONG
    } else {
        crate::theme::ORANGE
    }
}

fn death_flag(f: &FightRow, victim: &crate::fights::Fighter) -> Option<(String, egui::Color32)> {
    if f.ours(&victim.who) {
        Some((
            format!("{} died", victim.who.text()),
            death_tint(&victim.who),
        ))
    } else if !f.player(&victim.who) {
        Some((String::from("Kill"), GOLD))
    } else {
        None
    }
}

/// ONE FIGHT, SECOND BY SECOND, WITH THE PLAYERS NAMED.
///
/// The key is the point here: over one pull the question is who was doing what. See
/// [`whole_scope`] for the other half of the pair.
fn one_fight(ui: &mut Ui, f: &FightRow) {
    /* THE ROSTER'S POPULATION, off the same `widgets::charted` the Analysis timeline draws by: a
     * line here is a row on the roster beside it. */
    let who = crate::screens::widgets::charted(f, 6);
    if who.is_empty() {
        ui.label(
            /* THE ROSTER'S WORDS, `dps::nobody`: the lines here are the roster's, and on a fight
             * whose group is known a player outside it may well have dealt damage. */
            RichText::new(crate::screens::dps::nobody(f, Metric::Dealt)).color(TEXT_2),
        );
        return;
    }

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(f.headline.as_deref().unwrap_or(UNNAMED))
                .color(TEXT)
                .size(11.5)
                .strong(),
        );
        ui.label(RichText::new(clock(f.secs)).color(TEXT_3).size(11.0));
    });
    ui.add_space(14.0);

    let span = u32::try_from(f.secs.max(1)).unwrap_or(1);
    let peak = who
        .iter()
        .flat_map(|x| x.series.iter().map(|(_, a)| *a))
        .max()
        .unwrap_or(1)
        .max(1);
    let height = (ui.available_height() - 6.0).clamp(90.0, 320.0);
    let (plot, key) = chart_frame(ui, height, peak, true, FLAG_LANES);
    let x_of = |sec: u32| plot.left() + plot.width() * (sec as f32 / span as f32).clamp(0.0, 1.0);
    let y_of = |a: u64| plot.bottom() - plot.height() * (a as f32 / peak as f32).clamp(0.0, 1.0);

    /* THE CLOCK ALONG THE BOTTOM: the start, the quarters, and the end. */
    for i in 0..=4 {
        let sec = span * i / 4;
        x_label(ui, plot, x_of(sec), &clock(i64::from(sec)));
    }

    for (i, x) in who.iter().enumerate() {
        let tint = crate::screens::dps::rank_tint(i);
        /* ONE POINT PER SECOND, zero where the fighter dealt nothing. Walking the span rather
         * than the series is what puts the gaps in: a line over only the seconds that HAVE
         * entries would join across a lull and draw damage that never happened. */
        let mut pts: Vec<Pos2> = Vec::with_capacity(span as usize + 1);
        let mut at = 0usize;
        for sec in 0..=span {
            let amount = match x.series.get(at) {
                Some((s, a)) if *s == sec => {
                    at += 1;
                    *a
                }
                _ => 0,
            };
            pts.push(Pos2::new(x_of(sec), y_of(amount)));
        }
        ui.painter()
            .add(egui::Shape::line(pts, egui::Stroke::new(1.5, tint)));
    }

    /* THE FLAGS, over the lines. Engage is the fight's own first second. */
    flag(ui, plot, x_of(0), "Engage", SETTLED, 0);
    let mut flagged: Vec<(u32, String, egui::Color32)> = Vec::new();
    for m in &f.moments {
        if let crate::fights::Mark::Death { victim, .. } = &m.what {
            let Some(x) = f.fighters.get(*victim) else {
                continue;
            };
            if let Some((text, tint)) = death_flag(f, x) {
                flagged.push((m.at, text, tint));
            }
        }
    }
    /* FOLDED RATHER THAN CAPPED. This took the newest five and dropped the rest, so a wipe with
     * eight deaths in it silently lost three; folding keeps every one of them and says so. */
    let marks: Vec<(f32, egui::Color32, String)> = flagged
        .into_iter()
        .map(|(at, text, tint)| (x_of(at), tint, text))
        .collect();
    for (x, tint, text, lane) in fold_flags(ui, marks) {
        flag(ui, plot, x, &text, tint, lane);
    }

    /* THE PLAYERS IN THIS FIGHT, NAMED. */
    chart_key(
        ui,
        key,
        &who.iter()
            .enumerate()
            .map(|(i, x)| (x.who.text().to_owned(), crate::screens::dps::rank_tint(i)))
            .collect::<Vec<_>>(),
    );
}

/// YOUR DPS, FIGHT BY FIGHT ACROSS THE SCOPE: the progression the owner asked to look back at.
///
/// # A POINT PER FIGHT, AND THE AXIS IS THE NIGHT'S OWN CLOCK
///
/// One point per finished fight in the scope, in time order, its height your damage per second in
/// that fight off `dps::dps` (the same rate every table prints, floor and all). The x labels are
/// the wall clock the fights started at, so `I was doing this at nine and this by midnight` is a
/// thing the chart can be read for.
///
/// NO LINE IS FITTED AND NO TREND IS CLAIMED. A trend is a statement about fights being
/// comparable, and nothing in a log line says two of them are: see `screens::dps`'s module note.
/// What is drawn is the measurements and the line between them, which is a shape the reader reads
/// rather than a claim the app makes.
fn progression_body(ui: &mut Ui, fights: &[FightRow], why: NoFights) {
    let pts: Vec<(Option<u64>, &FightRow)> = fights
        .iter()
        .map(|f| {
            let you = f
                .fighters
                .iter()
                .find(|x| matches!(x.who, crate::fights::Who::You));
            (
                you.and_then(|y| crate::screens::dps::dps(y.dealt, f.secs)),
                f,
            )
        })
        .collect();
    if pts.is_empty() {
        ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
        return;
    }
    let Some(top) = pts.iter().filter_map(|(d, _)| *d).max() else {
        ui.label(RichText::new("You took no part in these fights.").color(TEXT_2));
        return;
    };
    let top = top.max(1);
    let height = (ui.available_height() - 24.0).clamp(80.0, 320.0);
    /* NO LANES: progression flags nothing. */
    let (plot, _) = chart_frame(ui, height, top, false, 0);
    let n = pts.len();
    let x_of = |i: usize| {
        if n <= 1 {
            plot.center().x
        } else {
            plot.left() + plot.width() * (i as f32 / (n - 1) as f32)
        }
    };
    let y_of = |d: u64| plot.bottom() - plot.height() * (d as f32 / top as f32).clamp(0.0, 1.0);

    /* THE WALL CLOCK OF THE FIRST, MIDDLE AND LAST FIGHT. `night::started_at` is the one place
     * the log's stamp is parsed; a fight it cannot date simply has no label. */
    for i in [0usize, n / 2, n.saturating_sub(1)] {
        if let Some(t) = night::started_at(&pts[i].1.start) {
            x_label(ui, plot, x_of(i), &t.format("%H:%M").to_string());
        }
    }

    /* THE LINE THROUGH EVERY FIGHT YOU WERE IN, then a dot on each so a single fight still
     * draws something and a reader can aim at one. */
    let line: Vec<Pos2> = pts
        .iter()
        .enumerate()
        .filter_map(|(i, (d, _))| d.map(|d| Pos2::new(x_of(i), y_of(d))))
        .collect();
    if line.len() > 1 {
        ui.painter().add(egui::Shape::line(
            line.clone(),
            egui::Stroke::new(1.5, GOLD),
        ));
    }
    let mut best = 0u64;
    for (i, (d, f)) in pts.iter().enumerate() {
        let Some(d) = d else {
            continue;
        };
        best = best.max(*d);
        let at = Pos2::new(x_of(i), y_of(*d));
        let last = i + 1 == n;
        ui.painter().circle_filled(
            at,
            if last { 3.5 } else { 2.5 },
            if last { GOLD_HI } else { GOLD },
        );
        let hit = ui.interact(
            Rect::from_center_size(
                Pos2::new(at.x, plot.center().y),
                Vec2::new(14.0, plot.height()),
            ),
            ui.id().with(("prog", i)),
            Sense::hover(),
        );
        if hit.hovered() {
            ui.painter().line_segment(
                [Pos2::new(at.x, plot.top()), Pos2::new(at.x, plot.bottom())],
                egui::Stroke::new(1.0, TEXT_3),
            );
            hit.on_hover_text(format!(
                "{}\n{} \u{b7} {} dps",
                f.headline.as_deref().unwrap_or(UNNAMED),
                clock(f.secs),
                crate::screens::dps::thousands(*d)
            ));
        }
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{n} fight{}", if n == 1 { "" } else { "s" }))
                .color(TEXT_3)
                .size(11.0),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("best {} dps", crate::screens::dps::thousands(best)))
                    .color(TEXT_2)
                    .size(11.0),
            );
        });
    });
}

fn fights_body(ui: &mut Ui, cx: &mut Cx, fights: &[FightRow], why: NoFights) {
    if fights.is_empty() {
        ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
        return;
    }
    let mut shown = 0usize;
    for f in fights.iter().rev() {
        if !room_for(ui, 20.0) {
            break;
        }
        shown += 1;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(
                RichText::new(f.headline.as_deref().unwrap_or("an unnamed fight"))
                    .color(TEXT)
                    .size(12.0),
            );
            if let Some(z) = f.zone.as_deref() {
                ui.label(RichText::new(z).color(TEXT_3).size(10.5));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(
                    RichText::new(clock(f.secs))
                        .color(TEXT_3)
                        .monospace()
                        .size(11.0),
                );
                ui.label(
                    RichText::new(format!("{} dmg", crate::screens::dps::short(f.damage)))
                        .color(TEXT_2)
                        .size(11.0),
                );
                let dead = your_deaths(f);
                if dead > 0 {
                    ui.label(
                        RichText::new(format!("{dead} \u{2620}"))
                            .color(WRONG)
                            .size(11.0),
                    )
                    .on_hover_text(if dead == 1 {
                        String::from("You died in this fight.")
                    } else {
                        format!("You died {dead} times in this fight.")
                    });
                }
            });
        });
    }
    more_line(
        ui,
        cx,
        Tile::Fights,
        fights.len().saturating_sub(shown),
        "fights",
    );
}

/// HOW MANY TIMES THE READER DIED IN THIS FIGHT, off `Mark::Death`.
///
/// # THE READER'S OWN AND NOBODY ELSE'S
///
/// This counted the fight's roster: the reader, the pets and whoever was in the group. The owner
/// read `1 Gartik` in the Deaths cell, a group member's death, and said this dashboard is a
/// personal one and not a guild's or a raid's. A pet dying is not the reader dying either, and a
/// charm that breaks ends in a kill. [`death_by`] is the one rule, and the strip counts by it too.
fn your_deaths(f: &FightRow) -> usize {
    f.moments
        .iter()
        .filter(|m| death_by(f, m).is_some())
        .count()
}

/// WHAT KILLED THE READER, IF THIS MOMENT IS THE READER DYING.
///
/// `None` when it is not the reader dying. `Some(None)` when it is and the log named nothing this
/// fight can spell, so a caption says nothing rather than borrowing an older killer's name.
fn death_by<'a>(f: &'a FightRow, m: &crate::fights::Moment) -> Option<Option<&'a str>> {
    match &m.what {
        crate::fights::Mark::Death { killer, victim }
            if f.fighters
                .get(*victim)
                .is_some_and(|x| x.who == crate::fights::Who::You) =>
        {
            Some(
                f.fighters
                    .get(*killer)
                    .filter(|k| {
                        !matches!(k.who, crate::fights::Who::You | crate::fights::Who::Unknown)
                    })
                    .map(|k| k.who.text()),
            )
        }
        _ => None,
    }
}

/// EVERY NAMED THING THAT DIED IN THE SCOPE, most killed first.
fn named_kills(fights: &[FightRow]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for f in fights {
        for m in &f.moments {
            if let crate::fights::Mark::Death { victim, .. } = &m.what {
                if let Some(x) = f.fighters.get(*victim) {
                    if f.player(&x.who) {
                        continue;
                    }
                    let name = x.who.text().to_owned();
                    match out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                        Some((_, c)) => *c += 1,
                        None => out.push((name, 1)),
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

fn night_body(ui: &mut Ui, cx: &mut Cx) {
    let Some(_) = cx.ingest.store() else {
        /* AND THE SAME HERE, for the same card width. */
        ui.label(RichText::new("Nothing on disk yet.").color(TEXT_2))
            .on_hover_text(
                "This build keeps finished fights in its own folder so a night survives the log's \
                 tail moving on. Nothing has opened that folder in this session.",
            );
        return;
    };
    let w = cx.ingest.stored();
    /* TWO FIGURES AND NOTHING ELSE. The sub-heading said `this app's own store`, which is what
     * the card is already called, and the caption said `fight written this session`, which is
     * what the number already means. Both went; see `stat_pair`. */
    let s = if w.added == 1 { "" } else { "s" };
    let (page, section) = Tile::Night.page();
    let there = crate::nav::name_of(page).unwrap_or("the page this reads");
    if stat_pair(ui, w.added, &format!("fight{s} this session"), GOLD_HI)
        .on_hover_text(format!(
            "Fights this session's scan wrote into the store. Open {there}."
        ))
        .clicked()
    {
        cx.ask = Ask::Open(page, section);
    }
    /* THE SECOND FIGURE IS ONE SMALL LINE AND IT IS DRAWN EVEN AT ZERO.
     *
     * ONE LINE BECAUSE IT IS NOT THE HEADLINE. What this card is about is what tonight put on
     * disk; what was already there is the context for it. Built as a second `stat_pair` it was
     * two headlines of equal weight arguing about which one the card was for, and the owner said
     * so on sight. The difference in weight IS the ranking, which is why this is `stat_line`.
     *
     * AND AT ZERO TOO, unlike the line it replaces, which appeared only above zero. A stat that
     * comes and goes changes the card's height under a reader, and `0 fights on disk` is a real
     * answer to the question this card exists to answer. */
    /* `208 on disk` AND NOT `208 fights on disk`. The card is called `Written to disk` and the
     * figure above it is counted in fights; the word was the widest thing in a card the owner
     * wanted narrow, and it was the third time the card said `fights`. */
    if stat_line(ui, format!("{} on disk", w.already), TEXT_2)
        .on_hover_text(format!(
            "Fights the store already held, keyed on each fight's own start stamp, so a rescan \
             of the same log never writes one twice. Open {there}."
        ))
        .clicked()
    {
        cx.ask = Ask::Open(page, section);
    }
}

/// THE MOB TILE: what a mob has been measured to absorb.
///
/// SETTLED READINGS ONLY, AND THE COUNT OF THE REST. `hp::Reading::expect` refuses on a first
/// meeting, on two kills, and on a mob whose kills disagree, which is most mobs most of the time;
/// a card that listed every mob it had ever seen with a number beside it would be putting a
/// measurement's name on a single sample.
///
/// # THE EMPTY STATE SAID NOBODY HAD KILLED ANYTHING, AND IT WAS READING A DIFFERENT NUMBER
///
/// It read "No mob has been killed yet.", and that sentence was true of the count it was guarding
/// while `Ingest::hp_known` returned `self.hp.len()`: `hp::read` puts a row in the book for every
/// name that ever died, so an empty book really did mean nothing had died.
///
/// IT STOPPED BEING TRUE WHEN `hp_known` STARTED MEANING WHAT THIS TILE ALREADY CLAIMED. The big
/// number sits under the words "mobs measured" and a tooltip promising kinds of mob killed often
/// enough for their kills to agree, and the book's LENGTH was mobs SEEN: every name whose kills
/// were all thrown away (a fight where the name died twice, a kill the grammar could not
/// attribute, a clipped fight) still left a row. `hp_known` is now the readings that can stand a
/// figure up, which is `Reading::expect` and nothing looser.
///
/// SO ZERO NOW MEANS "NOTHING IS MEASURED YET" AND NOT "NOTHING HAS DIED", and those come apart on
/// an ordinary night: mobs are killed all evening and none of them has three kills that agree
/// until late. The old sentence would then have told a stream that this character had killed
/// nothing, which is a new false sentence produced by fixing a number. The count and the words
/// that stand in for it have to mean the same thing, so the label moved with it. The hover was
/// already right -- it always described the measuring, never the killing -- and is untouched.
fn mobs_body(ui: &mut Ui, cx: &mut Cx) {
    let known = cx.ingest.hp_known();
    if known == 0 {
        /* SHORT ENOUGH FOR THE CARD IT IS IN. This is two columns wide now, and the sentence it
         * had (`No mob has been measured yet.`) wrapped to three lines and ran out of the bottom
         * of it. The distinction the long one was drawing, measured against killed, is the whole
         * point of the words and it is kept: `measured` is still the verb, and the hover below
         * still says the rest. */
        ui.label(RichText::new("None measured.").color(TEXT_2))
            .on_hover_text(
                "A mob's health is never printed. This reads it off kills that finished, and it \
                 needs several that agree before it will answer at all.",
            );
        return;
    }
    /* THE SAME SHAPE AS `Written to disk`, AND SO THE SAME FUNCTION.
     *
     * A figure over a caption is what this card was already: `stat_pair` sets the caption beside
     * it instead, which is what lets the two of them share one narrow strip on the grid rather
     * than each taking a third of a band to hold one number. */
    stat_pair(ui, known, "mobs measured", GOLD_HI).on_hover_text(
        "Kinds of mob this character has killed often enough for their kills to agree on what \
         they absorb. Hover a target on the live tile to see its figure.",
    );
}

/// THE KILLS TILE: what the tracker has seen die, newest first.
/// THE KILLS TILE: what died in the scope, off the fights' own death marks.
fn kills_body(ui: &mut Ui, cx: &mut Cx, fights: &[FightRow], why: NoFights) {
    let k = named_kills(fights);
    if k.is_empty() {
        ui.label(RichText::new(nothing_in_scope(why)).color(TEXT_2));
        return;
    }
    let total: usize = k.iter().map(|(_, c)| *c).sum();
    let mut shown = 0usize;
    for (name, n) in &k {
        /* THE TOTAL LINE UNDER THIS LOOP IS PART OF THE BUDGET, and it was not.
         *
         * `room_for` keeps back a `+N more` line and nothing else, so this body drew rows until
         * the card was full, then a `+N more` into the last of it, then `N kills in scope` through
         * the bottom edge. It was cut off at EVERY height, which is why raising the floor could
         * never fix it. */
        if !room_for(ui, 18.0 + TAIL_H) {
            break;
        }
        shown += 1;
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).color(TEXT).size(12.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("\u{d7}{n}"))
                        .color(TEXT_2)
                        .monospace()
                        .size(11.0),
                );
            });
        });
    }
    more_line(ui, cx, Tile::Kills, k.len().saturating_sub(shown), "kinds");
    ui.label(
        RichText::new(format!("{total} kills in scope"))
            .color(TEXT_3)
            .size(11.0),
    );
}

fn loot_body(ui: &mut Ui, cx: &mut Cx) {
    let l = cx.ingest.loot();
    if l.is_empty() {
        ui.label(RichText::new("Nothing looted yet.").color(TEXT_2));
        return;
    }
    let mut shown = 0usize;
    for e in l.iter().rev() {
        if !room_for(ui, 18.0) {
            break;
        }
        shown += 1;
        let qty = if e.qty > 1 {
            format!("{}x ", e.qty)
        } else {
            String::new()
        };
        ui.label(
            RichText::new(format!("{qty}{}", e.item))
                .color(TEXT)
                .size(12.0),
        )
        .on_hover_text(format!("from {}", e.mob));
    }
    more_line(ui, cx, Tile::Loot, l.len().saturating_sub(shown), "drops");
}

/// THE SAME PANEL, ASKED TO STOP WHEN IT RUNS OUT OF CARD.
///
/// A COPY AND NOT A MUTATION, because the config belongs to the reader and this card is read
/// only: see [`overlays_body`]. Every arm is named rather than defaulted, so a new arm in the
/// vocabulary is a compile error here and not a panel that quietly goes back to clipping.
fn fitted(w: &Widget) -> Widget {
    let table = |r: &Ranked| Ranked { fit: true, ..*r };
    let panel = |d: &Detail| Detail { fit: true, ..*d };
    match w {
        Widget::Meter(r) => Widget::Meter(table(r)),
        Widget::Ranked(r) => Widget::Ranked(table(r)),
        Widget::Abilities(d) => Widget::Abilities(panel(d)),
        Widget::Targets(d) => Widget::Targets(panel(d)),
        Widget::Elements(d) => Widget::Elements(panel(d)),
        Widget::Outcomes(d) => Widget::Outcomes(panel(d)),
        /* A CHART IS NOT ROWS. It is drawn at the height its config names or not at all. */
        Widget::Timeline(t) => Widget::Timeline(*t),
        /* ONE LINE AND ONE BLOCK, NEITHER A LIST OF ROWS, so neither has anything to stop. */
        Widget::Coach(c) => Widget::Coach(*c),
        Widget::Pill(p) => Widget::Pill(*p),
    }
}

/// THE OVERLAYS TILE: THE OVERLAYS THE OWNER BUILT, AT PAGE DENSITY.
///
/// READ ONLY, DELIBERATELY. Nothing here writes a setting: a page that rearranged his overlays
/// from a dashboard would be a second editor for one list.
///
/// `or_default` IS THE SAME ANSWER THE PARSER'S LIST GIVES. A fresh install and a deliberately
/// emptied list look identical in JSON, and both get the shipped DPS overlay back rather than a
/// card with nothing on it and no way to tell whether the feature is broken.
fn overlays_body(ui: &mut Ui, cx: &mut Cx, fight: Option<&FightRow>, live: bool, why: NoFights) {
    let list = crate::overlay::or_default(&cx.settings.overlays);
    let total = list.len();
    let mut shown = 0usize;
    for o in list {
        if !room_for(ui, 40.0) {
            break;
        }
        shown += 1;
        ui.label(
            RichText::new(o.name.to_uppercase())
                .font(crate::fonts::display(12.0))
                .color(GOLD),
        );
        ui.add_space(4.0);
        /* WHAT IT DRAWS AND NOT WHAT IS STORED. An overlay nobody has configured has no list
         * in the file at all; `panels` resolves that to the shipped one, which is what his
         * window is showing him. See `overlay::Overlay::widgets`. */
        let panels = o.panels();
        if panels.is_empty() {
            ui.label(RichText::new("This overlay shows nothing yet.").color(TEXT_3));
            ui.add_space(8.0);
            continue;
        }
        match fight {
            Some(f) => {
                /* THE READER'S OWN CONFIG, DRAWN TO FIT THIS CARD.
                 *
                 * His overlay says twelve rows because that is what he wants over his game,
                 * where the window is his ruler and clipping is the point. This card's height
                 * is the grid's, so a twelfth row here is a row sliced by the card's bottom
                 * edge. `fit` is asked for on a COPY: nothing about his overlay is changed by
                 * being previewed. See `overlay::Detail::fit`. */
                for w in &panels {
                    crate::screens::dps::draw_widget(
                        ui,
                        f,
                        crate::fights::Pulse::from_live(live),
                        &fitted(w),
                    );
                }
            }
            /* NO SAMPLE FIGHT, exactly as the builder's preview refuses one: a demonstration
             * filled with invented numbers is the one thing this app does not do. */
            None => {
                ui.label(RichText::new(no_fights_words(why)).color(TEXT_2));
            }
        }
        ui.add_space(8.0);
    }
    more_line(
        ui,
        cx,
        Tile::Overlays,
        total.saturating_sub(shown),
        "overlays",
    );
}

/* ================================================== the mock's encounter head and strip ==
 *
 * The encounter head and strip of the owner's dashboard mock, which is not in this tree; the
 * grid it places them on is transcribed in `screens::dashgrid`. Every size, colour and separator
 * below came off the mock rather than out of this file.
 */

/// THE ENCOUNTER HEAD: a sigil, the fight's name in the display face, and a meta row.
///
/// # WHAT THIS REPLACED, AND WHY THE MOCK IS RIGHT
///
/// This page opened with a horizontal run of small labels: `IN COMBAT`, the name at 16 points, a
/// clock, a player count, a zone. Five equal things on one line, so nothing was the subject.
///
/// THE MOCK GIVES THE FIGHT A TITLE AND EVERYTHING ELSE A CAPTION, and that is the whole
/// difference. `.encounter-title h1` is `clamp(20px,2vw,28px)` Georgia, uppercase, `--gold-hi`,
/// with `.045em` of tracking; the meta under it is `13px --muted` with a `1px x 12px #755a2c`
/// rule between items rather than a bullet. A reader glancing at this knows what is being fought
/// before he has focused on anything.
///
/// THE SIGIL IS THE MOB'S OWN INITIAL and not an invented icon. There is no bestiary art in this
/// build and there is not going to be one made up: the mock's `.boss-sigil` is a lit stone tile
/// with a glyph in it, and the honest glyph is the first letter of what the log called the thing.
/// THE ENCOUNTER HEAD, section 4 of the mock: sigil, title, meta row, and the scope chips.
///
/// THE TITLE IS THE SCOPE'S HEADLINE, the named thing that took the most damage across every fight
/// in it, which is a fact the fold states. A `raid night` title is not: nothing in a log line says
/// a fight was a raid, so the meta row says the date and the count and lets the reader call it
/// what he likes.
#[allow(clippy::too_many_arguments)]
/// Returns the fight the reader chose, or `None` when he chose nothing this pass. The inner
/// `Option` is the choice itself: `Some(start)` for one fight, `None` for all of them.
fn encounter_head(
    ui: &mut Ui,
    facts: &Facts,
    filter: &Filter,
    fold: Option<&FightRow>,
    fights: &[FightRow],
    choosable: &[FightRow],
    pick: Option<&str>,
) -> Option<Option<String>> {
    let (_state, watching) = facts.watching();
    /* TWO DIFFERENT ABSENCES, AND THEY USED TO SHARE ONE SENTENCE. No fold at all means the
     * store has nothing in this scope; a fold with no headline means the fights in it named
     * no mob that took or dealt damage, which is a real state (`UNNAMED` has its reasons) and
     * not `nothing stored`. Printing the second as the first was a false sentence at 23 points
     * over three real fights. */
    let name = match (fold, pick) {
        (None, _) => "Nothing stored yet",
        /* ALL FIGHTS SAYS ALL FIGHTS. See the note on the head below. */
        (Some(_), None) => ALL_FIGHTS,
        (Some(f), Some(_)) => f.headline.as_deref().unwrap_or(UNNAMED),
    };
    let mut chose: Option<Option<String>> = None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        sigil(ui, name);
        ui.vertical(|ui| {
            ui.add_space(1.0);
            /* THE NAME IS THE CONTROL, WHICH IS WHAT THE OWNER ASKED FOR: the same line in the
             * same face, with a caret beside it and the fights behind it.
             *
             * ON ALL FIGHTS IT SAYS ALL FIGHTS, AND IT USED TO NAME THE SCOPE'S BIGGEST MOB.
             * That was argued as keeping a fact, and the owner read it the way anybody would:
             * with ALL picked in the menu under it, a mob's name in the control reads as one
             * fight picked, which is the opposite of the truth. A control says what it is set
             * to. The biggest mob in the scope is still on the fight list and on every roster. */
            let head = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    let t = ui.add(
                        egui::Label::new(
                            RichText::new(name.to_uppercase())
                                .font(crate::fonts::display(23.0))
                                .color(GOLD_HI),
                        )
                        .sense(egui::Sense::click()),
                    );
                    t.union(caret(ui))
                })
                .inner
                .on_hover_text(if choosable.len() > 1 {
                    format!("Read one of these {} fights on its own", choosable.len())
                } else {
                    String::from("The fights in this scope")
                });
            if head.clicked() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if let Some(c) = fight_menu(&head, choosable, pick) {
                chose = Some(c);
            }
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut first = true;
                let mut item = |ui: &mut Ui, text: String, tint: egui::Color32, tip: Option<String>| {
                    if !first {
                        meta_rule(ui);
                    }
                    first = false;
                    let r = ui.label(RichText::new(text).size(11.5).color(tint));
                    if let Some(t) = tip {
                        r.on_hover_text(t);
                    }
                };
                let mut why = watching.clone();
                for (line, note) in facts.history() {
                    why.push('\n');
                    why.push_str(&line);
                    why.push_str(" -- ");
                    why.push_str(&note);
                }
                /* WHAT IS BEING LOOKED AT, IN FULL, and it used to be a row of chips under this.
                 *
                 * The owner's ruling: the dates should not all be on screen at once, they belong
                 * behind a filter key in the corner. So the head STATES the filter and the sheet
                 * CHANGES it, which is also the only way to show a filter that is four things
                 * (a night, a zone, a mob and a count) rather than one chip. */
                item(ui, filter.label(), GOLD, Some(why));
                let n = fights.len();
                item(
                    ui,
                    format!("{n} fight{}", if n == 1 { "" } else { "s" }),
                    TEXT_2,
                    Some(String::from(
                        "Finished fights in this scope, off this app's own store. A fight still \
                         going is not one of them.",
                    )),
                );
                if let Some(f) = fold {
                    item(ui, format!("{} in combat", long_clock(f.secs)), TEXT_2, None);
                    let mut zones: Vec<&str> = Vec::new();
                    for z in fights.iter().filter_map(|x| x.zone.as_deref()) {
                        if !zones.contains(&z) {
                            zones.push(z);
                        }
                    }
                    if !zones.is_empty() {
                        let shown: Vec<&str> = zones.iter().take(3).copied().collect();
                        let more = zones.len().saturating_sub(3);
                        let text = if more > 0 {
                            format!("{} +{more}", shown.join(", "))
                        } else {
                            shown.join(", ")
                        };
                        item(ui, text, TEXT_2, None);
                    }
                    if f.cut {
                        item(
                            ui,
                            String::from("a fight's opening was not read"),
                            WRONG,
                            Some(String::from(
                                "One fight in this scope had already produced more lines than \
                                 the live window holds when it was read, so its totals are a floor.",
                            )),
                        );
                    }
                }
            });
        });
    });
    chose
}

/// `3725` becomes `1:02:05`; under an hour, `04:26`.
fn long_clock(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        clock(s)
    }
}

fn sigil(ui: &mut Ui, name: &str) {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 7.0, egui::Color32::from_rgb(0x11, 0x15, 0x1B));
    p.rect_filled(
        rect.shrink(4.0),
        6.0,
        egui::Color32::from_rgb(0x28, 0x2F, 0x3A),
    );
    p.rect_filled(
        egui::Rect::from_center_size(
            rect.center() - Vec2::new(rect.width() * 0.05, rect.height() * 0.15),
            Vec2::splat(rect.width() * 0.42),
        ),
        8.0,
        egui::Color32::from_rgb(0x6A, 0x71, 0x80),
    );
    p.rect_stroke(
        rect,
        7.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x6D, 0x57, 0x34)),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        name.chars()
            .find(|c| c.is_alphabetic())
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
        crate::fonts::display(20.0),
        egui::Color32::WHITE,
    );
    r.on_hover_text(name.to_owned());
}

/// The mock's `span+span::before`: a `1px` by `12px` `#755a2c` rule between meta items.
fn meta_rule(ui: &mut Ui) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 11.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_rgb(0x75, 0x5A, 0x2C));
    ui.add_space(10.0);
}

/// THE SUMMARY STRIP: the mock's six stats in one bordered card, less the one that is a lie.
///
/// # FIVE AND NOT SIX, AND THE MISSING ONE IS THE POINT
///
/// The mock's fifth stat is `Group pressure: Medium 68%`. There is no such measurement anywhere
/// in an EverQuest Legends log, and there is no arithmetic over what the log DOES carry that
/// produces it. Section 15 of the spec says so in as many words.
///
/// THE HOUSE RULE IS THAT THIS APP NEVER INVENTS A NUMBER, and a strip is exactly where an
/// invented one does the most damage: it sits at the top of the page in 21 point type with a
/// label over it, next to four figures that ARE real, and it borrows their credibility. Shipping
/// `Medium 68%` would be this app making something up on the owner's stream.
///
/// THE OTHER FIVE ARE ALL REAL. Encounter and Raid DPS are the fold's own. Scope is what the store
/// holds. Deaths are `Mark::Death`.
///
/// AND TARGET HEALTH IS THE ONE THAT HAD TO EARN ITS PLACE TWICE. It is the median of finished
/// kills and refuses to answer until enough of them agree (`hp::Reading`), which was the first
/// half; the second half is that the mock asked for this figure as a PERCENTAGE, and a percentage
/// is a shape that cannot say "I do not know" -- it prints a number for every input, including the
/// inputs a subtraction has no answer for. This strip shipped `0.0%` in red for a mob nobody had
/// scratched because of exactly that. The refusals now live in `strip_cells`, argued where the
/// arithmetic is, and the cell falls back to figures rather than to a floored percentage.
/// THE SUMMARY STRIP, section 5 of the mock, over the SCOPE and not a fight.
fn summary_strip(ui: &mut Ui, filter: &Filter, fold: Option<&FightRow>, fights: &[FightRow]) {
    let cells = strip_cells(filter, fold, fights);
    /* DEFECT: THE FIVE CELLS STAIRCASED DOWN THE PAGE.
     *
     * Measured on a headless frame: the labels sat at y = 120, 149.5, 164.2, 171.6, 175.3, each
     * lower than the last by half the previous step. That is `ui.horizontal` (cross axis
     * `Align::Center`) placing each new child at the centre of a row whose height it then grows,
     * because every cell was allocated at height ZERO and only asked for its height afterwards.
     * egui does not go back and re-centre what it has already placed.
     *
     * SO THE ROW IS TOP ALIGNED AND EVERY CELL IS ALLOCATED AT ITS FULL HEIGHT UP FRONT, and so
     * is every divider. There is nothing left for the row to grow by. `STRIP_H` is one number for
     * all three so they cannot drift apart again. */
    const STRIP_H: f32 = 62.0;
    crate::theme::panel(ui, |ui| {
        let full = ui.available_width();
        let n = cells.len().max(1) as f32;
        let w = full / n;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, c) in cells.iter().enumerate() {
                ui.allocate_ui_with_layout(
                    Vec2::new(w, STRIP_H),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(w);
                        ui.set_min_height(STRIP_H);
                        ui.set_max_height(STRIP_H);
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(15, 10))
                            .show(ui, |ui| {
                                let r = ui
                                    .scope(|ui| {
                                        crate::theme::stat(
                                            ui,
                                            c.label,
                                            &c.value,
                                            c.small.as_deref(),
                                            c.tint,
                                        );
                                    })
                                    .response;
                                r.on_hover_text(c.why);
                            });
                    },
                );
                if i + 1 < cells.len() {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, STRIP_H), Sense::hover());
                    ui.painter()
                        .rect_filled(rect.shrink2(Vec2::new(0.0, 2.0)), 0.0, LINE_SOFT);
                }
            }
        });
    });
}

/// ONE CELL OF THE STRIP, AS DATA, so what the strip says is a pure function of the scope.
struct Stat {
    label: &'static str,
    value: String,
    small: Option<String>,
    tint: egui::Color32,
    why: &'static str,
}

/// THE FIVE FIGURES OVER THE SCOPE. Five and not the mock's six: `Group pressure` is not in the
/// log and `Target health` is a live figure with no meaning over a night.
fn strip_cells(filter: &Filter, fold: Option<&FightRow>, fights: &[FightRow]) -> Vec<Stat> {
    let mut out: Vec<Stat> = Vec::new();
    let n = fights.len();
    out.push(Stat {
        label: "Scope",
        value: filter.short(),
        small: Some(format!("{n} fight{}", if n == 1 { "" } else { "s" })),
        tint: TEXT,
        why: "What this page is looking back at. Every figure on it is over these fights and no \
              other; the Filter button on the header changes it.",
    });
    let (rate, total) = match fold {
        Some(f) => {
            let ranked = crate::screens::dps::ranked_dealers(f, Metric::Dealt);
            let ours: u64 = ranked.iter().map(|x| x.dealt).sum();
            (crate::screens::dps::dps(ours, f.secs), ours)
        }
        None => (None, 0),
    };
    out.push(Stat {
        label: "Group DPS",
        value: match rate {
            Some(r) => crate::screens::dps::thousands(r),
            None => crate::screens::dps::thousands(total),
        },
        small: Some(String::from(if rate.is_some() { "dps" } else { "dmg" })),
        tint: GOLD_HI,
        /* THE POPULATION IN THE WORDS, because the figure is the fold's roster and that is every
         * player on one scope and the reader's own people on the next. See `strip_population`. */
        why: match strip_population(fold) {
            Population::Everyone => {
                "Everything the players in these fights dealt, over the seconds they were in \
                 combat. Time between fights is not in the divisor."
            }
            Population::Solo => {
                "Everything you and your pets dealt, over the seconds you were in combat. Every \
                 fight in scope proved you solo, so a player fighting near you is not in it. Time \
                 between fights is not in the divisor."
            }
            Population::Group => {
                "Everything you, your group and your pets dealt, each fight counting who was in \
                 your group in that fight, over the seconds you were in combat. A player near you \
                 outside the group is not in it. Time between fights is not in the divisor."
            }
        },
    });
    out.push(Stat {
        label: "In combat",
        value: fold.map_or_else(|| String::from("--:--"), |f| long_clock(f.secs)),
        small: None,
        tint: TEXT,
        why: "The sum of every fight's own clock, first line to last. Looting, travel and chat \
              between fights are not counted.",
    });
    let longest = fights.iter().map(|f| f.secs).max().unwrap_or(0);
    out.push(Stat {
        label: "Fights",
        value: n.to_string(),
        small: (n > 0).then(|| format!("longest {}", clock(longest))),
        tint: TEXT,
        why: "Finished fights in the scope, off this app's own store. A fight is a run of combat \
              with no quiet gap in it, which is a rule this app applies to a clock.",
    });
    /* THE READER'S OWN DEATHS, and not the scope's roster's: see `your_deaths`. */
    let mut dead = 0usize;
    let mut last_by: Option<&str> = None;
    for f in fights {
        for m in &f.moments {
            if let Some(by) = death_by(f, m) {
                dead += 1;
                last_by = by;
            }
        }
    }
    out.push(Stat {
        label: "Deaths",
        value: dead.to_string(),
        small: last_by.map(|k| format!("by {k}")),
        tint: if dead == 0 { TEXT } else { WRONG },
        why: "Your own deaths, across every fight in the scope. Your pets, your group and anyone \
              else who died near you are not counted: this dashboard is yours. The name beside it \
              is what killed you last.",
    });
    out
}

/// WHOSE FIGURES THE STRIP'S GROUP DPS AND DEATHS CELLS ARE, off the same fold the roster reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Population {
    /// The scope's group is not known, or there is no fight to know it of: every player.
    Everyone,
    /// Every fight in scope proved the reader solo.
    Solo,
    /// Every fight in scope proved a group, and some fight had somebody in it.
    Group,
}

/// [`Population`] OF A SCOPE'S FOLD. `reports::roll` sets the fold's group to `rolled_group`, which
/// is what `reports::scope_roster` switches on, so this is the same question every cell of the
/// strip is counted by. With no fold there are no figures and the words fall back to everyone.
fn strip_population(fold: Option<&FightRow>) -> Population {
    match fold.and_then(|f| f.group.as_deref()) {
        None => Population::Everyone,
        Some([]) => Population::Solo,
        Some(_) => Population::Group,
    }
}

/// THE ROSTER: the mock's `.performance-panel` body, and the heart of the design.
///
/// # EVERY FIGURE HERE COMES OFF `screens::dps` AND NONE IS COMPUTED IN THIS FILE
///
/// `ranked_dealers` picks and orders the rows, `dps` turns a total into a rate behind its own
/// three second floor, `share` divides, `thousands` formats. That is not tidiness. The overlay
/// window and this page can be on screen at the same time showing the same fight, and two
/// implementations of one figure is how they come to disagree in front of an audience.
///
/// THE COLUMNS ARE THE MOCK'S: `34px | 1.5fr | 116px | 88px | 78px`, gap `12px`, and the header
/// row uses the same template so the words stay over their columns.
///
/// WHAT EACH ROW CARRIES, and every one of these was missing before: a rank badge tinted with the
/// row's own colour, the name with a `YOU` pill on the reader's row, the class trio as runed
/// chips, a relative bar against the TOP dealer rather than the total, the rate with its unit,
/// and the share of what the group did.
/// THE GAP BETWEEN ROSTER COLUMNS. Every other width the roster used to hold as a constant is
/// worked out per card now: see `roster_plan`.
const ROSTER_GAP: f32 = 10.0;

/// THE NARROWEST THE NAME COLUMN MAY BE, whatever the measurement says. A name elided to three
/// characters is not a name.
const NAME_MIN: f32 = 90.0;

/// THE NARROWEST SHARE COLUMN THAT CAN HOLD THE WORD `SHARE` rather than the sign.
/// THE NARROWEST NAME COLUMN THAT CAN HOLD THE WHOLE HEADING rather than just `COMBATANT`.
const NAME_HEAD_FULL: f32 = 190.0;

const SHARE_WORD: f32 = 46.0;

/// THE NARROWEST A RELATIVE BAR IS WORTH DRAWING. Under this it is a coloured pip that says
/// nothing about proportion, and the card is better off giving the width to the name.
const BAR_MIN: f32 = 40.0;
/// THE ROSTER'S HORIZONTAL MARGIN, a side. The column head and the rows share it, or the
/// headings drift off the columns they name: see `roster`.
const ROSTER_PAD: f32 = 10.0;

/// Both margins together, which is what `roster_plan` has to budget around.
const ROSTER_INSET: f32 = ROSTER_PAD * 2.0;

/// HOW BIG THE CONTENTS OF A ROSTER ARE, WORKED OUT FROM THE CARD AND WHAT IS IN IT.
///
/// # THE OWNER'S THREE COMPLAINTS ARE ONE ARITHMETIC
///
/// He said the name column had far too much blank space in it, that the vertical spacing made
/// no sense, and that four players and eight should both fit without him resizing anything, and
/// that a bigger window should mean bigger text. Those are the same sum done three ways: the
/// old plan gave the name a FIXED floor and the bar whatever was left, and drew every row at one
/// fixed height whatever the card was.
///
/// SO NOTHING HERE IS FIXED. The name column is MEASURED against the names actually in this
/// roster, the row height is the card's own height divided by the rows that are going in it, and
/// every type size and the bar's thickness come off that row height. Four people in a tall card
/// get tall rows and big figures; eight in the same card get tight ones; a wider card grows both.
///
/// THE CAPS ARE WHAT STOP IT LOOKING SILLY, and they are the only numbers here that are chosen
/// rather than derived: one person in a full-height card must not get a 200 point row.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RosterPlan {
    /// Rows that will actually be drawn.
    shown: usize,
    /// How tall one row is, head to foot.
    row_h: f32,
    /// The name column, measured against the names in it.
    name: f32,
    /// The bar column, or `None` when the card is too narrow to earn one.
    bar: Option<f32>,
    /// The share column, or `None` when the card is too narrow for that too.
    share: Option<f32>,
    /// The figure column.
    num: f32,
    /// The rank badge.
    rank: f32,
    /// Type sizes, all derived from [`RosterPlan::row_h`].
    name_pt: f32,
    num_pt: f32,
    chip_pt: f32,
    head_pt: f32,
    /// How thick the relative bar is.
    bar_h: f32,
}

impl RosterPlan {
    /// EVERYTHING THE ROW SPENDS, gaps included, for a check against the width it was planned
    /// for. See [`tests::the_roster_never_overflows_its_tile`].
    #[cfg(test)]
    fn spent(&self) -> f32 {
        let mut cols = 3.0;
        let mut w = self.rank + self.name + self.num;
        if let Some(b) = self.bar {
            w += b;
            cols += 1.0;
        }
        if let Some(s) = self.share {
            w += s;
            cols += 1.0;
        }
        w + ROSTER_GAP * (cols - 1.0)
    }
}

/// THE FRACTION OF A ROW'S HEIGHT EACH PIECE OF TYPE TAKES.
///
/// Chosen so that at [`ROW_MIN`] they land on the sizes the mock specifies, and grow from there.
const PT_NAME: f32 = 0.30;
const PT_NUM: f32 = 0.32;
const PT_CHIP: f32 = 0.23;
const PT_HEAD: f32 = 0.23;
const PT_BAR: f32 = 0.28;

/// THE SHORTEST AND TALLEST ONE ROSTER ROW MAY BE, in points.
///
/// THE FLOOR IS A NAME OVER A TRIO OF CHIPS, which is what a row of this card IS. The ceiling
/// stops a card holding one person from drawing him at the size of a headline.
const ROW_MIN: f32 = 42.0;
const ROW_MAX: f32 = 78.0;

/// THE MOST OF A ROSTER'S WIDTH THE NAME COLUMN MAY TAKE.
///
/// The measurement below is what usually decides it; this is the guard against one absurd name
/// (the log has met `Xicotl the Everburning`) squeezing the bar out of every row.
const NAME_SHARE: f32 = 0.42;

/// WORK OUT A ROSTER'S SIZES FROM THE CARD IT IS IN AND THE PEOPLE IN IT.
///
/// `avail` is the height under the column head. `wide` is the full width of the body. `names` is
/// what each row will have to fit, already laid out at the size it will be drawn at.
fn roster_plan(wide: f32, avail: f32, want: usize, widest: f32) -> RosterPlan {
    /* AS MANY ROWS AS FIT AT THE FLOOR, AND THEN AS TALL AS THE ROOM ALLOWS.
     *
     * Dropping a row makes every remaining row TALLER, so this walks down until they fit and
     * then spends what is left on height. Eight people in a card that holds six get six tight
     * rows and a `+2 more`; four in the same card get four tall ones. */
    let want = want.max(1);
    let mut shown = want;
    while shown > 1 && (avail / shown as f32) < ROW_MIN {
        shown -= 1;
    }
    let row_h = (avail / shown as f32).clamp(ROW_MIN, ROW_MAX);

    let name_pt = row_h * PT_NAME;
    let num_pt = row_h * PT_NUM;
    let chip_pt = row_h * PT_CHIP;
    let head_pt = (row_h * PT_HEAD).clamp(9.0, 13.0);
    let bar_h = row_h * PT_BAR;

    /* THE COLUMNS THAT HOLD FIGURES GROW WITH THE FIGURES, or a bigger card would draw bigger
     * numbers into the same narrow slot and clip them. */
    let rank = (row_h * 0.42).clamp(16.0, 30.0);
    let num = (num_pt * 4.2).clamp(46.0, 110.0);
    let share = (num_pt * 3.0).clamp(34.0, 78.0);

    let inner = wide - ROSTER_INSET;
    /* THE NAME IS AS WIDE AS THE WIDEST NAME IN IT, capped. This is the blank space the owner
     * was looking at: the old floor was 220 points whether the longest name was `Omny` or not. */
    let name_cap = (inner * NAME_SHARE).max(NAME_MIN);
    let name = widest.clamp(NAME_MIN.min(name_cap), name_cap);

    /* EVERYTHING: the bar takes what is left after the measured name. */
    let spare = inner - rank - name - num - share - ROSTER_GAP * 4.0;
    if spare >= BAR_MIN {
        return RosterPlan {
            shown,
            row_h,
            name,
            bar: Some(spare),
            share: Some(share),
            num,
            rank,
            name_pt,
            num_pt,
            chip_pt,
            head_pt,
            bar_h,
        };
    }
    /* WITHOUT THE BAR: the name takes the slack back. */
    let spare = inner - rank - num - share - ROSTER_GAP * 3.0;
    if spare >= NAME_MIN {
        return RosterPlan {
            shown,
            row_h,
            name: spare,
            bar: None,
            share: Some(share),
            num,
            rank,
            name_pt,
            num_pt,
            chip_pt,
            head_pt,
            bar_h,
        };
    }
    /* WITHOUT THE SHARE EITHER: rank, name, figure. */
    RosterPlan {
        shown,
        row_h,
        name: (inner - rank - num - ROSTER_GAP * 2.0).max(48.0),
        bar: None,
        share: None,
        num,
        rank,
        name_pt,
        num_pt,
        chip_pt,
        head_pt,
        bar_h,
    }
}

/// THE TRIO THIS FIGHTER IS KNOWN TO BE, and where that knowledge came from.
///
/// # THE READER'S OWN CLASSES ARE A SETTING AND EVERYBODY ELSE'S ARE AN INFERENCE
///
/// `ingest::read_casts` skips `Actor::You` on purpose: your trio is STATED under MY LEGEND /
/// Gear and inferring it as well would put a guess beside a fact and let the two disagree. The
/// cost was that your own row was the only one on the roster with no classes on it, which is
/// what the owner was looking at when he asked to see his classes and not just the word YOU.
///
/// SO THE ROW READS THE SETTING FOR HIM AND THE LOG FOR EVERYONE ELSE, and neither is invented.
///
/// NOTHING UNLESS HE HAS ACTUALLY CHOSEN ONE. `CharState::read` falls back to a default trio
/// (`WAR CLR WIZ`) for the Gear screens' arithmetic, and drawing THAT beside his name would be
/// this app telling him what he plays. `CharState::stored` is the difference.
fn trio_of(cx: &Cx, who: &crate::fights::Fighter) -> Vec<String> {
    if matches!(who.who, crate::fights::Who::You) {
        if !crate::screens::gear::CharState::stored(cx.settings) {
            return Vec::new();
        }
        return crate::screens::gear::CharState::read(cx.settings)
            .classes
            .iter()
            .map(|c| crate::screens::gear::class_name(c).to_owned())
            .collect();
    }
    let Some(data) = cx.data else {
        return Vec::new();
    };
    cx.ingest
        .classes()
        .of(who.who.text(), &data.spells)
        .map(|seen| seen.certain.iter().take(3).cloned().collect())
        .unwrap_or_default()
}

/// THE COLOUR THIS ROW IS DRAWN IN, AND THE REASON IT IS THAT COLOUR.
///
/// # RANK WAS A REASON AND IT WAS NOT A VISIBLE ONE
///
/// The bars were `theme::row_colour(i)`: gold, red, green, blue, then one grey for everybody
/// else. That is a real rule and no reader has ever guessed it, which is what the owner meant by
/// asking for some rhyme or reason. It is also the wrong rule, and `class::colour`'s own doc has
/// said so since it was written: rank MOVES, so a person you have learned to find by colour
/// changes colour by overtaking somebody.
///
/// THE CLASS IS THE REASON. `class::colour` gives every class its own hue and groups them by
/// archetype, melee warm, priests gold and green, casters cool, so the colours mean something
/// before the key is learned, and the trio chips under the name say which is which.
///
/// UNKNOWN IS ITS OWN COLOUR AND NOT A GUESS. A fighter nobody has seen cast anything gets the
/// neutral, which is honest: this app does not know what he is.
fn row_colour(trio: &[String]) -> egui::Color32 {
    trio.first()
        .and_then(|c| crate::class::colour(c))
        .unwrap_or(UNKNOWN_CLASS)
}

/// The bar and badge colour for a fighter whose class nothing has proved.
const UNKNOWN_CLASS: egui::Color32 = egui::Color32::from_rgb(0x5A, 0x63, 0x72);

fn roster(ui: &mut Ui, cx: &mut Cx, tile: Tile, f: &FightRow, metric: Metric, foot: bool) {
    let ranked = crate::screens::dps::ranked_dealers(f, metric);
    if ranked.is_empty() {
        crate::theme::panel_body(ui, |ui| {
            ui.label(RichText::new(crate::screens::dps::nobody(f, metric)).color(TEXT_3));
        });
        return;
    }
    /* THE TOP DEALER SETS THE FULL BAR AND THE GROUP'S SUM SETS THE SHARE, which is what
     * `dps::ranked_table` does and for the reasons written there. */
    let top = ranked.first().map_or(1, |x| metric.of(x)).max(1);
    let ours: u64 = ranked.iter().map(|x| metric.of(x)).sum();
    let rate_unit = metric.unit(crate::screens::dps::dps(1, f.secs).is_some());

    /* WHAT EACH ROW WILL HAVE TO SAY, READ BEFORE ANYTHING IS SIZED.
     *
     * The trio is wanted twice: once to measure the name column against and once to colour the
     * row, and it costs a spell-corpus lookup, so it is done once here. */
    let people: Vec<(&&crate::fights::Fighter, Vec<String>)> = ranked
        .iter()
        .take(GROUP_CAP)
        .map(|x| (x, trio_of(cx, x)))
        .collect();

    let wide = ui.available_width();
    /* THE HEIGHT THE ROWS HAVE: what is left under the column head and over the foot, both of
     * which this function draws itself. */
    let avail = (ui.available_height() - HEAD_BAND - FOOT_BAND).max(ROW_MIN);

    /* MEASURED AT A REFERENCE SIZE AND SCALED, because the size depends on the plan and the plan
     * depends on the measurement. One pass at a known size and one multiply is exact enough for
     * a column width and cannot loop. */
    let widest = {
        let probe = |s: &str| {
            ui.painter()
                .layout_no_wrap(s.to_owned(), egui::FontId::proportional(NAME_PROBE), TEXT)
                .size()
                .x
        };
        let mut w: f32 = 0.0;
        for (x, trio) in &people {
            let mut row = probe(x.who.text());
            if matches!(x.who, crate::fights::Who::You) {
                row += YOU_PILL;
            }
            /* THE CHIPS SIT UNDER THE NAME, so the column has to hold whichever line is wider. */
            let chips: f32 = trio
                .iter()
                .map(|c| probe(c) * CHIP_OF_NAME + CHIP_PAD)
                .sum();
            w = w.max(row.max(chips));
        }
        w + NAME_SLACK
    };

    let plan = roster_plan(wide, avail, people.len(), widest);
    let gap = ROSTER_GAP;

    /* THE COLUMN HEAD, the mock's `.roster-col-head`: muted uppercase with a `--line-soft` rule
     * under it. Its type follows the rows, so a big card does not put nine point headings over
     * twenty point figures. */
    /* A HEADING IS PAINTED INTO ITS COLUMN AND CANNOT LEAVE IT.
     *
     * DEFECT: THE HEADINGS DRIFTED RIGHT AND THE LAST ONE FELL OFF THE CARD. These were
     * `ui.label`s inside an allocation of the column's width, and a label WIDER than its
     * allocation pushes the next widget along instead of being cut. The name column is measured
     * against the names in it now, so on a roster of `You` and `Omny` it is about 145 points and
     * `COMBATANT \u{b7} CLASS TRIO` is 200: every heading after it sat 55 points right of the
     * column it named, and `SHARE` ran off the edge.
     *
     * SO THE CELL IS ALLOCATED AND THE WORD IS PAINTED INTO IT, clipped. A heading that does not
     * fit is cut off rather than moving something else, which is the behaviour every other
     * column on this page already has.
     */
    let head = |ui: &mut Ui, text: &str, w: f32, right: bool| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, plan.head_pt + 4.0), Sense::hover());
        ui.painter().with_clip_rect(rect).text(
            if right {
                rect.right_center()
            } else {
                rect.left_center()
            },
            if right {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            },
            text.to_uppercase(),
            egui::FontId::proportional(plan.head_pt),
            TEXT_2,
        );
    };
    /* THE HEAD SITS ON THE ROWS' OWN MARGIN, AND IT DID NOT.
     *
     * DEFECT: THE LAST COLUMN HEAD WAS CUT OFF. This frame had 14 points a side and the rows
     * below it 10, so every heading started four points right of the column it named and the
     * last one ran four points past the card and lost its second half: `SHARE` came out `SI`.
     * `ROSTER_INSET` is what the plan budgets, so all three have to be the same number. */
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(ROSTER_PAD as i8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                head(ui, "#", plan.rank, false);
                /* THE LONG HEADING WHEN THERE IS ROOM FOR IT. Clipping keeps it honest either
                 * way; this keeps it readable, since `COMBATAN` is not a word. */
                head(
                    ui,
                    if plan.name >= NAME_HEAD_FULL {
                        "Combatant \u{b7} class trio"
                    } else {
                        "Combatant"
                    },
                    plan.name,
                    false,
                );
                if let Some(w) = plan.bar {
                    head(ui, "Relative", w, false);
                }
                head(ui, rate_unit, plan.num, true);
                if let Some(w) = plan.share {
                    /* THE WORD WHEN IT FITS AND THE SIGN WHEN IT DOES NOT. On a span-4 card the
                     * share column is about thirty points and `SHARE` came out as `SHA`, which
                     * is a heading that has to be guessed at. The rows under it all end in a
                     * percent sign, so the sign is not a shorter word: it is the same word. */
                    head(ui, if w >= SHARE_WORD { "Share" } else { "%" }, w, true);
                }
            });
        });
    let rule = ui.available_rect_before_wrap();
    ui.painter().hline(
        rule.x_range(),
        rule.top(),
        egui::Stroke::new(1.0, LINE_SOFT),
    );

    let drawn = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(ROSTER_PAD as i8, 4))
        .show(ui, |ui| {
            /* NO ITEM SPACING: the row's own height is the spacing, which is what makes four
             * rows and eight both look deliberate rather than one being the other with gaps. */
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut n = 0usize;
            for (i, (x, trio)) in people.iter().enumerate().take(plan.shown) {
                /* THE PLAN'S BUDGET, NOT `room_for`'S, AND THE DIFFERENCE IS A WHOLE ROW.
                 *
                 * DEFECT: THE ROSTER SAID `+2 MORE COMBATANTS` WITH ROOM FOR BOTH ON SCREEN.
                 * `roster_plan` takes `MORE_H` off the height ONCE, up front, and then divides
                 * what is left between the rows it decided to draw. `room_for` reserves `MORE_H`
                 * as well, on EVERY row, so the last row or two of a plan that fitted exactly
                 * were refused and reported as missing. Two answers to `is there room` and the
                 * stricter one was wrong.
                 *
                 * SO THE LOOP ASKS FOR THE ROW AND NOTHING MORE. `plan.shown` is still the
                 * ceiling and this is the belt: a body drawn into less height than the plan was
                 * given (a card resized mid-frame) still stops rather than overflowing.
                 */
                if ui.available_height() < plan.row_h {
                    break;
                }
                n += 1;
                let colour = row_colour(trio);
                let value = metric.of(x);
                let mine = matches!(x.who, crate::fights::Who::You);
                let shown = crate::screens::dps::dps(value, f.secs)
                    .map(crate::screens::dps::thousands)
                    .unwrap_or_else(|| crate::screens::dps::thousands(value));
                let pct = crate::screens::dps::share(value, ours);

                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), plan.row_h),
                    Sense::hover(),
                );
                /* THE READER'S ROW IS LIT, and it is drawn into the row's own rect rather than
                 * as a frame around it, so the highlight is exactly one row tall whatever the
                 * row height works out to. */
                if mine {
                    ui.painter().rect_filled(
                        rect.shrink2(Vec2::new(1.0, 1.0)),
                        6.0,
                        egui::Color32::from_rgba_unmultiplied(0xF2, 0xB9, 0x4F, 16),
                    );
                    ui.painter().rect_stroke(
                        rect.shrink2(Vec2::new(1.0, 1.0)),
                        6.0,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(0xF2, 0xB9, 0x4F, 84),
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        /* SHRUNK ONLY IN Y. Taking four points off each side made every row
                         * eight points narrower than the column head above it, so the headings
                         * drifted right of their columns and the last one fell off the card. */
                        .max_rect(rect.shrink2(Vec2::new(0.0, 3.0)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let ui = &mut row;
                ui.spacing_mut().item_spacing.x = gap;
                crate::theme::rank_badge_at(ui, i + 1, colour, plan.rank, plan.chip_pt);
                /* A NAME WITH NOTHING UNDER IT IS CENTRED, and one with chips under it stacks
                 * from the top. Laid out top down either way, a fighter nobody has placed sat
                 * hard against the top of a row built to hold two lines. */
                let stacked = if trio.is_empty() && !mine {
                    Layout::left_to_right(Align::Center)
                } else {
                    Layout::top_down(Align::Min)
                };
                ui.allocate_ui_with_layout(Vec2::new(plan.name, plan.row_h - 6.0), stacked, |ui| {
                    ui.set_width(plan.name);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new(x.who.text())
                                .font(egui::FontId::proportional(plan.name_pt))
                                .strong()
                                .color(TEXT),
                        );
                        if mine {
                            crate::theme::you_pill(ui);
                        }
                    });
                    /* AND IF HE HAS NEVER SAID WHAT HE PLAYS, THE ROW SAYS SO RATHER THAN
                     * NOTHING. His trio is a setting and not an inference (`trio_of`), so
                     * an empty row here is not `unknown`, it is `unanswered`, and those
                     * read identically as blank space. */
                    if mine && trio.is_empty() {
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new("set your trio")
                                .size(plan.chip_pt)
                                .color(TEXT_3),
                        )
                        .on_hover_text(
                            "Your own classes are read from MY LEGEND \u{203a} Gear, where you \
                                 choose them, and not guessed from the log the way everybody \
                                 else's are. Choose them there and they appear here.",
                        );
                    }
                    trio_chips(ui, trio, plan.chip_pt);
                });
                if let Some(w) = plan.bar {
                    crate::theme::bar_at(ui, w, plan.bar_h, value as f32 / top as f32, colour);
                }
                ui.allocate_ui_with_layout(
                    Vec2::new(plan.num, plan.row_h - 6.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.set_width(plan.num);
                        ui.label(
                            RichText::new(shown)
                                .font(egui::FontId::proportional(plan.num_pt))
                                .strong()
                                .color(TEXT),
                        );
                    },
                );
                if let Some(w) = plan.share {
                    ui.allocate_ui_with_layout(
                        Vec2::new(w, plan.row_h - 6.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_width(w);
                            ui.label(
                                RichText::new(format!("{pct}%"))
                                    .font(egui::FontId::proportional(plan.chip_pt + 2.0))
                                    .color(egui::Color32::from_rgb(0xC0, 0xC7, 0xD1)),
                            );
                        },
                    );
                }
            }
            n
        })
        .inner;
    /* THE ROSTER DRAWS ITS OWN FOOT NOW, and it has to: the foot says how many were left out,
     * and how many were left out is decided HERE, by how many rows the plan fitted. The card
     * above knew the total and not the drawn count, so the two facts could not meet. */
    let _ = foot;
    roster_foot(
        ui,
        cx,
        tile,
        ranked.len(),
        ranked.len().saturating_sub(drawn),
        ours,
    );
}

/// The size names are measured at before being scaled to the plan's own.
const NAME_PROBE: f32 = 12.5;

/// How wide the `YOU` pill is, for the measurement.
const YOU_PILL: f32 = 34.0;

/// A class chip's type as a fraction of a name's, and the padding around one.
const CHIP_OF_NAME: f32 = 0.78;
const CHIP_PAD: f32 = 16.0;

/// Breathing room past the widest thing measured into the name column.
const NAME_SLACK: f32 = 10.0;

/// The column head band: its type plus its rule and margins.
const HEAD_BAND: f32 = 26.0;

/// The foot band under the rows: its rule, its margins and the line between them.
const FOOT_BAND: f32 = 42.0;

/// THE ROSTER FOOT, the mock's `.roster-foot`: how many named, and the group total.
///
/// DRAWN BY THE CARD AND NOT BY THE ROSTER, pinned under the scroll area, so it is on screen
/// however many rows there are. Inside the flow it scrolled off the bottom of a tile the moment
/// the roster had more rows than the tile had height, which on the reference capture is always.
/// THE ROSTER FOOT: how many are named and the group total.
///
/// IT STATES THE WHOLE POPULATION and the list above it says how much of that fitted, because
/// the card pins this foot before the rows are drawn and cannot know what fitted. See the
/// `more_line` at the end of `roster`.
fn roster_foot(ui: &mut Ui, cx: &mut Cx, tile: Tile, named: usize, hidden: usize, ours: u64) {
    ui.painter().hline(
        ui.available_rect_before_wrap().x_range(),
        ui.available_rect_before_wrap().top(),
        egui::Stroke::new(1.0, LINE_SOFT),
    );
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(15, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                /* WHAT WAS LEFT OUT IF ANYTHING WAS, AND HOW MANY THERE ARE IF NOT.
                 *
                 * These were two lines: a gold `+3 more combatants` under the last row and a
                 * `7 named` in the foot under that, with the card's blank space between them.
                 * The owner asked for one, and he is right that it is one fact: the foot is
                 * where a roster says how much of itself you are looking at.
                 *
                 * THE CONTROL SURVIVES THE MERGE. `more_line` is not a caption, it opens the
                 * page holding the rest, and it still does from here. */
                if hidden > 0 {
                    more_line(ui, cx, tile, hidden, "combatants");
                } else {
                    ui.label(
                        RichText::new(format!("{named} named"))
                            .size(11.0)
                            .color(TEXT_2),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(crate::screens::dps::thousands(ours))
                            .size(12.5)
                            .strong()
                            .color(TEXT),
                    );
                    ui.label(RichText::new("Group total").size(11.0).color(TEXT_2));
                });
            });
        });
}

/// THE CLASS TRIO UNDER A ROSTER NAME: the mock's `.trio` of runed chips.
///
/// # AN EQL CHARACTER IS THREE CLASSES AT ONCE AND THAT IS WHY THIS IS A ROW OF CHIPS
///
/// Every other parser in this genre prints ONE class per name. In EverQuest Legends a character
/// is a trio, so one class is two thirds of a lie, and the mock's answer is three small chips
/// with a coloured rune each.
///
/// # IT DRAWS NOTHING RATHER THAN GUESSING
///
/// A class is inferred from what somebody was seen CASTING, against the spell corpus. Two things
/// can be absent: the corpus (`Cx::data` is `None` until the snapshot loads, and on a machine
/// with no snapshot it never does) and the casts (a melee who cast nothing this fight). In both
/// cases this draws no chips at all, because a blank is honest and a guessed trio is somebody
/// else's class in the owner's row on his own stream.
///
/// COVERING AND NOT INTERSECTION, which is `class::read`'s own rule: a character casting a druid
/// spell and a shaman spell is a druid AND a shaman, not neither.
/// THE TRIO ALREADY READ, DRAWN AS CHIPS AT THE SIZE THE ROW IS BEING DRAWN AT.
///
/// TAKES THE TRIO RATHER THAN LOOKING IT UP, since `roster` needs the same answer to measure
/// its name column with and to colour the row by: see `trio_of`. Two lookups could not
/// disagree today, but they are two places to keep in step with where a trio comes from, and
/// the reader's own comes from somewhere else entirely.
fn trio_chips(ui: &mut Ui, trio: &[String], pt: f32) {
    if trio.is_empty() {
        return;
    }
    /* THE CHIPS ARE BOUNDED BY THE COLUMN THEY ARE DRAWN IN.
     *
     * DEFECT: THEY PUSHED THE RATE AND SHARE COLUMNS OFF THE TILE. `roster_plan` budgets the
     * name column for a name, and these are drawn under it; three proven classes are wider than
     * a span-4 tile's name column, `ui.horizontal` does not wrap, and the row overflowed exactly
     * as it did before the plan existed. So each chip is measured before it is drawn and the
     * row stops when the next one will not fit, with the count of what was dropped on the hover
     * rather than silently. */
    let room = ui.available_width();
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut used = 0.0f32;
        let mut shown = 0usize;
        for c in trio.iter().take(3) {
            /* `class_chip`'s own arithmetic, asked before it draws: the word plus its padding. */
            let w = ui
                .painter()
                .layout_no_wrap(c.to_owned(), egui::FontId::proportional(pt), TEXT_2)
                .size()
                .x
                + CHIP_PAD;
            let step = if shown == 0 { w } else { w + 4.0 };
            if shown > 0 && used + step > room {
                break;
            }
            crate::theme::class_chip_at(ui, c, crate::class::colour(c).unwrap_or(GOLD_HI), pt)
                .on_hover_text(format!(
                    "{c}: proven by a spell only that class has, out of what this character was \
                     seen casting in the log."
                ));
            used += step;
            shown += 1;
        }
        let hidden = trio.len().min(3).saturating_sub(shown);
        if hidden > 0 {
            ui.label(RichText::new(format!("+{hidden}")).size(pt).color(TEXT_3))
                .on_hover_text(format!(
                    "{hidden} more class{} than fits this column: {}",
                    if hidden == 1 { "" } else { "es" },
                    trio.join(", ")
                ));
        }
    });
}

/* ------------------------------------------------------------------------ tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{Fighter, Who};

    /// DEFECT: A TILE THAT EXISTS AND NOBODY CAN EVER PUT ON THE PAGE.
    ///
    /// This tree's signature defect is code with no caller, and a dashboard is the exact shape it
    /// takes here: a variant added to [`Tile`] with a body written for it and no entry in
    /// [`Tile::ALL`] is a panel that compiles, is tested, and cannot be reached by any reader.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every tile is in `ALL`, is in [`shipped`], has an id that round trips through [`Tile::of`],
    /// and has an id no other tile shares. The last one is not pedantry: two tiles with one id is a
    /// settings file where removing one removes the other.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping a tile from `ALL`, giving two tiles the same id, or
    /// a `shipped` that ships a subset.
    #[test]
    fn every_tile_is_offered_and_placed() {
        let mut ids: Vec<&str> = Tile::ALL.iter().map(|t| t.id()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "two tiles share an id");

        for t in Tile::ALL {
            assert_eq!(
                Tile::of(t.id()),
                Some(t),
                "{t:?} does not round trip its id"
            );
            assert!(
                shipped().iter().any(|s| s.tile == t.id()),
                "{t:?} is not on a fresh install's dashboard, so nothing ever shows it exists"
            );
            let c = t.place();
            assert!(
                c.placed() && c.col_end() <= u16::from(dashgrid::COLS) + 1,
                "{t:?} has a default cell the grid cannot place: {c:?}"
            );
            assert!(
                c.span >= t.min_span() && c.rows >= t.min_rows(),
                "{t:?} ships smaller than it says it may be made: {c:?}"
            );
            assert!(!t.label().is_empty() && !t.blurb().is_empty(), "{t:?}");
        }
        assert_eq!(
            shipped().len(),
            Tile::ALL.len(),
            "the shipped dashboard is not every tile"
        );
    }

    /// DEFECT: A TILE THAT OPENS A PAGE THAT IS NOT THERE.
    ///
    /// [`Tile::page`] names its section by NAME because the parser's section order has moved once
    /// already. A name resolved against nothing is a card whose `open` control does nothing at all,
    /// and the reader has no way to tell that from a slow frame.
    ///
    /// WHAT MUTATION MAKES THIS RED: renaming a section in `nav::SECTIONS` without renaming it
    /// here, or pointing a tile at a destination that has no such section.
    #[test]
    fn every_tile_opens_a_section_that_exists() {
        for t in Tile::ALL {
            let (id, section) = t.page();
            let Some(name) = section else {
                assert!(
                    crate::nav::sections_of(id).is_empty(),
                    "{t:?} names no section but {id:?} has some, so the card opens whichever one \
                     happened to be selected last"
                );
                continue;
            };
            assert!(
                crate::nav::sections_of(id)
                    .iter()
                    .any(|(n, _, _)| *n == name),
                "{t:?} opens {id:?} / {name}, which is not one of its sections"
            );
        }
    }

    /// DEFECT: A PANEL THAT IS NOT MADE OF THE SHARED VOCABULARY.
    ///
    /// The whole design rests on this page drawing nothing the overlay cannot, through one
    /// renderer. A widget here that the builder does not offer is also a panel a reader can see on
    /// the page and never put in a window, which is this tree's signature defect pointed the other
    /// way.
    ///
    /// THE TWO FLAGS ARE PART OF THE VOCABULARY CHECK AND NOT A SEPARATE TIDINESS RULE. The module
    /// note promises that every ranked table here prints a rate with its unit beside it, which is
    /// what stops a reader taking a total for a per second figure between this page and the overlay
    /// window next to it; and `headline: true` would draw the overlay's big number under the
    /// heading the card prints, which is one panel under two names.
    ///
    /// WHAT MUTATION MAKES THIS RED: a `Widget` arm here that `overlay::Widget::every` does not
    /// offer, a ranked table that asks for a headline, or one that drops its rate.
    #[test]
    fn every_panel_is_built_from_the_shared_vocabulary() {
        let offered = Widget::every();
        let mut n = 0;
        for t in Tile::ALL {
            for w in widgets(t) {
                n += 1;
                assert!(
                    offered
                        .iter()
                        .any(|o| std::mem::discriminant(o) == std::mem::discriminant(&w)),
                    "{t:?} draws a {w:?} the overlay builder does not offer"
                );
                if let Widget::Ranked(r) = &w {
                    assert!(
                        r.rate,
                        "{t:?} ranks by a total where the page promises a rate"
                    );
                    assert!(
                        !r.headline,
                        "{t:?} draws the overlay's big number on a page"
                    );
                }
            }
        }
        assert!(
            n >= 6,
            "only {n} panels were looked at, so this is passing by not looking"
        );
    }

    /// DEFECT: A DEFAULT DASHBOARD WITH TWO TILES ON ONE SQUARE, OR A HOLE NOBODY PUT THERE.
    ///
    /// # A PLACEMENT TABLE IS HAND WRITTEN AND CAN BE TYPED WRONG, AND THERE ARE THREE OF THEM
    ///
    /// [`Tile::place_in`] is three tables of hand written cells, one per [`Size`]. Two of them
    /// overlapping is a tile drawn under another on every fresh install, and `resolve` would
    /// silently shove the second one down, which hides the typo behind a layout nobody designed.
    /// So each arrangement must resolve to exactly what it says: every cell placed, in the grid,
    /// and none moved.
    ///
    /// AND EVERY SQUARE OF EACH IS WALKED, which is the owner's rule for a default: from the top
    /// row to the bottom of the lowest tile, every square is covered by exactly one tile. No hole
    /// in a band, no ragged bottom, no gap between bands. The small and medium tables are the
    /// ones this earns its keep on, because they are not the big one with rows deleted and a
    /// dropped card leaves a hole that nothing else on this page would notice.
    ///
    /// AND THE REPAIRS ARE EXERCISED FROM THIS SIDE TOO: a saved file from the flow layout (spans
    /// only) and one naming a tile from the future both come back as a grid this page can draw.
    /// The arithmetic of the repairs is `dashgrid`'s and is tested there; this is the wire.
    ///
    /// WHAT MUTATION MAKES THIS RED: two cells in a table sharing a square, a cell hanging off
    /// column 12, a tile left out of an arrangement that has room for it, or `layout` no longer
    /// falling back to the arrangement on an empty list.
    #[test]
    fn a_dashboard_of_any_size_fits_the_grid_with_nothing_on_top_of_anything() {
        for size in Size::ALL {
            let placed = layout(None, size);
            assert_eq!(
                placed.len(),
                size.shipped().len(),
                "an empty saved list is not the {} arrangement",
                size.label()
            );
            assert!(
                !placed.is_empty(),
                "the {} arrangement has no cards on it",
                size.label()
            );
            for (_, t, c) in &placed {
                assert_eq!(
                    Some(*c),
                    t.place_in(size),
                    "{t:?} was moved by resolve, so the {} table overlaps or leaves the grid",
                    size.label()
                );
                assert!(
                    c.span >= t.min_span() && c.rows >= t.min_rows(),
                    "{t:?} is placed smaller in the {} arrangement than it says it may be \
                     made: {c:?}",
                    size.label()
                );
            }
            for (i, (_, a, ca)) in placed.iter().enumerate() {
                for (_, b, cb) in placed.iter().skip(i + 1) {
                    assert!(
                        !dashgrid::overlaps(*ca, *cb),
                        "{a:?} and {b:?} ship on top of each other in the {} arrangement: \
                         {ca:?} / {cb:?}",
                        size.label()
                    );
                }
            }

            let bottom = placed
                .iter()
                .map(|(_, _, c)| c.row_end())
                .max()
                .unwrap_or(1);
            assert_eq!(
                bottom.saturating_sub(1),
                size.rows(),
                "the {} arrangement does not know how deep it is, so the window cannot be \
                 measured against it",
                size.label()
            );
            for row in 1..bottom {
                for col in 1..=dashgrid::COLS {
                    let probe = Cell::new(col, 1, row, 1);
                    let covering = placed
                        .iter()
                        .filter(|(_, _, c)| dashgrid::overlaps(*c, probe))
                        .count();
                    assert_eq!(
                        covering,
                        1,
                        "grid square column {col} row {row} of the {} arrangement is covered \
                         by {covering} tiles, so it has a hole or an overlap there",
                        size.label()
                    );
                }
            }
        }

        /* A FILE FROM THE FLOW LAYOUT: spans, nothing else. Everything still lands. */
        let old: Vec<Placement> = [
            ("damage", 2u8),
            ("live", 3),
            ("night", 1),
            ("from-the-future", 2),
        ]
        .iter()
        .map(|(id, span)| Placement {
            tile: (*id).to_owned(),
            col: 0,
            span: *span,
            row: 0,
            rows: 0,
        })
        .collect();
        let got = layout(Some(&old), Size::Large);
        assert_eq!(got.len(), 3, "the unknown tile was not dropped");
        for (_, t, c) in &got {
            assert!(c.placed(), "{t:?} from an old file was left unplaced");
        }
        /* THE SAVED INDEX SKIPS THE UNKNOWN ENTRY: put it first and the drawn tiles are saved
         * 1, 2, 3, which a write back by drawn index would get wrong on every one. */
        let mut unknown_first = old.clone();
        unknown_first.rotate_right(1);
        assert_eq!(
            layout(Some(&unknown_first), Size::Large)
                .iter()
                .map(|(at, _, _)| *at)
                .collect::<Vec<_>>(),
            vec![1, 2, 3],
            "the drawn tiles do not know their saved positions"
        );
        /* DEFECT: TAKING THE LAST CARD OFF PUT ALL THIRTEEN BACK. `Some(empty)` stays empty. */
        assert!(
            layout(Some(&[]), Size::Large).is_empty(),
            "an emptied dashboard came back as the shipped one"
        );
        for (i, (_, a, ca)) in got.iter().enumerate() {
            for (_, b, cb) in got.iter().skip(i + 1) {
                assert!(
                    !dashgrid::overlaps(*ca, *cb),
                    "{a:?} and {b:?} overlap after repair"
                );
            }
        }
    }

    /// DEFECT: A TILE THAT COULD BE RESIZED AND THEN NEVER MOVED AGAIN.
    ///
    /// The head grip was `head.width() - HANDLE_KEEP` wide, floored at zero. A tile narrower
    /// than 110 points (span 2 at the owner's width, every span-3 tile at the window's floor)
    /// had a zero-width grip on its left edge, and egui's hit test never picks a zero-width
    /// rect over the W and NW resize zones that share those pixels. Three reviewers found it.
    ///
    /// WHAT MUTATION MAKES THIS RED: flooring the grip at zero again, or letting it start at the
    /// head's left edge or top edge where the resize zones are.
    #[test]
    fn the_head_grip_never_vanishes_and_never_shares_a_pixel_with_a_resize_zone() {
        for w in [60.0f32, 98.0, 104.67, 116.0, 150.0, 300.0, 900.0] {
            let head = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(w, 40.0));
            let g = grip_rect(head);
            assert!(
                g.width() >= GRIP_MIN - 0.01,
                "at {w} wide the grip is {} wide, which cannot be grabbed",
                g.width()
            );
            assert!(
                g.left() >= head.left() + CORNER_K,
                "the grip reaches into the NW/W zones"
            );
            assert!(
                g.top() >= head.top() + EDGE_T,
                "the grip reaches into the N zone"
            );
            assert!(
                g.right() <= head.right() - CORNER_K + 0.01,
                "the grip reaches into the NE/E zones"
            );
            assert!(g.bottom() <= head.bottom());
        }
        /* AND ON A WIDE HEAD IT STILL LEAVES THE BUTTONS THEIR ROOM. */
        let wide = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 40.0));
        assert!((grip_rect(wide).right() - (600.0 - HANDLE_KEEP)).abs() < 0.01);
    }

    /// DEFECT: THE ROSTER ROW WAS EIGHTY POINTS WIDER THAN ITS TILE.
    ///
    /// WHAT MUTATION MAKES THIS RED: a plan that floors the name and keeps the bar regardless,
    /// or one that drops the rate column.
    #[test]
    fn the_roster_never_overflows_its_tile() {
        /* THE WIDEST NAME AND THE ROOM ARE FIXED HERE so the width ladder is the only thing
         * moving. `plan_at` is the same call the roster makes, with a name wide enough to want
         * more than the cap allows at every width below. */
        let plan_at = |full: f32| roster_plan(full, 300.0, 6, 260.0);
        for full in [180.0f32, 220.0, 260.0, 288.0, 356.0, 420.0, 600.0, 900.0] {
            let p = plan_at(full);
            let inner = full - ROSTER_INSET;
            assert!(
                p.spent() <= inner + 0.01
                    || (p.bar.is_none() && p.share.is_none() && p.name <= 60.01),
                "at {full} wide the roster spends {} of {inner}: {p:?}",
                p.spent()
            );
            assert!(p.name >= 48.0, "the name column vanished at {full}: {p:?}");
        }
        /* THE LADDER, IN ORDER: everything, then no bar, then no share. */
        let wide = plan_at(900.0);
        assert!(wide.bar.is_some() && wide.share.is_some());
        /* THE NAME IS CAPPED AND THE BAR TAKES THE REST, which is the way round the owner asked
         * for: the blank space he was looking at was a name column holding the slack. */
        assert!(
            wide.name <= (900.0 - ROSTER_INSET) * NAME_SHARE + 0.01,
            "the name column took more than its share of a wide card: {wide:?}"
        );
        assert!(
            wide.bar.unwrap_or(0.0) > wide.name,
            "a wide card spent its slack on the name rather than the bar: {wide:?}"
        );
        /* A SPAN-4 TILE AT THE OWNER'S WIDTH IS ABOUT 288 POINTS: rank, name and rate, and
         * nothing else fits. The reviewer's own arithmetic: # + name + rate need 242 of it. */
        let four_cols = plan_at(288.0);
        assert!(
            four_cols.bar.is_none(),
            "a span-4 tile cannot hold the relative bar: {four_cols:?}"
        );
        assert!(
            four_cols.name >= NAME_MIN,
            "the name column went under its floor: {four_cols:?}"
        );
        /* A LITTLE WIDER AND THE SHARE COMES BACK BEFORE THE BAR DOES. */
        let mid = plan_at(320.0);
        assert!(
            mid.bar.is_none() && mid.share.is_some(),
            "the ladder is out of order: {mid:?}"
        );
        let tiny = plan_at(200.0);
        assert!(tiny.bar.is_none() && tiny.share.is_none());
    }

    /// DEFECT: THE ARRANGEMENT WAS NEVER SAVED, AND THE EDIT DESTROYED WHAT IT COULD NOT READ.
    ///
    /// # TWO FAULTS AT ONE PLACE, BOTH INVISIBLE UNTIL THE NEXT LAUNCH
    ///
    /// The write back assigned `cx.settings.dashboard` and stopped: no `Settings::save`, so every
    /// move, resize, removal and picker change was lost on quit. And it REBUILT the list from
    /// `resolve`'s output, which drops every id this build does not know, so the first edit on an
    /// older build deleted a newer build's tiles from the file for good.
    ///
    /// # WHAT IS ASSERTED, AND WHY HALF OF IT IS READ OUT OF THE SOURCE
    ///
    /// The merge is pure and is driven directly. The SAVE cannot be: `Settings::save` writes to
    /// the platform path, which is the owner's real file, and a test must never touch it. So the
    /// save is pinned by reading this module's own source, which is what `main.rs` does for the
    /// poll that has to live in `heartbeat`.
    ///
    /// WHAT MUTATION MAKES THIS RED: rebuilding the list from `resolve` again, dropping the
    /// `save` call, or writing `cx.settings.dashboard` anywhere but `store`.
    #[test]
    fn an_edit_repairs_what_it_knows_and_keeps_what_it_does_not() {
        /* A FILE FROM A NEWER BUILD: one tile this build has, one it does not, and the known one
         * needs repairing (a zero row). */
        let list = vec![
            Placement {
                tile: String::from("threat"),
                col: 1,
                span: 3,
                row: 1,
                rows: 10,
            },
            Placement {
                tile: String::from("damage"),
                col: 0,
                span: 7,
                row: 0,
                rows: 0,
            },
        ];
        let out = merged(list.clone());
        assert_eq!(out.len(), 2, "an entry was dropped from the saved list");
        assert_eq!(
            out[0], list[0],
            "the tile this build cannot read was rewritten or removed; a newer build's dashboard \
             is destroyed by the first drag on this one"
        );
        assert!(
            out[1].cell().placed(),
            "the known tile was not repaired: {:?}",
            out[1]
        );

        /* AND THE SAVE IS WHERE IT HAS TO BE. Read out of the source, because the alternative is
         * a test that writes the owner's settings file. */
        let src = include_str!("dashboards.rs");
        let body = |sig: &str| -> String {
            src.split_once(sig)
                .map(|(_, rest)| rest.split("\n}").next().unwrap_or("").to_owned())
                .unwrap_or_default()
        };
        /* BOTH WRITERS, AND EACH ONE HAS TO SAVE. `store` keeps an arrangement and `forget`
         * drops it; a reset that only took effect until the next launch would be the same
         * defect this test was written for, wearing the opposite sign. */
        for sig in [
            "fn store(cx: &mut Cx, list: Vec<Placement>) {",
            "fn forget(cx: &mut Cx) {",
        ] {
            assert!(
                body(sig).contains("cx.settings.save()"),
                "`{sig}` does not save, so what it did is lost on quit"
            );
        }
        /* AND NOTHING ELSE WRITES THE SETTING, so a third write site cannot forget to save.
         *
         * THE NEEDLE IS BUILT RATHER THAN WRITTEN, because a literal of what it looks for is
         * itself a match: the first version of this counted two, one of them its own message. */
        let needle = format!("{}.dashboard = ", "cx.settings");
        assert_eq!(
            src.matches(needle.as_str()).count(),
            2,
            "the dashboard setting is assigned outside `store` and `forget`, which is how the \
             next edit ships unsaved"
        );
    }

    /// A TABBED CARD SHOWS ONE READING, AND THE TAB IS WHAT CHOOSES IT.
    ///
    /// # WHY THIS CARD IS TABBED AND THE ROSTERS ARE NOT
    ///
    /// `Your damage` stacked three panels in one box: what you used, what you hit, and how your
    /// swings ended. That could not obey the rule the rest of this page obeys, because a fit rule
    /// cannot cut across a stack, and the owner saw the result: a Targets heading clipped at the
    /// bottom edge with nothing saying so.
    ///
    /// TABS WHEN THE READINGS ARE ALTERNATIVES, separate cards when a reader wants them at once.
    /// These three answer one question and nobody needs all three in front of him; Damage,
    /// Healing and Damage taken are different people's contributions and a raid leader wants them
    /// side by side, so those stay three cards. The control is the mock's own `.metric-tab` in
    /// `.panel-actions`.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every tab name is on the card, and the BODY follows the tab: the panel drawn on tab 0 is
    /// not the one drawn on tab 2. A tab row that lights and changes nothing is the defect this
    /// tree has shipped before, in the context bar, and it is what this exists to catch.
    ///
    /// WHAT MUTATION MAKES THIS RED: a body that ignores `tab`, a head that draws no tabs, or
    /// `Tile::tabs` and `widgets` falling out of step.
    #[test]
    fn a_tabbed_card_draws_the_reading_its_tab_is_on() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("tabbed-card");

        /* THE THREE READINGS AND THE PANELS THEY DRAW MUST BE THE SAME LENGTH, or a tab points
         * at nothing. */
        assert_eq!(
            Tile::Yours.tabs().len(),
            widgets(Tile::Yours).len(),
            "the tab row and the readings behind it are different lengths"
        );

        let on = |ing: &mut Ingest, tab: usize| -> Vec<String> {
            let mut screen = DashboardsScreen::default();
            screen.tabs.insert(Tile::Yours.id(), tab);
            painted_tile(&ctx, ing, &mut screen, Tile::Yours)
        };

        let first = on(&mut ing, 0);
        let third = on(&mut ing, 2);
        for name in Tile::Yours.tabs() {
            assert!(
                first.iter().any(|s| s == &name.to_uppercase()),
                "the tab {name} is not on the card: {first:?}"
            );
        }
        /* ABILITIES NAMES SPELLS AND SWINGS; HIT RESULTS NAMES WHAT STOPPED THEM. The capture's
         * reader used `slash` and was `parr`ied, and neither word belongs to the other panel. */
        assert!(
            first.iter().any(|s| s == "slash"),
            "tab 0 is not the abilities reading: {first:?}"
        );
        assert!(
            !first.iter().any(|s| s == "parry"),
            "tab 0 drew the hit results as well, so the card is still a stack: {first:?}"
        );
        assert!(
            third.iter().any(|s| s == "parry"),
            "tab 2 is not the hit results reading: {third:?}"
        );
        assert!(
            !third.iter().any(|s| s == "slash"),
            "tab 2 still drew the abilities, so the body is ignoring its tab: {third:?}"
        );
    }

    /// DEFECT: A CARD SHIPPED SHORTER THAN THE THING IT DRAWS.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// `Written to disk` was cut off at the bottom. Its cell is set in [`Tile::place`] and its
    /// content is drawn by [`night_body`], and the two are in different halves of this file with
    /// nothing between them: a card trimmed to suit its contents, or contents that gain a line,
    /// and the card silently clips. Every test on this page passed while it did.
    ///
    /// # WHY THE OBVIOUS TEST DOES NOT WORK
    ///
    /// Asserting on the STRINGS a tile paints cannot see this. egui emits the shape either way
    /// and the clip rect hides it at paint time, so a clipped card and a whole one produce an
    /// identical list of strings. Neither does drawing it twice at two heights: top aligned
    /// content does not move when the card grows.
    ///
    /// # SO THIS READS THE CARD'S OWN FRAME
    ///
    /// The frame is a real [`egui::Shape::Rect`] in the same pass and the same coordinates as the
    /// text, so its bottom edge is the exact line the content may not cross. No constant is
    /// duplicated from the renderer and no margin is guessed: the assertion is that every glyph
    /// this card paints lands inside the box this card drew for itself.
    ///
    /// EVERY TILE, NOT JUST THE STAT CARDS. It began as a check on the two cards the owner was
    /// looking at and now walks `Tile::ALL`, because the defect it catches is a mismatch between
    /// a cell in `Tile::place` and a body in this file, and nothing about that is peculiar to a
    /// stat card. A list card is SUPPOSED to run out of room; that is
    /// what `room_for` and the `+N more` line exist for, and it stops drawing rows when it does.
    /// These two draw a fixed thing whose size is not the reader's business, so for them running
    /// out of room is a defect and there is no honest thing the card could do about it at
    /// runtime.
    ///
    /// WHAT MUTATION MAKES THIS RED: shipping `Tile::Night` at fewer rows than its figures need
    /// (measured: red at 7 rows, which is one row under what it takes), or adding a third line to
    /// either body without giving the card the row for it.
    #[test]
    fn no_card_ships_shorter_than_the_thing_it_draws() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("stat-fits");
        let mut bad = Vec::new();
        for tile in Tile::ALL {
            let mut screen = DashboardsScreen::default();
            let (card, text) = framed_tile(&ctx, &mut ing, &mut screen, tile);
            assert!(text.len() > 1, "{} painted only its heading", tile.label());
            let (what, low) = text.iter().fold((String::new(), f32::MIN), |a, (s, r)| {
                if r.bottom() > a.1 {
                    (s.clone(), r.bottom())
                } else {
                    a
                }
            });
            if low > card.bottom() {
                bad.push(format!(
                    "{} ({} rows): {what:?} to y{low:.0}, card ends y{:.0}",
                    tile.label(),
                    tile.place().rows,
                    card.bottom()
                ));
            }
            assert!(
                low <= card.bottom(),
                "{} ships {} rows, and `{what}` runs to y {low} in a card that ends at y {}. It \
                 is cut off at the bottom.",
                tile.label(),
                tile.place().rows,
                card.bottom()
            );
        }
        assert!(
            bad.is_empty(),
            "cards cut off:
  {}",
            bad.join(
                "
  "
            )
        );
    }

    /// ONE TILE AT ITS SHIPPED SIZE: THE BOX IT DREW FOR ITSELF, and the text inside that box.
    ///
    /// # FINDING THE CARD AMONG EVERYTHING ELSE ON THE PAGE
    ///
    /// The harness draws the WHOLE page, so the shapes hold the rail, the summary strip and the
    /// band this card sits in as well as the card. The first version of this took the tallest
    /// wide rect and got the BAND, whose bottom is below the card's; it passed at every height
    /// including ones that clip, which is worth writing down because a guard that cannot fail is
    /// worse than no guard.
    ///
    /// THE CARD IS THE SMALLEST RECT THAT CONTAINS THE CARD'S OWN HEADING. The heading is drawn
    /// by this tile and by nothing else on the page, the card's frame is drawn around it, and
    /// every larger container is exactly that: larger. No geometry is assumed and no constant is
    /// copied out of the renderer.
    ///
    /// AND THE TEXT IS THE TEXT IN THAT COLUMN, for the same reason: the summary strip's figures
    /// are lower down the page than this card and would otherwise be measured as its overflow.
    fn framed_tile(
        ctx: &egui::Context,
        ing: &mut Ingest,
        screen: &mut DashboardsScreen,
        tile: Tile,
    ) -> (egui::Rect, Vec<(String, egui::Rect)>) {
        let mut rects = Vec::new();
        let mut text: Vec<(String, egui::Rect)> = Vec::new();
        for sh in tile_shapes(ctx, ing, screen, tile) {
            match sh {
                egui::Shape::Rect(r) => rects.push(r.rect),
                egui::Shape::Text(t) => text.push((
                    t.galley.text().to_owned(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => {}
            }
        }

        let head = text
            .iter()
            .find(|(s, _)| s == tile.label())
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{} never drew its own heading", tile.label()));

        let card = the_card(&rects, head, tile);

        let mine = text
            .into_iter()
            .filter(|(_, r)| r.center().x >= card.left() && r.center().x <= card.right())
            .filter(|(_, r)| r.top() >= card.top())
            .collect();
        (card, mine)
    }

    /// ONE FRAME OF ONE TILE AT ITS SHIPPED SIZE, drawn by a screen the caller has set up.
    fn painted_tile(
        ctx: &egui::Context,
        ing: &mut Ingest,
        screen: &mut DashboardsScreen,
        tile: Tile,
    ) -> Vec<String> {
        tile_shapes(ctx, ing, screen, tile)
            .into_iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// EVERY SHAPE ONE TILE PAINTS AT ITS SHIPPED SIZE, flattened.
    ///
    /// THE SHAPES AND NOT THE STRINGS, because what a card CLIPS is invisible in its strings: the
    /// shape is emitted either way and the clip rect hides it at paint time. See
    /// [`no_card_ships_shorter_than_the_thing_it_draws`].
    fn tile_shapes(
        ctx: &egui::Context,
        ing: &mut Ingest,
        screen: &mut DashboardsScreen,
        tile: Tile,
    ) -> Vec<egui::Shape> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut cell = tile.place();
        cell.col = 1;
        cell.row = 1;
        let mut settings = crate::settings::Settings {
            dashboard: Some(vec![Placement::new(tile.id(), cell)]),
            ..Default::default()
        };
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let mut warm = ctx.run_ui(input.clone(), |ui| screen.ui(ui, &mut cx));
        warm.shapes.clear();
        warm.drop_without_applying_deltas();
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut out = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                other => out.push(other),
            }
        }
        out
    }

    /// DEFECT: WIDGETS YOU HAD TO SCROLL.
    ///
    /// # "IF YOU HAVE TO SCROLL A WIDGET, IS IT EVEN REALLY A WIDGET"
    ///
    /// The owner is right and the answer is no: a widget is a glance, and a thing you scroll is a
    /// page in a small box. Every list on this page was either UNBOUNDED (the fights list drew a
    /// whole night, seventy-six rows in a card that holds ten) or capped at a number with nothing
    /// to do with the card it was in (twelve roster rows in a card that fits six, six loot lines
    /// in a card that fits four).
    ///
    /// # WHAT IS ASSERTED
    ///
    /// A tall card and a short card of the same tile, over the same fights, draw DIFFERENT numbers
    /// of rows, and the short one says what it left out. That is the whole principle in one
    /// comparison, and it cannot pass by accident: a fixed cap gives the same count at both
    /// heights, and an unbounded list gives the same count too.
    ///
    /// WHAT MUTATION MAKES THIS RED: a `take(N)` back in any list body, dropping a `room_for`
    /// guard, or a `more_line` that draws nothing when rows were dropped.
    #[test]
    fn a_list_draws_what_fits_and_says_what_it_left_out() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("fits-in-its-card");

        /* THE SAME TILE AT TWO HEIGHTS. `Tile::place` is the shipped cell; the short one is the
         * floor the reader can drag it to. See [`shapes_at`] for how a row count is still a
         * height now that a row is a share of the viewport. */
        let count = |ing: &mut Ingest, rows: u16| -> (usize, bool) {
            let said = painted_at(&ctx, ing, Tile::Fights, rows);
            /* THE CAPTURE'S THREE STORED FIGHTS EACH NAME A MOB, so a drawn row is one of these. */
            let drawn = ["a dry bone skeleton", "A tormented dead", "Guard Ullindin"]
                .iter()
                .filter(|m| said.iter().any(|s| s == *m))
                .count();
            let says = said
                .iter()
                .any(|s| s.starts_with("+") && s.contains("more"));
            (drawn, says)
        };

        let (tall, tall_says) = count(&mut ing, Tile::Fights.place().rows);
        let (short, short_says) = count(&mut ing, Tile::Fights.min_rows());
        /* THE TALL CARD STILL FITS WHAT THIS FIXTURE HAS, so it has nothing to leave out and says so
         * by staying quiet. The count moved from three to two with the encounter boundaries: the
         * planted log's pulls run into each other inside the hold and read as one. What this test
         * is about is the SHORT card, which cannot fit them and has to say so. */
        assert_eq!(tall, 2, "the tall card does not fill its rows: {tall}");
        assert!(!tall_says, "the tall card claims it left something out");
        assert!(
            short < tall,
            "the short card drew {short} rows and the tall one {tall}: the list is not reading \
             the height it was given, so it is a scrolling box and not a widget"
        );
        assert!(
            short_says,
            "the short card dropped {} rows and said nothing about them",
            tall - short
        );
    }

    /// DEFECT: A CARD SLICED ITS LAST ROW IN HALF AT ANY SIZE THE READER CHOSE.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// The bottom of all the cards cut off, said twice, with a screenshot of four of them ringed.
    /// He was right and the cause was not the grid: `screens::widgets`' four `Detail` panels and
    /// `screens::dps`' two tables drew a FIXED number of rows, `cap`, into whatever height they
    /// were given. They were written for the overlay window, where that is correct and
    /// deliberate (the window is the reader's ruler and it clips), and the dashboard borrowed
    /// them later. On a card, whose height is the grid's, the last row that could be reached was
    /// cut in half by the card's own bottom edge with nothing saying so.
    ///
    /// # WHY `no_card_ships_shorter_than_the_thing_it_draws` DID NOT CATCH IT
    ///
    /// That guard checks the SHIPPED cell. This defect appears at any height a reader drags a
    /// card to, and every shipped cell happened to be tall enough for its own contents. A rule
    /// that only holds at the sizes shipped is not the rule this page needs.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// A card is drawn at its shortest legal height and then at every height up to its shipped
    /// one, and at NO height may a row start inside the card and finish outside it. What a card
    /// is allowed to do when it runs out of room is stop and say so, which is what
    /// [`overlay::Detail::fit`] and [`overlay::Ranked::fit`] are for.
    ///
    /// WHAT MUTATION MAKES THIS RED: clearing `fit` on the dashboard's own configs (`yours`, or
    /// `fitted` in the overlays card), or a panel that counts rows without asking for room.
    #[test]
    fn no_card_slices_a_row_at_any_height_a_reader_can_drag_it_to() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("short-cards");
        let mut bad = Vec::new();
        for tile in Tile::ALL {
            let full = tile.place().rows;
            for rows in tile.min_rows()..=full {
                let (card, text) = framed_at(&ctx, &mut ing, tile, rows);
                let (what, low) = text.iter().fold((String::new(), f32::MIN), |a, (s, r)| {
                    if r.bottom() > a.1 {
                        (s.clone(), r.bottom())
                    } else {
                        a
                    }
                });
                if low > card.bottom() {
                    bad.push(format!(
                        "{} at {rows} rows slices {what:?} (to y{low:.0}, card ends y{:.0})",
                        tile.label(),
                        card.bottom()
                    ));
                    /* ONE REPORT PER TILE. A card that slices at ten rows slices at nine too, and
                     * a hundred lines of the same fault is a worse message than one. */
                    break;
                }
            }
        }
        assert!(
            bad.is_empty(),
            "cards cut their last row in half:\n  {}",
            bad.join("\n  ")
        );
    }

    /// ONE TILE AT A CHOSEN HEIGHT: the box it drew for itself, and the text inside that box.
    ///
    /// [`framed_tile`] at a height the caller picks. See it for how the card is found.
    fn framed_at(
        ctx: &egui::Context,
        ing: &mut Ingest,
        tile: Tile,
        rows: u16,
    ) -> (egui::Rect, Vec<(String, egui::Rect)>) {
        let mut rects = Vec::new();
        let mut text: Vec<(String, egui::Rect)> = Vec::new();
        for sh in shapes_at(ctx, ing, tile, rows) {
            match sh {
                egui::Shape::Rect(r) => rects.push(r.rect),
                egui::Shape::Text(t) => text.push((
                    t.galley.text().to_owned(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => {}
            }
        }
        let head = text
            .iter()
            .find(|(s, _)| s == tile.label())
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{} never drew its own heading", tile.label()));
        let card = the_card(&rects, head, tile);
        let mine = text
            .into_iter()
            .filter(|(_, r)| r.center().x >= card.left() && r.center().x <= card.right())
            .filter(|(_, r)| r.top() >= card.top() && r.top() < card.bottom())
            .collect();
        (card, mine)
    }

    /// THE CARD'S OWN FRAME, out of everything the page painted.
    ///
    /// # THREE RECTS CONTAIN A CARD'S HEADING AND ONLY ONE IS THE CARD
    ///
    /// The page paints the band the card sits in, the card, and the card's own head bar, and the
    /// heading is inside all three. Picking the smallest took the HEAD BAR the day heads gained
    /// rounded corners; picking the tallest took the BAND, and passed at every height including
    /// ones that clip.
    ///
    /// # WHY IT IS NOT MEASURED AGAINST THE CELL ANY MORE
    ///
    /// It was: the card is the rect whose height is its cell's. That stopped being knowable when
    /// rows became a share of the viewport rather than a fixed twelve points, because a cell's
    /// height now depends on the whole layout's row count and the window it is in, neither of
    /// which a single-tile harness has.
    ///
    /// SO IT IS THE SMALLEST RECT THAT IS TALLER THAN A HEAD BAR. The head is a fixed forty
    /// points and a heading is about fourteen, so anything three headings tall is not the head;
    /// of what is left the card is smaller than the band it sits in. That holds at any pitch.
    fn the_card(rects: &[egui::Rect], head: egui::Rect, tile: Tile) -> egui::Rect {
        rects
            .iter()
            .copied()
            .filter(|r| r.contains(head.center()))
            .filter(|r| r.height() >= head.height() * 3.0)
            .min_by(|a, b| {
                (a.width() * a.height())
                    .partial_cmp(&(b.width() * b.height()))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|| {
                panic!(
                    "{} drew no frame around its heading taller than its own head bar: {:?}",
                    tile.label(),
                    rects
                        .iter()
                        .filter(|r| r.contains(head.center()))
                        .map(egui::Rect::height)
                        .collect::<Vec<_>>()
                )
            })
    }

    /// A PICKED FIGHT IS THAT FIGHT, NOT A FOLD OF ONE.
    ///
    /// # THE DIFFERENCE IS INVISIBLE UNTIL A CHART LOOKS FOR IT
    ///
    /// `reports::roll` sums a scope into one row, and the two things it cannot sum across fights
    /// are `moments` and every fighter's `series`: both are stamped from their OWN fight's start,
    /// so a roll drops them. Rolling a scope of ONE would drop them for nothing, and they are
    /// exactly what the DPS timeline draws. A page that rolled the single pick would look right
    /// on every card and have an empty timeline, which is the kind of defect that survives a
    /// glance.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// The fold IS the stored row, field for field, and the scope behind it is that one fight.
    /// Field for field rather than by name, because the name is the one thing a roll gets right.
    ///
    /// WHAT MUTATION MAKES THIS RED: folding the pick through `roll`, or leaving the scope list
    /// at the whole night while the fold narrows.
    #[test]
    fn a_picked_fight_is_the_fight_and_not_a_fold_of_one() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("one-fight");
        let stored = ing.history().to_vec();
        assert!(
            stored.len() > 1,
            "the capture stored {} fights, so picking one proves nothing",
            stored.len()
        );
        /* A FIGHT WITH A TIMELINE IN IT, because a fight with no moments cannot tell a roll from
         * the row itself and would pass this test either way. */
        let want = stored
            .iter()
            .rev()
            .find(|f| !f.moments.is_empty())
            .cloned()
            .expect("the capture has a fight with moments in it");

        let mut screen = DashboardsScreen {
            pick: Some(want.start.clone()),
            ..DashboardsScreen::default()
        };
        /* THE WHOLE STORE IN SCOPE, so the pick is doing the narrowing and not the filter. */
        screen.filter = Some(crate::screens::night::Filter::default());
        run_dash(&ctx, &mut ing, &mut screen);

        let held = screen.folded.as_ref().expect("the page folded");
        assert_eq!(
            held.fights.len(),
            1,
            "picking one fight left {} in the page's scope",
            held.fights.len()
        );
        let got = held.fold.as_ref().expect("a picked fight folds to itself");
        assert_eq!(
            got, &want,
            "the picked fight was rebuilt rather than taken whole, so it has lost whatever a \
             roll cannot carry"
        );
        assert!(
            !got.moments.is_empty(),
            "the picked fight reached the page with no moments, so its timeline is empty"
        );
    }

    /// THE FILTER SHEET DRAWS A CALENDAR OF THE NIGHTS THAT ARE STORED.
    ///
    /// # WHY THIS IS WORTH A TEST AND NOT A LOOK
    ///
    /// A modal is drawn on top of everything, so a defect in it is invisible in every screenshot
    /// of the page behind it, and it is reached by a control in the window CHROME rather than on
    /// this page. That is two things between a reader and the calendar, and neither of them is
    /// exercised by drawing the dashboard.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// With the sheet open the month of the newest stored night is named, its weekday header is
    /// there, and the day number of a night that HAS fights in it is drawn. The last one is the
    /// point: a calendar that drew a grid and no days would look fine in a thumbnail.
    ///
    /// WHAT MUTATION MAKES THIS RED: a calendar that never opens, one that opens on today rather
    /// than on the newest night stored, or a grid that draws no day cells.
    #[test]
    fn the_filter_sheet_draws_a_calendar_of_the_nights_stored() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("filter-calendar");
        let stored = ing.history().to_vec();
        let newest = crate::screens::night::latest(&stored).expect("the capture stored nights");

        let mut screen = DashboardsScreen {
            filtering: true,
            ..DashboardsScreen::default()
        };
        let said = painted_dash(&ctx, &mut ing, &mut screen);

        assert!(
            said.iter().any(|s| s == "FILTER"),
            "the sheet did not open at all: {said:?}"
        );
        let month = newest.format("%B %Y").to_string();
        assert!(
            said.contains(&month),
            "the calendar did not open on {month}, which is where the newest stored night is"
        );
        /* THE DAY OF THE NEWEST NIGHT IS ON THE GRID. Its own number, drawn as a cell. */
        let day = {
            use chrono::Datelike;
            newest.day().to_string()
        };
        assert!(
            said.contains(&day),
            "the calendar drew no cell for day {day}, the newest night stored"
        );
    }

    /// ONE PASS OF THE WHOLE DASHBOARD, AND EVERY STRING IT PAINTED.
    /// DEFECT: A BAND OF DEAD PAGE UNDER THE LAST WIDGET, AND A SCROLLBAR ON A SMALL SCREEN.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// A screenshot of the bottom of the page, ringed: the black strip between the last card and
    /// the `v0.1.0` line. He asked whether the page could stop scrolling and take the room it has
    /// instead, sized to whatever window he put it in.
    ///
    /// # THE CAUSE WAS ONE ASYMMETRY IN THE GRID
    ///
    /// A column is a twelfth of the width, so the grid always filled the window ACROSS. A row was
    /// twelve points flat, so the page was a fixed height DOWN: on a short window the last band
    /// fell past the fold and the page scrolled, and on a tall one the page ran out before the
    /// window did and left that strip. Both complaints are the same defect from either side.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// The bottom band's card ends the same distance from the bottom of the window in a window
    /// five hundred points taller, which is what filling means: a page of a fixed height leaves a
    /// gap that grows point for point with the window. And that distance is small, so the page is
    /// not merely consistent but actually reaching the bottom.
    ///
    /// WHAT MUTATION MAKES THIS RED: a [`dashgrid::row_height`] that ignores the height it is
    /// given, which is the page as it was: the bottom band falls hundreds of points past the fold
    /// in the smaller window and stops hundreds short of it in the larger one.
    ///
    /// WHAT IT DOES NOT CATCH: a caller that allocates a MARGIN past the last tile. That is dead
    /// page too, but it is dead page BELOW the fold rather than on screen, and it does not move
    /// the last card by a point. [`dashgrid::grid_height`] is where that one is pinned, against
    /// the bottom edge of the lowest cell.
    #[test]
    fn the_page_ends_where_its_last_widget_ends_in_any_window() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("fills-its-window");
        let gap = |ing: &mut Ingest, h: f32| -> f32 {
            let (rects, text) = dash_at(&ctx, ing, h);
            /* THE LOWEST ONE. The widget sheet names every tile too, and the heading wanted here
             * is the one the bottom band actually painted. */
            let head = text
                .iter()
                .filter(|(s, _)| s == Tile::Overlays.label())
                .map(|(_, r)| *r)
                .max_by(|a, b| a.top().total_cmp(&b.top()))
                .unwrap_or_else(|| panic!("the bottom band never drew in a {h}pt window"));
            h - the_card(&rects, head, Tile::Overlays).bottom()
        };
        /* BOTH TALL ENOUGH FOR THE WHOLE ARRANGEMENT, so what is compared is one layout in two
         * windows and not two layouts. Under about nineteen hundred points the page takes a
         * smaller [`Size`], which has no bottom band to measure and is a different question;
         * `the_window_takes_the_biggest_arrangement_it_has_room_for` asks that one. */
        let small = gap(&mut ing, 1950.0);
        let big = gap(&mut ing, 2450.0);
        assert!(
            (big - small).abs() < 2.0,
            "the last widget ends {small:.0} points off the bottom in a 1950 point window and \
             {big:.0} in a 2450 point one: the page is a fixed height and the difference is dead \
             page the reader can scroll to"
        );
        assert!(
            small < 90.0,
            "the last widget ends {small:.0} points off the bottom of the window: the page is \
             consistent about not reaching the bottom, which is not the same as filling it"
        );
    }

    /// THE OWNER'S OWN ANSWER TO `HOW MUCH DASHBOARD FITS IN THIS WINDOW`.
    ///
    /// # WHAT HE ASKED FOR
    ///
    /// A screenshot of the dead strip at the bottom of the page, and then the question: "we
    /// either need to NOT allow scrolling and allow x y space depending on how the person has it
    /// sized (thinking possibly allowing quarter 4k what we have right now half screen and full
    /// screen there without resize?". Asked which cards a quarter of a 4K screen should carry he
    /// named four: the timeline, damage, the live fight and recent fights. Asked how the page
    /// should switch between them he said automatically, by window size.
    ///
    /// # WHAT IS ASSERTED, AND WHY IT IS ASKED OF THE PAGE AND NOT OF [`Size::fitting`]
    ///
    /// `Size::fitting` compares two numbers and a test of it would compare the same two numbers
    /// back. What the owner asked about is what is ON THE PAGE, so the page is drawn, in the
    /// three windows he named, and the CARDS are counted: a 4K quarter carries his four and not
    /// one more, a 4K screen carries the whole mock, and the step between them carries the middle
    /// one. Whether that lands at 1064 points of grid or 1080 of window is arithmetic; whether
    /// `Named kills` is on a 1080 tall screen is the question he asked.
    ///
    /// AND NOTHING EVER DISAPPEARS AS THE WINDOW GROWS: every card in a smaller arrangement is in
    /// every bigger one. A card that came and went with the window would be worse than a card
    /// that was never there.
    ///
    /// WHAT MUTATION MAKES THIS RED: a [`ROOMY`] moved to `dashgrid::ROW_UNIT`, which puts seven
    /// cards on the quarter screen he asked for four on; a tile dropped from an arrangement; or
    /// `Size::fitting` reading the arrangements in the wrong order.
    #[test]
    fn the_window_takes_the_biggest_arrangement_it_has_room_for() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("how-much-fits");
        let cards = |ing: &mut Ingest, h: f32| -> Vec<&'static str> {
            let (_, text) = dash_at(&ctx, ing, h);
            Tile::ALL
                .iter()
                .filter(|t| text.iter().any(|(s, _)| s == t.label()))
                .map(|t| t.label())
                .collect()
        };

        /* A QUARTER OF A 4K SCREEN: 1080 points tall, which is the window he was looking at. */
        let quarter = cards(&mut ing, 1080.0);
        let mut want: Vec<&str> = [Tile::Timeline, Tile::Damage, Tile::Live, Tile::Fights]
            .iter()
            .map(|t| t.label())
            .collect();
        want.sort_unstable();
        let mut got = quarter.clone();
        got.sort_unstable();
        assert_eq!(
            got, want,
            "a quarter of a 4K screen does not carry the four cards the owner asked for"
        );

        /* THE STEP UP: a full 1440p screen. The two rosters and the chart come back. */
        let middle = cards(&mut ing, 1440.0);
        for t in [Tile::Healing, Tile::Taken, Tile::Progression] {
            assert!(
                middle.contains(&t.label()),
                "{t:?} is not on a 1440 point screen, which has room for it"
            );
        }
        assert!(
            !middle.contains(&Tile::Kills.label()),
            "the whole mock is on a 1440 point screen, so the middle arrangement is not reachable \
             at all"
        );

        /* AND A 4K SCREEN CARRIES EVERYTHING. */
        let whole = cards(&mut ing, 2160.0);
        assert_eq!(
            whole.len(),
            Tile::ALL.len(),
            "a 4K screen does not carry the whole dashboard: {whole:?}"
        );

        /* NOTHING COMES AND GOES. Each arrangement contains the one below it. */
        for (small, big, what) in [
            (&quarter, &middle, "1080 to 1440"),
            (&middle, &whole, "1440 to 2160"),
        ] {
            for card in small.iter() {
                assert!(
                    big.contains(card),
                    "{card} is on the page at the smaller of {what} and gone at the larger, so a \
                     card disappears when the window grows"
                );
            }
        }

        /* AND AN ARRANGEMENT A READER MADE IS HIS AT EVERY SIZE. The window chooses only when
         * `Settings::dashboard` is `None`; see [`layout`]. */
        let mine = vec![Placement::new(Tile::Kills.id(), Tile::Kills.place())];
        for size in Size::ALL {
            let drawn = layout(Some(&mine), size);
            assert_eq!(
                drawn.len(),
                1,
                "the {} window rewrote a dashboard the reader arranged himself",
                size.label()
            );
            assert_eq!(drawn[0].1, Tile::Kills);
        }
    }
    /// THE WHOLE DASHBOARD IN A WINDOW OF A CHOSEN HEIGHT: every box and every string it drew.
    ///
    /// [`painted_dash`] with the window as a parameter and the boxes kept, for the guards that
    /// measure where the page ends rather than what it says.
    fn dash_at(
        ctx: &egui::Context,
        ing: &mut Ingest,
        height: f32,
    ) -> (Vec<egui::Rect>, Vec<(String, egui::Rect)>) {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut screen = DashboardsScreen::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                egui::vec2(1400.0, height),
            )),
            ..Default::default()
        };
        let mut warm = ctx.run_ui(input.clone(), |ui| screen.ui(ui, &mut cx));
        warm.shapes.clear();
        warm.drop_without_applying_deltas();
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut rects = Vec::new();
        let mut text = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(x) => stack.extend(x),
                egui::Shape::Rect(r) => rects.push(r.rect),
                egui::Shape::Text(t) => text.push((
                    t.galley.text().to_owned(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => {}
            }
        }
        (rects, text)
    }

    fn painted_dash(
        ctx: &egui::Context,
        ing: &mut Ingest,
        screen: &mut DashboardsScreen,
    ) -> Vec<String> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                egui::vec2(1100.0, 820.0),
            )),
            ..Default::default()
        };
        /* TWO PASSES, because a modal lays out against what it measured on the pass before. */
        let mut warm = ctx.run_ui(input.clone(), |ui| screen.ui(ui, &mut cx));
        warm.shapes.clear();
        warm.drop_without_applying_deltas();
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut said = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(x) => stack.extend(x),
                egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        said
    }

    /// DEFECT: A REFILL THE DASHBOARD NEVER SAW.
    ///
    /// The page keeps one fold of its scope and recomputes it only when its key moves, and the key
    /// was the history's length and newest stamp. A refill rewrites rows already there and moves
    /// neither, so the page went on captioning a night `group not known` out of the fold it took
    /// before the store learned the group, until something unrelated changed the scope.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `history_gen` from the fold's key.
    #[test]
    fn a_refilled_history_is_folded_again_and_not_served_from_the_old_fold() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("refill-refold");
        let mut screen = DashboardsScreen::default();
        /* A NIGHT AS AN OLDER BUILD STORED IT: the same rows, no group. Put in through the same
         * call a refill's reread uses, so the page's first fold is taken from rows that do not
         * know, exactly as it was on the owner's machine. */
        let mut rows = ing.history().to_vec();
        assert!(
            !rows.is_empty(),
            "the capture stored no fights, so there is no night to refill"
        );
        for r in &mut rows {
            r.group = None;
        }
        ing.replace_history(rows.clone());
        let scope_says_not_known = |said: &[String]| {
            said.iter()
                .any(|s| s.starts_with("in scope") && s.contains("group not known"))
        };
        let before = painted_dash(&ctx, &mut ing, &mut screen);
        assert!(
            scope_says_not_known(&before),
            "the scope's cards do not say the group is not known, so a refill has nothing to \
             change on this page: {before:?}"
        );

        /* WHAT A REFILL DOES TO THE HISTORY: the same rows, now knowing the reader was solo. */
        for r in &mut rows {
            r.group = Some(Vec::new());
        }
        ing.replace_history(rows);
        let after = painted_dash(&ctx, &mut ing, &mut screen);
        assert!(
            !scope_says_not_known(&after),
            "the history now knows the group and the page is still captioning its old fold: \
             {after:?}"
        );
    }

    /// DEFECT: A RELEASE THAT MOVED NOTHING SAVED THE DASHBOARD. See [`committed`].
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the same-cell test from `committed`, or either
    /// release site going back to its own inline commit.
    #[test]
    fn a_release_that_moves_nothing_writes_nothing() {
        let cells = vec![Cell::new(1, 6, 1, 10), Cell::new(7, 6, 1, 10)];
        let moved = Cell::new(1, 6, 11, 10);
        assert_eq!(
            committed(&cells, Some((0, cells[0], true)), 0),
            None,
            "a card let go on its own cell was committed, which writes the whole dashboard as an \
             arrangement the reader never made"
        );
        assert_eq!(
            committed(&cells, Some((0, moved, true)), 0),
            Some(moved),
            "a real move was lost"
        );
        assert_eq!(
            committed(&cells, Some((0, moved, false)), 0),
            None,
            "a red ghost was committed"
        );
        assert_eq!(
            committed(&cells, Some((0, moved, true)), 1),
            None,
            "another card's ghost was committed"
        );
        assert_eq!(committed(&cells, None, 0), None);

        /* AND BOTH RELEASES ASK IT. A move and a resize are the two ways a release commits. */
        let body = include_str!("dashboards.rs");
        let grid = &body[body.find("    fn grid(").expect("grid")..];
        let grid = &grid[..grid.find("\n    }\n").expect("its end")];
        assert_eq!(
            grid.matches("committed(&cells, landing, i)").count(),
            2,
            "a release in the grid commits without asking whether anything moved"
        );
    }

    /// A SAVED ARRANGEMENT EQUAL TO A SHIPPED ONE IS HANDED BACK, AND A REAL ONE IS NOT.
    ///
    /// WHAT MUTATION MAKES THIS RED: `forget_unchosen` comparing in order, missing a size, or
    /// forgetting a list that differs by one cell or one card.
    #[test]
    fn an_arrangement_nobody_chose_is_handed_back_to_the_window() {
        for size in Size::ALL {
            let mut list = Some(size.shipped());
            forget_unchosen(&mut list);
            assert_eq!(
                list,
                None,
                "the {} arrangement was kept as a choice",
                size.label()
            );

            let mut reversed = size.shipped();
            reversed.reverse();
            let mut list = Some(reversed);
            forget_unchosen(&mut list);
            assert_eq!(
                list,
                None,
                "the {} arrangement in another order was kept",
                size.label()
            );
        }
        let mut moved = shipped();
        moved[0].rows += 1;
        let mut list = Some(moved.clone());
        forget_unchosen(&mut list);
        assert_eq!(
            list,
            Some(moved),
            "a card the reader resized was thrown away"
        );

        let mut fewer = shipped();
        fewer.pop();
        let mut list = Some(fewer.clone());
        forget_unchosen(&mut list);
        assert_eq!(list, Some(fewer), "a card the reader removed came back");

        /* A LIST WITH A CARD IN IT TWICE HOLDS EVERY SHIPPED CELL AND IS NOT THE SHIPPED LIST. */
        let mut doubled = shipped();
        doubled.push(doubled[0].clone());
        let mut list = Some(doubled.clone());
        forget_unchosen(&mut list);
        assert_eq!(
            list,
            Some(doubled),
            "a list with a card in it twice was read as the shipped one"
        );

        let mut empty = Some(Vec::new());
        forget_unchosen(&mut empty);
        assert_eq!(
            empty,
            Some(Vec::new()),
            "a dashboard the reader emptied was refilled"
        );
    }

    /// DEFECT: A CARD'S CAPTION RAN UNDER ITS OWN TABS.
    ///
    /// The owner's screenshot: the four column `Damage taken` card, its caption `in scope,
    /// everyone in range, group not known` printed straight through its DEALT and TAKEN tabs. The
    /// head laid the words out first and the tabs after, in whatever width was left, which on a
    /// narrow card is none.
    ///
    /// # THREE WIDTHS, SO NO HALF OF THE RULE CAN PASS BY NOT BEING NEEDED
    ///
    ///   * SEVEN COLUMNS: the whole caption fits, and must be drawn.
    ///   * FIVE COLUMNS: some of it fits, so it must be drawn CUT. Without the cut a label either
    ///     runs on under the tabs or wraps onto lines the forty point head does not have, so every
    ///     word on the head's line is asserted to stop short of the tabs AND to be one line.
    ///   * FOUR COLUMNS: none of it fits, and whatever is drawn stops short of the tabs; the
    ///     caption is on the title's hover instead.
    ///
    /// WHAT MUTATION MAKES THIS RED: `panel_head` ignoring the width its controls took; the
    /// caption not being truncated; the room test drawing an ellipsis with no room.
    #[test]
    fn a_card_heads_words_stop_where_its_tabs_start() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("head-caption");
        let mut rows = ing.history().to_vec();
        for r in &mut rows {
            r.group = None;
        }
        ing.replace_history(rows);
        for (tile, span, drawn) in [
            (Tile::Damage, 7, true),
            (Tile::Damage, 5, true),
            (Tile::Taken, 4, false),
        ] {
            let mut text: Vec<(String, egui::Rect)> = Vec::new();
            for sh in shapes_in(&ctx, &mut ing, tile, tile.place().rows, span) {
                if let egui::Shape::Text(t) = sh {
                    text.push((
                        t.galley.text().to_owned(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                    ));
                }
            }
            let tabs: Vec<egui::Rect> = tile
                .tabs()
                .iter()
                .filter_map(|t| {
                    text.iter()
                        .find(|(s, _)| s == &t.to_uppercase())
                        .map(|(_, r)| *r)
                })
                .collect();
            assert_eq!(
                tabs.len(),
                tile.tabs().len(),
                "a tab is missing from the {} head at {span} columns: {text:?}",
                tile.label()
            );
            let first = tabs.iter().map(|r| r.left()).fold(f32::MAX, f32::min);
            let line = tabs.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
            /* THE RIGHT EDGE OF THE LAST TAB, so the arrow beyond it is not one of the words. */
            let last = tabs.iter().map(|r| r.right()).fold(f32::MIN, f32::max);
            /* EVERYTHING THAT STARTS ON THE HEAD'S LINE AND LEFT OF ITS TABS: the title and the
             * caption. By its TOP and not its centre, so a caption that wrapped onto three lines
             * is still found rather than slipping out of the filter. */
            let words: Vec<&(String, egui::Rect)> = text
                .iter()
                .filter(|(s, r)| {
                    (r.top() - line).abs() < 10.0
                        /* LEFT OF THE LAST TAB AND NOT OF THE FIRST. An ellipsis drawn with no room
                         * starts BETWEEN the tabs, and a filter on the first tab would never see it. */
                        && r.left() < last
                        && !tile.tabs().iter().any(|t| s == &t.to_uppercase())
                        && !s.trim().is_empty()
                })
                .collect();
            let caption = words.iter().find(|(s, _)| s.starts_with("in "));
            if drawn {
                assert!(
                    caption.is_some(),
                    "the {} card has room for a caption at {span} columns and drew none: {words:?}",
                    tile.label()
                );
            }
            for (s, r) in &words {
                assert!(
                    r.right() <= first + 0.5,
                    "on the {} card at {span} columns {s:?} runs to x{:.0} and the tabs start at \
                     x{first:.0}, so the words print through the controls",
                    tile.label(),
                    r.right()
                );
                assert!(
                    r.height() < 20.0,
                    "on the {} card at {span} columns {s:?} wrapped to {:.0} points, taller than a \
                     line of the head it sits in",
                    tile.label(),
                    r.height()
                );
            }
        }
    }

    /// ONE PASS OF THE WHOLE DASHBOARD, for a test that reads what the page decided.
    fn run_dash(ctx: &egui::Context, ing: &mut Ingest, screen: &mut DashboardsScreen) {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        out.shapes.clear();
        out.drop_without_applying_deltas();
    }

    /// DEFECT: TIMELINE FLAGS PRINTED ON TOP OF EACH OTHER.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// `YoYouYeYuYiXited died`, which is five labels in one place and none of them readable. The
    /// timeline draws the scope now, so five deaths inside one pull that were seconds apart on a
    /// fight's own clock are the same three pixels on eight hours of wall clock.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// No two tabs overlap, at any spacing, including all of them on one pixel. And nothing is
    /// LOST to make that true: the folded labels account for every mark that went in, which is
    /// the difference between folding and the cap this replaced. A cap kept the newest five and
    /// dropped the rest without saying so.
    ///
    /// WHAT MUTATION MAKES THIS RED: folding on a fixed gap rather than the measured label (a
    /// long name overlaps the next), or measuring the fold against the label before it grew.
    #[test]
    fn timeline_flags_never_print_on_top_of_each_other() {
        let ctx = prepared_dash();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let names = [
                "Tanefilo died",
                "You died",
                "Kill",
                "A very long necromancer name died",
                "Poguhy died",
                "Fylasem died",
            ];
            /* FROM ALL ON ONE PIXEL TO COMFORTABLY APART. */
            for step in [0.0f32, 1.0, 7.0, 30.0, 90.0, 400.0] {
                let marks: Vec<(f32, egui::Color32, String)> = names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (100.0 + i as f32 * step, WRONG, (*n).to_owned()))
                    .collect();
                let out = fold_flags(ui, marks);

                /* NO TWO TABS SHARE A POINT.
                 *
                 * MEASURED THE WAY `flag` MEASURES, and deliberately NOT through `flag_width`,
                 * which is the fold's own idea of how much room a label wants. A test that asked
                 * the fold to check the fold's own arithmetic passes when both are wrong
                 * together: replacing `flag_width` with a constant left this green until it was
                 * written this way. What has to hold is that the TABS do not overlap, and a tab
                 * is the galley plus its padding, centred.
                 */
                let tab = |t: &str| {
                    ui.painter()
                        .layout_no_wrap(t.to_owned(), egui::FontId::proportional(FLAG_PT), TEXT)
                        .size()
                        .x
                        + FLAG_PAD
                };
                /* WITHIN A LANE, which is the invariant now that flags stack. Two tabs at
                 * different heights are allowed to share an x; two in the same lane are not. */
                for lane in 0..FLAG_LANES {
                    let row: Vec<&(f32, egui::Color32, String, usize)> =
                        out.iter().filter(|(_, _, _, l)| *l == lane).collect();
                    for pair in row.windows(2) {
                        let (ax, _, at, _) = &pair[0];
                        let (bx, _, bt, _) = &pair[1];
                        let a_right = ax + tab(at) / 2.0;
                        let b_left = bx - tab(bt) / 2.0;
                        assert!(
                            b_left >= a_right,
                            "at {step} apart in lane {lane}, {bt:?} starts at {b_left} and \
                             {at:?} runs to {a_right}"
                        );
                    }
                }
                /* AND NOBODY IS PUT IN A LANE THAT DOES NOT EXIST. */
                assert!(
                    out.iter().all(|(_, _, _, l)| *l < FLAG_LANES),
                    "a flag was given a lane the chart has no room for: {out:?}"
                );

                /* AND EVERY MARK IS STILL ACCOUNTED FOR. A label is either one name or a name
                 * and `+N`; those have to add up to what went in. */
                let counted: usize = out
                    .iter()
                    .map(|(_, _, t, _)| match t.rsplit_once(" +") {
                        Some((_, n)) => 1 + n.parse::<usize>().unwrap_or(0),
                        None => 1,
                    })
                    .sum();
                assert_eq!(
                    counted,
                    names.len(),
                    "at {step} apart the flags account for {counted} of {} marks: {out:?}",
                    names.len()
                );
            }
        })
        .drop_without_applying_deltas();
    }

    /// A CONTEXT WITH THIS APP'S FONTS AND THEME, for the fit tests.
    fn prepared_dash() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx
    }

    /// ONE FRAME OF ONE TILE AT A CHOSEN HEIGHT IN ROWS, and every string INSIDE THAT CARD.
    ///
    /// THROUGH [`framed_at`] AND NOT STRAIGHT OFF THE PAGE, because the page is no longer just
    /// this tile: [`shapes_at`] pins the page's height with a second tile under this one, and a
    /// caller counting rows would otherwise count that tile's rows too.
    fn painted_at(ctx: &egui::Context, ing: &mut Ingest, tile: Tile, rows: u16) -> Vec<String> {
        framed_at(ctx, ing, tile, rows)
            .1
            .into_iter()
            .map(|(s, _)| s)
            .collect()
    }

    /// THE ROW COUNT EVERY PAGE IN THIS HARNESS IS, so that a row count is a height.
    ///
    /// Above the tallest shipped cell, which is `Damage` at thirty one rows, so a tile can be
    /// asked for its full height and still leave the spacer something to stand in.
    const PAGE_ROWS: u16 = 36;

    /// EVERY SHAPE ONE TILE PAINTS AT A CHOSEN HEIGHT IN ROWS, flattened.
    ///
    /// # A PAGE OF ONE TILE HAS NO SHORT CARD IN IT
    ///
    /// A row is a share of the viewport, so the ONLY tile on a page is the whole page whatever
    /// its row count says: asked for a card at eight rows and at twenty two this returned the
    /// same card twice, and the guards built on it could not fail. That is not a fault in the
    /// page, it is what filling the window means.
    ///
    /// SO THE PAGE'S ROW COUNT IS PINNED WITH A SPACER TILE UNDER THE ONE BEING MEASURED, which
    /// is what a real dashboard has and what a reader dragging a card shorter actually does: the
    /// page stays the same height and the card gives its rows up to what is below it. A row is a
    /// height again, [`PAGE_ROWS`] over the window, and the card being measured is the only one
    /// [`framed_at`] looks at.
    ///
    /// See [`tile_shapes`], which is this at the tile's own shipped cell.
    fn shapes_at(ctx: &egui::Context, ing: &mut Ingest, tile: Tile, rows: u16) -> Vec<egui::Shape> {
        shapes_in(ctx, ing, tile, rows, tile.place().span)
    }

    /// [`shapes_at`] AT A CHOSEN WIDTH TOO, in columns. A card's head is laid out against its
    /// width, and the shipped width is only one of the widths a reader can drag a card to.
    fn shapes_in(
        ctx: &egui::Context,
        ing: &mut Ingest,
        tile: Tile,
        rows: u16,
        span: u8,
    ) -> Vec<egui::Shape> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut cell = tile.place();
        cell.col = 1;
        cell.row = 1;
        cell.rows = rows;
        cell.span = span;
        /* THE SPACER TAKES WHAT IS LEFT OF THE PAGE. Any tile but the one being measured will
         * do: it is never looked at, it is only there so the page has a fixed row count. */
        let spacer = if tile == Tile::Mobs {
            Tile::Night
        } else {
            Tile::Mobs
        };
        let under = Cell::new(1, 12, cell.row_end(), PAGE_ROWS.saturating_sub(rows).max(1));
        let mut settings = crate::settings::Settings {
            dashboard: Some(vec![
                Placement::new(tile.id(), cell),
                Placement::new(spacer.id(), under),
            ]),
            ..Default::default()
        };
        let mut screen = DashboardsScreen::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let mut warm = ctx.run_ui(input.clone(), |ui| screen.ui(ui, &mut cx));
        warm.shapes.clear();
        warm.drop_without_applying_deltas();
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut said: Vec<egui::Shape> = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                other => said.push(other),
            }
        }
        said
    }

    /// DEFECT: A LOCK THAT DOES NOT LOCK.
    ///
    /// The owner asked for the lock and the Widgets button as one pair of controls on the header,
    /// and they are one pair: a sheet whose rows add and remove tiles is a rearrangement, so an
    /// open sheet over a locked grid would be a control that still worked under a lock saying
    /// nothing moves. The two directions are asserted because only one of them is obvious.
    ///
    /// WHAT MUTATION MAKES THIS RED: a `toggle_lock` that leaves the sheet open, a `toggle_picker`
    /// that leaves the grid locked, or a default that starts unlocked.
    #[test]
    fn the_lock_and_the_widget_sheet_agree_with_each_other() {
        let mut s = DashboardsScreen::default();
        assert!(
            s.locked(),
            "a fresh dashboard is unlocked, so the first stray drag rearranges it"
        );
        assert!(!s.picking());

        /* OPENING THE SHEET UNLOCKS, or its rows would be inert. */
        s.toggle_picker();
        assert!(s.picking() && !s.locked());

        /* AND LOCKING SHUTS IT. */
        s.toggle_lock();
        assert!(s.locked(), "the lock did not take");
        assert!(
            !s.picking(),
            "the widget sheet is still open over a locked grid, so a tile can be added to a \
             dashboard that says nothing moves"
        );

        s.toggle_lock();
        assert!(
            !s.locked() && !s.picking(),
            "unlocking opened the sheet by itself"
        );
    }

    /// HOW LONG AGO IS SAID WITHOUT CLAIMING A TIMEZONE.
    ///
    /// WHAT MUTATION MAKES THIS RED: printing a clock time, or losing the `never` case.
    #[test]
    fn how_long_ago_is_said_without_claiming_a_timezone() {
        assert_eq!(since(None), "never");
        let now = Utc::now();
        assert_eq!(since(Some(now)), "just now");
        assert_eq!(since(Some(now - chrono::Duration::seconds(30))), "30s ago");
        assert_eq!(since(Some(now - chrono::Duration::minutes(9))), "9m ago");
        assert_eq!(
            since(Some(now - chrono::Duration::minutes(125))),
            "2h 5m ago"
        );
        for t in [0i64, 30, 600, 7300] {
            let said = since(Some(now - chrono::Duration::seconds(t)));
            assert!(
                !said.contains(':'),
                "a clock time reached the page, which is right in one timezone: {said}"
            );
        }
    }

    /// THE LANDING STRIP COUNTS THE FIGHTS THE INGEST FOUND AND SAYS HOW OLD THEY ARE.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the staleness sentence, or counting in the plural at
    /// one.
    #[test]
    fn the_landing_state_counts_the_fights_and_says_how_old_they_are() {
        let base = Facts {
            scanning: false,
            folder: true,
            folder_problem: None,
            log: Some(String::from("eqlog_Reviir_neriak.txt")),
            log_problem: None,
            character: Some(String::from("Reviir")),
            read: Some(Utc::now()),
            scanned: Some(Utc::now()),
            clipped: false,
            fights: 0,
            unreadable: 0,
        };

        assert!(
            base.history().is_empty(),
            "a count of zero was drawn under a line that already says why there is nothing"
        );

        /* THE AGE OF THE READ, AND NOT A SECOND FIGHTS COUNT. The head already states how many
         * fights are in scope; this used to put the BOOTSTRAP's count in the same hover under
         * the same word, and that fold is not what this page reads. */
        let one = Facts { fights: 1, ..base };
        let h = one.history();
        assert_eq!(h.len(), 1);
        assert!(
            h[0].0.starts_with("log scanned"),
            "the hover is naming something other than the age of the read: {}",
            h[0].0
        );
        assert!(
            !h[0].0.contains("fight"),
            "a second fights count is back in the head's hover: {}",
            h[0].0
        );
        assert!(
            h[0].1.contains("store"),
            "the note does not say what this page actually counts"
        );

        let many = Facts {
            fights: 12,
            unreadable: 3,
            clipped: true,
            ..one
        };
        let h = many.history();
        assert_eq!(
            h.len(),
            3,
            "the unreadable and clipped lines are not both drawn"
        );
        /* THE SCAN AGE STILL LEADS, whatever the count behind the guard is. */
        assert!(h[0].0.starts_with("log scanned"));
        assert!(h[1].0.contains("3 lines not placed"));
    }

    /// AN INGEST WITH A STORE IN A TEMP FOLDER, booted over the reference capture.
    ///
    /// THE STORE IS THE DASHBOARD'S SOURCE, so a test with no store is a test of the empty page.
    /// `Store::at` on a temp path is the ONLY constructor a test may use; `Store::app_data` is
    /// the owner's real fights folder and pointing a test at it is the accident `use_store`
    /// being opt in exists to prevent.
    fn booted_stored(tag: &str) -> Ingest {
        let dir = crate::fights::probe::planted(tag, crate::fights::probe::CAPTURE);
        let root =
            std::env::temp_dir().join(format!("grimoire-dash-store-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let settings = crate::settings::Settings {
            log_dir: Some(dir.clone()),
            data_root: Some(dir.clone()),
            ..crate::settings::Settings::default()
        };
        let mut ig = Ingest::new(&settings);
        ig.use_store(Some(crate::store::Store::at(&root)));
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                return ig;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the bootstrap never landed for {}", dir.display());
    }

    /// EVERY TEXT ON ONE FRAME OF THE PAGE, WITH WHERE IT LANDED, at a window the owner's size.
    ///
    /// TWO FRAMES, because scroll areas settle on the second; the first is thrown away with its
    /// font delta declared dropped, which epaint otherwise refuses.
    fn placed_at(
        ctx: &egui::Context,
        ing: &mut Ingest,
        tiles: &[Tile],
    ) -> Vec<(String, egui::Pos2)> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings {
            dashboard: if tiles.is_empty() {
                None
            } else {
                Some(
                    tiles
                        .iter()
                        .map(|t| Placement::new(t.id(), t.place()))
                        .collect(),
                )
            },
            ..Default::default()
        };
        let mut screen = DashboardsScreen::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
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
                /* TALL ENOUGH TO HOLD THE TILE BEING MEASURED, which is not a detail.
                 *
                 * egui does not paint a label whose rect falls outside the clip, so a card below
                 * the fold contributes NO text to these positions and every question asked about
                 * it answers . This was 560 points, and the day the timeline
                 * moved to the top of the shipped layout it pushed the Damage roster past it and
                 * the foot assertion below started reporting the foot missing rather than
                 * misplaced. The viewport is the measuring instrument here and it has to be
                 * bigger than the thing being measured. */
                egui::vec2(1000.0, 1000.0),
            )),
            ..Default::default()
        };
        let mut warm = ctx.run_ui(input.clone(), |ui| screen.ui(ui, &mut cx));
        warm.shapes.clear();
        warm.drop_without_applying_deltas();
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        let mut at = Vec::new();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => at.push((t.galley.text().to_owned(), t.pos)),
                _ => {}
            }
        }
        at
    }

    /// THE PAGE READS THE STORE, FOLDS THE NIGHT, AND KEEPS THE MOBS OUT OF THE CHART.
    ///
    /// # THIS DRIVES A REAL FRAME OVER A REAL STORE AND READS THE INK
    ///
    /// Everything above is a pure function. This is the only thing that says they are WIRED to
    /// a page: a real `Ingest` over the reference capture with a temp store, a real headless
    /// frame, and the text shapes that came out of it.
    ///
    /// THE DASHBOARD IS NOT LIVE. The capture has four fights and the last one is still open
    /// when the bootstrap runs, so the store holds THREE, and the page must show those three and
    /// not the fourth. `Guard V`Lex` is only in the fourth: it out-damaged every player in it,
    /// so if the page were reading the live fold it would sit at the top of the damage roster.
    /// Its absence is what says the page is reading the store.
    ///
    /// WHAT MUTATION MAKES THIS RED: reading `current_fight` instead of `history`, a grid that
    /// draws no tiles, a chart of people with a mob on it, the scope chips not being drawn, or
    /// an empty state that blames the build rather than the absent log.
    #[test]
    fn the_page_reads_the_store_and_keeps_the_mobs_out_of_the_chart() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);

        let painted = |ing: &mut Ingest, tiles: &[Tile]| -> Vec<String> {
            let live = crate::watcher::Status {
                twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
                youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
            };
            let mut settings = crate::settings::Settings {
                dashboard: if tiles.is_empty() {
                    None
                } else {
                    Some(
                        tiles
                            .iter()
                            .map(|t| Placement::new(t.id(), t.place()))
                            .collect(),
                    )
                },
                ..Default::default()
            };
            let mut screen = DashboardsScreen::default();
            let mut cx = Cx {
                data: None,
                railed: true,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: ing,
                chat: crate::chat::ChatHandle::idle(),
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
                    egui::vec2(1400.0, 1800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
            let shapes = std::mem::take(&mut out.shapes);
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
            said
        };

        let mut ing = booted_stored("screen-dashboards");
        assert_eq!(ing.fights().len(), 12, "the ingest found no fights");
        assert_eq!(
            ing.history().len(),
            11,
            "the store did not take the three CLOSED fights: {:?}",
            ing.history()
                .iter()
                .map(|f| f.start.clone())
                .collect::<Vec<_>>()
        );

        /* ---- the whole shipped dashboard ---- */

        let said = painted(&mut ing, &[]);
        for t in [Tile::Damage, Tile::Live, Tile::Fights, Tile::Timeline] {
            assert!(
                said.iter().any(|s| s == t.label()),
                "{t:?}'s head did not reach the page, so the grid is not drawing every tile: {said:?}"
            );
        }
        /* THE HEAD STATES THE FILTER AND NO LONGER CARRIES A CHIP PER NIGHT.
         *
         * The owner's ruling: the dates do not all belong on screen at once, they belong behind
         * a filter key in the corner. So the head says WHAT is being looked at, in full, and the
         * sheet is where it is changed. The capture is one night, so that is `Jul 15, 2026`. */
        assert!(
            said.iter().any(|s| s == "Jul 15, 2026"),
            "the head does not state what is being looked at: {said:?}"
        );
        for chip in ["JUL 15", "7 NIGHTS", "ALL"] {
            assert!(
                !said.iter().any(|s| s == chip),
                "the chip row is back in the head: {chip}"
            );
        }

        /* THE CHARTS CARRY THEIR AXES, THEIR CLOCK AND THEIR FLAGS.
         *
         * The owner held up the mock's DPS timeline against a chart that was bare lines on a
         * flat panel: no scale, no clock, no key. A chart without a y axis is a shape and not a
         * measurement. `0` is the bottom rule's label.
         *
         * THE FLAG IT LOOKS FOR DEPENDS ON WHAT THE TIMELINE IS DRAWING, and that is the point
         * of the card now: over ONE fight it flags `Engage` on the fight's own first second,
         * and over a scope of many there is no single engagement to flag, so it marks the
         * deaths instead. This capture's scope holds three fights and one death in it.
         */
        assert!(
            said.iter().any(|s| s == "Engage" || s.ends_with(" died")),
            "the timeline draws no flag at all, so the chart chrome is not painting: {said:?}"
        );
        /* THE CLOCK ALONG THE X AXIS IS THE WALL CLOCK OVER A SCOPE and the fight's own clock
         * over one fight, so this looks for either shape rather than for `00:00`. */
        assert!(
            said.iter().any(|s| {
                let d: Vec<char> = s.chars().collect();
                d.len() == 5 && d[2] == ':' && d.iter().filter(|c| c.is_ascii_digit()).count() == 4
            }),
            "the timeline has no clock along its x axis: {said:?}"
        );
        /* AND IT SAYS WHAT IT IS DRAWING: a span of fights with the wall clock ends of it, or
         * one fight with its name. `newest of N with a reading` is gone with the behaviour it
         * described, which was a card that ignored the filter above it. */
        assert!(
            said.iter().any(|s| s == "11 fights"),
            "the timeline does not say how much of the scope it drew: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s.contains("with a reading")),
            "the timeline still says it picked one fight out of the scope: {said:?}"
        );
        assert!(
            said.iter().filter(|s| *s == "0").count() >= 1,
            "no y axis tick label reached the page: {said:?}"
        );
        /* THE STRIP: five real figures, and none of the invented ones. */
        for label in ["SCOPE", "GROUP DPS", "IN COMBAT", "FIGHTS", "DEATHS"] {
            assert!(
                said.iter().any(|s| s == label),
                "the summary strip is missing {label}: {said:?}"
            );
        }
        for invented in ["GROUP PRESSURE", "THREAT", "OVERHEAL", "PRIOR COMPARABLE"] {
            assert!(
                !said.iter().any(|s| s.to_uppercase().contains(invented)),
                "{invented} reached the page. Nothing in a log line states it, so this app is \
                 making a number up on the owner's stream: {said:?}"
            );
        }
        /* THE ROSTER'S OWN COLUMNS AND FOOT.
         *
         * EITHER SPELLING OF THE FIRST TWO, because a heading is written to the width its column
         * actually got: `roster_plan` measures the name column against the names in it, so a
         * roster of `You` and `Omny` gets `COMBATANT` and a wide one gets the whole phrase, and
         * the share column says `%` where the word will not fit. What this is guarding is that
         * the COLUMN is on the page, not which of its two names it went out under. */
        for col in [
            &["COMBATANT \u{b7} CLASS TRIO", "COMBATANT"][..],
            &["RELATIVE"][..],
            &["SHARE", "%"][..],
        ] {
            assert!(
                col.iter().any(|c| said.iter().any(|s| s == c)),
                "the roster's {col:?} column is not on the page: {said:?}"
            );
        }
        assert!(
            said.iter().any(|s| s == "Group total"),
            "no roster foot: {said:?}"
        );
        /* AND THE ROSTER'S SUB SAYS WHOSE ROWS THOSE ARE, on the card the owner looks at.
         *
         * THE FORM AND NOT ONE OF THE THREE ANSWERS, because which answer this scope earns is the
         * group engine's measurement and not this page's: it depends on which of the capture's
         * fights the store holds and on the removal line at 23:21:54. What this page owes is that
         * the answer, whichever it is, reached the head of the card in the sub that already says
         * `in scope`. `card_sub_says_whose_rows_a_roster_draws` pins which words go with which
         * answer. A card drawing `Tile::sub` bare would print `in scope` alone and fail here. */
        let captions = ["your group", "everyone in range, group not known", "solo"];
        assert!(
            said.iter().any(|s| s
                .strip_prefix("in scope \u{b7} ")
                .is_some_and(|rest| captions.contains(&rest))),
            "no roster card says whether its rows are the reader's group or everyone in range: \
             {said:?}"
        );

        /* DEFECT: THE STRIP STAIRCASED AND THE ROSTER FOOT ESCAPED ITS TILE. Both were only
         * visible as POSITIONS, which a list of strings cannot see, so the strip's five labels
         * must share one baseline and the foot must sit inside the Damage tile, under its head
         * and above the tile's bottom. `placed_at` reads the same ink the string checks read. */
        let at = placed_at(&ctx, &mut ing, &[]);
        /* THE TOPMOST INSTANCE, because the Live tile prints its own `IN COMBAT` under the strip
         * and a first-match lookup read that one. */
        let y_of = |label: &str| -> f32 {
            at.iter()
                .filter(|(s, _)| s == label)
                .map(|(_, p)| p.y)
                .fold(f32::MAX, f32::min)
        };
        for l in [
            "SCOPE",
            "GROUP DPS",
            "IN COMBAT",
            "FIGHTS",
            "DEATHS",
            "Damage",
        ] {
            assert!(y_of(l) < f32::MAX, "{l} is not on the page");
        }
        let strip: Vec<f32> = ["SCOPE", "GROUP DPS", "IN COMBAT", "FIGHTS", "DEATHS"]
            .iter()
            .map(|l| y_of(l))
            .collect();
        let (lo, hi) = strip
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
        assert!(
            hi - lo < 1.0,
            "the summary strip staircases: its labels sit at {strip:?}"
        );
        /* AND THE FOOT IS MEASURED AGAINST THE CARD THE PAGE ACTUALLY DREW.
         *
         * This multiplied the tile's row count by `ROW_UNIT` to work out where its bottom was.
         * A row is a share of the viewport now (see [`Size`]), so a tile's height is its rows
         * times whatever pitch the window bought, and a constant is the one thing it is not.
         * `dash_at` keeps the boxes as well as the words, so the card is asked rather than
         * calculated, and `the_card` is the same lookup every other fit guard on this page uses.
         *
         * IT ALSO SUBSUMES `the foot is drawn up in the summary strip`: the strip is above the
         * grid, so a foot below this card's own head bar is below the strip by construction. */
        let (rects, text) = dash_at(&ctx, &mut ing, 1000.0);
        let head = text
            .iter()
            .filter(|(s, _)| s == Tile::Damage.label())
            .map(|(_, r)| *r)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("the Damage card never drew its own heading");
        let card = the_card(&rects, head, Tile::Damage);
        let foot = text
            .iter()
            .filter(|(s, _)| s == "Group total")
            .map(|(_, r)| r.top())
            .fold(f32::MAX, f32::min);
        assert!(
            foot > card.top() + 40.0 && foot < card.bottom(),
            "the roster foot is at y={foot}, outside the Damage card ({}..{}); it has escaped \
             into the page",
            card.top(),
            card.bottom()
        );
        /* THE EIGHT ROLES STAY GONE. */
        for word in ["Raid Leader", "Healer", "Tank", "Pet", "Solo", "Custom"] {
            assert!(
                !said.iter().any(|s| s == word),
                "a role tab is back: {word}"
            );
        }

        /* ---- the charts alone: people, and no mob, and no fourth fight ---- */

        let charts = painted(&mut ing, &[Tile::Damage, Tile::Taken]);
        let players = ["You", "Tanefilo", "Poguhy", "Fylasem", "Rykabe", "Tanefi"];
        assert!(
            players.iter().any(|p| charts.iter().any(|s| s == p)),
            "no player from the capture's closed fights is on the damage roster, so the page is \
             not reading the store: {charts:?}"
        );
        for mob in [
            "a dry bone skeleton",
            "A tormented dead",
            "Guard Ullindin",
            "a skeleton",
        ] {
            assert!(
                !charts.iter().any(|s| s == mob),
                "a mob is on the chart of people: {mob}: {charts:?}"
            );
        }
        assert!(
            !charts.iter().any(|s| s.contains("Guard V`Lex")),
            "Guard V`Lex is only in the OPEN fourth fight, so its presence means the page is \
             reading the live fold and not the store: {charts:?}"
        );
        assert!(
            !charts.iter().any(|s| s.contains("eqlog_Reviir")),
            "a log file name on a page of charts: {charts:?}"
        );

        /* ---- AND NO CARD NAMES THE LOG FILE. The owner's verdict on the log tile was that it
         * is a stupid widget; the file is the Logs page's subject, not a dashboard's. ---- */
        assert!(
            !said.iter().any(|s| s.contains("eqlog_Reviir")),
            "a log file name is on the dashboard again: {said:?}"
        );

        /* ---- and a folder with no log in it at all ---- */

        let empty = crate::fights::probe::logs_dir("screen-dashboards-empty");
        let mut none = crate::fights::probe::booted(&empty);
        assert!(none.fights().is_empty(), "there is no log in that folder");
        let said = painted(&mut none, &[]);
        assert!(
            !said.is_empty(),
            "a dashboard with no log to read painted nothing"
        );
        let tight = said.concat().to_lowercase();
        for lie in ["not built", "no combat engine", "coming soon"] {
            assert!(
                !tight.contains(lie),
                "with no log to read, the page blamed the build ({lie:?}): {said:?}"
            );
        }
        assert!(
            !said.iter().any(|s| s.contains("skeleton")),
            "the empty page painted a fight out of a folder with no log in it: {said:?}"
        );
    }

    /// THE DASHBOARD COUNTS THE READER'S OWN DEATHS AND NOBODY ELSE'S.
    ///
    /// The owner read `1 Gartik` in the Deaths cell, a group member's death, and said this is a
    /// personal dashboard and not a guild's or a raid's. Each fight kills everyone an older rule
    /// counted beside the reader: a group member, a pet, and in a fight whose group is not known a
    /// player, so a rule any wider than the reader's own shows as a larger count.
    ///
    /// WHAT MUTATION MAKES THIS RED: the count asking the roster (`FightRow::ours`) or
    /// `FightRow::player`; the caption naming anything but the reader's last killer; a last death
    /// with no killer keeping an older killer's name; the fights list counting some other way.
    #[test]
    fn the_dashboard_counts_the_readers_own_deaths_and_nobody_elses() {
        let death = |killer: usize, victim: usize| crate::fights::Moment {
            at: 1,
            what: crate::fights::Mark::Death { killer, victim },
        };
        let fighter = |who: Who| Fighter {
            who,
            ..Default::default()
        };
        let grouped = FightRow {
            secs: 30,
            start: "Wed Jul 15 23:30:00 2026".to_owned(),
            end: "Wed Jul 15 23:30:30 2026".to_owned(),
            group: Some(vec!["Gartik".to_owned()]),
            pets: vec!["Xabn".to_owned()],
            fighters: vec![
                fighter(Who::You),
                fighter(Who::Named("Gartik".to_owned())),
                fighter(Who::Named("Xabn".to_owned())),
                fighter(Who::Named("a dry bone skeleton".to_owned())),
            ],
            moments: vec![death(3, 1), death(3, 2), death(3, 0)],
            ..Default::default()
        };
        let unknown = FightRow {
            group: None,
            pets: Vec::new(),
            ..grouped.clone()
        };
        let others_only = FightRow {
            moments: vec![death(3, 1), death(3, 2)],
            ..grouped.clone()
        };
        let no_killer = FightRow {
            moments: vec![death(9, 0)],
            ..grouped.clone()
        };

        assert_eq!(
            your_deaths(&grouped),
            1,
            "a group member's or a pet's death was counted as the reader's"
        );
        assert_eq!(
            your_deaths(&unknown),
            1,
            "where the group is not known, another player's death was counted as the reader's"
        );
        assert_eq!(
            your_deaths(&others_only),
            0,
            "the reader did not die and the fight shows a death"
        );

        let strip = |fights: &[FightRow]| -> (String, Option<String>) {
            strip_cells(&crate::screens::night::Filter::default(), None, fights)
                .into_iter()
                .find(|s| s.label == "Deaths")
                .map(|s| (s.value, s.small))
                .expect("the strip has a Deaths cell")
        };
        assert_eq!(
            strip(&[grouped.clone(), unknown, others_only.clone()]),
            ("2".to_owned(), Some("by a dry bone skeleton".to_owned())),
            "the Deaths cell counted somebody other than the reader, or named the wrong thing \
             beside the count"
        );
        assert_eq!(
            strip(&[others_only]),
            ("0".to_owned(), None),
            "the reader never died and the Deaths cell says somebody did"
        );
        assert_eq!(
            strip(&[grouped, no_killer]),
            ("2".to_owned(), None),
            "the reader's last death named no killer and the cell kept an older killer's name"
        );

        let src = include_str!("dashboards.rs");
        let src = &src[..src.find("mod tests {").expect("the test module")];
        assert!(
            src.contains("let dead = your_deaths(f);"),
            "the fights list counts deaths by some rule other than the reader's own"
        );
    }

    /// DEFECT: A CARD WHOSE FIGURES ARE THE READER'S GROUP ON ONE SCOPE AND EVERYONE IN RANGE ON
    /// THE NEXT, HEADED THE SAME WAY BOTH TIMES.
    ///
    /// The three roster tiles and the timeline carry `in scope` in the sub beside their title, and
    /// the Live card carries `the fight going now`; nothing else on any of them says which
    /// population the figures are. `card_sub` adds `dps::whose` there: over the SCOPE's fold for the
    /// scope cards, over the LIVE fight for the Live card, and on no other card.
    ///
    /// THE LIVE FIGHT AND THE FOLD DISAGREE IN EVERY CASE BELOW, so a card that captioned the wrong
    /// one names the wrong group and fails.
    ///
    /// WHAT MUTATION MAKES THIS RED: `card_sub` answering `Tile::sub` bare; a roster tile or the
    /// timeline left out of its arm; the Live card captioned from the fold, or left bare; a scope
    /// card captioned from the live fight; the caption added to a card that has no roster; a caption
    /// on a card with no fight.
    #[test]
    fn card_sub_says_whose_rows_a_roster_draws() {
        let with = |group: Option<Vec<String>>| FightRow {
            group,
            ..Default::default()
        };
        let known = with(Some(vec!["Hert".to_owned()]));
        let unknown = with(None);
        let solo = with(Some(Vec::new()));
        for tile in [Tile::Damage, Tile::Healing, Tile::Taken, Tile::Timeline] {
            let scope = tile.sub().expect("a scope card has a sub");
            for (fold, words) in [
                (&known, "your group"),
                (&unknown, "everyone in range, group not known"),
                (&solo, "solo"),
            ] {
                let live = if words == "solo" { &known } else { &solo };
                assert_eq!(
                    card_sub(tile, Some(fold), Some(live)),
                    Some(format!("{scope} \u{b7} {words}")),
                    "the {tile:?} card over a scope that is {words:?} does not say so, or named \
                     the live fight's group instead"
                );
            }
            assert_eq!(
                card_sub(tile, None, Some(&known)),
                Some(scope.to_owned()),
                "the {tile:?} card has no fight in scope and still captioned a group"
            );
        }
        assert_eq!(
            card_sub(Tile::Live, Some(&known), Some(&solo)).as_deref(),
            Some("live \u{b7} the fight going now \u{b7} solo"),
            "the Live card's count is the fight going now, and its caption named the scope's group \
             or none at all"
        );
        assert_eq!(
            card_sub(Tile::Live, Some(&known), None),
            Tile::Live.sub().map(str::to_owned),
            "nothing is being fought and the Live card still captioned a group"
        );
        for tile in Tile::ALL {
            if matches!(
                tile,
                Tile::Damage | Tile::Healing | Tile::Taken | Tile::Timeline | Tile::Live
            ) {
                continue;
            }
            assert_eq!(
                card_sub(tile, Some(&known), Some(&known)),
                tile.sub().map(str::to_owned),
                "the {tile:?} card draws no roster and was captioned with whose rows it has"
            );
        }
    }

    /// DEFECT: A STRANGER'S DEATH ON ONE FIGHT'S CHART FLAGGED AS A KILL.
    ///
    /// Three cases and not two: somebody on the roster died, a mob was killed, and a player the log
    /// proved was not in the reader's group died beside him, which the chart says nothing about.
    /// Before the group existed every player was on the roster and the third case could not
    /// happen, so `else` meant "a mob"; after it, `else` put `Kill` in gold over a person.
    ///
    /// WHAT MUTATION MAKES THIS RED: `death_flag` flagging `Kill` for every fighter off the roster.
    #[test]
    fn a_death_on_one_fights_chart_is_a_death_a_kill_or_nothing() {
        let f = FightRow {
            group: Some(Vec::new()),
            fighters: vec![
                Fighter {
                    who: Who::You,
                    ..Default::default()
                },
                Fighter {
                    who: Who::Named("Losumyda".to_owned()),
                    ..Default::default()
                },
                Fighter {
                    who: Who::Named("a dry bone skeleton".to_owned()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            death_flag(&f, &f.fighters[0]),
            Some((String::from("You died"), WRONG)),
            "the reader's own death is not flagged as a death"
        );
        assert_eq!(
            death_flag(&f, &f.fighters[2]),
            Some((String::from("Kill"), GOLD)),
            "a mob going down is not flagged as a kill"
        );
        assert_eq!(
            death_flag(&f, &f.fighters[1]),
            None,
            "a player the log proved was not in the reader's group died beside him and the chart \
             flagged it, as a kill or as one of the roster's deaths"
        );
        /* A ONE-WORD MOB THE ROW PROVED IS A MOB GOES DOWN AS A KILL, NOT AS A PERSON. */
        let mut mobbed = f.clone();
        mobbed.fighters.push(Fighter {
            who: Who::Named("Xicotl".to_owned()),
            ..Default::default()
        });
        mobbed.foes = vec!["Xicotl".to_owned()];
        assert_eq!(
            death_flag(&mobbed, &mobbed.fighters[3]),
            Some((String::from("Kill"), GOLD)),
            "a named mob with a one-word name died and the chart flagged it as a person"
        );
        let unknown = FightRow {
            group: None,
            ..f.clone()
        };
        assert_eq!(
            death_flag(&unknown, &unknown.fighters[1]),
            Some((String::from("Losumyda died"), crate::theme::ORANGE)),
            "a player's death was dropped from a fight whose group is not known"
        );
    }

    /// DEFECT: THE READER'S DEATH, A GROUPMATE'S AND A KILL WERE ONE COLOUR, OR TWO.
    ///
    /// The owner asked for the three to look different. Asked of the colours and not of the
    /// words, because the words already differed and that was not enough to read at a glance.
    ///
    /// WHAT MUTATION MAKES THIS RED: `death_tint` returning one colour for everybody, or the
    /// scope timeline flagging with a fixed colour instead of asking it.
    #[test]
    fn a_death_is_coloured_by_whose_it_was() {
        let you = death_tint(&Who::You);
        let them = death_tint(&Who::Named("Losumyda".to_owned()));
        assert_ne!(
            you, them,
            "the reader's death and a groupmate's are one colour"
        );
        assert_ne!(you, GOLD, "the reader's death is the colour of a kill");
        assert_ne!(them, GOLD, "a groupmate's death is the colour of a kill");

        /* THE SCOPE TIMELINE ASKS THE SAME QUESTION, and it is the chart the owner was reading. */
        let body = include_str!("dashboards.rs");
        let scope = &body[body.find("fn whole_scope(").expect("whole_scope")..];
        let scope = &scope[..scope.find("\n}").expect("its end")];
        assert!(
            scope.contains("death_tint(&x.who)"),
            "the scope timeline flags deaths in a colour of its own choosing"
        );
    }

    /// DEFECT: WITH ALL FIGHTS PICKED, THE HEAD NAMED ONE MOB.
    ///
    /// The owner had ALL selected in the menu and the head read `A WILL SAPPER`, which is what a
    /// head reads when that one fight is picked. A control says what it is set to.
    ///
    /// WHAT MUTATION MAKES THIS RED: `encounter_head` naming the scope's headline on no pick, or
    /// naming `ALL_FIGHTS` when a fight IS picked.
    #[test]
    fn the_head_says_all_fights_until_one_fight_is_picked() {
        let ctx = prepared_dash();
        let mut ing = booted_stored("head-all");
        let names: Vec<String> = ing
            .history()
            .iter()
            .filter_map(|f| f.headline.as_deref())
            .map(str::to_uppercase)
            .collect();
        assert!(
            !names.is_empty(),
            "the capture stored no named fight to pick"
        );

        let mut screen = DashboardsScreen::default();
        let all = painted_dash(&ctx, &mut ing, &mut screen);
        assert!(
            all.iter().any(|s| s == &ALL_FIGHTS.to_uppercase()),
            "with no fight picked the head does not say all fights: {all:?}"
        );
        assert!(
            !names.iter().any(|n| all.iter().any(|s| s == n)),
            "with no fight picked the head still names one fight: {all:?}"
        );

        let one = ing.history().last().expect("a fight").clone();
        screen.pick = Some(one.start.clone());
        let picked = painted_dash(&ctx, &mut ing, &mut screen);
        let want = one.headline.as_deref().unwrap_or(UNNAMED).to_uppercase();
        assert!(
            picked.iter().any(|s| s == &want),
            "a picked fight's head does not name it: {picked:?}"
        );
        assert!(
            !picked.iter().any(|s| s == &ALL_FIGHTS.to_uppercase()),
            "a picked fight's head still says all fights"
        );
    }

    /// DEFECT: THE SCOPE TIMELINE DRAWING A STRANGER'S DAMAGE THE ROSTER BESIDE IT LEAVES OFF.
    ///
    /// `group_seconds` is one fight's line on the scope timeline, and it has to sum exactly the
    /// people `reports::roll` merged into the roster card: that fight's own roster when the scope's
    /// group is known, every player when it is not. A mob is on neither.
    ///
    /// WHAT MUTATION MAKES THIS RED: `group_seconds` filtering on `Who::player` again; asking
    /// `FightRow::ours` whatever `known` says.
    #[test]
    fn the_scope_timeline_sums_the_scopes_roster_second_by_second() {
        let s = |who: Who, pairs: &[(u32, u64)]| Fighter {
            who,
            series: pairs.to_vec(),
            ..Default::default()
        };
        let f = FightRow {
            group: Some(Vec::new()),
            fighters: vec![
                s(Who::You, &[(0, 10), (1, 5)]),
                s(Who::Named("Losumyda".to_owned()), &[(0, 99)]),
                s(Who::Named("a dry bone skeleton".to_owned()), &[(0, 7)]),
            ],
            ..Default::default()
        };
        assert_eq!(
            group_seconds(true, &f).into_iter().collect::<Vec<_>>(),
            vec![(0, 10), (1, 5)],
            "over a scope whose group is known, a stranger or a mob reached the line"
        );
        assert_eq!(
            group_seconds(false, &f).into_iter().collect::<Vec<_>>(),
            vec![(0, 109), (1, 5)],
            "over a scope whose group is not known, a player was left off the line, or a mob was put on it"
        );
    }

    /// DEFECT: THE STRIP'S HOVER DESCRIBING EVERY PLAYER OVER FIGURES THAT COUNT THE ROSTER.
    ///
    /// `Group DPS` sums `ranked_dealers` over the fold, and its hover said every player in these
    /// fights whoever the fold counted. The Deaths cell is the reader's own on every scope, and its
    /// hover says that on every scope.
    ///
    /// WHAT MUTATION MAKES THIS RED: `strip_population` answering `Everyone` whatever the fold's
    /// group is; the `Solo` and `Group` words swapped; the Deaths hover describing a roster.
    #[test]
    fn the_strips_hover_names_whose_figures_its_cells_count() {
        let row = |group: Option<Vec<String>>| FightRow {
            secs: 30,
            group,
            fighters: vec![Fighter {
                who: Who::You,
                dealt: 100,
                ..Default::default()
            }],
            ..Default::default()
        };
        let why = |fold: &FightRow, label: &str| -> &'static str {
            strip_cells(
                &crate::screens::night::Filter::default(),
                Some(fold),
                std::slice::from_ref(fold),
            )
            .into_iter()
            .find(|s| s.label == label)
            .map(|s| s.why)
            .expect("the strip draws this cell")
        };
        {
            let label = "Group DPS";
            let everyone = why(&row(None), label);
            let solo = why(&row(Some(Vec::new())), label);
            let group = why(&row(Some(vec!["Hert".to_owned()])), label);
            assert!(
                everyone.contains("layers"),
                "the {label} cell over a scope whose group is not known no longer says it counts \
                 players: {everyone}"
            );
            assert!(
                solo.contains("proved you solo"),
                "the {label} cell over a solo scope does not say it counts the reader alone: {solo}"
            );
            assert!(
                group.contains("your group in that fight"),
                "the {label} cell over a known group does not say it counts that group: {group}"
            );
        }
        /* THE DEATHS CELL IS THE READER'S OWN WHATEVER THE GROUP: see `your_deaths`. */
        for group in [None, Some(Vec::new()), Some(vec!["Hert".to_owned()])] {
            let words = why(&row(group), "Deaths");
            assert!(
                words.starts_with("Your own deaths") && words.contains("your group"),
                "the Deaths cell's hover does not say it counts the reader alone: {words}"
            );
        }
    }

    /// DEFECT: THE EMPTY SCOPE TIMELINE BLAMING THE LOG, AND A ROW'S COUNT WITH NOTHING SAYING WHOSE.
    ///
    /// Over a known scope the timeline is the roster's, so `the log recorded none` is false when a
    /// stranger dealt damage; and each row of a fights list counts its own fight's roster under one
    /// heading, so its hover has to tell the three answers apart.
    ///
    /// WHAT MUTATION MAKES THIS RED: `no_timeline_why` ignoring `known`; `row_population` answering
    /// one sentence for every group.
    #[test]
    fn empty_timelines_and_fight_rows_say_whose_figures_they_are() {
        assert!(
            !no_timeline_why(true).contains("recorded none"),
            "over a known scope the empty timeline says the log recorded no damage: {}",
            no_timeline_why(true)
        );
        assert!(no_timeline_why(false).contains("recorded none"));
        let with = |group: Option<Vec<String>>| FightRow {
            group,
            ..Default::default()
        };
        let said = [
            crate::screens::dps::row_population(&with(None)),
            crate::screens::dps::row_population(&with(Some(Vec::new()))),
            crate::screens::dps::row_population(&with(Some(vec!["Hert".to_owned()]))),
        ];
        assert!(
            said[0].contains("every player"),
            "a fight row whose group is not known does not say it counts every player: {}",
            said[0]
        );
        assert!(
            said[1].contains("solo"),
            "a solo fight row's hover does not say its figures are the solo reader's: {}",
            said[1]
        );
        assert!(
            said[2].contains("your group"),
            "a fight row with a known group does not say its figures are that group's: {}",
            said[2]
        );
    }

    /// DEFECT: `0 players named` OVER A FIGHT THE LOG NAMES A PLAYER IN.
    ///
    /// The Live card's head count is the roster's (`players_in`), and on a filtered roster the log
    /// names more people than that. The capture's last fight is solo with `Losumyda` in it, and the
    /// card said `0 players named`.
    ///
    /// WHAT MUTATION MAKES THIS RED: `roster_count` saying `players named` whatever the group is.
    #[test]
    fn the_live_cards_head_count_says_it_is_the_rosters_when_the_roster_is_filtered() {
        let fighters = vec![
            Fighter {
                who: Who::You,
                ..Default::default()
            },
            Fighter {
                who: Who::Named("Losumyda".to_owned()),
                ..Default::default()
            },
        ];
        let solo = FightRow {
            group: Some(Vec::new()),
            fighters: fighters.clone(),
            ..Default::default()
        };
        assert_eq!(
            roster_count(&solo),
            "1 on your roster",
            "a solo roster's count was printed as a count of the players the log named"
        );
        let unknown = FightRow {
            group: None,
            fighters,
            ..Default::default()
        };
        assert_eq!(roster_count(&unknown), "2 players named");
    }

    /// THE PLAYER COUNT IS PLAYERS AND NOT PARTICIPANTS.
    ///
    /// WHAT MUTATION MAKES THIS RED: counting `participants`, or dropping the space rule.
    #[test]
    fn the_player_count_counts_players() {
        let f = |names: &[&str]| -> FightRow {
            FightRow {
                fighters: names
                    .iter()
                    .map(|n| Fighter {
                        who: Who::Named((*n).to_owned()),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }
        };
        assert_eq!(players_in(&f(&["Reviir", "a spite golem", "Losumyda"])), 2);
        assert_eq!(players_in(&f(&["a thunder spirit princess"])), 0);
        assert_eq!(players_in(&f(&[])), 0);
    }

    /// DEFECT: TWO WINDOWS, ONE FIGHT, TWO DIFFERENT MOBS NAMED MOST PROMINENTLY.
    ///
    /// # WHAT A READER SAW
    ///
    /// The Live page and this dashboard read the SAME `FightRow` and both open with the same
    /// `IN COMBAT` mark. Live then printed `FightRow::current_target` in its big gold slot with
    /// that target's own clock; this card printed `FightRow::headline` in ITS big gold slot with
    /// `FightRow::secs`, the whole chain's clock. On a long pull those are different mobs and
    /// different times, so the two surfaces named different things as what is being fought, next
    /// to each other, on a live stream. Neither figure was wrong. The disagreement was.
    ///
    /// # THE FIXTURE IS THE LIVE PAGE'S OWN, AND IT HAS TO DISCRIMINATE
    ///
    /// A golem that took the most damage and stopped being hit early, a spawn that is still being
    /// worked, and an add that arrived after the spawn and stopped before it. Biggest says the
    /// golem, newest-first-hit says the add, and only newest-last-hit (which is what
    /// `current_target` measures) says the spawn. A fixture where the two rules agree proves
    /// nothing at all, which is why this one is copied from the page that already argued it.
    ///
    /// # IT READS THE FONT SIZE AND NOT ONLY THE TEXT
    ///
    /// The old card painted BOTH names: the headline large and the target small under an `on`. A
    /// test that only asked whether the target appears was green on the defect. The claim being
    /// made here is about which name is drawn MOST PROMINENTLY, so the assertion is over the
    /// largest text on the card.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `f.headline` back in the big slot, putting `f.secs`
    /// back beside the name, or dropping the words that label the chain's own clock.
    #[test]
    fn the_live_card_names_what_the_live_page_names() {
        use crate::fights::{FightRow, Fighter, Who};

        let mob = |name: &str, taken: u64, first: u32, last: u32| Fighter {
            who: Who::Named(name.into()),
            taken,
            first_taken_at: Some(first),
            last_taken_at: Some(last),
            ..Fighter::default()
        };
        let f = FightRow {
            secs: 486,
            headline: Some("a spite golem".into()),
            fighters: vec![
                mob("a spite golem", 90_000, 0, 120),
                mob("a spite spawn", 500, 100, 480),
                mob("a spite add", 400, 470, 475),
                Fighter {
                    who: Who::You,
                    dealt: 50_000,
                    taken: 900,
                    first_taken_at: Some(10),
                    last_taken_at: Some(485),
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };

        /* THE RULE, BEFORE THE PAINT. `live_subject` is what both surfaces are meant to ask, so it
         * is checked on its own first: a card that painted the right name by some other route
         * would leave the two surfaces free to drift again on the next fight shape. */
        assert_eq!(
            live_subject(&f),
            LiveSubject::Fighting {
                who: "a spite spawn",
                secs: 380
            },
            "the subject rule named the biggest thing in the chain rather than the newest"
        );

        /* AND THE CARD ITSELF, DRAWN HEADLESS. `Shape::Vec` nests, so the walk is a stack; the
         * font size comes off the layout job's first section, which is where `RichText::font`
         * puts it. */
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, 300.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| live_body(ui, &mut cx, Some(&f), true));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut said: Vec<(String, f32)> = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => {
                    let size = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map(|s| s.format.font_id.size)
                        .unwrap_or(0.0);
                    said.push((t.galley.text().to_owned(), size));
                }
                _ => {}
            }
        }
        assert!(!said.is_empty(), "the live card painted nothing at all");
        let flat: Vec<&str> = said.iter().map(|(s, _)| s.as_str()).collect();

        /* THE BIGGEST TEXT ON THE CARD IS THE SUBJECT. */
        let biggest = said
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("something was painted");
        assert_eq!(
            biggest.0, "a spite spawn",
            "the most prominent name on the live card is not what is being fought: {said:?}"
        );

        /* AND THE THING IT USED TO NAME IS NOT ON THE CARD AT ALL. This is the assertion the old
         * code fails: it painted the golem large and the spawn small, so a test that only looked
         * for the spawn would have passed on the defect. */
        assert!(
            !flat.contains(&"a spite golem"),
            "the chain's biggest damage sponge is still named on the live card: {said:?}"
        );

        /* THE CLOCK BESIDE THE NAME IS THAT TARGET'S OWN, and the chain's is still drawn with the
         * words that say what it measures. 380 seconds is the spawn; 486 is the whole run. */
        assert!(
            flat.contains(&"06:20"),
            "the target's own clock is missing: {said:?}"
        );
        assert!(
            flat.contains(&"in combat for") && flat.contains(&"08:06"),
            "the chain's duration is unlabelled or absent, so 08:06 reads as this mob's age: \
             {said:?}"
        );
    }

    /// THE OTHER TWO SUBJECTS, WHICH ARE REAL STATES AND NOT FALLBACKS.
    ///
    /// A fight stays open for a quiet window after the last blow, so for half a minute after a
    /// kill there is a live fight with nothing alive in it. Naming the corpse in the same gold as
    /// a live target would say the reader is still fighting it.
    ///
    /// WHAT MUTATION MAKES THIS RED: collapsing `Slain` into `Label`, or reaching for the headline
    /// before the last kill.
    #[test]
    fn a_run_with_nothing_alive_in_it_names_the_kill_and_then_the_label() {
        use crate::fights::{FightRow, Fighter, Mark, Moment, Who};

        /* A mob that was hit and then died. `last_slain` reads the death marks and refuses
         * players, so the victim has to be a real fighter slot. */
        let killed = FightRow {
            secs: 60,
            headline: Some("a spite golem".into()),
            fighters: vec![Fighter {
                who: Who::Named("a spite golem".into()),
                taken: 90_000,
                first_taken_at: Some(0),
                last_taken_at: Some(40),
                ..Fighter::default()
            }],
            moments: vec![Moment {
                at: 40,
                what: Mark::Death {
                    killer: 0,
                    victim: 0,
                },
            }],
            ..FightRow::default()
        };
        assert_eq!(
            live_subject(&killed),
            LiveSubject::Slain {
                who: "a spite golem"
            },
            "a corpse was named as the thing being fought"
        );

        /* NOTHING NAMED WAS HIT OR KILLED AT ALL: the fight's own label is the honest answer, and
         * a fight with no named entity in it has no label either. */
        let only_players = FightRow {
            headline: None,
            fighters: vec![Fighter {
                who: Who::You,
                taken: 1_700,
                first_taken_at: Some(0),
                last_taken_at: Some(30),
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        assert_eq!(
            live_subject(&only_players),
            LiveSubject::Label { text: UNNAMED },
            "a fight with nothing named in it invented a subject"
        );
    }

    /* ======================================= a name is not a mob == */

    /// THREE CLEAN KILLS OF ONE MOB AND A FIGHT AFTER THEM TO CLOSE THE LAST.
    ///
    /// THE TRAILING BEETLE IS NOT DECORATION. `Ingest::keep_fights` never stores the LAST row of a
    /// tail read, because from the end of a file still being appended to a finished fight and an
    /// open one look identical. Without a fourth fight the third skeleton never reaches the store,
    /// the book has two samples, `Reading::expect` refuses on two, and every assertion below would
    /// be testing the "no measure yet" arm by accident.
    ///
    /// 1000, 1020 AND 1040 SETTLE. The spread is four percent of the median, inside `TIGHT_PERCENT`
    /// and over `ENOUGH`, so `expect` answers 1,020 and the strip has a denominator to misuse.
    const THREE_AGREEING_KILLS: &str = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 1000 points of damage.\n",
        "[Wed Jul 15 23:16:51 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:20:00 2026] You slash a dry bone skeleton for 1020 points of damage.\n",
        "[Wed Jul 15 23:20:01 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:24:00 2026] You slash a dry bone skeleton for 1040 points of damage.\n",
        "[Wed Jul 15 23:24:01 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:40:00 2026] You slash a fire beetle for 3 points of damage.\n",
    );

    /// An `Ingest` whose hit point book has actually been built, off a store in a temp folder.
    ///
    /// THE STORE IS HANDED OVER BEFORE THE BOOTSTRAP LANDS, which `probe::booted` cannot do:
    /// `keep_fights` is what fills the book and it runs inside that landing, so an ingest given a
    /// store afterwards has already thrown the bootstrap's fights away and its book is empty.
    ///
    /// NOTHING HERE GOES NEAR THE OWNER'S OWN FOLDER. `Store::at` is the only constructor used and
    /// its root is a temp path this function made; `Store::app_data` is never called.
    fn booted_with_book(tag: &str, text: &str) -> crate::ingest::Ingest {
        let dir = crate::fights::probe::planted(tag, text);
        let root = std::env::temp_dir().join(format!("grimoire-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let settings = crate::settings::Settings {
            log_dir: Some(dir.clone()),
            data_root: Some(dir),
            ..crate::settings::Settings::default()
        };
        let mut ig = crate::ingest::Ingest::new(&settings);
        ig.use_store(Some(crate::store::Store::at(root)));
        /* Ten seconds is a ceiling and not a measurement: a bootstrap over these few lines is
         * instant. It is here so a broken scan fails with a sentence rather than hanging CI. */
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                return ig;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the bootstrap never landed for the {tag} fixture");
    }

    /// One skeleton row holding `taken` damage, with `deaths` of them already buried.
    ///
    /// THE SHAPE IS THE ONE `FightRow::current_target` LETS THROUGH: the last hit lands strictly
    /// after the death mark, which is that function's rule for "one of them is up again". A fixture
    /// where the mob is simply dead would be refused earlier by a different rule and would prove
    /// nothing about this one.
    fn repeat_pull(taken: u64, deaths: usize) -> FightRow {
        use crate::fights::{Mark, Moment};
        FightRow {
            secs: 300,
            headline: Some("a dry bone skeleton".into()),
            fighters: vec![
                Fighter {
                    who: Who::You,
                    dealt: taken,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("a dry bone skeleton".into()),
                    taken,
                    first_taken_at: Some(0),
                    last_taken_at: Some(200),
                    ..Fighter::default()
                },
            ],
            moments: (0..deaths)
                .map(|i| Moment {
                    at: 100 + i as u32,
                    what: Mark::Death {
                        killer: 0,
                        victim: 1,
                    },
                })
                .collect(),
            ..FightRow::default()
        }
    }

    /// DEFECT: THE FIGURE AND THE COUNT BESIDE IT CAME OFF TWO DIFFERENT BRANCHES.
    ///
    /// The live tile read the figure from `hp::Reading::expect` and the count from
    /// `Reading::modal`, which are two different groups whenever the samples are SETTLED: `expect`
    /// is the median of every sample, `modal` is the densest agreeing window inside them. The
    /// fixture is `hp`'s own, chosen because it tells the two apart: 100 to 111 is nine percent of
    /// the median 111 (so the whole set settles) and eleven percent of the low 100 (so the densest
    /// window holds five of the six). The old line printed `5 kills of 6 agreed on ~111` where the
    /// 111 was measured off all six.
    ///
    /// AND THE OTHER HALF OF THE RULE, which is the one the summary strip needed: a run that has
    /// already buried one of these gets no comparison at all, whatever the book says. The fold
    /// keys a fighter by NAME, so the damage is more than one mob's; `hp::fold_into` throws away
    /// any fight where a name died twice, so the reading is exactly one mob's. Dividing one by the
    /// other is this app inventing a number.
    ///
    /// WHAT MUTATION MAKES THIS RED: taking the count from `modal()` beside a figure from
    /// `expect()`, or dropping the `buried > 0` refusal.
    #[test]
    fn the_comparison_is_refused_unless_the_figure_and_its_count_were_measured_together() {
        let settled = crate::hp::Reading {
            samples: vec![100, 110, 110, 111, 111, 111],
            ..crate::hp::Reading::default()
        };
        assert!(
            settled.settled() && settled.modal() == Some((111, 5)),
            "the fixture no longer tells the two statistics apart, so this test proves nothing"
        );
        assert_eq!(
            measured_against(Some(&settled), 0),
            Some((111, 6, 6)),
            "the count beside the figure belongs to a group the reader is not being shown"
        );

        /* THE BURIAL REFUSES THE WHOLE THING, and it refuses it for a mob the book CAN answer
         * for, which is the only case where the refusal costs anything. */
        assert_eq!(
            measured_against(Some(&settled), 1),
            None,
            "a run that has buried one of these still got a one-mob denominator for a two-mob \
             numerator"
        );

        /* AND A BOOK WITH NOTHING USABLE IN IT IS STILL NOTHING, so the new refusal is not quietly
         * doing the reading's own job for it. */
        assert_eq!(measured_against(None, 0), None);
        let thin = crate::hp::Reading {
            samples: vec![900],
            ..crate::hp::Reading::default()
        };
        assert_eq!(
            measured_against(Some(&thin), 0),
            None,
            "one kill is not a measurement"
        );
    }

    /* THE STRIP'S `Target health` TEST STOOD HERE AND IS GONE WITH THE CELL. The dashboard looks
     * back over a scope now and a target's health is a LIVE figure with no meaning over a night,
     * so the strip does not carry it. The rule it guarded (no percentage that cannot be stood
     * up) lives on in `measured_against`, tested directly above, and in the Live tile's own
     * test below, which draws the same figure off the same function. */

    /// DEFECT: THE LIVE TILE'S CLOCK AND HEALTH BAR BOTH DESCRIBED A MOB THAT WAS ALREADY LOOTED.
    ///
    /// The Live page was fixed for this and the dashboard tile draws the same subject off the same
    /// row, so it carried both halves verbatim: a clock labelled "how long this target has been
    /// under fire" that actually ran from the first hit on the FIRST mob of the name, through the
    /// looting, to now; and `1,600 of ~1,020`, more damage into a target than the target can
    /// absorb, because the numerator counts every mob of the name and the denominator was measured
    /// on one. This is the adjacent-surface miss this tree keeps paying for, so it is asserted on
    /// the surface and not only on the rule.
    ///
    /// THE NAME ITSELF STAYS, and that is asserted too: it IS what is being fought, and a fix that
    /// blanked the tile would have traded a false figure for a missing one.
    ///
    /// WHAT MUTATION MAKES THIS RED: drawing the target clock unconditionally, comparing `taken`
    /// to `hp_of(..)` without the `deaths_of` gate, or dropping the kill count that explains why
    /// the clock is gone.
    #[test]
    fn the_live_tile_refuses_the_clock_and_the_bar_once_one_of_them_is_buried() {
        let mut ing = booted_with_book("dash-live-buried", THREE_AGREEING_KILLS);
        assert_eq!(
            ing.hp_of("a dry bone skeleton").and_then(|r| r.expect()),
            Some(1_020),
            "the fixture's book never settled, so the tile has nothing to be wrong with"
        );

        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ing,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };

        let row = repeat_pull(1_600, 1);
        assert_eq!(
            live_subject(&row),
            LiveSubject::Fighting {
                who: "a dry bone skeleton",
                secs: 200
            },
            "the fixture is not in the state under test: `current_target` refused the row, so \
             nothing below would be about a repeat pull"
        );

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, 300.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| live_body(ui, &mut cx, Some(&row), true));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut said: Vec<String> = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        let flat: Vec<&str> = said.iter().map(|s| s.as_str()).collect();
        assert!(!flat.is_empty(), "the live tile painted nothing at all");

        assert!(
            flat.contains(&"a dry bone skeleton"),
            "the tile stopped naming what is being fought, which is the one thing on it the log \
             does state: {said:?}"
        );
        assert!(
            !flat.iter().any(|s| s.contains("of ~")),
            "the tile still divided two mobs' damage by one mob's measured health: {said:?}"
        );
        assert!(
            flat.contains(&"1,600"),
            "the damage went with the comparison; what went in is always true and has to stay: \
             {said:?}"
        );
        assert!(
            !flat.contains(&"03:20"),
            "the target clock is still drawn, and it spans a dead mob, the looting and this one: \
             {said:?}"
        );
        assert!(
            flat.contains(&"1 killed"),
            "nothing on the tile says why the clock is gone: {said:?}"
        );
        assert!(
            flat.contains(&"in combat for") && flat.contains(&"05:00"),
            "the chain's own clock is a real figure and is labelled, so it stays: {said:?}"
        );
    }

    /// DEFECT: THE MOB TILE'S EMPTY STATE SAID NOBODY HAD KILLED ANYTHING.
    ///
    /// `Ingest::hp_known` used to return the hit point book's LENGTH, which is a row per mob SEEN,
    /// so zero really did mean nothing had died and "No mob has been killed yet." was true. It now
    /// returns the readings that can stand a figure up, which is what the tile's own big number
    /// ("mobs measured") and its hover always claimed. Those two come apart on an ordinary night:
    /// mobs are killed all evening and none of them has three kills that agree until late, and the
    /// old sentence would then have told a stream this character had killed nothing.
    ///
    /// A NEW FALSE SENTENCE PRODUCED BY FIXING A NUMBER is the thing this audit exists to catch,
    /// so the words are asserted rather than left to a reader to notice.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting "killed" back in the empty state's label.
    #[test]
    fn the_mob_tile_says_nothing_is_measured_and_not_that_nothing_has_died() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        /* BOTH ROOTS AT AN EMPTY TEMP FOLDER, and that is not tidiness. `Settings::default` leaves
         * `data_root` as `None`, which sends `Ingest::new` to `data::Snapshot::locate()` and walks
         * the owner's own machine looking for a game install. A test asserting an empty state has
         * no business reading his disk at all. */
        let empty = crate::fights::probe::logs_dir("dash-mobs-empty");
        let mut settings = crate::settings::Settings {
            log_dir: Some(empty.clone()),
            data_root: Some(empty),
            ..crate::settings::Settings::default()
        };
        let mut ing = crate::ingest::Ingest::new(&settings);
        assert_eq!(
            ing.hp_known(),
            0,
            "a fresh ingest has measured nothing, which is the state these words are for"
        );
        let mut cx = Cx {
            data: None,
            railed: true,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ing,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 200.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| mobs_body(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut said: Vec<String> = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        /* THE VERB AND NOT THE SENTENCE. What this test is for is that the empty state talks
         * about MEASURING and never about killing; it used to pin the exact string, and the
         * sentence then had to be shortened to fit a narrower card, which made a wording change
         * look like a regression in meaning. The meaning is what is asserted. */
        assert!(
            said.iter().any(|s| s.to_lowercase().contains("measured")),
            "the empty state does not say what the count it guards actually means: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s.contains("killed")),
            "the tile still tells a stream this character has killed nothing, off a number that \
             counts mobs MEASURED: {said:?}"
        );
    }
}
