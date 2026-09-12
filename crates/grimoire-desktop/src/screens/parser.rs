//! Screen: parser. Decision D9.
//!
//! The tracker, the loot feed and the session strip as one screen, with the fight table and the
//! overlay list beside them:
//!
//!   Kills   the kill tracker: every zone's completion over the wiki roster, ordered by how little
//!           is left, expandable to the roster with killed rows kept in place, checked and struck
//!           through, because what is killed is as much of the answer as what is left.
//!   Loot    the loot feed: newest first, kills interleaved when asked, quantity, who dropped it,
//!           what happened to it.
//!   Fights  the combat engine's own reading of the same file, and this app's own record of the
//!           fights it has read before: what each was against, when it started, how long it ran,
//!           what it cost, and what stopped it.
//!   Analysis one fight, read as deeply as the log supports. The page is `screens::analysis` and
//!           it is a VIEW of this screen, which is why its state is a field on it.
//!   Overlays the owner's combat overlays: what he has, and whether each one is on screen. D11
//!           stage two.
//!
//! THE FIGHTS BODY IS ONE SECTION OF SEVEN AND NOT THE SCREEN. Log Parser is a destination with
//! seven sections, and `nav::SECTIONS` lists them in this order: Dashboards (0), Live (1),
//! Fights (2), Reports (3), Logs (4), Analysis (5), Overlays (6). [`ParserScreen::fights`] draws
//! one of them and nothing else of that list.
//!
//! THIS PARAGRAPH SAID FIVE SECTIONS, IN ANOTHER ORDER, WITH FIGHTS AT INDEX 1. It was written
//! when the list read Live, Fights, Reports, Dashboards, Logs. Dashboards then moved to the top
//! because it is the landing page (`nav.rs` carries that argument), and Analysis and Overlays
//! were added to the end when it turned out the main window could reach neither page at all.
//! Nothing went wrong on screen: the sentence quietly became a map of a rail that no longer
//! exists, and a reader counting to the second section landed on Live.
//!
//! FOUR OF THE SEVEN ARE SCREENS OF THEIR OWN in `nav::SECTIONS`, and `main::draw_screen` hops to
//! them: `screens::dashboards`, `screens::live`, `screens::reports` and `screens::logs`. Drawing
//! a body for one of them HERE would still be wrong, for the same reason it always was: this
//! screen owns a section and the shell decides which. The other three carry no screen id because
//! they are VIEWS of this one, drawn by `ParserScreen::fights`, by `screens::analysis` through
//! the field this screen holds, and by `ParserScreen::overlays`. `main::on_section` turns each of
//! those three section NAMES into a `View` and this screen draws it.
//!
//! THIS PARAGRAPH SAID ALL FOUR WERE ANSWERED BY THE UNBUILT PAGE, which was true when it was
//! written and false the day they landed. None of the four reaches `main::unbuilt` now, and the
//! guard that keeps that honest is `main::the_unbuilt_route_and_the_unbuilt_list_hold_the_same_
//! rows`.
//!
//! THE COMBAT ENGINE EXISTS, AND THIS IS ITS FIRST DESKTOP READER. The Fights body said, for the
//! whole of round one, that the engine "is not built in this release" and pointed at
//! `docs/COMBAT-PARSER.md` as a grammar nothing implemented. That was true when it was written
//! and it is false now: `grimoire_parse::combat` and `grimoire_parse::fights` are ~3,000 lines
//! with 112 tests in this same workspace, they read the reference capture with zero unrecognised
//! lines, and `grimoire-forge`'s `fights` command has been printing four fights and a
//! twenty-three row participant table off `web/fixtures/eqlog-tail-200k.txt` for a while. A
//! screen that keeps saying a thing in the next crate along does not exist is the same class of
//! defect as an invented number, only pointed at ourselves.
//!
//! EVERYTHING THIS BODY DRAWS IS COPIED, NOT COMPUTED. `crate::fights::FightRow` is an OWNED
//! mirror of one `grimoire_parse::fights::Fight`, and it has to be owned: `Fight`, `Participant`
//! and `Entry` all borrow the log text, the desktop parses on a worker thread, and what crosses
//! an mpsc channel must be Send plus static. Nothing borrowed leaves the function that owns the
//! text. Two of that row's fields are copies of a JUDGEMENT and not of a fact: `secs` is
//! `Fight::seconds()`, which floors a span at one second because it uses the log stamps only to the
//! second, and `headline` is `Fight::headline()`, which has a taken-then-dealt rule and a
//! tie-break. Recomputing either here would fork the engine quietly, in the direction of a
//! desktop that disagrees with its own CLI about how long a fight was.
//!
//! WHAT THE FIGHTS BODY DOES NOT DO, so the words on it are not read as a promise:
//!
//!   * NO RATE, ANYWHERE, and this is the load bearing one. `Fight::dps()` divides by that span
//!     floored at one second, and the engine's own test pins 40.0 dps for a fight that begins and
//!     ends inside a single printed second. A dps column would therefore ship the engine's single
//!     largest misreport as the most eye catching number on the page. Rates land when there is a
//!     publishability floor to hide the ones the log cannot support. `no_column_in_this_table_is_a_rate`
//!     is what stops one arriving by accident.
//!   * No per fight detail. The participant table the CLI prints exists and is not drawn here;
//!     a row is a row, and clicking it does nothing, so nothing on it is drawn as clickable.
//!   * No timeline, no graph, no session rollup.
//!
//! Every rule this screen draws with lives in `crate::ingest` or `crate::fights` with its test.
//! What is here is the two orderings (zones by remaining, feed by log time), the fight table's
//! geometry, and paint.
use crate::chrome::{self, State};
use crate::fights::FightRow;
use crate::ingest::{self, Disposition, KillEvent, LootEvent, MobRow, Roster, Summary};
use crate::screens::items::{head_row, list_row, Col};
use crate::screens::Cx;
use crate::theme::*;
use chrono::{DateTime, Utc};
use egui::{Align2, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::HashSet;
use std::time::Duration;

const VIEWS: [&str; 5] = ["Kills", "Loot", "Fights", "Analysis", "Overlays"];
/// The feed shows this many rows.
const FEED_SHOWN: usize = 200;

/// The three views, in tab order. D5 gives Kill tracker, Loot and Fights their own nav rows
/// under PLAY, so the integrator routes those rows here with `ParserScreen::show`; the Parser
/// row opens whichever view was last on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Kills,
    Loot,
    Fights,
    /// One fight, read as deeply as the log supports.
    Analysis,
    /// The owner's combat overlays: what he has, and whether each is on screen. D11 stage two.
    Overlays,
}

pub struct ParserScreen {
    view: usize,
    /// The tracker's zone filter box.
    filter: String,
    /// The feed's text filter box.
    feed_filter: String,
    expanded: HashSet<String>,
    /// Whether kills are interleaved in the loot feed.
    show_kills: bool,
    /// The Analysis page's own state: which fight, which tab, the note being typed.
    ///
    /// OWNED BY THE PARSER SCREEN RATHER THAN BY THE APP, because Analysis is a SECTION of this
    /// destination and not a destination of its own: it comes and goes with the view row above it,
    /// and a person switching to Loot and back expects the fight they were reading.
    analysis: crate::screens::analysis::AnalysisScreen,
    /// WHICH OVERLAY THE BUILDER IS OPEN ON, by id.
    ///
    /// AN ID AND NOT AN INDEX, because the list underneath can be reordered or shortened on the
    /// same frame; an index would then edit whatever moved into that position.
    editing: Option<String>,
    /// THIS CHARACTER'S FIGHTS AS THEY ARE ON DISK. See [`Kept`].
    ///
    /// ON THE SCREEN AND NOT ON THE INGEST, because it is a READ of something the ingest already
    /// owns and writes. `Ingest::store` is the door and `Ingest::keep_fights` is the writer; a
    /// second copy of the history living on the ingest would be a second thing to keep in step
    /// with the disk, and the one place that needs it is this page.
    kept: Kept,
}

impl Default for ParserScreen {
    fn default() -> Self {
        ParserScreen {
            view: 0,
            filter: String::new(),
            feed_filter: String::new(),
            expanded: HashSet::new(),
            analysis: Default::default(),
            editing: None,
            show_kills: true,
            kept: Kept::default(),
        }
    }
}

/* ------------------------------------------------------------ the orderings -- */

/// Zones in tracker order: only zones with a roster, not ignored, name
/// matching the filter (case folded substring), sorted by how many are LEFT ascending, then
/// name. The zone nearest done sits on top because that is the one worth finishing.
pub fn order_zones<'a>(sum: &Summary, roster: &'a Roster, filter: &str) -> Vec<&'a str> {
    let q = filter.trim().to_lowercase();
    let mut v: Vec<(&str, &ingest::ZoneRoster)> = roster
        .zones
        .iter()
        .filter(|(_, z)| !z.mobs.is_empty())
        .filter(|(k, _)| sum.zones.get(*k).is_some_and(|zs| !zs.ignored))
        .filter(|(_, z)| q.is_empty() || z.name.to_lowercase().contains(&q))
        .map(|(k, z)| (k.as_str(), z))
        .collect();
    v.sort_by(|a, b| {
        let left = |k: &str| sum.zones.get(k).map(|z| z.total - z.done).unwrap_or(0);
        left(a.0)
            .cmp(&left(b.0))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    v.into_iter().map(|(k, _)| k).collect()
}

/// The Kills head ratio, or None when there is nothing to take a ratio of.
///
/// NOTHING COUNTED MEANS NO RATIO, AND NO RATIO MEANS NO NUMBER. `done / total` is undefined at
/// total 0, and the head used to paint that undefined ratio as a literal "0%" at 20pt: the largest
/// number on the screen, sitting over "0 of 0 mobs", "0/0 zones cleared" and an empty bar, with
/// "No zones with a roster." underneath it. Nothing computed that 0. It is exactly the `Some(0)`
/// the rest of the app refuses by rule. The count badge that used to state that rule in `chrome`,
/// and the `nav::some_if_counted` that folded a zero into None, are both deleted now (the badges
/// were corpus totals; `nav.rs` has the argument), so this is the surviving statement of it and
/// not an echo of one. A headline is the worst place in the app to break it, because 0% reads as
/// "you have killed none of them",
/// which is a claim about the PLAYER, and the screen is in no position to make it.
///
/// It returns an Option rather than clamping so the caller cannot paint a head at all without a
/// measured denominator; the else arm shows [`nothing_counted`] instead.
pub fn head_pct(sum: &Summary) -> Option<u32> {
    if sum.total == 0 {
        return None;
    }
    Some((100.0 * sum.done as f64 / sum.total as f64).round() as u32)
}

/// Why the Kills head has no ratio, in the words the screen shows.
///
/// TWO CAUSES, TWO SENTENCES, BECAUSE THE FIX DIFFERS. `Roster::load` does not require a non-empty
/// file, so a syntactically valid kills-data.json that lists no zone with mobs in it clears the
/// roster guard and leaves total 0; there the answer is the data. Separately, every zone the roster
/// does list can be excluded by the tracker settings a few rows above the head, which is not an
/// exotic case: `TrackerSettings::default` has the ignore cities box on, so a city-only roster
/// lands here on a fresh install with nothing wrong at all; there the answer is a checkbox. One
/// sentence covering both would send half its readers to the wrong one.
///
/// `sum.zones` is what tells them apart: `ingest::summarize` inserts a row for every zone with mobs
/// in it INCLUDING the ignored ones, and only leaves the ignored out of the totals. So an empty map
/// means the roster had nothing, and a full one with total 0 means the settings took all of it.
pub fn nothing_counted(sum: &Summary) -> &'static str {
    if sum.zones.is_empty() {
        "The roster loaded and lists no zone with mobs in it, so there is nothing to count yet."
    } else {
        "The roster loaded, and the settings above exclude every zone in it, so there is nothing to count yet."
    }
}

/// One feed row, either kind.
#[derive(Clone, Debug, PartialEq)]
pub enum FeedRow<'a> {
    Loot(&'a LootEvent),
    Kill(&'a KillEvent),
}

impl FeedRow<'_> {
    fn ts(&self) -> i64 {
        match self {
            FeedRow::Loot(l) => l.ts,
            FeedRow::Kill(k) => k.ts,
        }
    }

    /// The feed's filter: a kill matches on the mob's name, loot on the item or the mob that
    /// dropped it. The names of quests wanting the item are resolved by the quests screen from
    /// the snapshot, not here, so they are not part of this match. `needle` is already trimmed
    /// and lowercased.
    fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        match self {
            FeedRow::Kill(k) => k.name.to_lowercase().contains(needle),
            FeedRow::Loot(l) => {
                l.item.to_lowercase().contains(needle) || l.mob.to_lowercase().contains(needle)
            }
        }
    }
}

/// The feed, newest first. Emission order is not log order: a slain_by resolves a couple of
/// seconds late by design, so the DISPLAY sorts by log time, ties broken by emission order,
/// which keeps a kill and the loot it produced adjacent instead of letting the late arrival jump
/// the feed. `feed_sorts_by_log_time_newest_first_then_emission_order_and_caps` below pins that.
/// Loot always; kills only when asked; `filter` is a case folded substring over the fields
/// `FeedRow::matches` names.
pub fn feed_rows<'a>(
    loot: &'a [LootEvent],
    kills: &'a [KillEvent],
    show_kills: bool,
    filter: &str,
    cap: usize,
) -> Vec<FeedRow<'a>> {
    let needle = filter.trim().to_lowercase();
    let mut rows: Vec<(FeedRow<'a>, usize)> = Vec::new();
    for (i, l) in loot.iter().enumerate() {
        rows.push((FeedRow::Loot(l), i));
    }
    if show_kills {
        for (i, k) in kills.iter().enumerate() {
            rows.push((FeedRow::Kill(k), i));
        }
    }
    rows.retain(|(r, _)| r.matches(&needle));
    rows.sort_by(|a, b| b.0.ts().cmp(&a.0.ts()).then_with(|| b.1.cmp(&a.1)));
    rows.into_iter().take(cap).map(|(r, _)| r).collect()
}

/* --------------------------------------------------------------------- paint -- */

fn age(t: DateTime<Utc>) -> String {
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

/// A status line with the leading square, the Gnomish vocabulary: idle is a hollow ring.
fn state_line(ui: &mut Ui, st: State, text: &str, col: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    let p = ui.painter();
    let c = Pos2::new(rect.left() + 6.0, rect.center().y);
    let sq = Rect::from_center_size(c, Vec2::splat(6.0));
    match st {
        State::Idle => {
            p.rect_stroke(sq, 0.0, Stroke::new(1.0, IDLE), egui::StrokeKind::Middle);
        }
        _ => {
            p.rect_filled(sq, 0.0, st.color());
        }
    }
    p.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.5),
        col,
    );
}

fn mono(s: impl Into<String>, size: f32, col: egui::Color32) -> RichText {
    RichText::new(s).font(FontId::monospace(size)).color(col)
}

fn body(s: impl Into<String>, col: egui::Color32) -> RichText {
    RichText::new(s).font(FontId::proportional(12.5)).color(col)
}

impl ParserScreen {
    /// WHICH VIEW IS SHOWING, as an index into `VIEWS`. Read only; see the note on
    /// `sky::SkyScreen::showing` for why it carries no `cfg(test)` and who calls it.
    ///
    /// IT CARRIED `show`'S FIRST LINE FOR A WHILE, so an accessor whose next two lines say
    /// "read only" opened by claiming to switch a view.
    pub fn showing(&self) -> usize {
        self.view
    }

    /// WRITE OUT A FIGHT NOTE THAT WAS TYPED AND NEVER BLURRED, because this screen is closing.
    ///
    /// THE ANALYSIS PAGE IS A VIEW OF THIS SCREEN and owns its own note buffer, so the App and
    /// the window registry, which are the only two things that know a close is happening, cannot
    /// reach it. This is that reach, and it is the whole of what this function is for.
    ///
    /// IT TAKES THE SETTINGS AND NOT A `Cx` for the reason `AnalysisScreen::flush_notes_into`
    /// does: at teardown there is no context to build. See that function.
    pub fn flush_notes(&mut self, settings: &mut crate::settings::Settings) {
        self.analysis.flush_notes_into(settings);
    }

    /// PUT A NOTE IN THE ANALYSIS BUFFER, FOR A TEST IN ANOTHER MODULE.
    ///
    /// `windows::closing_the_parser_window_writes_out_the_note_being_typed` has to set up the
    /// state a reader leaves behind (text typed, field never blurred) and cannot reach the buffer:
    /// it lives two structs down and both are private to their own modules. Driving the real page
    /// to produce it would mean a headless frame per assertion inside a test about WINDOWS.
    #[cfg(test)]
    pub fn seed_note_for_test(&mut self, key: &str, typed: &str) {
        self.analysis.seed_note_for_test(key, typed);
    }

    /// Switch to a view. What the nav's Kill tracker, Loot and Fights rows call, and what
    /// `main::on_section` calls for the Fights, Analysis and Overlays sections of LOG PARSER.
    ///
    /// # THIS DOC SAID TWO OF THE FIVE ARMS WERE UNREACHABLE, AND THEN SAID WHY THEY WERE NOT
    ///
    /// It read: "`View::Analysis` and `View::Overlays` appear nowhere in the crate except the two
    /// lines below that map them, so those two arms are unreachable: the Analysis page and the
    /// Overlays page are reached as SECTIONS through `main::draw_screen`, not as views of this
    /// screen." Both halves of that could not be true at once, and neither was. The two variants
    /// really were constructed nowhere, and they were in no section list either, so the main
    /// window could open neither page by any route: `screens::analysis` is a whole page no reader
    /// could reach, and `Settings::fight_notes` was a setting only that page writes. The one door
    /// left was this screen's own view row, which it draws ONLY when `Cx::railed` is false, and in
    /// the main window it never is.
    ///
    /// WHAT IS TRUE NOW. `nav::SECTIONS` carries Analysis and Overlays as LOG PARSER sections with
    /// no screen id of their own; `main::on_section` matches those names and calls this function;
    /// and `windows::ParserWindow` offers both as pages of the pop-out, so neither window has a
    /// page the other lacks. All five arms have production callers.
    /// `the_fights_section_paints_the_fights_the_engine_found_and_claims_no_missing_engine` is
    /// what holds that from this side: it walks every LOG PARSER section with no screen of its own
    /// and drives each one through here, and it fails if one of them has no view or if two of them
    /// land on the same one.
    pub fn show(&mut self, view: View) {
        self.view = match view {
            View::Kills => 0,
            View::Loot => 1,
            View::Fights => 2,
            View::Analysis => 3,
            View::Overlays => 4,
        };
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* The pump. Rate limited to one stat a second inside, so calling it every frame is
         * free, and the repaint request is what makes the poll happen while nothing else is.
         * In the main window the App already pumped this frame and this returns 0 at once; in
         * the Parser tool window this is the ONLY pump of that window's own ingest. Lines that
         * arrived are a reason to repaint now, not at the next tick. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        /* THE VIEW ROW ONLY WHERE NOTHING ELSE OFFERS IT, and NOTHING ELSE ON THIS LINE.
         *
         * The log's file name used to lead here, and it was the same name the status line under
         * it prints inside a whole sentence saying what is being read and when it was last read.
         * One file name, twice, eight points apart. It also followed the screen into Hunt
         * Journal and Loot Journal, where a raw `eqlog_*.txt` is the least interesting thing on
         * a page about what you have killed.
         *
         * The row itself stays for the pop-out Parser window, which has no rail to offer it. */
        if !cx.railed {
            chrome::context_bar(ui, &VIEWS, &mut self.view);
            ui.add_space(4.0);
        }
        self.status(ui, cx);
        self.session(ui, cx);
        ui.add_space(8.0);

        /* A NOTE TYPED ON ANALYSIS AND NOT BLURRED IS WRITTEN OUT WHEN THE READER STEPS AWAY.
         *
         * `AnalysisScreen` has two writers of its own and BOTH need that page to be drawing: the
         * field's `lost_focus` needs the widget laid out again to observe the focus going, and the
         * repointing flush runs from that page's own body. So stepping to Kills, Loot, Fights or
         * Overlays dropped whatever was in the buffer, silently, on the most ordinary click there
         * is. This is the frame after that step, and it is the only frame in the app that can see
         * it, because the Analysis page is a VIEW of this screen and this is where the view is
         * chosen.
         *
         * AFTER THE VIEW ROW AND NOT BEFORE IT, so a tab clicked in the pop-out (which writes
         * `self.view` directly through `chrome::context_bar` rather than through `show`) is
         * answered on the same frame it happened rather than one frame later.
         *
         * FREE WHEN THERE IS NOTHING TO WRITE. `flush_notes` takes its key, so every frame after
         * the first returns immediately, and a note nobody changed costs no file write even on the
         * first. See its own doc for the two cases no caller in this crate can reach (closing the
         * app, closing the pop-out). */
        if self.view != 3 {
            self.analysis.flush_notes(cx);
        }

        match self.view {
            0 => self.kills(ui, cx),
            1 => self.loot(ui, cx),
            2 => self.fights(ui, cx),
            3 => self.analysis.ui(ui, cx),
            _ => self.overlays(ui, cx),
        }
    }

    /// THE OWNER'S COMBAT OVERLAYS: what he has, and whether each one is on screen. D11 stage two.
    ///
    /// IT LIVES IN PARSER AND NOT IN SETTINGS, which is where the pin checkbox used to be. An
    /// overlay is a reading of the combat log and this is the destination about the combat log;
    /// Settings is where a thing is configured once and forgotten, and these are toggled mid-play.
    ///
    /// THE LIST IS `Settings::overlays` AND THE WINDOWS FOLLOW IT. Ticking a box here writes the
    /// setting; `Windows::show` brings its window population into line on the next pass. There is
    /// no second place that decides whether an overlay is open.
    fn overlays(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let mut list = crate::overlay::or_default(&cx.settings.overlays);
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(body(
                "An overlay is an always-on-top window over the game. Drag one to move it, \
                 Ctrl+Alt+D shows or hides them all.",
                TEXT_2,
            ));
        });
        ui.add_space(6.0);

        let mut remove: Option<usize> = None;
        for (i, o) in list.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if ui.checkbox(&mut o.open, "").changed() {
                    changed = true;
                }
                /* THE NAME IS EDITABLE IN PLACE. It is the owner's word for his own window and
                 * there is nothing to confirm; the id underneath never moves, so renaming keeps
                 * the window's size, its pin and its place in the list. */
                /* SAVED WHEN THE FIELD IS LEFT, NOT ON EVERY KEYSTROKE.  fires per
                 * character and each one would write the settings file. */
                if ui
                    .add(egui::TextEdit::singleline(&mut o.name).desired_width(160.0))
                    .lost_focus()
                {
                    changed = true;
                }
                ui.label(body(
                    o.panels()
                        .iter()
                        .map(crate::overlay::Widget::codename)
                        .collect::<Vec<_>>()
                        .join(", "),
                    TEXT_3,
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Remove").clicked() {
                        remove = Some(i);
                    }
                    /* THE BUILDER OPENS ON THIS OVERLAY. Toggling rather than only opening, so
                     * the same button closes it: there is nothing to save and nothing to cancel,
                     * because every edit is written as it is made. */
                    let open = self.editing.as_deref() == Some(o.id.as_str());
                    if ui.button(if open { "Done" } else { "Edit" }).clicked() {
                        self.editing = if open { None } else { Some(o.id.clone()) };
                    }
                    if ui.checkbox(&mut o.pinned, "on top").changed() {
                        changed = true;
                    }
                    if ui.checkbox(&mut o.chips, "chips").changed() {
                        changed = true;
                    }
                });
            });
            if self.editing.as_deref() == Some(o.id.as_str()) {
                changed |= builder(
                    ui,
                    o,
                    cx.ingest.current_fight(),
                    cx.ingest.fight_is_live(),
                    cx.ingest.history(),
                );
            }
        }

        /* REMOVED AFTER THE LOOP, because a `Vec` cannot be shortened while it is being walked
         * and a list that reordered itself under the pointer would remove the wrong row. */
        if let Some(i) = remove {
            list.remove(i);
            changed = true;
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            if ui.button("+  New overlay").clicked() {
                let taken: Vec<String> = list.iter().map(|o| o.id.clone()).collect();
                list.push(crate::overlay::Overlay::fresh(&taken, "New overlay"));
                changed = true;
            }
            /* THE SENTENCE THAT STOOD HERE SAID CHOOSING WHAT AN OVERLAY SHOWS "is the next
             * piece and is not built yet", beside the builder that does it. It was true on the
             * day a new overlay was a fixed damage meter and nothing else; the widget list, the
             * metric picker and the column flags landed after it and it was never taken out.
             *
             * A PAGE THAT DENIES ITS OWN CONTROLS IS WORSE THAN A PAGE WITH NO WORDS ON IT. A
             * reader believes the sentence over the button. */
            ui.label(body("A new overlay starts as a damage meter.", TEXT_3));
        });

        /* ONE BUTTON PER WIDGET, EACH A WINDOW OF JUST THAT WIDGET. The owner asked for every
         * widget to be KNOWN working in a real overlay before its look is worked on. See
         * `overlay::pop`. The list is `Widget::every`, so a widget that exists has a button. */
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(14.0);
            ui.label(body("Pop out:", TEXT_2));
            for w in crate::overlay::Widget::every() {
                let id = crate::overlay::pop_id(&w);
                let open = list.iter().any(|o| o.id == id && o.open);
                let tip = if open {
                    "Close this window"
                } else {
                    "Open a window of just this widget, on top of the game"
                };
                if ui
                    .selectable_label(open, w.codename())
                    .on_hover_text(format!("{}. {tip}", w.label()))
                    .clicked()
                {
                    crate::overlay::pop(&mut list, &w);
                    changed = true;
                }
            }
        });
        ui.add_space(6.0);

        /* WRITTEN HERE, AND THE WRITE IS THE PLAIN ONE. `Settings::save` REFUSES when the file
         * on disk is unreadable, which is the safety the Settings screen deliberately opts out of
         * with `save_replacing_unreadable`: that screen says out loud that it replaces the file and
         * only runs because a person typed into it. This list has no such licence, so a broken
         * settings file stays broken rather than being overwritten by a checkbox. */
        if changed {
            cx.settings.overlays = list;
            if let Err(e) = cx.settings.save() {
                log::warn!("the overlay list was not saved: {e}");
            }
        }
    }

    /// What is being read, or exactly why nothing is.
    fn status(&self, ui: &mut Ui, cx: &Cx) {
        let ig = &*cx.ingest;
        if ig.scanning() {
            state_line(
                ui,
                State::Working,
                &format!(
                    "reading the log folder and the last {} of the newest log",
                    ingest::tail_cap_text()
                ),
                TEXT_2,
            );
            return;
        }
        if let Some(p) = &ig.log_dir().problem {
            state_line(ui, State::Wrong, p, TEXT);
            return;
        }
        if let Some(p) = ig.active_problem() {
            state_line(ui, State::Wrong, p, TEXT);
            return;
        }
        match (ig.active_log(), ig.last_read()) {
            (Some(f), Some(t)) => {
                let cut = match ig.tail_start() {
                    Some(s) if s > 0 => format!(
                        ", the last {} of {} read (the file is bigger)",
                        ingest::tail_cap_text(),
                        f.name()
                    ),
                    _ => String::new(),
                };
                let zone = match ig.current_zone() {
                    Some("?") | None => String::from("zone not yet seen in the log"),
                    Some(z) => format!("in {z}"),
                };
                state_line(
                    ui,
                    State::Settled,
                    &format!(
                        "tailing {}, last read {}, {zone}{cut}",
                        f.path.display(),
                        age(t)
                    ),
                    TEXT_2,
                );
            }
            _ => state_line(ui, State::Idle, "nothing is being tailed", TEXT_2),
        }
    }

    /// WHAT THE SESSION STRIP IS COUNTING, on the strip itself. See [`ParserScreen::session`].
    const SESSION_SCOPE: &'static str = "this window, since it started reading:";

    /// The rest of it, on the hover, because the lead-in has to fit on one line beside six figures
    /// in a 320 point window.
    const SESSION_SCOPE_WHY: &'static str =
        "Each window reads the log with a reader of its own and counts only what the file has \
         gained since that reader started, so the main window and a pop-out opened later will not \
         agree. None of these is a total for the character, for the night, or for the app.";

    /// WHAT THE THREE TRACKER FILTERS ACTUALLY REACH, on the hover of each of them.
    ///
    /// THIS SAID "This window and this run only", AND IT WAS TRUE. `TrackerState::settings` lives
    /// on the `Ingest` and there is one `Ingest` per window, nothing wrote the three anywhere, and
    /// a restart put all three back at `TrackerSettings::default`. The hover was the honest thing a
    /// screen could say about a control that meant less than it looked like it meant; it was never
    /// the fix, and it named the outside-lane change that would be. That change landed:
    /// [`crate::ingest::TrackerSettings`] carries serde and [`crate::settings::Settings::tracker`]
    /// holds it, so these three are one value for the app, saved with everything else.
    const TRACKER_FILTER_SCOPE: &'static str =
        "These three are a counting rule and not a view: they change what the percentage above \
         MEANS, they are saved with the rest of the settings, and both this window and the other \
         one follow them.";

    /// The session strip: live events only, rates once there is enough
    /// active time for them to mean anything (two minutes).
    ///
    /// # SIX FIGURES UNDER SIX WORDS THAT NEVER SAID WHOSE, AND THE TWO WINDOWS DISAGREED
    ///
    /// `Session` counts LIVE events only, which its own doc gives the reason for: the bootstrap
    /// tail is history and a strip that counted it would report a week of kills as this sitting's.
    /// So the strip starts at zero and grows from the lines the log gains after the READER that
    /// owns it started. Each window has a reader of its own: the main window's is built in
    /// `main::App::new` at launch, and the tool windows share one built in `windows::ChildCx::new`
    /// the first time any tool window opens, which is a different moment and usually a much later
    /// one.
    ///
    /// So the two strips were two different measurements drawn under identical words: `active`,
    /// `kills`, `xp`, `kills/h`, `xp/h`, `loots`, with nothing anywhere saying either was per
    /// window. The owner plays with the parser popped out over the game and the main window behind
    /// it: two kill counts for one night, and no way to tell which is his.
    ///
    /// # THE FIX IS THE LABEL AND NOT A SHARED COUNTER, DELIBERATELY
    ///
    /// One `Session` shared by both windows would be a second owner of a number the `Ingest`
    /// already owns, reached across the registry's mutex from a deferred viewport's callback, and
    /// two readers feeding one counter is as good a way to count a line twice as it is to agree.
    /// The honest answer is cheaper and truer: the strip says what it is counting. It is what THIS
    /// window's reader has seen the log gain since it started, and now it says so in five words
    /// with the rest on the hover.
    fn session(&self, ui: &mut Ui, cx: &Cx) {
        let s = cx.ingest.session();
        if s.is_empty() {
            return;
        }
        let dur = if s.active_sec >= 3600 {
            format!("{}h{:02}m", s.active_sec / 3600, (s.active_sec % 3600) / 60)
        } else {
            format!("{}m", s.active_sec / 60)
        };
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(body(Self::SESSION_SCOPE, TEXT_3))
                .on_hover_text(Self::SESSION_SCOPE_WHY);
            ui.add_space(8.0);
            let cell = |ui: &mut Ui, v: String, label: &str| {
                ui.label(mono(v, 12.0, TEXT));
                ui.label(body(label, TEXT_3));
                ui.add_space(10.0);
            };
            cell(ui, dur, "active");
            cell(ui, s.kills.to_string(), "kills");
            cell(ui, format!("{:.2}%", s.xp_sum), "xp");
            if s.active_sec >= 120 {
                let hrs = s.active_sec as f64 / 3600.0;
                cell(ui, format!("{:.0}", s.kills as f64 / hrs), "kills/h");
                cell(ui, format!("{:.1}%", s.xp_sum / hrs), "xp/h");
            }
            cell(ui, s.loots.to_string(), "loots");
        });
    }

    fn kills(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE THREE COUNTING FILTERS ARE THE SETTINGS' AND THE INGEST HOLDS A WORKING COPY, so the
         * copy is put back in step HERE, before anything reads it.
         *
         * WHAT THEY USED TO BE. `TrackerState::settings` lived on the `Ingest` and the checkboxes
         * below wrote it directly. There is one `Ingest` per window (`main::App::new` and
         * `windows::ChildCx::new`), so ticking a box here moved THIS window's completion percentage
         * and left the other window's where it was: two headline percentages for one roster, six
         * inches apart, with nothing on either screen saying why. Nothing wrote them to `Settings`
         * either, so all three were back at `TrackerSettings::default` on the next launch, and a
         * reader who does not count witnessed kills had to say so every time he opened the app.
         *
         * THE SETTING IS THE VALUE AND THE INGEST'S FIELD IS A CACHE OF IT. `Ingest::new` and
         * `Ingest::reconfigure` seed it, which covers launch and every settings change that reaches
         * a window through `windows::sync` -- with one hole, and this line is what closes it:
         * `sync` adopts a POP-OUT'S settings into the main window WITHOUT reconfiguring the main
         * window's ingest (the other direction does). So a box ticked in the pop-out reaches the
         * root's `Settings` and would stop one field short of the reader that counts the kills.
         *
         * IT IS THE WHOLE OF WHAT HAS TO FOLLOW. These three feed `ingest::summarize`,
         * `ingest::credited` and `ingest::zone_ignored` and nothing else in the crate reads them,
         * so putting the copy in step at the one place that asks those questions is complete
         * rather than merely convenient.
         *
         * AND IT IS WHAT MAKES A CONTROL BOUND TO THE INGEST INERT: anything written to the cache
         * by a widget below would be overwritten from the setting on the very next frame, so the
         * only useful place for these boxes to write is the setting itself. */
        if cx.ingest.tracker().settings != cx.settings.tracker {
            cx.ingest.tracker_mut().settings = cx.settings.tracker.clone();
        }

        /* The toggles first, while the settings can be borrowed mutably; the roster and the
         * summary borrow the ingest immutably afterwards. */
        let mut filters_changed = false;
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let te = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("filter zones")
                .desired_width(220.0)
                .font(FontId::proportional(12.5));
            ui.add(te);
            /* THE SETTING AND NOT THE INGEST'S COPY. See the reconcile at the top of this function
             * for what these used to write and what that cost. */
            let st = &mut cx.settings.tracker;
            ui.add_space(12.0);
            filters_changed |= ui
                .checkbox(&mut st.witnessed, body("count witnessed kills", TEXT_2))
                .on_hover_text(Self::TRACKER_FILTER_SCOPE)
                .changed();
            filters_changed |= ui
                .checkbox(
                    &mut st.generic_everywhere,
                    body("generic kills count in every zone", TEXT_2),
                )
                .on_hover_text(Self::TRACKER_FILTER_SCOPE)
                .changed();
            filters_changed |= ui
                .checkbox(&mut st.ignore_cities, body("ignore cities", TEXT_2))
                .on_hover_text(Self::TRACKER_FILTER_SCOPE)
                .changed();
        });
        if filters_changed {
            /* THE INGEST FOLLOWS ON THE SAME FRAME, not on the next one. The summary a few lines
             * down is computed from this copy, so seeding it only at the top of the function would
             * paint one frame of the OLD percentage under the new checkbox: a tick that visibly
             * does nothing, then quietly does something. */
            cx.ingest.tracker_mut().settings = cx.settings.tracker.clone();
            /* WRITTEN WITH THE PLAIN `save`, exactly as the overlay list below is and for the same
             * reason: `Settings::save` REFUSES when the file on disk is unreadable, and only the
             * Settings screen has licence to replace one of those. A broken settings file stays
             * broken rather than being overwritten by a checkbox. */
            if let Err(e) = cx.settings.save() {
                log::warn!("the tracker filters were not saved: {e}");
            }
        }
        ui.add_space(8.0);

        let Some(roster) = cx.ingest.roster() else {
            let why = cx.ingest.roster_problem().unwrap_or("no roster");
            state_line(
                ui,
                State::Wrong,
                &format!("Mob roster not loaded: {why}"),
                TEXT,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(body(
                    "The tracker reads kills-data.json from the snapshot root: the Data root on \
                     Settings when set, else the first of the folders the loader tries.",
                    TEXT_2,
                ));
            });
            /* AND THE DOOR, WHICH THIS BRANCH DID NOT HAVE AND THE ONE BELOW IT DID.
             *
             * # THE WORDS USED TO SEND THE READER SOMEWHERE HE COULD NOT GET TO
             *
             * This sentence ran on into an apology: `The main window's Settings screen lists them
             * in order, and it opens from the gear at the foot of that window's rail; a pop-out
             * has no gear of its own, so this is a trip to the main window and the words have to
             * say so.` That was written when it was true. `ParserScreen::no_fights`, forty lines
             * down this same file, was then given a real control for exactly the same problem:
             * `ghost_btn` raising `Ask::OpenSettings`, which the shell routes whichever window it
             * came from.
             *
             * SO THE APOLOGY OUTLIVED THE THING IT APOLOGISED FOR, and it was the worse half of
             * the pair: one empty state on this page hands you the door and the other tells you
             * to go and find it yourself, for two faults with the same fix.
             *
             * `live::TO_SETTINGS` AND NOT A SECOND SPELLING, for the reason `no_fights` gives:
             * three empty states across two pages offer ONE door with one label on it. */
            let mut door = false;
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if crate::chrome::ghost_btn(ui, crate::screens::live::TO_SETTINGS, false).clicked()
                {
                    door = true;
                }
            });
            if door {
                cx.ask = crate::screens::Ask::OpenSettings;
            }
            return;
        };
        let tracker = cx.ingest.tracker();
        let sum = ingest::summarize(tracker, roster);

        /* NO RATIO, NO HEAD. `head_pct` is the rule and it is None when nothing is counted, which
         * takes the whole head line with it rather than printing an undefined ratio as a 0. */
        let Some(pct) = head_pct(&sum) else {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(body(nothing_counted(&sum), TEXT_2));
            });
            return;
        };
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(mono(format!("{pct}%"), 20.0, GOLD_HI));
            ui.add_space(8.0);
            ui.label(body(format!("{} of {} mobs", sum.done, sum.total), TEXT));
            ui.add_space(8.0);
            ui.label(body(
                format!("{}/{} zones cleared", sum.zones_done, sum.zones_total),
                TEXT_2,
            ));
        });
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width() - 14.0, 4.0), Sense::hover());
            bar(ui, rect, sum.done, sum.total);
        });
        let files: Vec<&String> = tracker.files.keys().collect();
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let from = if files.is_empty() {
                String::from("no kills read yet")
            } else {
                format!(
                    "counted from {} this run",
                    files
                        .iter()
                        .map(|f| f.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            ui.label(mono(from, 10.5, TEXT_3));
        });
        ui.add_space(8.0);

        let keys = order_zones(&sum, roster, &self.filter);
        if keys.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(body(
                    if self.filter.trim().is_empty() {
                        "No zones with a roster."
                    } else {
                        "No zone name matches the filter."
                    },
                    TEXT_2,
                ));
            });
            return;
        }

        let mut toggle: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for key in keys {
                let z = &roster.zones[key];
                let zs = sum.zones[key];
                let open = self.expanded.contains(key);
                let full = zs.done == zs.total;

                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                if resp.clicked() {
                    toggle = Some(key.to_string());
                }
                let p = ui.painter();
                if open || resp.hovered() {
                    p.rect_filled(rect, 0.0, PANEL);
                }
                let name_col = if full { GOLD } else if resp.hovered() { GOLD_HI } else { TEXT };
                p.text(Pos2::new(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, &z.name, FontId::proportional(12.5), name_col);
                let count = format!("{}/{}", zs.done, zs.total);
                let galley = p.layout_no_wrap(count.clone(), FontId::monospace(11.5), TEXT_2);
                p.text(Pos2::new(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, count, FontId::monospace(11.5), TEXT_2);
                let bar_w = 120.0;
                let bar_rect = Rect::from_min_size(
                    Pos2::new(rect.right() - 14.0 - galley.rect.width() - 10.0 - bar_w, rect.center().y - 2.0),
                    Vec2::new(bar_w, 4.0),
                );
                bar(ui, bar_rect, zs.done, zs.total);

                if open {
                    let refs: Vec<&MobRow> = z.mobs.iter().collect();
                    for row in ingest::sort_rows(&refs) {
                        let dead = ingest::credited(tracker, &sum.glob, key, row);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                        let p = ui.painter();
                        p.rect_filled(r, 0.0, SUNK);
                        if dead {
                            p.text(Pos2::new(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, "\u{2713}", FontId::proportional(12.5), TEXT_3);
                        }
                        let name_f = FontId::proportional(12.5);
                        let name_col = if dead { TEXT_3 } else { TEXT };
                        let g = p.layout_no_wrap(row.n.clone(), name_f.clone(), name_col);
                        let at = Pos2::new(r.left() + 46.0, r.center().y);
                        p.text(at, Align2::LEFT_CENTER, &row.n, name_f, name_col);
                        if dead {
                            /* struck through, not removed: the row keeps its place in the list */
                            let y = r.center().y;
                            p.line_segment([Pos2::new(at.x, y), Pos2::new(at.x + g.rect.width(), y)], Stroke::new(1.0, TEXT_3));
                        }
                        if row.named {
                            p.text(Pos2::new(at.x + g.rect.width() + 8.0, r.center().y), Align2::LEFT_CENTER, "named", FontId::monospace(10.0), GOLD_DIM);
                        }
                        if let Some(lvl) = row.lvl.as_deref().filter(|l| !l.is_empty()) {
                            p.text(Pos2::new(r.right() - 14.0, r.center().y), Align2::RIGHT_CENTER, lvl, FontId::monospace(11.0), TEXT_3);
                        }
                    }
                    ui.add_space(4.0);
                }
            }
        });
        if let Some(k) = toggle {
            if !self.expanded.remove(&k) {
                self.expanded.insert(k);
            }
        }
    }

    fn loot(&mut self, ui: &mut Ui, cx: &mut Cx) {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let te = egui::TextEdit::singleline(&mut self.feed_filter)
                .hint_text("filter items and mobs")
                .desired_width(220.0)
                .font(FontId::proportional(12.5));
            ui.add(te);
            ui.add_space(12.0);
            ui.checkbox(&mut self.show_kills, body("show kills", TEXT_2));
        });
        ui.add_space(6.0);

        let ig = &*cx.ingest;
        let rows = feed_rows(
            ig.loot(),
            ig.kills(),
            self.show_kills,
            &self.feed_filter,
            FEED_SHOWN,
        );
        if rows.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let why = if ig.active_log().is_none() {
                    "Nothing is being tailed, so there is no loot to show. See the line above for why."
                } else if !self.feed_filter.trim().is_empty() {
                    "No item, mob or kill in the feed matches the filter."
                } else if ig.loot().is_empty() && ig.kills().is_empty() {
                    "No loot or kills in the part of the log that was read. Lines appear here as the game writes them."
                } else {
                    "No loot in the part of the log that was read. Turn on show kills to see the kills."
                };
                ui.label(body(why, TEXT_2));
            });
            return;
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("loot-feed").num_columns(4).spacing([12.0, 3.0]).min_col_width(40.0).show(ui, |ui| {
                for row in rows {
                    match row {
                        FeedRow::Loot(l) => {
                            ui.label(mono(ingest::hhmmss(l.ts), 11.0, TEXT_3));
                            let qty = if l.qty > 1 { format!(" x{}", l.qty) } else { String::new() };
                            ui.label(body(format!("{}{qty}", l.item), TEXT));
                            ui.horizontal(|ui| {
                                ui.label(body(format!("from {}", l.mob), TEXT_2));
                                let d = l.disp.label();
                                if !d.is_empty() {
                                    ui.label(body(d, TEXT_3));
                                }
                            });
                            /* coin, right aligned, mono: a column of money has to line up */
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if l.disp == Disposition::Sold {
                                    if let Some(c) = &l.sold_for {
                                        ui.label(mono(c.clone(), 11.0, TEXT_2));
                                    }
                                }
                            });
                        }
                        FeedRow::Kill(k) => {
                            ui.label(mono(ingest::hhmmss(k.ts), 11.0, TEXT_3));
                            ui.label(body(format!("\u{2715} {}", k.name), TEXT_2));
                            ui.label(body(k.credit.label(), TEXT_3));
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    /// THE FIGHTS SECTION: every fight this app has for the character it is reading, newest first.
    ///
    /// # A LINE ABOVE THIS SAID THE VIEW HAS NO DATA, DIRECTLY OVER THE FUNCTION THAT FILLS IT
    ///
    /// It read "The Fights view has no data and says so, in the words of FIGHTS_EMPTY", and the
    /// line under it said the opposite. It was the stranded doc of a function that had already
    /// been replaced, left sitting above the real one, and `FIGHTS_EMPTY` went out with the banner
    /// it held: the name appears nowhere in this crate now except in `nav.rs`, which names it as a
    /// thing that is gone. A doc that points at a symbol which does not exist costs a reader the
    /// whole trip before they find that out.
    ///
    /// THE VIEW DRAWS REAL FIGHTS, and it is empty only when there are none: the words it shows
    /// then are chosen by [`why_no_fights`] and worded by [`no_fights_words`], which are shared
    /// with six other pages and are where the four causes are argued.
    ///
    /// # TWO SOURCES, AND THIS LIST USED TO BE ONE OF THEM
    ///
    /// `Ingest::fights` is the bootstrap's fold: at most the last 40 MB of the newest log, folded
    /// once when a scan lands and never written again while the app runs. Reading only that meant
    /// this table forgot every fight of every previous launch, on a build that had been writing
    /// those fights to disk the whole time. `Ingest::keep_fights` appends every CLOSED row of
    /// every scan to `store::Store`, keyed on character, server and the fight's own start stamp,
    /// and `Ingest::hp_of` has been reading that same history back since it landed. Play a night,
    /// restart, open Fights, and the night was gone from the one page whose entire subject is it.
    ///
    /// So the rows are the store's history for the character being read PLUS the current scan's
    /// own rows, deduplicated on the start stamp, which is the key the store itself dedupes on.
    /// The store is read through [`Kept`], which reads it when the answer can have changed rather
    /// than once a frame.
    ///
    /// # AND THE OTHER HALF OF THAT AMNESIA WAS THE EVENING IN FRONT OF THE READER
    ///
    /// Reading the store fixed the launches BEFORE this one and could not fix this one. A fight
    /// that CLOSED while the app ran was in the live re-fold and nowhere else: `Ingest::fights` is
    /// written by `adopt` and never again, and at that time `Ingest::keep_fights` was the only
    /// writer to the store and also ran only from `adopt`. So a reader who played from eight until
    /// midnight saw, at midnight, the fights that were in the tail at eight. This table could not
    /// close that from here: merging the live fold blind is worse than the gap, because
    /// `fold_recent`'s oldest row is a slice out of the middle of the file and its start stamp is
    /// not the fight's own, so it would neither dedupe against the scan nor be true.
    ///
    /// `Ingest::keep_live_fights` closed it at the source instead, which is the right place: it
    /// writes each fight it watched open and then saw close, so the row reaches the STORE as it
    /// finishes and this table draws it off the disk like any other kept row, under the same dedupe
    /// key, with the same wholeness rule the store has always had. `the_fights_list_shows_a_fight_
    /// that_closed_after_launch_without_waiting_for_another_scan` is what holds the screen's half
    /// of that: `Kept`'s key carries `Ingest::stored`, and that is the only reason a write that
    /// happened after the last frame is on this one.
    ///
    /// # NEWEST FIRST, AND NOW BY SORTING, WHICH THIS DELIBERATELY DID NOT DO
    ///
    /// What stood here said that reversing the engine's list is not a sort, and that re-ordering
    /// by a key of this screen's own would mean parsing a stamp. That was a good rule while there
    /// was ONE list arriving in log order. There are two now, and the second cannot be folded into
    /// the first by walking it: `Store::all` orders each month's rows by the start stamp AS TEXT,
    /// and `Wed Jul 15 ...` sorts before `Wed Jul 5 ...` and after `Mon Jul 20 ...`, so its order
    /// inside a month is very nearly arbitrary. The merged list is ordered by [`stamp`], which is
    /// `grimoire_parse::fights::seconds`: not a second stamp reader but the engine's own, the one
    /// `ingest::still_going` asks. A row whose stamp even that cannot read is not dropped and not
    /// placed; it sits at the foot of the list.
    ///
    /// # THE NEWEST ROW OF THE SCAN IS NOT A FINISHED FIGHT AND IT WAS DRAWN AS ONE
    ///
    /// `fold_text` closes the last fight it folds with `Ended::EndOfLog`, whose words are "the log
    /// stopped", because the TEXT ran out; from the end of a file the game is still appending to,
    /// that is exactly what a fight in progress looks like. `Ingest::keep_fights` refuses to store
    /// that row for precisely this reason, in as many words. This table drew it anyway, newest and
    /// nearest the eye, with "ended: the log stopped" in the last column and a duration and a
    /// damage total frozen at whatever the read happened to reach. It carries [`OPEN_NOTE`] now,
    /// on the two witnesses [`merge`] takes.
    ///
    /// NO ROW IS CLICKABLE AND NO ROW PRETENDS TO BE. `list_row` senses clicks and tints on hover,
    /// and that tint is kept because it is how an eye tracks one fight across seven columns; what
    /// is deliberately not done is anything a click promises. The cursor is not changed and the
    /// response is dropped. When per fight detail lands, this is where it hooks.
    fn fights(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.kept.follow(cx);
        /* THE EMPTY STATE IS ITS OWN FUNCTION AND THE REASON IS A BORROW.
         *
         * It now carries a CONTROL (see `no_fights`), so it has to be able to write `Cx::ask`,
         * which means `&mut Cx`. The rest of this function holds `&*cx.ingest` across its whole
         * body to read the scan, and the two cannot overlap. Splitting on the branch is what
         * keeps the mutable borrow inside the arm that needs it instead of widening it over a
         * table that only reads. */
        if cx.ingest.fights().is_empty() && self.kept.rows.is_empty() {
            /* THE CAUSE IS WORKED OUT HERE AND HANDED OVER, so `no_fights` can be driven for a
             * cause this machine cannot be put into. `NoFights::NoFolder` needs an `Ingest` with
             * no Logs folder, and `resolve_log_dir` with no setting walks the usual places and
             * would land on the owner's real EverQuest install. `screens::live` lifted its own
             * empty state for the same reason and in the same shape. */
            let why = why_no_fights(
                cx.ingest.scanning(),
                cx.ingest.log_dir().dir.is_some(),
                cx.ingest.active_log().is_some(),
            );
            self.no_fights(ui, cx, why);
            return;
        }

        let ig = &*cx.ingest;
        let scanned = ig.fights();

        let rows = merge(scanned, &self.kept.rows);
        let kept_n = rows.iter().filter(|s| s.from == Source::Kept).count();
        let shown = rows.len().min(FIGHTS_SHOWN);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(body(head_line(rows.len() - kept_n, kept_n, shown), TEXT));
        });
        if kept_n > 0 {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(body(KEPT_RULE, TEXT_3));
            });
        }
        if let Some(words) = torn_words(self.kept.torn) {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(body(words.as_str(), TEXT_3));
            });
        }
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(body(CUT_RULE, TEXT_3));
        });
        ui.add_space(6.0);

        /* Both facts are read ONCE, outside the loop: the clip flag because it belongs to the
         * file and not to a row, and the column set because a table whose columns were decided
         * per row could disagree with its own heading halfway down. */
        let clipped = ig.tail_start().is_some_and(|s| s > 0);
        let cols = visible_cols(ui.available_width());

        /* The heading is built from the same answer the rows are, so the table cannot name a
         * column it does not draw or draw one it does not name. */
        let heads: Vec<(&str, bool, f32)> = std::iter::once(("fight", false, 0.0))
            .chain(cols.iter().map(|c| (c.label, true, c.width)))
            .collect();
        head_row(ui, &heads);

        egui::ScrollArea::vertical()
            .id_salt("fights_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                /* `show` and not `show_rows`, which is what the six thousand row item and zone
                 * lists use. Rows here are not a uniform height (a row can carry a note under it),
                 * and the list is capped at `FIGHTS_SHOWN`, which is what makes that affordable
                 * now that it reads a history rather than a tail. A virtualised list that has to
                 * lie about row heights to work is a worse trade than laying out two hundred. */
                ui.spacing_mut().item_spacing.y = 0.0;
                for s in rows.iter().take(FIGHTS_SHOWN) {
                    let r = s.row;
                    let cells: Vec<String> = cols.iter().map(|c| (c.cell)(r)).collect();
                    let mut painted: Vec<Col<'_>> = Vec::with_capacity(cells.len() + 1);
                    painted.push(Col {
                        text: headline_text(r),
                        mono: false,
                        color: if r.headline.is_some() { TEXT } else { TEXT_3 },
                        right: false,
                        /* 0.0 is `list_row`'s "the rest of the row", for exactly one left column. */
                        width: 0.0,
                    });
                    for (c, text) in cols.iter().zip(cells.iter()) {
                        painted.push(Col {
                            text,
                            mono: c.mono,
                            color: c.color,
                            right: true,
                            width: c.width,
                        });
                    }
                    /* WHOSE `group dmg` AND `your deaths` THIS ROW ARE, on hover: each row counts
                     * its own fight's roster, and one heading sits over rows that count two
                     * different populations. See `dps::row_population`. */
                    list_row(ui, false, &painted)
                        .on_hover_text(crate::screens::dps::row_population(r));
                    /* THE NOTE IS ASKED OF THE ROW AND NEVER OF THE ROW'S POSITION, which is why
                     * it survived this list learning to sort itself. What did change is the second
                     * witness. `Ingest::tail_start` describes the read THIS window has just done,
                     * and a row off the disk was folded by a different read on a different day;
                     * `fights::mark_clipped` only ever sets `cut` when that read was genuinely
                     * clipped (`ingest::fold_tail` hands it `tt.start > 0`), so a kept row already
                     * carries its own second witness and asking today's read about it would drop a
                     * floor warning that is still true. */
                    if let Some(note) = cut_note(r, clipped || s.from == Source::Kept) {
                        cut_row(ui, &note);
                    }
                    if s.open {
                        cut_row(ui, OPEN_NOTE);
                    }
                }
            });
    }

    /// WHAT THIS PAGE SAYS WHEN THERE IS NOTHING TO LIST, AND THE ONE CONTROL IT OFFERS.
    ///
    /// # THE WORDS NAMED A PLACE THIS WINDOW CANNOT REACH
    ///
    /// `no_fights_words`' NoFolder arm sends the reader to Settings, which opens from the gear at
    /// the foot of the MAIN window's rail. Inside the Parser tool window there is no rail, no gear
    /// and no Settings page, and that is the window the owner keeps over the game while he plays.
    /// So this page described a trip the reader could not make from where he was standing.
    ///
    /// `Ask::OpenSettings` IS WHAT MADE A CONTROL POSSIBLE. `windows::Windows::show` already
    /// routes a child's ask up to the root and brings the main window forward, so one button does
    /// from either window what the sentence could only describe from one of them.
    ///
    /// NoFolder ONLY, AND THE GATE IS THE POINT. A control is a claim that pressing it helps.
    /// Settings does not fix Reading (wait), NoLog (`/log on`, inside the game) or NoCombat (fight
    /// something), so a door under all four would send a reader somewhere useless three times out
    /// of four.
    ///
    /// THE LABEL IS `screens::live`'s OWN, so the two pages offer one door rather than two
    /// spellings of one.
    fn no_fights(&mut self, ui: &mut Ui, cx: &mut Cx, why: NoFights) {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(body(no_fights_words(why), TEXT_2));
        });
        if why == NoFights::NoFolder {
            let mut door = false;
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if crate::chrome::ghost_btn(ui, crate::screens::live::TO_SETTINGS, false).clicked()
                {
                    door = true;
                }
            });
            if door {
                cx.ask = crate::screens::Ask::OpenSettings;
            }
        }
        /* AND A DAMAGED FILE IS NOT AN ABSENCE. Without this line the four sentences above would
         * blame the log for an empty table whose rows are on disk and unreadable, which is the
         * one cause none of them can be fixed by. */
        if let Some(words) = torn_words(self.kept.torn) {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(body(words.as_str(), TEXT_3));
            });
        }
    }
}

/// A completion bar. Track in the deep gold, fill in the dim gold, full in the body gold: the
/// metal turning toward the light as the zone fills. Not a state, so not a state colour.
fn bar(ui: &Ui, rect: Rect, done: usize, total: usize) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, GOLD_DEEP);
    if total > 0 && done > 0 {
        let w = rect.width() * (done as f32 / total as f32);
        let col = if done == total { GOLD } else { GOLD_DIM };
        p.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(w, rect.height())),
            0.0,
            col,
        );
    }
}

/// The clipped row's note, drawn as a continuation of the row above it: sunk ground, indented to
/// the row's own text inset, the smallest mono in the app.
///
/// IN THE ROW, NOT IN A LEGEND. A legend at the foot of a table is a second place to look and a
/// thing a reader has to carry back up the list, and the fact it carries is about ONE row. The
/// sunk fill is the same one the Kills view uses for a zone's unfolded roster, which is the
/// vocabulary this tree already has for "this belongs to the row above".
fn cut_row(ui: &mut Ui, words: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, SUNK);
    p.text(
        Pos2::new(rect.left() + ROW_PAD, rect.center().y),
        Align2::LEFT_CENTER,
        words,
        FontId::monospace(10.5),
        TEXT_3,
    );
}

/* --------------------------------------------------------------- the fights table -- */

/// One column of the fight table: what it is called, how wide, how it is set, and how to get its
/// text out of a row.
///
/// THE HEADING AND THE VALUE ARE DECLARED ON ONE LINE ON PURPOSE. The first cut of this had a
/// `[&str; 6]` of headings and a separate `[String; 6]` of cells zipped together by position, and
/// that shape has exactly one failure mode, which is silent: swap two entries in one array and
/// deaths are drawn under "damage" for the rest of the product's life, in a table where every
/// column is a small integer and nothing looks wrong. A function pointer beside the label makes
/// that mistake unwritable rather than untested.
struct FightCol {
    label: &'static str,
    /// Points. Wide enough for the widest value this column can hold AND for its own heading,
    /// which `every_column_is_wide_enough_for_its_own_heading` checks, because `list_row` clips a
    /// cell rather than growing it and a clipped heading reads as a broken word.
    width: f32,
    /// Monospace for anything a person reads digit by digit or compares character by character,
    /// which is every column here except the reason the fight ended, which is words.
    mono: bool,
    color: egui::Color32,
    /// The cell's text, taken from the row as the row already holds it. NOTHING IN HERE MAY
    /// DIVIDE, SUBTRACT STAMPS, OR RE-DERIVE ANYTHING: see the module note on copied judgements.
    cell: fn(&FightRow) -> String,
}

/// The columns, left to right, after the fight's name.
///
/// `lines` IS DELIBERATELY NOT A COLUMN. `FightRow` carries it and this table does not draw it:
/// the owner's list is the name, when, how long, damage, deaths, who was in it and why it
/// stopped. Combat lines are evidence for a coverage report, which is a section this build does
/// not have, and a column nobody asked for is a column somebody has to read.
const FIGHT_COLS: [FightCol; 6] = [
    FightCol {
        label: "started",
        /* The stamp AS THE LOG PRINTED IT, `Wed Jul 15 23:16:50 2026`, 24 characters. The desktop
         * does not cut it down to a time of day, and that is not laziness: re-cutting a stamp
         * means parsing one, and a second stamp parser in this tree is a second clock that can
         * disagree with `grimoire_parse::fights::seconds` about a log the engine already read. */
        width: 170.0,
        mono: true,
        color: TEXT_3,
        cell: |r| r.start.clone(),
    },
    FightCol {
        label: "ran",
        width: 56.0,
        mono: true,
        color: TEXT,
        /* `secs` as copied from `Fight::seconds()`. Never `end - start`: the engine floors a span
         * at one second and a desktop that subtracted the two stamps here would print 0s for the
         * fights the log cannot resolve, and would drift from the CLI on every one of them. */
        cell: |r| format!("{}s", r.secs),
    },
    FightCol {
        /* THE GROUP'S DAMAGE, AND THE HEADING HAS TO SAY WHOSE, exactly as `your deaths` beside it
         * does.
         *
         * This printed `FightRow::damage` under the bare word `damage`. That field is every point
         * ANYBODY dealt to anybody inside the fight, the pull included, while the Analysis page's
         * `Group damage` tile for the same fight prints `FightRow::group_sum`, which is the
         * players only. On the reference capture's first fight that is 16,526 against 12,976: two
         * surfaces of one destination, one word, two numbers, and a reader who opens a row in
         * Analysis to learn more about it is told the fight shrank by three and a half thousand.
         *
         * THE NUMBER MOVED AND NOT ONLY THE LABEL, because this is the third place the same defect
         * has been answered and the other two both moved. `screens::live` printed the fight total
         * in a header over three tables that rank players only, and `screens::analysis` divided it
         * into a `Raid dps` tile; both read `group_sum` now, and `FightRow::group_sum`'s own doc is
         * where the measurement is written down. A table that kept the pull's damage would be the
         * one surface left disagreeing with the other two.
         *
         * `group dmg` AND NOT `group damage`, WHICH IS THE ONE COMPROMISE HERE. A heading is set in
         * mono at 11.5 and this column is 78 points, which holds eleven characters; a twelfth
         * would be clipped, and `every_column_is_wide_enough_for_its_own_heading_and_its_widest_
         * value` fails on that rather than letting it ship. Widening instead would push the two
         * columns this table never drops past the narrowest row it has to survive, which
         * `the_fight_table_narrows_rather_than_overlapping_at_the_windows_this_app_allows` holds
         * it to. `dmg` is this app's own short unit for damage dealt (`overlay::Metric::unit`), so
         * the word is not invented here. */
        label: "group dmg",
        /* 12,976 in the reference capture's first fight; eleven digits before this clips. */
        width: 78.0,
        mono: true,
        color: TEXT,
        cell: |r| r.group_sum(|x| x.dealt).to_string(),
    },
    FightCol {
        /* PLAYER DEATHS, AND THE WORD IS LOAD BEARING. `FightRow::deaths` counts every death
         * inside the fight, the mobs included, so a farming night put the reader's own KILL COUNT
         * in a column headed `deaths`: the reports page measured 33 against a real 1. Every other
         * surface in the app already draws this through `FightRow::group_count`; this table was
         * reading the raw field. */
        /* AND THE HEADING SAYS WHOSE, because `took part` on the other side of it still counts
         * EVERYTHING that was hit. A bare `deaths` between two columns like that reads as a count
         * of the whole fight, which is exactly what the field behind it is. */
        label: "your deaths",
        /* WIDER THAN THE OLD BARE `deaths`, because the heading grew and this table pins every
         * column against its own heading. `every_column_is_wide_enough_for_its_own_heading_and_
         * its_widest_value` caught it on the rename. */
        width: 78.0,
        mono: true,
        color: TEXT_2,
        cell: |r| r.group_count(|x| x.deaths).to_string(),
    },
    FightCol {
        label: "took part",
        width: 66.0,
        mono: true,
        color: TEXT_2,
        cell: |r| r.participants().to_string(),
    },
    FightCol {
        label: "ended",
        /* Words, so proportional and not mono: "the clock stepped back" is a sentence, not an id.
         * They are the engine's own words, carried through `FightRow::ended` from `Ended`, so the
         * desktop and the CLI cannot describe the same cut two ways. */
        width: 150.0,
        mono: false,
        color: TEXT_2,
        cell: |r| r.ended.clone(),
    },
];

/// `items::list_row`'s own padding and gap, mirrored here because the geometry has to be known
/// BEFORE the row is drawn in order to decide how many columns fit in it.
///
/// A COPY, AND THE ONLY HONEST WAY TO SAY SO IS OUT LOUD. They are locals inside `list_row`
/// (items.rs, `let pad = 10.0; let gap = 8.0;`) and there is nothing to import. If they ever
/// change there, this table's fit calculation is wrong by a few points in the direction of a
/// slightly too narrow name column, which is why `MIN_HEAD` below is a fat floor and not a tight
/// one: a drift of a handful of points cannot make the columns overlap.
const ROW_PAD: f32 = 10.0;
const COL_GAP: f32 = 8.0;

/// The narrowest name column worth drawing, measured off the real capture: the first fight in
/// `web/fixtures/eqlog-tail-200k.txt` is `a dry bone skeleton`, nineteen characters, which sets
/// at about 118 points in Plex Sans at 12.5. Under that the one column a reader came for is a
/// clipped stub.
const MIN_HEAD: f32 = 120.0;

/// IBM Plex Mono's advance, 600/1000 em (fonts.rs vendors the face), at the 11.5 that
/// `items::col_font` sets for a mono cell and that `head_row` sets for every heading. Used only
/// to keep a heading from being clipped by its own column.
/// IBM Plex Mono`s advance at 11.5, 600/1000 em. Used by the column-width tests to check that a
/// heading is not clipped by its own column.
///
/// `#[cfg(test)]` BECAUSE THE SCREEN DOES NOT MEASURE ITSELF. The columns are laid out by
/// `items::head_row` and `items::list_row` from the widths beside them; this number exists so a
/// TEST can say what those widths have to hold. Ungated it is a constant with no production
/// reader, which is the shape this codebase treats as a defect, and deleting it takes the tests
/// with it.
#[cfg(test)]
const MONO_ADVANCE: f32 = 6.9;

/// Which columns are given up, in order, when the window is too narrow to hold them all.
///
/// WIDEST AND MOST REDUNDANT FIRST. `started` goes first because it is by far the widest and
/// because the list is in time order, so position already says roughly when, and the status line
/// above already names the file and the session. `ended` goes next, then the two small counts.
/// `ran` and `damage` are never dropped: they are how big the fight was, which is the question
/// the list is for.
///
/// BY LABEL AND NOT BY INDEX. An index into `FIGHT_COLS` silently points at the wrong column the
/// day somebody reorders the array, and the symptom is a table that overlaps itself at narrow
/// widths on somebody else's monitor. `every_name_in_the_drop_order_is_a_real_column` fails on a
/// typo instead, because a label that matches nothing makes the drop a no-op.
const DROP_ORDER: [&str; 4] = ["started", "ended", "took part", "your deaths"];

/// What is left of the row for the fight's name once `cols` have taken their fixed widths. This
/// is `list_row`'s own arithmetic for the one `width: 0.0` column, restated: the row is padded by
/// `ROW_PAD` at both ends and every right anchored column costs its width plus a gap.
fn head_width(width: f32, cols: &[&FightCol]) -> f32 {
    width - 2.0 * ROW_PAD - cols.iter().map(|c| c.width + COL_GAP).sum::<f32>()
}

/// The columns that fit a row `width` points wide.
///
/// THE TABLE NARROWS, IT DOES NOT OVERLAP AND IT DOES NOT SCROLL SIDEWAYS. `list_row` lays right
/// anchored columns from the right edge inward and clips each cell to its own rectangle, so a set
/// of columns wider than the row does not fail loudly: the later ones are laid at negative
/// offsets and paint over each other and over the name, off the left edge of the row. The Parser
/// screen is drawn in a pop-out tool window whose minimum is 320 points wide (`windows.rs`,
/// `with_min_inner_size([320.0, 200.0])`), so that is not a hypothetical width.
///
/// A DROPPED COLUMN IS VISIBLY GONE, WHICH IS WHY THIS IS ALLOWED AT ALL. The heading row is
/// built from this same answer, so the table always names exactly the columns it draws. Nothing
/// is relabelled, nothing is folded into a neighbour, and widening the window brings it back.
fn visible_cols(width: f32) -> Vec<&'static FightCol> {
    let mut cols: Vec<&'static FightCol> = FIGHT_COLS.iter().collect();
    for give_up in DROP_ORDER {
        if head_width(width, &cols) >= MIN_HEAD {
            break;
        }
        cols.retain(|c| c.label != give_up);
    }
    cols
}

/// What the fight was about, or the CLI's own words for a fight where nothing named dealt or took
/// anything.
///
/// THE WORDS ARE THE FORGE'S, VERBATIM (`grimoire-forge/src/fights.rs`, `f.headline().unwrap_or`).
/// A blank cell would be the alternative and it is worse than useless: an empty name column reads
/// as a bug in the app rather than as a fact about a log line, and the reader has no way to tell
/// which it is. `Fight::headline` returns None only when no named entity dealt or took a point,
/// which happens.
const NOTHING_NAMED: &str = "(nothing named)";

fn headline_text(r: &FightRow) -> &str {
    match &r.headline {
        Some(h) => h.as_str(),
        None => NOTHING_NAMED,
    }
}

/// The note the oldest row carries when the app did not read the whole file, in the row itself.
///
/// IT TAKES TWO WITNESSES AND DRAWS ON NEITHER ALONE, because each one is true on its own for an
/// innocent reason. `FightRow::cut` marks the OLDEST row, and there is always an oldest row even
/// when the whole file was read from byte zero. `Ingest::tail_start` is non-zero when the 40 MB
/// cap clipped the read, and that says nothing about which fight sits at the boundary. Only both
/// together mean "this fight ran into the edge of what was read". Drawing on `cut` alone would
/// tell the owner of a two megabyte log that his oldest fight might be truncated, which is a
/// false claim about his data, printed by the one screen whose whole job is to be checkable.
///
/// THE CAP IS NOT RESTATED. `ingest::tail_cap_text` is the one place the number lives, by the rule
/// written on it, so this sentence cannot go stale when the cap moves.
fn cut_note(r: &FightRow, clipped: bool) -> Option<String> {
    if !(r.cut && clipped) {
        return None;
    }
    Some(format!(
        "the reading starts inside this fight: only the last {} of the file was read, so it may \
         have begun earlier and these numbers cover only the part that was read",
        ingest::tail_cap_text()
    ))
}

/// HOW MANY ROWS THE TABLE DRAWS, whatever it holds.
///
/// The loot feed has had a cap from the start (`FEED_SHOWN`) and this list needs one now for the
/// same reason and a sharper one: it reads the store, which is every fight this character has
/// ever finished, and `ScrollArea::show` lays out every row it is handed on every frame. The cap
/// is stated in the line over the table whenever it bites, because a list that quietly stopped at
/// two hundred would have a reader believing his history ends there.
const FIGHTS_SHOWN: usize = 200;

/// The engine's words for a fight it closed because the log text ran out, `Ended::EndOfLog`.
///
/// A THIRD COPY OF A STRING, AND IT IS SAID OUT LOUD. `fights::ended_words` is private and its own
/// doc records why those four strings are duplicated from the CLI at all: `Ended` has no
/// `as_words` in `grimoire-parse`, and adding one is a change to a crate that piece does not
/// touch. `ingest::still_going` already carries the second copy, comparing against this same
/// literal. What keeps this one honest is
/// `the_words_for_a_fight_the_log_ran_out_on_are_the_engines_own`, which folds the real capture
/// through `fights::fold_text` and reads the last row's `ended` back out: a rewording in the
/// engine turns that test red, rather than quietly turning this comparison into one that never
/// matches, which would take [`OPEN_NOTE`] off the screen with it and leave an unfinished fight
/// drawn as a finished one again.
const ENDED_AT_THE_END_OF_THE_LOG: &str = "the log stopped";

/// The note the newest row of a scan carries when the reading ran out inside it. See [`merge`] for
/// the two witnesses, and [`ParserScreen::fights`] for what was drawn before it existed.
const OPEN_NOTE: &str =
    "the reading ends inside this fight: the parser closed it because the log ran out, not \
     because the fight did, so it may still be going and these numbers cover only the part that \
     was read";

/// Where the rows that are not in the current scan came from, said once, under the count.
///
/// AN EMPTY STATE NAMES ITS SOURCE AND SO SHOULD A FULL ONE. A list that grew from four rows to
/// two hundred on the day the store was wired in, with nothing on the page saying where the other
/// hundred and ninety six came from, is a page asking to be read as a bigger read of one log.
const KEPT_RULE: &str = "The kept fights are this app's own record: every finished fight it reads \
                         is written to its fights folder, so a night survives the log's tail \
                         moving on, and a restart.";

/// How many lines of the kept files could not be read, when any could not.
///
/// SAID AND NOT SWALLOWED, because a torn line is the difference between "no fights" and "the file
/// is damaged", which is `Store::read_month`'s own argument for counting them. An append
/// interrupted by a crash or a full disk leaves a partial last line; the fights around it are
/// fine, and this is what says the missing one is missing.
fn torn_words(torn: usize) -> Option<String> {
    if torn == 0 {
        return None;
    }
    let line = if torn == 1 { "line" } else { "lines" };
    Some(format!(
        "{torn} {line} in the kept files could not be read, so that many fights are not in this \
         list."
    ))
}

/// Where one row of the table came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// The bootstrap fold of the tail, `Ingest::fights`.
    Scan,
    /// This app's own fights folder, `store::Store`.
    Kept,
}

/// One row of the table: the fight, where it came from, and whether the reading stopped inside it.
struct Shown<'a> {
    row: &'a FightRow,
    from: Source,
    /// THE READ RAN OUT INSIDE THIS FIGHT, so it may not have ended at all. See [`merge`].
    open: bool,
}

/// A fight's start stamp as the engine reads it, for ORDERING and nothing else.
///
/// NOT A SECOND STAMP PARSER, WHICH IS THE RULE THIS TABLE IS BUILT ON.
/// `grimoire_parse::fights::seconds` is the engine's own reader, the one `ingest::still_going`
/// measures its gaps with, and this asks it exactly the question it answers: where in time is this
/// stamp. Nothing here recomputes a duration, a rate or a boundary, which is what the module note
/// forbids.
///
/// `i64::MIN` FOR A STAMP EVEN THAT CANNOT READ, which sinks the row to the foot of a newest first
/// list. The alternative is dropping it, and a fight silently missing from a list of fights is the
/// worse of the two: this way it is on screen with everything the row does say about it, in the
/// one place where nothing is claimed about when it happened.
fn stamp(r: &FightRow) -> i64 {
    grimoire_parse::fights::seconds(&r.start).unwrap_or(i64::MIN)
}

/// THE TABLE'S ROWS: the scan and the store, merged, newest first.
///
/// DEDUPLICATED ON THE START STAMP, which is the key `store::Store::append` itself dedupes on, so
/// the two lists agree about what "the same fight" means. Where both hold one, the KEPT copy is
/// the one drawn, and that is not arbitrary: a row only reaches the store once a scan has seen it
/// closed, so a stored copy is a finished reading of it.
///
/// THE NEWEST SCANNED ROW IS THE ONE THE READING RAN OUT ON, AND IT TAKES TWO WITNESSES.
/// `fold_text` always closes its final fight with `Ended::EndOfLog` because the text stopped, so
/// those words alone would put the note on the last fight of a log that ended for real hours ago:
/// the crying wolf `cut_note` is built to avoid at the other end of the list. The second witness
/// is the store, and it is the evidence the words lack: a fight it holds is a fight some scan saw
/// closed for a reason. A window with no store (no config folder, or a log whose name yields no
/// character) has no second witness, and there the note stands on the first alone, which is the
/// honest reading of a machine where nothing has ever recorded that fight as over.
fn merge<'a>(scanned: &'a [FightRow], kept: &'a [FightRow]) -> Vec<Shown<'a>> {
    let on_disk: HashSet<&str> = kept.iter().map(|r| r.start.as_str()).collect();
    let newest = scanned.len().saturating_sub(1);
    let mut out: Vec<Shown<'a>> = Vec::with_capacity(kept.len() + scanned.len());
    for r in kept {
        out.push(Shown {
            row: r,
            from: Source::Kept,
            open: false,
        });
    }
    for (i, r) in scanned.iter().enumerate() {
        if on_disk.contains(r.start.as_str()) {
            continue;
        }
        out.push(Shown {
            row: r,
            from: Source::Scan,
            open: i == newest && r.ended == ENDED_AT_THE_END_OF_THE_LOG,
        });
    }
    /* STABLE, so two fights the engine cannot tell apart in time keep the order they arrived in
     * rather than swapping places between frames. `sort_by` is stable by contract. */
    out.sort_by_key(|x| std::cmp::Reverse(stamp(x.row)));
    out
}

/// THIS CHARACTER'S FIGHTS AS THEY ARE ON DISK, read when the answer can have changed.
///
/// # A DISK READ PER FRAME WOULD BE A DISK READ PER FRAME
///
/// `Store::all` opens every month file this character has and parses every line of it into a whole
/// `FightRow`, fighters, moments and series included. The Fights section repaints at least once a
/// second by its own `request_repaint_after`, and faster while the log is moving, so reading the
/// store inside the draw would put an unbounded parse of the owner's entire history on the frame
/// clock.
///
/// # WHAT THE KEY IS, AND WHAT IT DELIBERATELY DOES NOT CATCH
///
/// The key is the owner, the store's root, and `Ingest::stored`, which counts what THIS window's
/// ingest has written to the store and what it skipped as already there. So it moves on exactly the
/// events that can change what is on disk for this window.
///
/// THIS SAID "The store changes when a scan lands and at no other time", AND IT IS THE ONE CLAIM
/// HERE THAT HAS STOPPED BEING TRUE. It was: `Ingest::keep_fights` ran from `adopt` and nowhere
/// else, so a fight that FINISHED while the app was running reached the live re-fold and nothing
/// that lasts, and this page showed the launch-time history all evening. `Ingest::keep_live_fights`
/// runs on every poll now and writes each fight it watched open and then saw close, so the store
/// grows through the evening, `stored` moves with it, and this cache re-reads on the next frame.
/// That is what puts a fight fought at nine o'clock on this list at nine o'clock. Nothing about the
/// key changed; what changed is how often it moves, and it was already the right key.
///
/// A FIGHT WRITTEN BY THE OTHER WINDOW'S INGEST STILL DOES NOT MOVE IT, and that is a known gap
/// rather than an oversight. It is a narrower one than it was: both windows tail the same file, so
/// both watch the same fight open and both offer it to the store when it closes, and the loser of
/// that race gets `already` back, which moves `stored` and this key just as an `added` would. What
/// is left is a fight the OTHER window watched and this one did not (this window opened after that
/// fight had already started, so it was never the open row of a fold with a row before it). Both
/// windows write the same fights to the same folder under the same dedupe key, so that miss is a
/// lag and never a wrong number. A clock here would trade the lag for a disk read every few seconds
/// on a page that is often left open all night.
///
/// IT FORGETS WHEN THERE IS NOTHING TO READ FROM, which matters more than it sounds: pointing the
/// app at another folder changes the character, and a list that kept the last one's rows would
/// draw one man's history under another man's log.
#[derive(Default)]
struct Kept {
    key: Option<KeptKey>,
    rows: Vec<FightRow>,
    /// Lines the store could not parse, across every month read. See [`torn_words`].
    torn: usize,
}

/// WHAT THE KEPT ROWS WERE READ FOR. When any part of this changes they are read again.
///
/// # NO DERIVED `PartialEq`, AND THE REASON IS A GUARD IN THIS CRATE
///
/// This derived `PartialEq` and compared whole, which reads beautifully and hid a field from
/// `reach::every_field_behind_a_masking_derive_is_read_in_production`. That guard exists because a
/// field that is only ever touched by a derive looks used to rustc and is invisible to a reader:
/// `wrote` went red on it the moment this struct landed, and the guard was right to ask, because
/// nothing in this file NAMED the field.
///
/// SO THE COMPARISON IS WRITTEN OUT, and writing it out is worth more than the derive: each part
/// of the key gets to say why a change in it means the rows on screen are wrong. A key compared
/// whole is a key nobody can audit one part of.
#[derive(Clone)]
struct KeptKey {
    /// WHOSE NIGHTS THESE ARE. The store is keyed on (character, server), so a different owner is
    /// a different history: pointing the app at another folder changes the character, and rows
    /// kept from the last one would draw one man's fights under another man's log.
    who: crate::store::Owner,
    /// WHICH STORE THEY CAME OUT OF. `Store::app_data` is the only production root today, but a
    /// root that moved is a different set of files and the rows in hand describe the old one.
    root: std::path::PathBuf,
    /// HOW MUCH THIS SESSION HAD WRITTEN WHEN THEY WERE READ, and this is the part that makes the
    /// list live rather than a launch-time snapshot. `Ingest::keep_fights` writes the bootstrap's
    /// closed rows and `Ingest::keep_live_fights` writes each fight as it finishes while the app
    /// runs; `Ingest::stored` counts both, so a change here means there is a fight on disk that is
    /// not in `rows`. Without it this cache would be correct exactly once, at the first frame, and
    /// would then show a stale night for the rest of the evening.
    wrote: crate::store::Wrote,
}

impl KeptKey {
    /// Is the reading in hand still the reading this key describes? See the type's own note for
    /// why this is spelled out rather than derived.
    fn same(&self, other: &KeptKey) -> bool {
        self.who == other.who && self.root == other.root && self.wrote == other.wrote
    }
}

impl Kept {
    fn follow(&mut self, cx: &Cx) {
        let (Some(store), Some(file)) = (cx.ingest.store(), cx.ingest.active_log()) else {
            self.forget();
            return;
        };
        let (Some(character), Some(server)) = (file.character.clone(), file.server.clone()) else {
            /* A LOG WHOSE NAME THIS BUILD CANNOT SPLIT has no owner to read under, and reading
             * under a guess would put another character's nights on this page.
             * `Ingest::keep_fights` refuses to WRITE on the same test and in the same words. */
            self.forget();
            return;
        };
        let key = KeptKey {
            who: crate::store::Owner { character, server },
            root: store.root().to_path_buf(),
            wrote: cx.ingest.stored(),
        };
        /* NOTHING TO DO WHEN EVERY PART OF THE KEY STILL HOLDS. `KeptKey::same` is what says so,
         * and it names the three parts rather than comparing the struct whole: see the type. */
        if self.key.as_ref().is_some_and(|k| k.same(&key)) {
            return;
        }
        let (rows, torn) = store.all(&key.who);
        self.rows = rows;
        self.torn = torn;
        self.key = Some(key);
    }

    fn forget(&mut self) {
        self.key = None;
        self.rows.clear();
        self.torn = 0;
    }
}

/// The line over the table. Says how many, from where, and how many of them are drawn.
///
/// "AS IT WAS LAST SCANNED" IS NOT HEDGING, IT IS THE SCOPE. The scan sees at most the last 40 MB
/// of the newest log and is folded once: `Ingest::fights` is written by `adopt` and never again
/// while the app runs. So a bare "4 fights" is a claim about a FILE that nothing here measured,
/// and a present tense one would read as a live count, which is how a reader who pulled four more
/// camps and watched the number sit still would conclude the parser had stopped. The live end is
/// `Ingest::current_fight` and it is a different list.
///
/// SAID IN THE PAST TENSE RATHER THAN GIVEN A CLOCK, because a duration here would be a second
/// number to keep in step with the one on the Dashboards strip, and this screen cannot ask when
/// the scan happened without an accessor it does not have. `scanned` is the Dashboards page's
/// answer and it says how long ago in words.
///
/// # AND IT IS NO LONGER ONE NUMBER, BECAUSE THE TABLE IS NO LONGER ONE SOURCE
///
/// It merges the scan with this app's own fights folder (see [`Kept`]), and the two answer
/// different questions: one is what is in the log now, the other is what this app has read before.
/// Printing "132 fights in the log as it was last scanned" over a list that is mostly disk would
/// be the very claim the scope phrase exists to refuse, and the reader could not tell. So each
/// source is counted in its own words and neither is left to be assumed.
fn head_line(scanned: usize, kept: usize, shown: usize) -> String {
    let fights = |n: usize| if n == 1 { "fight" } else { "fights" };
    let held = scanned + kept;
    let mut line = if kept == 0 {
        format!(
            "{scanned} {} in the log as it was last scanned",
            fights(scanned)
        )
    } else if scanned == 0 {
        format!(
            "{kept} {} kept from earlier reads, and none in the log as it was last scanned",
            fights(kept)
        )
    } else {
        format!(
            "{held} {}: {scanned} in the log as it was last scanned and {kept} kept from earlier \
             reads",
            fights(held)
        )
    };
    if shown < held {
        line.push_str(&format!(", showing the newest {shown}"));
    }
    line
}

/// Where a fight comes from, said once, under the count.
///
/// THE BOUNDARY IS THE PARSER'S INVENTION AND THE SCREEN SAYS SO. EverQuest writes no "combat
/// begins" line, no encounter id and no instance id; `grimoire_parse::fights` cuts a fight where
/// combat goes quiet, and the width of that quiet is a measured judgement written up on
/// `QUIET_SECONDS`. A list of fights that does not say where its boundaries came from is asking
/// to be read as something the game reported.
///
/// IT NAMES NO NUMBER OF SECONDS, on purpose. The window is `Ingest`'s to choose and pass to
/// `crate::fights::fold_text`, and this screen has no accessor for the value that was actually
/// used. Printing `grimoire_parse`'s default here would be a number this screen cannot prove the
/// engine ran with, which is the invention rule with extra steps. It goes in the moment `Ingest`
/// can be asked.
const CUT_RULE: &str = "The log writes no fight boundary, so the parser cuts one: a fight here is \
                        a run of combat lines with no long quiet in it.";

/// Why the fight table has no rows.
///
/// FOUR CAUSES, FOUR SENTENCES, BECAUSE THE FIX DIFFERS EVERY TIME, which is the rule
/// [`nothing_counted`] already states for the Kills head a hundred lines above: one sentence
/// covering several causes sends most of its readers to the wrong one. Here the four fixes are a
/// path in Settings, a slash command in the game, waiting a second, and going and fighting
/// something. Nothing but different words can tell them apart, because the table looks identical
/// in all four.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoFights {
    /// The bootstrap is still on the worker thread.
    Reading,
    /// No Logs folder resolved at all.
    NoFolder,
    /// A folder, but nothing is being tailed out of it. The status line above already prints the
    /// reason (`Ingest::active_problem`: no `eqlog_*.txt`, or the file would not open).
    NoLog,
    /// A log is being read and nothing in the part that was read attacked anything.
    NoCombat,
}

impl NoFights {
    /// Every case, for the test that holds the four sets of words apart. `no_fights_words` is a
    /// match, so the compiler catches a new variant with no words; this list is what catches a
    /// new variant that quietly says what an old one already said.
    pub const ALL: [NoFights; 4] = [
        NoFights::Reading,
        NoFights::NoFolder,
        NoFights::NoLog,
        NoFights::NoCombat,
    ];
}

/* ------------------------------------------------------------------ the builder -- */

/// THE OVERLAY BUILDER: pick what the window shows, and watch it while you pick.
///
/// # The preview is the same call the window makes
///
/// The owner's requirement was that the `+` show a mockup as you build. It does, and it is not a
/// mockup: `screens::dps::DpsScreen::ui` is pure in (config, fight), so the rectangle below the
/// controls is drawn by the identical call the real window makes with the identical config. There
/// is no second renderer to drift, and a widget added to the vocabulary appears here for free.
///
/// # Every edit is written as it is made
///
/// No Save and no Cancel. The list this mutates is written to settings by the caller the moment
/// anything returns true, and the window population follows settings on the next pass, so a change
/// is on screen in the overlay before a hand has left the mouse. A Save button would introduce a
/// state where what is previewed and what is running disagree.
///
/// Returns whether anything changed.
fn builder(
    ui: &mut Ui,
    o: &mut crate::overlay::Overlay,
    fight: Option<&FightRow>,
    live: bool,
    history: &[FightRow],
) -> bool {
    use crate::overlay::{Metric, Subject, Widget};
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.add_space(28.0);
        ui.vertical(|ui| {
            ui.add_space(4.0);

            /* ---------------------------------------------------- what it shows -- */
            let mut drop: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            /* OPENING THE BUILDER ON AN OVERLAY NOBODY HAS CONFIGURED MAKES ITS LIST REAL,
             * and this is the ONE place allowed to do that: editing here IS the choice. Every
             * other path reads through `Overlay::panels` and leaves `None` alone, which is
             * what stops a shipped default from being frozen into a settings file by a window
             * drag. See the defect on `overlay::Overlay::widgets`.
             *
             * WHAT IS MATERIALISED IS WHAT HE IS LOOKING AT: `panels` resolved it for the
             * preview a few lines down, so the list he starts editing is the list on screen.
             *
             * DRAWING IS NOT YET WRITING. The caller saves only when this returns true, so an
             * overlay merely LOOKED at keeps its `None` on disk. */
            let panels = o
                .widgets
                .get_or_insert_with(crate::overlay::Overlay::shipped_panels);
            for (i, w) in panels.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(body(format!("{} \u{b7} {}", w.codename(), w.label()), TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("x").on_hover_text("remove this panel").clicked() {
                            drop = Some(i);
                        }
                        if i > 0 && ui.button("^").on_hover_text("move up").clicked() {
                            move_up = Some(i);
                        }
                    });
                });

                /* THE OPTIONS FOR THIS PANEL, and only the ones that mean something for it. A
                 * control that does nothing is worse than a missing one: it teaches a person that
                 * the builder lies. */
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    match w {
                        /* THE METER TAKES WHAT THE METER READS, AND NOT THE COLUMN FLAGS.
                         *
                         * `Widget::Meter` carries a `Ranked` because the two shapes are configured
                         * the same way (which metric, rate or total, how many rows), but it has
                         * no columns: the row IS the bar. Offering `Cols`'s five checkboxes here
                         * would be five controls that move nothing on screen, which is the exact
                         * thing the comment above this match forbids. */
                        Widget::Meter(r) => {
                            egui::ComboBox::from_id_salt(("metric", i))
                                .selected_text(r.metric.label())
                                .show_ui(ui, |ui| {
                                    for m in Metric::ALL {
                                        if ui
                                            .selectable_label(r.metric == m, m.label())
                                            .clicked()
                                        {
                                            r.metric = m;
                                            changed = true;
                                        }
                                    }
                                });
                            changed |= ui.checkbox(&mut r.rate, "per second").changed();
                            changed |= ui.checkbox(&mut r.headline, "headline").changed();
                            /* THE TOTAL IS SAID ONCE: with the headline on it already is. See
                             * `dps::meter`. */
                            changed |= ui
                                .add_enabled(!r.headline, egui::Checkbox::new(&mut r.foot, "total"))
                                .on_disabled_hover_text("The headline already shows this total.")
                                .changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut r.cap).range(1..=40).prefix("rows "))
                                .changed();
                        }
                        Widget::Ranked(r) => {
                            egui::ComboBox::from_id_salt(("metric", i))
                                .selected_text(r.metric.label())
                                .show_ui(ui, |ui| {
                                    for m in Metric::ALL {
                                        if ui
                                            .selectable_label(r.metric == m, m.label())
                                            .clicked()
                                        {
                                            r.metric = m;
                                            changed = true;
                                        }
                                    }
                                });
                            changed |= ui.checkbox(&mut r.rate, "per second").changed();
                            changed |= ui.checkbox(&mut r.headline, "headline").changed();
                            changed |= ui.checkbox(&mut r.cols.rank, "rank").changed();
                            changed |= ui.checkbox(&mut r.cols.value, "value").changed();
                            changed |= ui.checkbox(&mut r.cols.share, "share").changed();
                            changed |= ui.checkbox(&mut r.cols.bar, "bar").changed();
                            /* THE COLUMN HEADER, AND IT WAS THE ONE FLAG WITH NO CONTROL.
                             *
                             * `Cols::head` is where the unit lives when `headline` is off, and
                             * a person who turned the headline off had no way to turn the
                             * header on: his overlay printed dps with no unit anywhere and
                             * nothing in this builder could bring it back. The comment eight
                             * lines up says a control that does nothing teaches a person that
                             * the builder lies; a MISSING control that traps a config in a
                             * state the app forbids on its own pages is the same lesson. */
                            changed |= ui.checkbox(&mut r.cols.head, "header").changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut r.cap).range(1..=40).prefix("rows "))
                                .changed();
                        }
                        /* OUTCOMES IS SPLIT OFF BELOW, because `widgets::outcomes` never reads
                         * `Detail::cap`: it draws the five measured outcomes and whichever
                         * hypothesised ones fired, and that list is the grammar's, not the
                         * reader's. A `rows` spinner over it moved a number that changed
                         * nothing on screen, which is the exact thing the comment above this
                         * match forbids. */
                        Widget::Abilities(d) | Widget::Targets(d) | Widget::Elements(d) => {
                            egui::ComboBox::from_id_salt(("subject", i))
                                .selected_text(d.who.label())
                                .show_ui(ui, |ui| {
                                    for s in Subject::ALL {
                                        if ui.selectable_label(d.who == s, s.label()).clicked() {
                                            d.who = s;
                                            changed = true;
                                        }
                                    }
                                });
                            changed |= ui
                                .add(egui::DragValue::new(&mut d.cap).range(1..=40).prefix("rows "))
                                .changed();
                        }
                        /* OUTCOMES TAKES THE SUBJECT AND NOT THE ROW COUNT. It is a panel about
                         * one person, so whose swings it counts is a real choice; how many rows it
                         * draws is the grammar's answer and not the reader's. */
                        Widget::Outcomes(d) => {
                            egui::ComboBox::from_id_salt(("subject", i))
                                .selected_text(d.who.label())
                                .show_ui(ui, |ui| {
                                    for s in Subject::ALL {
                                        if ui.selectable_label(d.who == s, s.label()).clicked() {
                                            d.who = s;
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        Widget::Coach(c) => {
                            changed |= ui
                                .add(egui::DragValue::new(&mut c.window).range(0..=120).prefix("last ").suffix("s"))
                                .changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut c.abilities).range(0..=8).prefix("abilities "))
                                .changed();
                            changed |= ui.checkbox(&mut c.usual, "vs your usual").changed();
                        }
                        Widget::Pill(p) => {
                            changed |= ui.checkbox(&mut p.clock, "clock").changed();
                            changed |= ui.checkbox(&mut p.rank, "rank").changed();
                            changed |= ui.checkbox(&mut p.target, "target").changed();
                        }
                        Widget::Timeline(t) => {
                            changed |= ui
                                .add(egui::DragValue::new(&mut t.cap).range(1..=8).prefix("lines "))
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut t.height)
                                        .range(60.0..=400.0)
                                        .suffix("pt"),
                                )
                                .changed();
                            changed |= ui.checkbox(&mut t.marks, "event marks").changed();
                        }
                    }
                });
            }

            /* AFTER THE LOOP, because a `Vec` cannot be reordered while it is being walked. */
            if let Some(i) = drop {
                panels.remove(i);
                changed = true;
            }
            if let Some(i) = move_up {
                panels.swap(i - 1, i);
                changed = true;
            }

            /* ------------------------------------------------------ what to add -- */
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(body("Add:", TEXT_3));
                /* EVERY ARM THE VOCABULARY HAS. Derived from `Widget::every`, so an arm that
                 * exists is an arm a person can add: a panel nobody can reach is this tree's
                 * signature defect. */
                for w in Widget::every() {
                    if ui.button(w.codename()).on_hover_text(w.label()).clicked() {
                        panels.push(w);
                        changed = true;
                    }
                }
            });

            /* --------------------------------------------------- the live preview -- */
            ui.add_space(6.0);
            ui.label(body("Preview", TEXT_3));
            egui::Frame::NONE
                .fill(INK)
                .inner_margin(egui::Margin::same(10))
                .stroke(egui::Stroke::new(1.0, RULE))
                .show(ui, |ui| {
                    ui.set_width(o.w.clamp(240.0, 620.0));
                    match fight {
                        Some(f) => {
                            /* THE PREVIEW TAKES THE REAL LIVENESS AND NOT A LITERAL `true`.
                             *
                             * It passed `true`, which is the one argument that changes what a
                             * reader sees rather than what it says: out of combat the real
                             * window prints `0` in a dim tint with no mark, and the preview
                             * printed a gold rate and a pulsing live dot for a fight that ended
                             * hours ago. The doc above claims this is the identical call the
                             * real window makes, and with a literal in it that was not true. */
                            for w in panels.iter() {
                                crate::screens::dps::draw_widget_in(ui, f, crate::fights::Pulse::from_live(live), w, history);
                            }
                            if panels.is_empty() {
                                ui.label(body("This overlay shows nothing yet.", TEXT_3));
                            }
                        }
                        /* NO SAMPLE FIGHT, DELIBERATELY. A preview filled with invented numbers is
                         * the one thing this app does not do, and a person who cannot tell a
                         * demonstration from their own parse has been misled by the demonstration. */
                        None => {
                            ui.label(body(
                                "Nothing has been fought yet, so there is nothing to preview. The \
                                 layout above is what the window will draw.",
                                TEXT_3,
                            ));
                        }
                    }
                });
            ui.add_space(8.0);
        });
    });

    changed
}

/// READING IS ASKED FIRST AND THAT ORDERING IS THE WHOLE FUNCTION.
///
/// `Ingest::new` starts the bootstrap on a worker thread and `adopt` is what installs the log
/// folder it resolved, so for the second or so before the first scan lands, `log_dir().dir` is
/// None and `active_log()` is None on a perfectly healthy machine with a perfectly good Logs
/// folder set. Asked in any other order this function opens the Fights section, on every cold
/// start, with "No Logs folder is set": an accusation about the reader's configuration, made by a
/// screen that has not looked yet. It then vanishes, which makes it worse rather than better,
/// because a complaint nobody can reproduce is a complaint nobody can fix.
pub fn why_no_fights(scanning: bool, folder: bool, tailing: bool) -> NoFights {
    if scanning {
        return NoFights::Reading;
    }
    if !folder {
        return NoFights::NoFolder;
    }
    if !tailing {
        return NoFights::NoLog;
    }
    NoFights::NoCombat
}

/// The words for each cause. Each one says what is missing AND where it would come from, which is
/// the empty state rule this tree holds everywhere (`main::unbuilt` names the store, `items::no_data`
/// prints the loader's probe list, `nothing_counted` sends the reader to the checkbox above).
///
/// NONE OF THEM MENTIONS AN ENGINE, A GRAMMAR OR A DOCUMENT, and that is the difference from the
/// three lines these replace. The engine is built and running two crates away; the only reason
/// this table is ever empty now is the log, and every sentence here is about the log.
///
/// # AND NONE OF THEM SAYS "HERE", BECAUSE SEVEN PAGES PRINT THEM AND ONLY ONE HAS THE FURNITURE
///
/// The NoCombat arm ended "Rows appear here as the game writes them." Reports, Live, Dps, Logs,
/// Dashboards and Analysis all draw this same sentence, and NONE of them has rows for "here" to
/// point at: on Dashboards it is a status strip, on Reports it is a fold that has nothing to fold.
/// It is the defect the Reports lane found one page further out, where two sentences pointed
/// upwards at a line that page had returned before drawing. "Fights appear as the game writes
/// them" is true on all seven, and `the_empty_states_point_at_no_furniture_only_one_page_has`
/// is what stops the next one arriving.
///
/// THE TWO THAT DO POINT UPWARDS ARE DIFFERENT AND ARE KEPT. Reading and NoLog say "the line
/// above", and every page that draws these words draws a source or state line over them; Reports
/// moved its own `source_line` above the empty branch for exactly that reason, and records why.
///
/// # AND "IN SETTINGS" IS NOT A PLACE FROM HALF THE WINDOWS THIS SCREEN IS DRAWN IN
///
/// This screen is drawn in the main window AND in `windows::ParserWindow`, and the pop-out has no
/// rail, no gear and no Settings page: Settings is not a `nav::ScreenId` at all, it is a `Body`
/// the persona footer's gear opens, and that footer is at the foot of the MAIN window's rail. So
/// "Set the Logs folder in Settings" was, in the pop-out, a next step with no next step: the one
/// window the owner keeps over the game while he plays telling him to go somewhere it cannot take
/// him and does not say how to reach.
///
/// SO THE ROUTE IS NAMED AND NOT JUST THE DESTINATION. It costs eight words in the main window,
/// where it is merely true, and it is the difference between an instruction and a dead end in the
/// other. A BUTTON here would be better still and is deliberately not done from this file: setting
/// the folder means writing `Settings` and calling `Ingest::reconfigure`, and `windows::sync`
/// adopts a tool window's settings WITHOUT reconfiguring the root window's ingest (the other
/// direction does). A control that repointed the pop-out and left the main window reading the old
/// folder would answer this finding by opening a worse one.
pub fn no_fights_words(why: NoFights) -> &'static str {
    match why {
        NoFights::Reading => {
            "The log is still being read. Fights appear when that first pass finishes, and the \
             line above says what is being read."
        }
        NoFights::NoFolder => {
            "No Logs folder is set, so there is no log to cut into fights. It is set in the main \
             window's Settings, which opens from the gear at the foot of that window's rail: give \
             it the folder the game writes eqlog_<character>_<server>.txt into."
        }
        NoFights::NoLog => {
            "Nothing is being tailed, so there is no fight to show. The line above says why, and \
             it is usually that logging is off in the game (/log on)."
        }
        NoFights::NoCombat => {
            "Nothing in the part of the log that was read attacked anything, so there is no fight \
             to cut. Fights appear as the game writes them."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{
        Credit, Disposition, KillEvent, LootEvent, MobRow, TrackerState, ZoneRoster,
    };
    use std::collections::BTreeMap;

    fn mob(n: &str) -> MobRow {
        MobRow {
            n: n.to_string(),
            named: false,
            lv: None,
            lvl: None,
            t: None,
            extra: Default::default(),
        }
    }

    fn zone(name: &str, city: bool, mobs: &[&str]) -> ZoneRoster {
        ZoneRoster {
            name: name.to_string(),
            city,
            mobs: mobs.iter().map(|m| mob(m)).collect(),
            extra: Default::default(),
        }
    }

    fn roster(zs: Vec<(&str, ZoneRoster)>) -> Roster {
        let mut zones = BTreeMap::new();
        for (k, z) in zs {
            zones.insert(k.to_string(), z);
        }
        /* Roster::load builds its indexes from a file; the tracker ordering only needs zones,
         * so the indexes are rebuilt here through the same JSON path the app uses. */
        let json = serde_json::json!({ "zones": zones.iter().map(|(k, z)| (k.clone(), serde_json::json!({
            "name": z.name, "city": z.city, "mobs": z.mobs.iter().map(|m| serde_json::json!({"n": m.n, "named": m.named})).collect::<Vec<_>>()
        }))).collect::<serde_json::Map<String, serde_json::Value>>() });
        /* A COUNTER AND NOT `zones.len()`. The key used to be the zone count, so any two tests
         * building rosters of the same size wrote one another's kills-data.json in the same temp
         * folder, and cargo runs them on threads of one process so the pid did not separate them
         * either. It held only because every caller happened to pick a different size. */
        static NTH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let nth = NTH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("grimoire-parser-{}-{nth}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kills-data.json"), json.to_string()).unwrap();
        Roster::load(&dir).unwrap()
    }

    /// A roster whose only zone is a city, which `TrackerSettings::default` ignores. This is the
    /// fresh install case, not a contrived one: nothing is wrong, and nothing is counted.
    #[test]
    fn a_ratio_with_no_denominator_is_not_a_zero_it_is_no_head_at_all() {
        let r = roster(vec![("a", zone("Akanon", true, &["x", "y"]))]);
        let sum = ingest::summarize(&TrackerState::default(), &r);
        assert_eq!(
            sum.total, 0,
            "the only zone is a city and cities are ignored"
        );
        assert_eq!(
            head_pct(&sum),
            None,
            "0/0 is undefined, and the head printed it as a 20pt 0% over 0 of 0 mobs"
        );
        assert!(
            sum.zones.contains_key("a"),
            "summarize keeps ignored zones in the map, which is what tells the two causes apart"
        );
        assert!(
            nothing_counted(&sum).contains("settings above"),
            "the cause here is a checkbox, so the words have to send the reader to it"
        );
    }

    #[test]
    fn a_roster_with_nothing_in_it_blames_the_roster_and_not_the_settings() {
        let r = roster(vec![("e", zone("Empty", false, &[]))]);
        let sum = ingest::summarize(&TrackerState::default(), &r);
        assert_eq!(sum.total, 0);
        assert_eq!(head_pct(&sum), None);
        assert!(
            sum.zones.is_empty(),
            "a zone with no mobs is never inserted, so the map is the discriminator"
        );
        let words = nothing_counted(&sum);
        assert!(words.contains("lists no zone with mobs"), "{words}");
        assert!(
            !words.contains("settings above"),
            "nothing here is a setting, and sending the reader to a checkbox would waste the trip"
        );
    }

    #[test]
    fn a_measured_ratio_is_still_drawn_and_rounds_the_way_it_always_did() {
        let r = roster(vec![("b", zone("Befallen", false, &["a", "b", "c"]))]);
        let mut st = TrackerState::default();
        st.kills
            .entry("b".into())
            .or_default()
            .insert("a".into(), Default::default());
        let sum = ingest::summarize(&st, &r);
        assert_eq!(sum.total, 3);
        assert_eq!(
            head_pct(&sum),
            Some(33),
            "one of three, rounded: the guard must not cost a real ratio"
        );
    }

    #[test]
    fn zones_order_by_remaining_then_name_and_drop_ignored_and_empty() {
        let r = roster(vec![
            ("b", zone("Befallen", false, &["a", "b", "c"])),
            ("a", zone("Akanon", true, &["x"])),
            ("u", zone("Unrest", false, &["a", "b"])),
            ("e", zone("Empty", false, &[])),
            ("c", zone("Commons", false, &["a", "b"])),
        ]);
        let mut st = TrackerState::default();
        st.kills
            .entry("b".into())
            .or_default()
            .insert("a".into(), Default::default());
        st.kills
            .entry("b".into())
            .or_default()
            .insert("b".into(), Default::default());
        let sum = ingest::summarize(&st, &r);
        let keys = order_zones(&sum, &r, "");
        assert_eq!(
            keys,
            vec!["b", "c", "u"],
            "1 left before 2 left; ties by name; the city and the empty zone are out"
        );
        assert_eq!(
            order_zones(&sum, &r, "UNR"),
            vec!["u"],
            "filter is a case folded substring"
        );
        st.settings.ignore_cities = false;
        let sum = ingest::summarize(&st, &r);
        assert_eq!(order_zones(&sum, &r, ""), vec!["a", "b", "c", "u"]);
    }

    fn loot(ts: i64, item: &str) -> LootEvent {
        LootEvent {
            ts,
            zone: "?".into(),
            qty: 1,
            item: item.into(),
            mob: "m".into(),
            disp: Disposition::Kept,
            sold_for: None,
        }
    }

    fn kill(ts: i64, n: &str) -> KillEvent {
        KillEvent {
            ts,
            zone: "?".into(),
            name: n.into(),
            credit: Credit::Blow,
        }
    }

    #[test]
    fn feed_sorts_by_log_time_newest_first_then_emission_order_and_caps() {
        let loot = vec![loot(10, "first"), loot(12, "third"), loot(12, "fourth")];
        let kills = vec![kill(11, "second"), kill(13, "fifth")];
        let rows = feed_rows(&loot, &kills, true, "", 10);
        let names: Vec<String> = rows
            .iter()
            .map(|r| match r {
                FeedRow::Loot(l) => l.item.clone(),
                FeedRow::Kill(k) => k.name.clone(),
            })
            .collect();
        assert_eq!(
            names,
            vec!["fifth", "fourth", "third", "second", "first"],
            "same second: the later emitted row sits above"
        );
        let rows = feed_rows(&loot, &kills, false, "", 10);
        assert!(
            rows.iter().all(|r| matches!(r, FeedRow::Loot(_))),
            "kills only when asked"
        );
        assert_eq!(feed_rows(&loot, &kills, true, "", 2).len(), 2);
    }

    #[test]
    fn show_routes_the_nav_rows_to_their_view() {
        let mut s = ParserScreen::default();
        assert_eq!(s.view, 0, "opens on the tracker");
        s.show(View::Fights);
        assert_eq!(s.view, 2);
        s.show(View::Loot);
        assert_eq!(s.view, 1);
        assert_eq!(
            VIEWS[s.view], "Loot",
            "the context bar's tab for the same index"
        );
    }

    #[test]
    fn feed_filter_matches_kill_name_and_loot_item_or_mob_case_folded() {
        let mut l1 = loot(10, "Froglok Meat");
        l1.mob = "a froglok ton knight".into();
        let mut l2 = loot(11, "Mote of Potential");
        l2.mob = "a skeletal monk".into();
        let loot = vec![l1, l2];
        let kills = vec![kill(12, "dusty werebat"), kill(13, "skeletal monk")];
        let names = |rows: Vec<FeedRow>| -> Vec<String> {
            rows.iter()
                .map(|r| match r {
                    FeedRow::Loot(l) => l.item.clone(),
                    FeedRow::Kill(k) => k.name.clone(),
                })
                .collect()
        };
        assert_eq!(
            names(feed_rows(&loot, &kills, true, " FROGLOK ", 10)),
            vec!["Froglok Meat"],
            "loot matches on item; trimmed and case folded"
        );
        assert_eq!(
            names(feed_rows(&loot, &kills, true, "skeletal", 10)),
            vec!["skeletal monk", "Mote of Potential"],
            "a kill matches on its name, loot on the mob that dropped it"
        );
        assert_eq!(
            names(feed_rows(&loot, &kills, false, "skeletal", 10)),
            vec!["Mote of Potential"],
            "hidden kills do not match either"
        );
        assert!(feed_rows(&loot, &kills, true, "nothing here", 10).is_empty());
    }

    /// A REAL HEADLESS FRAME AROUND ONE CALL, with every string egui actually laid out read back
    /// off the `Shape::Text` galleys. The technique is
    /// `main::the_rail_paints_no_row_called_settings`'s, and the reason is the same: a row painted
    /// from a literal appears in no plan, so only the paint can answer.
    ///
    /// ONE HELPER AND NOT A CLOSURE INSIDE EACH TEST, which is what it was. The `Cx` it builds is
    /// thirty lines, and the second test that needed one is why this exists: two copies drift the
    /// day a field is added to `Cx`, and the copy in the test nobody happens to be editing is the
    /// one that rots.
    fn painted(
        ing: &mut crate::ingest::Ingest,
        draw: impl FnMut(&mut Ui, &mut ParserScreen, &mut Cx),
    ) -> Vec<String> {
        let mut settings = crate::settings::Settings::default();
        let mut screen = ParserScreen::default();
        painted_with(ing, &mut settings, &mut screen, draw)
    }

    /// The same, for the tests that have to READ what the frame wrote or hand it a screen that was
    /// already in some state. A THIN WRAPPER RATHER THAN A SECOND COPY: `painted` above supplies a
    /// fresh `Settings` and a fresh `ParserScreen` and nothing else, so the thirty lines of `Cx`
    /// that the note above warns about drifting still exist exactly once.
    ///
    /// `&mut Cx` AND NOT `&Cx`, because `ParserScreen::ui` takes one and the routing tests drive
    /// the real entry point. The bodies that take `&Cx` (`fights`, `session`) are reached from the
    /// same closures unchanged, by the ordinary reborrow.
    /// DEFECT: AN EMPTY STATE THAT NAMED A PLACE THIS WINDOW CANNOT REACH.
    ///
    /// # WHAT WAS WRONG
    ///
    /// `no_fights_words`' NoFolder arm tells the reader to set a Logs folder in Settings. Settings
    /// opens from the gear at the foot of the MAIN window's rail. This screen is also the whole
    /// body of the Parser tool window, which has no rail, no gear and no route to Settings at all,
    /// and that is the window the owner keeps over the game while he plays. So on the surface where
    /// the sentence mattered most, it described a trip that could not be made.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// The door is drawn for the one cause it fixes and for none of the others. Both halves are
    /// the test: a control under all four causes would send a reader somewhere useless three times
    /// out of four, which is worse than the sentence it replaced, and a fix that only checked
    /// NoFolder would not catch that.
    ///
    /// AND THE LABEL IS COMPARED AGAINST `screens::live`'s CONSTANT rather than a literal, because
    /// the two pages must offer one door and not two spellings of one.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the control, ungating it, or spelling its label here
    /// instead of taking `screens::live::TO_SETTINGS`.
    #[test]
    fn the_empty_state_offers_settings_for_the_one_cause_settings_fixes() {
        let dir = crate::fights::probe::logs_dir("parser-no-folder-door");
        let mut ing = crate::fights::probe::booted(&dir);

        let drew = |ing: &mut crate::ingest::Ingest, why: NoFights| -> Vec<String> {
            painted(ing, move |ui, screen, cx| screen.no_fights(ui, cx, why))
        };

        let open = drew(&mut ing, NoFights::NoFolder);
        assert!(
            open.iter().any(|s| s == crate::screens::live::TO_SETTINGS),
            "the one cause Settings can fix draws no control, so the page still only describes \
             the trip: {open:?}"
        );
        assert!(
            open.iter()
                .any(|s| s == no_fights_words(NoFights::NoFolder)),
            "the words went with the control; a reader who cannot press it is now told nothing"
        );

        for why in [NoFights::Reading, NoFights::NoLog, NoFights::NoCombat] {
            let said = drew(&mut ing, why);
            assert!(
                said.iter().any(|s| s == no_fights_words(why)),
                "{why:?} drew no words at all: {said:?}"
            );
            assert!(
                !said.iter().any(|s| s == crate::screens::live::TO_SETTINGS),
                "{why:?} offers a door to Settings, which fixes none of it: waiting, `/log on` \
                 inside the game, and fighting something are the three answers, and none of them \
                 is on that screen: {said:?}"
            );
        }
    }

    fn painted_with(
        ing: &mut crate::ingest::Ingest,
        settings: &mut crate::settings::Settings,
        screen: &mut ParserScreen,
        mut draw: impl FnMut(&mut Ui, &mut ParserScreen, &mut Cx),
    ) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut cx = Cx {
            data: None,
            /* True, because these sections are reached through the rail: the shell is already
             * offering LOG PARSER's sections and the screen must not draw a switcher of its own
             * beside them. */
            railed: true,
            data_err: None,
            live: &live,
            settings,
            ingest: ing,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: crate::player::PlayerView::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| draw(ui, screen, &mut cx));
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
        said
    }

    /// THE BANNER IS GONE, AND THE TEST THAT GUARDED IT COULD NOT HAVE TOLD YOU.
    ///
    /// What stood here read a const array and joined it. It never drew a frame, never touched a
    /// screen and never looked at an ingest, so it would have stayed green over a Fights section
    /// listing four live fights UNDER a paragraph saying the combat engine is not built in this
    /// release. That is not a hypothetical failure mode of that test, it is the only outcome it
    /// had left once the engine was wired: the words it asserted on were still in the file, still
    /// correctly worded, and no longer true. A test that reads a constant is a test of the
    /// constant.
    ///
    /// SO THIS DRIVES A REAL HEADLESS FRAME AND READS THE PAINT BACK OUT. `ParserScreen::fights`
    /// is the body of the Fights section, tier 3 under LOG PARSER, and it is handed a `Ui` and a
    /// `Cx` exactly as the shell hands them to it. What comes back is every string egui actually
    /// laid out, pulled off the `Shape::Text` galleys, which is the same technique
    /// `main::the_rail_paints_no_row_called_settings` uses and for the same reason: a row painted
    /// from a literal never appears in any plan, so only the paint can answer.
    ///
    /// THE INGEST IS REAL AND SO IS THE LOG. `probe::planted` writes the reference capture into a
    /// temp folder as `eqlog_Reviir_freeport.txt` and `probe::booted` pumps the ingest until its
    /// worker's bootstrap lands, so the fights on screen came through `read_tail`, the scan
    /// thread and the mpsc channel. Handing the screen a hand built list would prove the screen
    /// can draw a list, which is not the thing in doubt.
    ///
    /// THE MUTATIONS THAT MUST MAKE IT FAIL, one per assertion.
    ///   Put the old paragraph back above the table: `tight` contains "not built" and the first
    ///   loop goes red. This is the exact defect the whole task exists to close, and the only
    ///   assertion in this file that can see it.
    ///   Draw the section from an empty `Vec` and never call `cx.ingest.fights()`: no headline is
    ///   painted and the second assertion goes red. A screen wired to nothing looks identical to a
    ///   screen wired to an empty log, and only feeding it a log with fights in it separates them.
    ///   Copy the CLI's participant table over, dps column and all: `tight` contains "dps" and the
    ///   rate assertion goes red. `Fight::dps` divides by a span floored at one second and the
    ///   engine's own test asserts 40.0 for a fight inside a single printed second, so a rate here
    ///   is the largest misreport this build could make. Rates land behind a publishability floor
    ///   or they do not land.
    ///   Print a placeholder total instead of the fight's own: the damage assertion goes red.
    ///   Separators are stripped before the search, so 16,526 and 16526 both pass and only a
    ///   different NUMBER fails.
    ///
    /// THE EMPTY PASS IS THE HALF THE OLD TEST GOT RIGHT AND IT IS KEPT. A folder with no eqlog in
    /// it must still paint something a reader can act on, must not claim the engine is missing,
    /// and must not invent a fight. The words themselves are the screen's to choose; what is
    /// asserted is that there are some, that they are not the old lie, and that they are not a
    /// fight that did not happen.
    #[test]
    fn the_fights_section_paints_the_fights_the_engine_found_and_claims_no_missing_engine() {
        /* WHAT A DEAD BANNER LOOKS LIKE, in the words this file actually shipped for a round.
         * Checked against the strings JOINED WITH NOTHING because `chrome::section` lays a heading
         * out one glyph at a time, and a claim painted glyph by glyph is still a claim. */
        let denials = [
            "not built",
            "no combat engine",
            "nothing in this build",
            "combat-parser.md",
            "not ported",
        ];

        /* ---- a folder with the reference capture in it ---- */

        let dir = crate::fights::probe::planted("screen-fights", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        assert_eq!(
            ing.fights().len(),
            12,
            "the ingest found no fights, so this frame would be testing the empty state by \
             accident. Problem: {:?}",
            ing.active_problem()
        );

        let said = painted(&mut ing, |ui, s, cx| s.fights(ui, cx));
        assert!(
            !said.is_empty(),
            "the Fights section painted nothing at all with four fights to show"
        );
        let tight = said.concat().to_lowercase();
        for lie in denials {
            assert!(
                !tight.contains(lie),
                "the Fights section is showing four live fights and still says {lie:?}. What it \
                 painted: {said:?}"
            );
        }
        assert!(
            said.iter().any(|s| s.contains("a lurking mummy")),
            "the first encounter in the capture is against a lurking mummy and the section never \
             names it, so nothing on this screen is reading `Ingest::fights()`. What it painted: \
             {said:?}"
        );
        /* Separators stripped, so any thousands style passes and only a wrong number fails. */
        let plain: Vec<String> = said
            .iter()
            .map(|s| {
                s.chars()
                    .filter(|c| !matches!(c, ',' | ' ' | '_' | '\u{202f}' | '\u{00a0}'))
                    .collect()
            })
            .collect();
        /* THE GROUP'S DAMAGE AND NOT THE FIGHT'S, WHICH IS THE NUMBER THIS ASSERTION USED TO PIN.
         *
         * It read `16526`, which is `FightRow::damage`: every point anybody dealt inside the
         * fight, the pull included. The Analysis page's `Group damage` tile for that same fight
         * prints `FightRow::group_sum`, the players only, and the two were on screen under one
         * word. Both figures are asked of the row the ingest actually folded rather than typed in
         * here, and the first assertion is what stops this passing on a capture where they happen
         * to be equal. */
        let first = &ing.fights()[0];
        let group = first.group_sum(|x| x.dealt).to_string();
        assert_ne!(
            group,
            first.damage.to_string(),
            "the capture's first fight has no pull damage in it, so this frame cannot tell the two \
             populations apart and proves nothing"
        );
        assert!(
            plain.contains(&group),
            "the group's damage for the first fight is {group} and that number is not on the \
             screen: {said:?}"
        );
        assert!(
            !plain.iter().any(|s| *s == first.damage.to_string()),
            "the whole fight's damage, the mobs' output included, is on a table whose every other \
             surface prints the group's: {said:?}"
        );
        assert!(
            !tight.contains("dps") && !tight.contains("per second"),
            "a rate column. `Fight::dps` divides by a span floored at one second, and the engine's \
             own test asserts 40.0 for a fight inside a single printed second. Rates land behind a \
             publishability floor, not here. What it painted: {said:?}"
        );

        /* ---- and a folder with no log in it at all ---- */

        let empty = crate::fights::probe::logs_dir("screen-fights-empty");
        let mut none = crate::fights::probe::booted(&empty);
        assert!(none.fights().is_empty(), "there is no log in that folder");
        let said = painted(&mut none, |ui, s, cx| s.fights(ui, cx));
        assert!(
            !said.is_empty(),
            "a Fights section with no log to read painted nothing, so a reader is told nothing \
             about why"
        );
        let tight = said.concat().to_lowercase();
        for lie in denials {
            assert!(
                !tight.contains(lie),
                "with no log to read, the section blamed the engine ({lie:?}) rather than the \
                 absent log: {said:?}"
            );
        }
        assert!(
            !said.iter().any(|s| s.contains("a lurking mummy")),
            "the empty section painted a fight out of a folder with no log in it: {said:?}"
        );

        /* AND THE SECTIONS ARE WHERE THE NAV SAYS THEY ARE. `ParserScreen::fights` drawing
         * correctly proves nothing if no path reaches it, and this file cannot see
         * `main::draw_screen` (it is in the binary target). This is the half of the route that IS
         * visible from here.
         *
         * IT IS ASSERTED BY SHAPE AND NOT BY NUMBER, and the number is why. This read `.get(1)`,
         * which was true while Live led the list and became false the moment Dashboards moved to
         * the top; the test went red for a reordering that broke nothing.
         *
         * # IT USED TO ASSERT THIS SCREEN OWNED EXACTLY ONE SECTION, AND THAT WAS THE BUG
         *
         * A section with no `ScreenId` is exactly the case `main::draw_screen` answers by painting
         * `ScreenId::Parser`'s own body, so a screenless section is a VIEW of this screen. Fights
         * was the only one, and this pinned it there. Meanwhile Analysis and Overlays, which are
         * also views of this screen, were in no section list at all, which is why the main window
         * could not open either page: `ParserScreen::show`'s own doc recorded that nothing in the
         * crate ever constructs those two variants.
         *
         * SO THE RULE IS NOT `EXACTLY ONE` BUT `EVERY ONE OF THEM IS A VIEW THIS SCREEN HAS, AND
         * NO TWO NAME THE SAME ONE`. That is the property that actually keeps the rail honest: a
         * screenless section whose name this screen cannot switch to is a rail row that paints
         * whichever view happened to be selected last, and two sections mapping to one view is two
         * rail rows that light separately and show the same page.
         *
         * MAPPED BY THE SAME NAME `main::on_section` MATCHES ON, so a section renamed in `SECTIONS`
         * without being renamed there is red here rather than silent in the app. */
        let own: Vec<&str> = crate::nav::sections_of(crate::nav::ScreenId::Parser)
            .iter()
            .filter(|(_, inner, _)| inner.is_none())
            .map(|(name, _, _)| *name)
            .collect();
        assert_eq!(
            own,
            ["Fights", "Analysis", "Overlays"],
            "the LOG PARSER sections the shell draws this screen's own body at are not this \
             screen's own views, so a rail row paints a page that does not mean it"
        );
        /* AND EACH ONE REACHES A DIFFERENT VIEW. Driven through `show`, which is the door
         * `main::on_section` uses, so this fails if a `View` arm stops being distinct. */
        let mut seen: Vec<usize> = Vec::new();
        for name in &own {
            let mut s = ParserScreen::default();
            let view = match *name {
                "Fights" => View::Fights,
                "Analysis" => View::Analysis,
                "Overlays" => View::Overlays,
                other => panic!(
                    "LOG PARSER has a screenless section {other} that this screen has no view \
                     for, so the rail can light a row nothing switches to"
                ),
            };
            s.show(view);
            assert!(
                !seen.contains(&s.showing()),
                "two LOG PARSER sections open the same view, so two rail rows show one page"
            );
            seen.push(s.showing());
        }
    }
    /* ------------------------------------------------------------- the fights table -- */

    /// The reference capture's first fight, as the forge prints it:
    ///   `#1  a dry bone skeleton  266s  16526 damage  27 deaths  ended: quiet`, 23 participants,
    ///   `[Wed Jul 15 23:16:50 2026] .. [Wed Jul 15 23:21:16 2026]  1626 combat lines`.
    /// Built by hand rather than by running the engine, because these tests are about what the
    /// SCREEN does with a row and must fail on a screen defect, not on an engine change.
    fn fight_row_fixture() -> FightRow {
        FightRow {
            start: "Wed Jul 15 23:16:50 2026".into(),
            end: "Wed Jul 15 23:21:16 2026".into(),
            secs: 266,
            damage: 16526,
            deaths: 27,
            lines: 1626,
            ended: "quiet".into(),
            headline: Some("a dry bone skeleton".into()),
            /* 23 FIGHTERS AND ONLY THE COUNT MATTERS TO THIS SCREEN, so they are built as 23
             * identical unnamed rows rather than transcribed from the capture. The Fights table
             * prints `participants()` and nothing else off this vector; the overlay that reads the
             * fighters themselves has its own fixtures. */
            fighters: (0..23)
                .map(|i| crate::fights::Fighter {
                    who: crate::fights::Who::Named(format!("fighter {i}")),
                    dealt: 0,
                    taken: 0,
                    healed: 0,
                    received: 0,
                    swings: 0,
                    landed: 0,
                    avoided: 0,
                    kills: 0,
                    deaths: 0,
                    ..Default::default()
                })
                .collect(),
            cut: false,
            ..Default::default()
        }
    }

    fn cells_of(r: &FightRow) -> Vec<String> {
        FIGHT_COLS.iter().map(|c| (c.cell)(r)).collect()
    }

    fn cell_named(r: &FightRow, label: &str) -> String {
        let c = FIGHT_COLS
            .iter()
            .find(|c| c.label == label)
            .unwrap_or_else(|| panic!("no column called {label}"));
        (c.cell)(r)
    }

    /// THE ENGINE FLOORS A SPAN AT ONE SECOND AND THE DESKTOP MUST PRINT THE FLOOR.
    ///
    /// `Fight::seconds()` returns `(end_secs - start_secs).max(1)` because the log stamps only to
    /// the second and up to 32 lines share one second in the capture. A desktop that took the two
    /// stamps off the row and subtracted them would print `0s` for exactly those fights, and
    /// would disagree with `grimoire forge fights` on the same file, which is the quiet fork this
    /// contract exists to prevent. Both rows below carry IDENTICAL stamps: any cell computed from
    /// them is 0, and any cell copied from `secs` is what the engine said.
    #[test]
    fn the_duration_is_copied_from_the_engine_and_never_recomputed_from_the_stamps() {
        let mut r = fight_row_fixture();
        r.start = "Wed Jul 15 23:16:50 2026".into();
        r.end = "Wed Jul 15 23:16:50 2026".into();
        r.secs = 1;
        assert_eq!(
            cell_named(&r, "ran"),
            "1s",
            "a fight inside one printed second must show the engine's floor, not a subtraction"
        );
        r.secs = 266;
        assert_eq!(
            cell_named(&r, "ran"),
            "266s",
            "the cell reads `secs`; recomputing it from these identical stamps would give 0s"
        );
    }

    /// A HEADLINE IS A JUDGEMENT AND IT IS COPIED TOO. `Fight::headline` prefers the named entity
    /// that TOOK the most damage and falls back to the one that DEALT the most, with a tie-break
    /// by name so the answer does not depend on the order participants appeared in the file. A
    /// screen that picked its own would rename the fight the CLI names, and the two would argue
    /// about the log they both read. There is nothing on `FightRow` to pick FROM, so the test
    /// that matters is the fallback: the words when the engine had no answer.
    #[test]
    fn a_fight_with_nothing_named_says_so_instead_of_leaving_the_column_blank() {
        let mut r = fight_row_fixture();
        assert_eq!(headline_text(&r), "a dry bone skeleton");
        r.headline = None;
        assert_eq!(
            headline_text(&r),
            "(nothing named)",
            "an empty name cell reads as a broken app, not as a fact about the log"
        );
        assert_eq!(
            headline_text(&r),
            NOTHING_NAMED,
            "and it is the forge's own words, so the two readers of one engine agree"
        );
    }

    /// NO RATE COLUMN, EVER, AND NOT BY GOOD INTENTIONS.
    ///
    /// `Fight::dps()` divides by a span floored at one second, and `grimoire_parse`'s own test
    /// pins 40.0 dps for a fight that begins and ends inside a single printed second. The forge
    /// prints that column, this table does not, and the reason it does not is a decision that has
    /// to survive the next person who notices the CLI has one. Two checks, because a rate can
    /// arrive under any heading: no column is NAMED like one, and no cell can produce the shape
    /// of one. Every number here is a count the row already holds, so a decimal point in a cell
    /// means somebody divided.
    #[test]
    fn no_column_in_this_table_is_a_rate() {
        for c in FIGHT_COLS.iter() {
            let l = c.label.to_lowercase();
            for banned in ["dps", "per second", "/s", "rate", "avg", "average"] {
                assert!(
                    !l.contains(banned),
                    "the column {:?} reads as a rate, and every rate this engine can produce is \
                     divided by a span floored at one second",
                    c.label
                );
            }
        }
        let mut r = fight_row_fixture();
        /* The engine's own worked example: 40 damage in a fight the log cannot resolve. */
        r.secs = 1;
        r.damage = 40;
        for cell in cells_of(&r) {
            assert!(
                !cell.contains('.'),
                "a decimal point in a fights cell means something was divided: {cell:?}"
            );
        }
    }

    /// A HEADING CLIPPED BY ITS OWN COLUMN READS AS A BROKEN WORD ("took par"), and `list_row`
    /// clips rather than growing, so nothing goes red when it happens: it just looks wrong on
    /// somebody's screen. The widest VALUE is checked in the same breath, using the stamp the log
    /// really prints, because a clipped clock ("Wed Jul 15 23:16:50 20") is worse than a clipped
    /// word: it looks like a working clock.
    #[test]
    fn every_column_is_wide_enough_for_its_own_heading_and_its_widest_value() {
        for c in FIGHT_COLS.iter() {
            let need = c.label.chars().count() as f32 * MONO_ADVANCE;
            assert!(
                c.width >= need,
                "the heading {:?} needs {need} points and its column is {}",
                c.label,
                c.width
            );
        }
        let r = fight_row_fixture();
        let stamp = cell_named(&r, "started");
        assert_eq!(stamp, "Wed Jul 15 23:16:50 2026");
        let need = stamp.chars().count() as f32 * MONO_ADVANCE;
        let started = FIGHT_COLS
            .iter()
            .find(|c| c.label == "started")
            .map(|c| c.width)
            .unwrap_or(0.0);
        assert!(
            started >= need,
            "a stamp as the log prints it needs {need} points and the column is {started}, so the \
             clock would be clipped mid number"
        );
    }

    /// A DROP THAT NAMES NOTHING IS A DROP THAT NEVER HAPPENS. `visible_cols` gives columns up by
    /// LABEL, and a typo or a rename in `FIGHT_COLS` turns one of those give-ups into a no-op:
    /// the loop then runs out of names with the table still too wide, and the columns paint over
    /// each other off the left edge of the row. Nothing about that is visible in a wide window,
    /// which is where it would be written and tested.
    #[test]
    fn every_name_in_the_drop_order_is_a_real_column() {
        for name in DROP_ORDER {
            assert!(
                FIGHT_COLS.iter().any(|c| c.label == name),
                "{name:?} is in the drop order and is not a column, so giving it up does nothing"
            );
        }
        for keep in ["ran", "damage"] {
            assert!(
                !DROP_ORDER.contains(&keep),
                "{keep:?} is how big the fight was, which is the question the list is for"
            );
        }
    }

    /// THE TABLE NARROWS INSTEAD OF RUNNING OFF THE LEFT EDGE OF ITS OWN ROW.
    ///
    /// `list_row` lays right anchored columns inward from the right and clips each cell to its
    /// own rectangle, so a column set wider than the row does not fail: the later columns are
    /// laid at negative offsets and paint on top of each other and of the fight's name. This
    /// screen is drawn in the pop-out Parser window, whose minimum is 320 by 200 (`windows.rs`,
    /// `with_min_inner_size`), and every column of this table together needs about 640, so the
    /// narrow case is the shipped case and not a hypothetical one.
    #[test]
    fn the_fight_table_narrows_rather_than_overlapping_at_the_windows_this_app_allows() {
        let mut w = 170.0f32;
        while w <= 1600.0 {
            let cols = visible_cols(w);
            let fixed: f32 = cols.iter().map(|c| c.width + COL_GAP).sum();
            assert!(
                fixed + 2.0 * ROW_PAD <= w,
                "at {w} points the fixed columns need {fixed} and would be laid off the left edge \
                 of the row, on top of the fight's name"
            );
            w += 10.0;
        }
        /* The pop-out's own minimum, and the main window's (880 wide, less the rail). */
        assert!(
            head_width(320.0, &visible_cols(320.0)) >= MIN_HEAD,
            "at the narrowest window this app opens, the one column the reader came for has to \
             still be readable"
        );
        assert!(
            head_width(660.0, &visible_cols(660.0)) >= MIN_HEAD,
            "the main window at its minimum, less the rail"
        );
        assert_eq!(
            visible_cols(1080.0).len(),
            FIGHT_COLS.len(),
            "a full window shows every column; narrowing is a response to the room, not a default"
        );
        assert!(
            visible_cols(320.0).len() < FIGHT_COLS.len(),
            "at 320 points every column cannot fit, and pretending it can is the overlap this \
             function exists to prevent"
        );
        /* The heading is built from the same answer, so it can never name a column that is not
         * drawn. Checked as a list rather than as a comment, because the two are built in two
         * places in `fights` and this is what holds them together. */
        let cols = visible_cols(400.0);
        let heads: Vec<&str> = std::iter::once("fight")
            .chain(cols.iter().map(|c| c.label))
            .collect();
        assert_eq!(heads.len(), cols.len() + 1);
        assert_eq!(heads[0], "fight");
    }

    /// A LOG READ WHOLE MUST NEVER CLAIM IT WAS CLIPPED, AND A CLIPPED ONE MUST SAY SO.
    ///
    /// The note needs two independent witnesses and each is innocently true on its own: there is
    /// always an oldest row (`FightRow::cut`), and the 40 MB cap can clip a read without the
    /// oldest fight touching the boundary (`Ingest::tail_start`). If `fold_text` ever marks the
    /// oldest row unconditionally, drawing on that flag alone would tell every owner of a small
    /// log that his first fight might be truncated. That is a false claim about his data, made by
    /// the one screen whose whole argument is that its numbers are checkable.
    #[test]
    fn the_clipped_note_needs_both_witnesses_and_names_the_cap_from_the_constant() {
        let mut r = fight_row_fixture();
        assert_eq!(
            cut_note(&r, true),
            None,
            "not the oldest row: nothing to say"
        );
        r.cut = true;
        assert_eq!(
            cut_note(&r, false),
            None,
            "the whole file was read, so the oldest fight is genuinely the oldest"
        );
        let note = cut_note(&r, true).unwrap_or_default();
        assert!(
            note.contains(&ingest::tail_cap_text()),
            "the note must name the cap from `tail_cap_text`, not restate the number: {note}"
        );
        assert!(
            note.contains("may have begun earlier"),
            "it has to say what is uncertain, not just that something is: {note}"
        );
    }

    /// THE NOTE FOLLOWS THE FLAG AND NOT THE POSITION, which is worth more now that the list SORTS
    /// rather than reverses. A loop that marked the first row it drew would put "the reading
    /// starts inside this fight" on the newest fight in the log: the one row on screen that is
    /// certainly not truncated, and the one a reader is most likely to be looking at.
    ///
    /// DRIVEN THROUGH `merge`, which is the order `fights` walks its rows in, so this goes red if
    /// the ordering stops agreeing with the flag and not only if the flag moves.
    ///
    /// WHAT MUTATION MAKES THIS RED: `merge` sorting oldest first, or `mark_clipped` marking a row
    /// other than the oldest.
    #[test]
    fn the_clipped_note_lands_on_the_oldest_fight_and_not_on_the_first_row_drawn() {
        let at = |start: &str, name: &str| {
            let mut r = fight_row_fixture();
            r.start = start.to_string();
            r.headline = Some(name.to_string());
            r
        };
        let mut oldest = at("Sun Jul 12 20:10:00 2026", "a dry bone skeleton");
        oldest.cut = true;
        let rows = vec![
            oldest,
            at("Wed Jul 15 23:16:50 2026", "A tormented dead"),
            at("Fri Jul 17 22:00:00 2026", "a skeleton"),
        ];

        /* Exactly the rows `fights` walks, in exactly the order it walks them. */
        let drawn: Vec<(String, bool)> = merge(&rows, &[])
            .iter()
            .map(|s| {
                (
                    headline_text(s.row).to_string(),
                    cut_note(s.row, true).is_some(),
                )
            })
            .collect();
        assert_eq!(drawn[0].0, "a skeleton", "newest first, like the loot feed");
        assert!(!drawn[0].1, "the newest fight was not clipped by the read");
        assert!(!drawn[1].1);
        assert_eq!(drawn[2].0, "a dry bone skeleton");
        assert!(
            drawn[2].1,
            "the note belongs to the oldest fight, which is the last row painted"
        );
    }

    /* ------------------------------------------------------- the three empty states -- */

    /// A COLD START IS NOT A MISCONFIGURATION, AND SAYING IT IS, IS THE DEFECT.
    ///
    /// `Ingest::new` hands the bootstrap to a worker thread and `adopt` is what installs the
    /// folder it resolved, so `log_dir().dir` and `active_log()` are both None for the second or
    /// so the scan runs, on a machine where everything is set correctly. Asked in any other
    /// order, this opens the Fights section on every cold start with "No Logs folder is set",
    /// an accusation about the reader's setup made by a screen that has not looked yet, and then
    /// it vanishes, which makes it a bug report nobody can reproduce.
    #[test]
    fn a_scan_in_flight_is_never_reported_as_a_folder_the_owner_failed_to_set() {
        assert_eq!(
            why_no_fights(true, false, false),
            NoFights::Reading,
            "the cold start: scanning, nothing resolved yet, and nothing wrong"
        );
        assert_eq!(why_no_fights(true, true, true), NoFights::Reading);
        assert_eq!(why_no_fights(false, false, false), NoFights::NoFolder);
        assert_eq!(
            why_no_fights(false, true, false),
            NoFights::NoLog,
            "a folder with no readable eqlog in it is not a missing folder: the fix is /log, not \
             Settings"
        );
        assert_eq!(why_no_fights(false, true, true), NoFights::NoCombat);
    }

    /// EACH CAUSE GETS ITS OWN SENTENCE AND ITS OWN NEXT STEP, the rule `nothing_counted` states
    /// for the Kills head: one sentence covering several causes sends most of its readers to the
    /// wrong one. Here the four fixes are a path in Settings, a slash command in the game,
    /// waiting, and going and fighting something, and the table looks identical in all four.
    #[test]
    fn the_four_empty_states_differ_and_each_names_a_next_step() {
        let all: Vec<&str> = NoFights::ALL.iter().map(|w| no_fights_words(*w)).collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!a.is_empty());
            for (j, b) in all.iter().enumerate() {
                assert!(
                    i == j || a != b,
                    "two causes with one sentence: {:?} and {:?} both say {a}",
                    NoFights::ALL[i],
                    NoFights::ALL[j]
                );
            }
        }
        let folder = no_fights_words(NoFights::NoFolder);
        assert!(
            folder.contains("Settings"),
            "the fix is a path and the screen has to name where it is set: {folder}"
        );
        assert!(
            folder.contains("eqlog_"),
            "and what the folder is recognised by: {folder}"
        );
        let no_log = no_fights_words(NoFights::NoLog);
        assert!(
            no_log.contains("/log"),
            "the usual cause is logging being off in the game: {no_log}"
        );
        assert!(
            !no_log.contains("Settings"),
            "a folder is already set, so sending the reader to Settings wastes the trip: {no_log}"
        );
        let reading = no_fights_words(NoFights::Reading);
        assert!(
            reading.contains("still being read"),
            "an absence and a delay are different things: {reading}"
        );
        let none = no_fights_words(NoFights::NoCombat);
        assert!(
            none.contains("part of the log that was read"),
            "the scope is the tail, never the file: {none}"
        );
        assert!(
            none.contains("as the game writes them"),
            "nothing is wrong here, so the words have to say what happens next: {none}"
        );
    }

    /// DEFECT: WORDS THAT POINT AT FURNITURE ONLY ONE OF THE SEVEN PAGES DRAWING THEM HAS.
    ///
    /// The NoCombat sentence ended "Rows appear here as the game writes them". `no_fights_words` is
    /// printed by `screens::reports`, `screens::live`, `screens::dps`, `screens::logs`,
    /// `screens::dashboards` and `screens::analysis` as well as by the Fights section it was
    /// written for, and only this page has rows for "here" to mean: on Dashboards it is a status
    /// strip, on Reports a fold with nothing to fold. It is the same defect the Reports lane found
    /// one page further out, where two sentences pointed upwards at a line that page returned
    /// before drawing.
    ///
    /// "the line above" IS NOT THE SAME AND IS NOT BANNED. Every page that prints these words draws
    /// a source or state line over them; Reports moved its own `source_line` above the empty branch
    /// for exactly that reason and records why. What cannot be said is a claim about a LIST, which
    /// is furniture six of the seven do not have.
    ///
    /// WHAT MUTATION MAKES THIS RED: any of the four sentences saying "here", or naming rows, a
    /// table, a list or a column.
    #[test]
    fn the_empty_states_point_at_no_furniture_only_one_page_has() {
        for why in NoFights::ALL {
            let words = no_fights_words(why);
            let lower = words.to_lowercase();
            /* SPLIT ON NON-LETTERS so "there" and "where" do not read as "here". */
            assert!(
                !lower
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w == "here"),
                "{why:?} says \"here\", and six of the seven pages that print this sentence have \
                 nothing there: {words}"
            );
            for furniture in ["rows", "the table", "the list", "column"] {
                assert!(
                    !lower.contains(furniture),
                    "{why:?} names {furniture:?}, which is the Fights section's furniture and not \
                     every reader's: {words}"
                );
            }
        }
    }

    /// DEFECT: AN EMPTY STATE THAT SENT THE READER TO A PLACE HALF THE WINDOWS CANNOT REACH.
    ///
    /// This screen is drawn in the main window and in `windows::ParserWindow`. The pop-out has no
    /// rail, no gear and no Settings page (Settings is not a `nav::ScreenId`: it is a `Body` the
    /// persona footer's gear opens, at the foot of the MAIN window's rail), so "Set the Logs
    /// folder in Settings" was a next step with no next step in the one window the owner keeps
    /// over the game. The empty state rule this tree holds everywhere is that a page says what is
    /// missing AND where it comes from; naming a destination the reader cannot get to from where
    /// he is standing meets the letter of that and not the point of it.
    ///
    /// EVERY SENTENCE THAT NAMES A SCREEN IS CHECKED, not just the one that was wrong, because the
    /// next one written here will be written by somebody sitting in the main window where the gear
    /// is six inches away.
    ///
    /// WHAT MUTATION MAKES THIS RED: any of these words naming Settings, or a screen, without
    /// saying which window it is in.
    #[test]
    fn an_empty_state_that_names_a_screen_says_which_window_it_is_in() {
        for why in NoFights::ALL {
            let words = no_fights_words(why);
            if !words.contains("Settings") {
                continue;
            }
            assert!(
                words.contains("main window"),
                "{why:?} sends the reader to Settings without saying where that is, and from the \
                 parser pop-out there is no way to get there: {words}"
            );
        }
    }
    /// DEFECT: AN EMPTY STATE THAT SENDS THE READER SOMEWHERE AND HANDS HIM NO DOOR.
    ///
    /// # ONE OF THE TWO GOT A CONTROL AND THE OTHER GOT AN APOLOGY
    ///
    /// Two empty states on this page have the same cause and the same fix: something the reader
    /// sets on Settings is not set. [`ParserScreen::no_fights`] was given a real control for it, a
    /// `ghost_btn` raising `Ask::OpenSettings`, which the shell routes from whichever window it
    /// came from. The roster branch of [`ParserScreen::kills`] was left with a sentence explaining
    /// that the reader would have to go and find the gear in the other window himself.
    ///
    /// THAT SENTENCE WAS TRUE WHEN IT WAS WRITTEN AND STOPPED BEING TRUE, which is why this is a
    /// test and not a fix. The two branches drifted apart silently because nothing compared them.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every empty state on this page that names Settings offers the door, drawn, in the frame.
    /// Read off the ink rather than off the source, because a control that is constructed and not
    /// reached is exactly the defect this tree keeps producing.
    ///
    /// AND THE LABEL IS `live::TO_SETTINGS` AND NOT A SECOND SPELLING, so the pages offer one door
    /// with one name rather than three doors a reader has to learn separately.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `ghost_btn` from either branch, or spelling the
    /// label by hand in one of them.
    #[test]
    fn every_empty_state_that_names_settings_offers_the_door_to_it() {
        let door = crate::screens::live::TO_SETTINGS;

        /* THE ROSTER BRANCH. A folder with a log in it but no snapshot root, which is the state a
         * fresh install is in: the tracker has no `kills-data.json` to read. */
        let dir =
            crate::fights::probe::planted("parser-roster-door", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        assert!(
            ing.roster().is_none(),
            "this probe has a roster, so the branch under test is not the one being drawn"
        );
        let said = painted(&mut ing, |ui, s, cx| s.kills(ui, cx));
        assert!(
            said.iter().any(|t| t == door),
            "the Kills view says the roster is missing and offers no way to Settings, so a \
             pop-out reader is told to go and find a gear this window does not have: {said:?}"
        );
        /* AND THE APOLOGY IS GONE WITH IT. A door beside a sentence saying there is no door is
         * worse than either alone. */
        let tight = said.concat().to_lowercase();
        for stale in ["has no gear of its own", "trip to the main window"] {
            assert!(
                !tight.contains(stale),
                "the words still apologise for the door that is now beside them: {stale:?}"
            );
        }

        /* THE FIGHTS BRANCH, which is the one that already had it: this is the control half, and
         * without it a fix that removed BOTH doors would pass. */
        let empty = crate::fights::probe::logs_dir("parser-fights-door");
        let mut none = crate::fights::probe::booted(&empty);
        let said = painted(&mut none, |ui, s, cx| {
            s.no_fights(ui, cx, NoFights::NoFolder);
        });
        assert!(
            said.iter().any(|t| t == door),
            "the no-folder empty state lost its door: {said:?}"
        );
    }

    /// THE BANNER THAT SAID THE ENGINE DOES NOT EXIST IS GONE, AND THIS IS WHAT KEEPS IT GONE.
    ///
    /// It said, in three lines: "The combat engine is not built in this release, so nothing here
    /// counts a fight", and pointed at `docs/COMBAT-PARSER.md` as a written grammar nothing
    /// implemented. Every word of that was true when it was written. It is false now:
    /// `grimoire_parse::fights` is in this workspace with its tests, and `grimoire-forge`'s
    /// `fights` command prints four fights and a 23 row participant table off
    /// `web/fixtures/eqlog-tail-200k.txt`. The test that used to stand here asserted the OPPOSITE
    /// of this one, and it passed, which is the point: a screen can be pinned to a claim about
    /// its own product long after the product has moved. So this fails on any revival of those
    /// words, whichever empty state they are put back into.
    #[test]
    fn no_empty_state_still_claims_the_combat_engine_is_missing() {
        let all = NoFights::ALL
            .iter()
            .map(|w| no_fights_words(*w))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for stale in [
            "not built",
            "not ported",
            "no combat engine",
            "combat-parser.md",
            "nothing in this build",
        ] {
            assert!(
                !all.contains(stale),
                "an empty state still says {stale:?}, and the engine it is talking about is two \
                 crates away with 112 green tests"
            );
        }
        assert!(
            !all.contains("engine"),
            "these four sentences are about the LOG. The only reason this table is empty now is \
             the log, so nothing here should be explaining an engine to anybody"
        );
    }

    /// WHAT IS OVER THE TABLE IS A SCOPE AND A DISCLOSURE, and both are load bearing. "4 fights"
    /// alone is a claim about a FILE that nothing on this screen measured: the bootstrap reads at
    /// most the last 40 MB. And a list of fights that does not say where its boundaries came from
    /// will be read as something the game reported, when in truth EverQuest writes no fight
    /// boundary at all and `grimoire_parse::fights` cuts one at a measured quiet.
    ///
    /// AND THE TWO SOURCES ARE COUNTED APART. The table merges the scan with this app's own fights
    /// folder, so one total under "in the log as it was last scanned" would make that claim about
    /// rows that came off the disk, which is the same defect the scope phrase exists to refuse.
    ///
    /// WHAT MUTATION MAKES THIS RED: folding the kept rows into the scanned count, dropping the
    /// cap notice, or the sentence going back to the present tense.
    #[test]
    fn the_line_over_the_table_says_what_was_read_and_who_decided_where_a_fight_ends() {
        assert_eq!(
            head_line(4, 0, 4),
            "4 fights in the log as it was last scanned"
        );
        assert_eq!(
            head_line(1, 0, 1),
            "1 fight in the log as it was last scanned",
            "one fight is not 1 fights"
        );
        let both = head_line(4, 128, 132);
        assert!(
            both.contains("132 fights"),
            "the table holds all of them and the line has to total them: {both}"
        );
        assert!(
            both.contains("4 in the log as it was last scanned"),
            "the scan's own count, in the words that scope it: {both}"
        );
        assert!(
            both.contains("128 kept from earlier reads"),
            "and the disk's, said apart from it: {both}"
        );
        let kept_only = head_line(0, 3, 3);
        assert!(
            kept_only.contains("3 fights kept from earlier reads"),
            "a log with no combat in its tail still has a history: {kept_only}"
        );
        assert!(
            kept_only.contains("none in the log"),
            "and the reader has to be told the scan found nothing, or the three read as this \
             evening's: {kept_only}"
        );
        let capped = head_line(4, 400, FIGHTS_SHOWN);
        assert!(
            capped.contains("showing the newest 200"),
            "a list that quietly stopped at the cap would have a reader believing his history \
             ends there: {capped}"
        );
        assert!(
            !head_line(4, 0, 4).contains("showing"),
            "and it says nothing about a cap that is not biting"
        );
        /* PAST TENSE, AND THAT IS THE FIX RATHER THAN THE WORDING. `Ingest::fights` is written
         * once by the bootstrap and never again, so the present tense read as a live count and a
         * reader who pulled four more camps would watch the number sit still and conclude the
         * parser had stopped. */
        for n in [1usize, 4] {
            assert!(
                !head_line(n, 0, n).contains("that was read"),
                "the count still reads as a running total: {}",
                head_line(n, 0, n)
            );
        }
        assert!(
            CUT_RULE.contains("no fight boundary"),
            "the boundary is the parser's, and the screen has to say so: {CUT_RULE}"
        );
        assert!(
            !CUT_RULE.contains("column"),
            "it must not point at a column, because a narrow window drops columns and the \
             sentence would then be false: {CUT_RULE}"
        );
        assert!(
            KEPT_RULE.contains("fights folder"),
            "a row that is not in the log has to say where it did come from: {KEPT_RULE}"
        );
    }

    /// DEFECT: A CONFIG FIELD THE BUILDER CANNOT SET, TRAPPING AN OVERLAY IN A STATE THE APP
    /// FORBIDS ON ITS OWN PAGES.
    ///
    /// `Cols::head` is where the unit lives when `headline` is off. The builder wrote `rank`,
    /// `value`, `share`, `bar` and `headline` and never `head`, so a person who turned the
    /// headline off got an overlay printing dps with no unit anywhere on it and nothing in this
    /// builder could bring one back. `dps::a_table_of_rates_always_says_somewhere_that_they_are_
    /// rates` holds every SHIPPED config to the opposite rule, so the app was enforcing on its own
    /// pages a thing it let a reader break on his.
    ///
    /// THE COMMENT ABOVE THE CONTROLS SAYS "a control that does nothing is worse than a missing
    /// one: it teaches a person that the builder lies". A missing control that traps a config is
    /// the same lesson learned the other way round.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every field of `Cols` is written somewhere in this file. Read out of the SOURCE, because
    /// the alternative is driving a `Ui` and clicking five checkboxes, and because what went wrong
    /// is precisely that a field was added and the builder was not visited.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting any `r.cols.<field>` checkbox, or adding a `Cols`
    /// field without a control.
    #[test]
    fn every_column_flag_has_a_control_in_the_builder() {
        let src = include_str!("parser.rs");
        /* THE FIELD NAMES, TAKEN OFF `Cols` ITSELF so a new one is covered the day it lands. */
        let decl = include_str!("../overlay.rs");
        let start = decl
            .find("pub struct Cols {")
            .expect("overlay declares Cols");
        let end = decl[start..].find("\n}").expect("Cols closes") + start;
        let fields: Vec<&str> = decl[start..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub "))
            /* A FIELD LINE AND NOT THE DECLARATION. `pub struct Cols {` also starts with `pub `,
             * and the first draft of this test reported it as an uncontrolled field, which is a
             * guard failing for a reason that has nothing to do with what it guards. */
            .filter(|l| l.contains(':') && l.ends_with(','))
            .filter_map(|l| l.split(':').next())
            .collect();
        assert!(
            fields.len() >= 5,
            "only {} fields found on Cols, so this is passing by not looking: {fields:?}",
            fields.len()
        );

        for f in fields {
            assert!(
                src.contains(&format!("r.cols.{f}")),
                "`Cols::{f}` has no control in the overlay builder, so an overlay can be left in a \
                 state no reader can change"
            );
        }
    }

    /* ------------------------------------------------- the two sources and their order -- */

    /// One fighter, for the tests that care about the population a column counts.
    ///
    /// `Who::player` IS A SPACE IN THE NAME and that is a rule of the game rather than a guess: the
    /// client will not make a character name with a space in it, and nearly everything the world
    /// spawns has one. So `Reviir` is a player here and `a lurking mummy` is the pull, exactly as
    /// the engine reads them.
    fn fighter(name: &str, dealt: u64) -> crate::fights::Fighter {
        crate::fights::Fighter {
            who: crate::fights::Who::Named(name.to_string()),
            dealt,
            ..Default::default()
        }
    }

    /// DEFECT: ONE WORD, TWO POPULATIONS, TWO SURFACES OF ONE DESTINATION.
    ///
    /// The table's damage column printed `FightRow::damage`, which is every point anybody dealt
    /// inside the fight INCLUDING the pull, while the Analysis page's `Group damage` tile for the
    /// same fight prints `FightRow::group_sum`, which is the players. A reader who noted the
    /// number in the list and opened that fight in Analysis watched the fight shrink, with both
    /// figures labelled damage and nothing on either page accounting for the difference.
    ///
    /// THE SAME DEFECT HAS BEEN ANSWERED TWICE BEFORE AND BOTH TIMES THE NUMBER MOVED.
    /// `screens::live` printed the fight total in a header over three tables that rank players
    /// only, and `screens::analysis` divided it into a `Raid dps` tile that read 62 where the
    /// honest figure is 48. `FightRow::group_sum` exists because of those two, and this table was
    /// the third surface, still reading the raw field.
    ///
    /// THE FIGURES ARE THE CAPTURE'S FIRST FIGHT AS THE OTHER TWO SURFACES MEASURE IT: 16,526
    /// dealt in the fight, 12,976 of it by the players, the 3,550 difference being the mummy and
    /// the skeletons hitting the group.
    ///
    /// WHAT MUTATION MAKES THIS RED: the heading going back to a bare `damage` that does not say
    /// whose (the `col.label` assertion), or any cell going back to `FightRow::damage` (the loop
    /// over `FIGHT_COLS` under it).
    #[test]
    fn the_damage_column_counts_the_group_and_the_heading_says_so() {
        let mut r = fight_row_fixture();
        r.fighters = vec![
            fighter("Reviir", 12_000),
            fighter("Tanefilo", 976),
            fighter("a lurking mummy", 3_550),
        ];
        r.damage = r.fighters.iter().map(|x| x.dealt).sum();
        assert_eq!(r.damage, 16_526, "the fight's own total includes the pull");
        assert_eq!(
            r.group_sum(|x| x.dealt),
            12_976,
            "and the group's does not, which is the whole reason the word matters"
        );

        let group = r.group_sum(|x| x.dealt).to_string();
        let col = FIGHT_COLS
            .iter()
            .find(|c| (c.cell)(&r) == group)
            .expect("a column prints the group's damage");
        assert!(
            col.label.contains("group"),
            "the players' damage is drawn under {:?}, which does not say whose it is, and \
             Analysis prints a different number under the same word",
            col.label
        );
        for c in FIGHT_COLS.iter() {
            assert_ne!(
                (c.cell)(&r),
                r.damage.to_string(),
                "the column {:?} prints the whole fight's damage, the mobs' output included, in a \
                 table whose every other surface prints the group's",
                c.label
            );
        }
    }

    /// THE STRING `merge` RECOGNISES AN UNFINISHED FIGHT BY IS THE ENGINE'S, AND THIS IS THE ONLY
    /// THING HOLDING THE TWO TOGETHER.
    ///
    /// `ENDED_AT_THE_END_OF_THE_LOG` is a third copy of `Ended::EndOfLog`'s words:
    /// `fights::ended_words` is private, `grimoire-parse` has no `as_words` on the enum, and
    /// `ingest::still_going` already carries the second copy. A copy that drifts does not fail
    /// loudly here, it fails SILENTLY: the comparison in `merge` stops matching, `OPEN_NOTE` never
    /// draws, and an unfinished fight goes back to being drawn as a finished one, which is the
    /// defect this whole mechanism exists to answer.
    ///
    /// FOLDED THROUGH THE ENGINE RATHER THAN ASSERTED AGAINST ITSELF, on the real capture, so the
    /// words are the ones a shipped fold actually produces.
    ///
    /// WHAT MUTATION MAKES THIS RED: rewording the constant, or `Ended::EndOfLog` being reworded
    /// under it in `crate::fights`.
    #[test]
    fn the_words_for_a_fight_the_log_ran_out_on_are_the_engines_own() {
        let (rows, _) = crate::fights::fold_text(
            crate::fights::probe::CAPTURE,
            crate::fights::quiet_window(),
            Some(crate::fights::probe::OWNER),
        );
        let last = rows.last().expect("the capture holds fights");
        assert_eq!(
            last.ended, ENDED_AT_THE_END_OF_THE_LOG,
            "`fold_text` closes its final fight because the text ran out, and this is the string \
             `merge` recognises that by"
        );
        assert!(
            rows.iter()
                .rev()
                .skip(1)
                .all(|r| r.ended != ENDED_AT_THE_END_OF_THE_LOG),
            "only the newest fight can end because the text ran out; `merge` marks by position as \
             well as by words and both halves have to be true"
        );
    }

    /// THE TWO SOURCES, MERGED, NEWEST FIRST, AND THE ROW THE READING RAN OUT ON.
    ///
    /// # THE STORE'S OWN ORDER IS NOT TIME ORDER AND THIS IS WHERE THAT BITES
    ///
    /// `Store::all` sorts its rows by the start stamp AS TEXT, and a log stamp leads with the day
    /// of the week: `Fri Jul 17` sorts before `Sun Jul 12`. So the kept list below arrives newest
    /// first by accident of the alphabet, and a merge that REVERSED its input rather than sorting
    /// it would put the twelfth on top of the seventeenth. That is what the old list did, honestly
    /// and correctly, while there was only one source and it arrived in log order.
    ///
    /// WHAT MUTATION MAKES THIS RED: reversing instead of sorting (the order assertion); dropping
    /// the dedupe on the start stamp (the length assertion); marking the newest scanned row open
    /// on its words alone, without asking whether the store already holds it (the last two).
    #[test]
    fn the_list_merges_the_store_with_the_scan_newest_first_and_dedupes_on_the_start_stamp() {
        let at = |start: &str, name: &str, ended: &str| {
            let mut r = fight_row_fixture();
            r.start = start.to_string();
            r.headline = Some(name.to_string());
            r.ended = ended.to_string();
            r
        };
        let kept = vec![
            at("Fri Jul 17 22:00:00 2026", "a lurking mummy", "zoned"),
            at("Sun Jul 12 20:10:00 2026", "a gnoll pup", "quiet"),
        ];
        let scanned = vec![
            /* The same fight the store holds, folded again by this launch's scan. */
            at("Fri Jul 17 22:00:00 2026", "a lurking mummy", "zoned"),
            at("Wed Jul 22 12:00:00 2026", "a dry bone skeleton", "quiet"),
            at(
                "Wed Jul 22 23:40:00 2026",
                "a skeleton",
                ENDED_AT_THE_END_OF_THE_LOG,
            ),
        ];

        let rows = merge(&scanned, &kept);
        let names: Vec<&str> = rows.iter().map(|s| headline_text(s.row)).collect();
        assert_eq!(
            names,
            vec![
                "a skeleton",
                "a dry bone skeleton",
                "a lurking mummy",
                "a gnoll pup"
            ],
            "newest first by the engine's own reading of the stamp, whatever order either source \
             handed its rows over in"
        );
        assert_eq!(rows.len(), 4, "the fight in both lists is one row, not two");
        assert_eq!(
            rows[2].from,
            Source::Kept,
            "where both hold a fight, the copy drawn is the one a scan saw CLOSED"
        );
        assert!(
            rows[0].open,
            "the newest scanned fight ended because the text ran out and nothing on disk has ever \
             recorded it as finished, so the reading stopped inside it"
        );
        assert!(
            rows.iter().skip(1).all(|s| !s.open),
            "no other row is open: two of them ended for a reason and one is off the disk"
        );
    }

    /// THE ROW THE STORE ALREADY HOLDS IS FINISHED, AND THAT IS THE SECOND WITNESS.
    ///
    /// `fold_text` closes its final fight with `Ended::EndOfLog` whatever happened, because the
    /// text ran out and it cannot know why. On a log that ended for real hours ago that is still
    /// the last row's ending, so the words alone would put "the reading ends inside this fight" on
    /// a fight that certainly finished: the same crying wolf `cut_note` takes two witnesses to
    /// avoid at the other end of the list.
    ///
    /// WHAT MUTATION MAKES THIS RED: `merge` setting `open` on the words alone.
    #[test]
    fn a_fight_the_store_has_recorded_is_never_drawn_as_one_the_reading_ran_out_on() {
        let mut r = fight_row_fixture();
        r.start = "Wed Jul 22 23:40:00 2026".into();
        r.ended = ENDED_AT_THE_END_OF_THE_LOG.into();
        let scanned = vec![r.clone()];

        let alone = merge(&scanned, &[]);
        assert!(
            alone[0].open,
            "nothing has ever recorded this fight as over, and the parser closed it because the \
             log ran out"
        );

        let known = merge(&scanned, std::slice::from_ref(&r));
        assert_eq!(known.len(), 1, "one fight, one row");
        assert_eq!(known[0].from, Source::Kept);
        assert!(
            !known[0].open,
            "a scan saw this fight closed and wrote it to disk, so the reading did not stop inside \
             it and the note would be a false alarm"
        );
    }

    /// DEFECT: THE ONE PAGE ABOUT FIGHTS FORGOT EVERY FIGHT AT EVERY RESTART.
    ///
    /// `Ingest::keep_fights` has written every closed fight to `store::Store` since the store
    /// landed, and this table read `Ingest::fights` alone, which is the bootstrap's fold of at
    /// most the last 40 MB of the current log. So a night played, an app restarted and a Fights
    /// page opened showed only what that tail still happens to hold. The history was on disk the
    /// whole time and the page whose entire subject is it was the one page not reading it.
    ///
    /// THE FIGHT PLANTED HERE IS IN NO LOG THIS INGEST CAN SEE. It is written straight into a
    /// store at a temp root (`Store::at`, the only constructor a test may use; nothing here goes
    /// near the owner's own `%APPDATA%`), and the ingest is booted on the reference capture, which
    /// does not contain it. A screen that paints it can only have read the store.
    ///
    /// WHAT MUTATION MAKES THIS RED: `fights` reading `cx.ingest.fights()` alone again, or `Kept`
    /// never being asked to follow.
    #[test]
    fn the_fights_list_shows_the_history_on_disk_and_not_only_this_launchs_scan() {
        let root =
            std::env::temp_dir().join(format!("grimoire-parser-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let who = crate::store::Owner {
            character: crate::fights::probe::OWNER.to_string(),
            server: "freeport".to_string(),
        };
        let mut old = fight_row_fixture();
        old.start = "Sun Jul 12 20:10:00 2026".into();
        old.end = "Sun Jul 12 20:14:00 2026".into();
        old.headline = Some("a gnoll pup".into());
        old.ended = "quiet".into();
        let wrote = crate::store::Store::at(&root)
            .append(&who, std::slice::from_ref(&old))
            .expect("the store takes a fight");
        assert_eq!(wrote.added, 1, "the fixture never reached the store");

        let dir =
            crate::fights::probe::planted("screen-fights-kept", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        /* AFTER the bootstrap, so this ingest has written nothing of its own: everything the
         * table draws off the disk is the fight planted above. */
        ing.use_store(Some(crate::store::Store::at(&root)));
        assert!(
            !ing.fights()
                .iter()
                .any(|r| r.headline.as_deref() == Some("a gnoll pup")),
            "the planted fight is not in the log, which is the whole point of it"
        );

        let said = painted(&mut ing, |ui, s, cx| s.fights(ui, cx));
        assert!(
            said.iter().any(|s| s.contains("a gnoll pup")),
            "a fight of this character's is on disk and the table did not draw it: {said:?}"
        );
        assert!(
            said.iter().any(|s| s.contains("a lurking mummy")),
            "and the scan's own fights are still there: {said:?}"
        );
        assert!(
            said.concat().contains("kept from earlier reads"),
            "the line over the table has to say that some of these rows are not in the log it just \
             scanned, or the count reads as a claim about the log: {said:?}"
        );
    }

    /// DEFECT: THE EVENING IN FRONT OF THE READER WAS NOT ON THE PAGE ABOUT IT.
    ///
    /// Reading the store fixed the launches before this one. It did not fix this one: a fight that
    /// CLOSED while the app ran was in the live re-fold and nowhere else, because `Ingest::fights`
    /// is written by `adopt` and never again and the only writer to the store also ran from
    /// `adopt`. A reader who played from eight until midnight saw, at midnight, the fights that
    /// were in the tail at eight, on the one page whose entire subject is his fights.
    ///
    /// THE FIX IS `Ingest::keep_live_fights` AND THE SCREEN'S HALF OF IT IS `Kept`'S KEY. That
    /// function writes each fight it watched open and then saw close, so the row is on DISK as it
    /// finishes; this cache re-reads because `Ingest::stored` is part of what it is keyed on, and
    /// that term is the only reason a write that happened after the last frame is on this one.
    ///
    /// THE FIGHT IS FOUGHT AND NOT PLANTED. The two appends are the two the ingest needs: a fight
    /// is stored only once this app has WATCHED it open (a live window is a slice out of the middle
    /// of a file, so its oldest row's start stamp is not the fight's), so the first poll sees the
    /// pull begin and the second sees the quiet that ends it. Nothing is written to the store by
    /// this test: everything on that disk got there through the shipped path.
    ///
    /// AND IT IS ONE SCREEN ACROSS TWO FRAMES, WHICH IS THE HALF THIS TEST GOT WRONG FIRST. Drawn
    /// with a FRESH `ParserScreen` each time, `Kept` has no key yet and reads the store whatever
    /// its key is made of, so the frame passed with the `wrote` term deleted: it was proving that
    /// the table reads the store at all, which the test above it already proved. The screen has to
    /// have drawn BEFORE the fight closed for the cache to be the thing under test.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `wrote` from `KeptKey::same` (or from `KeptKey`),
    /// which leaves this page correct exactly once, at its first frame, and stale for the rest of
    /// the evening; or `fights` reading `cx.ingest.fights()` alone. (Removing `keep_live_fights`
    /// from `Ingest::tail` turns it red too, and `ingest` has its own guard for that end.)
    #[test]
    fn the_fights_list_shows_a_fight_that_closed_after_launch_without_waiting_for_another_scan() {
        let root =
            std::env::temp_dir().join(format!("grimoire-parser-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let dir =
            crate::fights::probe::planted("screen-fights-live", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        /* AFTER the bootstrap, so `keep_fights` wrote nothing: every row that ends up on that disk
         * was put there by the live path this test is about. */
        ing.use_store(Some(crate::store::Store::at(&root)));
        assert_eq!(
            ing.stored(),
            crate::store::Wrote::default(),
            "the store must start empty or this proves nothing about what the evening added"
        );

        /* THE PAGE IS ALREADY OPEN AND ITS CACHE IS ALREADY FILLED, which is the state a reader is
         * in all evening and the only state in which the key is asked anything. */
        let mut screen = ParserScreen::default();
        let mut settings = crate::settings::Settings::default();
        let opening = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.fights(ui, cx)
        });
        assert!(
            !opening.iter().any(|s| s.contains("a practice dummy")),
            "nothing has been fought yet: {opening:?}"
        );

        let log = dir.join(format!(
            "eqlog_{}_freeport.txt",
            crate::fights::probe::OWNER
        ));
        let append = |text: &str| {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log)
                .unwrap_or_else(|e| panic!("{}: {e}", log.display()));
            f.write_all(text.as_bytes())
                .expect("append to the planted log");
        };
        /* `Ingest::tail` reads at most once a second, so this waits for a poll rather than
         * assuming one. Ten seconds is a ceiling and not a measurement. */
        let pump = |ing: &mut crate::ingest::Ingest| {
            let began = std::time::Instant::now();
            while began.elapsed() < Duration::from_secs(10) {
                if ing.tail() > 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("the appended lines were never read");
        };

        /* THE PULL BEGINS, well clear of the capture's last line at 23:46:18, so the fold closes
         * what was open and opens this one. */
        append("[Wed Jul 15 23:52:00 2026] You slash a practice dummy for 700 points of damage.\n");
        pump(&mut ing);
        /* AND IT ENDS. The later line is the quiet that tells the fold the pull is over. */
        append("[Wed Jul 15 23:52:02 2026] You have slain a practice dummy!\n");
        append("[Wed Jul 15 23:58:00 2026] You slash a fire beetle for 4 points of damage.\n");
        pump(&mut ing);

        assert!(
            !ing.fights()
                .iter()
                .any(|r| r.headline.as_deref() == Some("a practice dummy")),
            "the bootstrap ran before this fight existed, which is the whole point of it: if the \
             scan holds it, this frame is testing the launch-time list again"
        );

        /* THE SAME SCREEN, one frame later, exactly as the reader's open page would be. */
        let said = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.fights(ui, cx)
        });
        assert!(
            said.iter().any(|s| s.contains("a practice dummy")),
            "a fight fought and finished while the app was running is not on the list of fights: \
             {said:?}"
        );
        assert!(
            said.concat().contains("kept from earlier reads"),
            "the row came off the disk, and the line over the table has to say that some of these \
             rows are not in the log as it was last scanned: {said:?}"
        );
    }

    /* -------------------------------------------------- the three counting filters -- */

    /// DEFECT: THREE COUNTING FILTERS THAT WERE PER WINDOW AND PER RUN.
    ///
    /// `TrackerState::settings` lives on the `Ingest` and there is one `Ingest` per window
    /// (`main::App::new`, `windows::ChildCx::new`), so ticking a box here moved THIS window's
    /// completion percentage and left the other window's where it was: two headline percentages for
    /// one roster, six inches apart, with nothing on either screen saying why. Nothing wrote them
    /// anywhere either, so all three were back at `TrackerSettings::default` on the next launch.
    ///
    /// THE SETTING IS THE VALUE NOW AND THE INGEST'S FIELD IS A CACHE OF IT, which is why this
    /// drives the SCREEN and not the checkbox: what has to be true is that the head is computed
    /// from `Settings::tracker` and that the ingest's copy cannot outvote it. A control bound to
    /// the cache is inert under that rule, because the reconcile overwrites the cache from the
    /// setting on the next frame, and the source floor at the end is what says the controls know
    /// it.
    ///
    /// THE ROSTER IS ONE CITY, WHICH IS THE FRESH-INSTALL CASE AND NOT A CONTRIVED ONE:
    /// `TrackerSettings::default` has the ignore-cities box on, so the head has no denominator and
    /// says so, and turning the box off is the whole difference between the two frames.
    ///
    /// WHAT MUTATION MAKES THIS RED: the reconcile at the top of `kills` being dropped (the first
    /// two frames stop differing), the checkboxes going back to `cx.ingest.tracker_mut().settings`
    /// (the source floor), or the ingest's copy being made to win over the setting (the third
    /// frame).
    #[test]
    fn the_tracker_filters_are_the_settings_and_the_ingest_follows_them() {
        let dir =
            crate::fights::probe::planted("screen-tracker-filters", crate::fights::probe::CAPTURE);
        /* THE ROSTER GOES IN BEFORE THE BOOT, because `Ingest::new` loads it on the worker out of
         * the data root, which `probe::booted` points at this same folder. */
        std::fs::write(
            dir.join("kills-data.json"),
            serde_json::json!({
                "zones": { "ak": { "name": "Ak'Anon", "city": true, "mobs": [
                    { "n": "a clockwork guard", "named": false },
                    { "n": "a minotaur hero", "named": true }
                ] } }
            })
            .to_string(),
        )
        .expect("write the roster beside the planted log");
        let mut ing = crate::fights::probe::booted(&dir);
        assert!(
            ing.roster().is_some(),
            "no roster loaded, so `kills` returns at its roster guard and this proves nothing: {:?}",
            ing.roster_problem()
        );

        let mut screen = ParserScreen::default();
        let mut settings = crate::settings::Settings::default();
        assert!(
            settings.tracker.ignore_cities,
            "the shipped default, and the reason a city-only roster is the fresh-install case"
        );
        let said = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.kills(ui, cx)
        });
        assert!(
            said.concat().contains("exclude every zone"),
            "with the shipped filter on, the only zone in the roster is excluded and the head has \
             no denominator to draw: {said:?}"
        );

        /* THE READER TURNS IT OFF, which is what the checkbox writes. */
        settings.tracker.ignore_cities = false;
        let said = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.kills(ui, cx)
        });
        assert!(
            said.iter().any(|s| s == "0 of 2 mobs"),
            "the city is counted now and the head has to say so, so the filter never reached \
             `summarize`: {said:?}"
        );
        assert!(
            !ing.tracker().settings.ignore_cities,
            "the ingest's working copy did not follow the setting, so the OTHER window would go on \
             printing the old percentage under the same words"
        );

        /* AND THE SETTING WINS, WHICH IS WHAT MAKES A CONTROL BOUND TO THE CACHE INERT. */
        ing.tracker_mut().settings.witnessed = true;
        assert!(!settings.tracker.witnessed);
        let _ = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.kills(ui, cx)
        });
        assert!(
            !ing.tracker().settings.witnessed,
            "the ingest's copy outvoted the setting, so a filter could be true in one window, false \
             in the file, and neither would ever converge"
        );

        /* THE CONTROLS BIND TO THE SETTING, read out of the SOURCE for the reason
         * `every_column_flag_has_a_control_in_the_builder` reads it: driving three checkboxes needs
         * a synthetic pointer landed on three rectangles, which tests egui's hit testing, and what
         * went wrong is precisely which value a `&mut` pointed at. */
        let src = include_str!("parser.rs");
        assert!(
            src.contains("let st = &mut cx.settings.tracker;"),
            "the three counting filters must be bound to the SETTING; bound to the ingest's copy \
             they are per window, per run, and now inert as well"
        );
        for field in ["witnessed", "generic_everywhere", "ignore_cities"] {
            assert!(
                src.contains(&format!("&mut st.{field}")),
                "`TrackerSettings::{field}` has no control on the Kills view, so a counting rule \
                 the head depends on cannot be changed"
            );
        }
    }

    /* ------------------------------------------------ the note that outlives the page -- */

    /// DEFECT: A NOTE TYPED ON ANALYSIS AND THROWN AWAY BY STEPPING TO ANOTHER VIEW.
    ///
    /// `AnalysisScreen` has two writers and both need that page to be DRAWING: the field's
    /// `lost_focus` needs the widget laid out again to observe the focus going, and the repointing
    /// flush runs from that page's own body. Analysis is a VIEW of this screen, so stepping to
    /// Kills, Loot, Fights or Overlays stops it being drawn at all: the buffer went with the frame,
    /// silently, on the most ordinary click on the page. The round that fixed the arrows recorded
    /// this half and deliberately did not add a `flush_notes` with no caller; the caller is here
    /// and they landed together.
    ///
    /// THE STATE IS STOOD UP BY ASSIGNMENT (`AnalysisScreen::typed_for_test`) because "typed and
    /// not blurred" is otherwise reachable only by landing a synthetic pointer on a `TextEdit` and
    /// feeding it key events, which tests egui rather than this rule. Everything after that is the
    /// shipped path: `ParserScreen::ui`, in a real frame, with the view the reader stepped to.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the `self.analysis.flush_notes(cx)` call from
    /// `ParserScreen::ui`, or moving it inside the Analysis arm of the match (the first half); or
    /// dropping the `self.view != 3` guard so it runs on every frame including the ones the reader
    /// is typing into (the second half, which is a settings file write per frame and a buffer
    /// emptied under the cursor).
    #[test]
    fn a_note_typed_on_analysis_is_written_when_the_reader_steps_to_another_view() {
        const TYPED: &str = "adds on the third pull";
        let dir = crate::fights::probe::planted("screen-note-flush", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        let drawn = ing
            .fights()
            .last()
            .expect("the capture holds fights")
            .start
            .clone();

        /* ---- he steps away, and the note is written under the fight he typed it about ---- */

        let mut screen = ParserScreen::default();
        let mut settings = crate::settings::Settings::default();
        screen.show(View::Analysis);
        screen
            .analysis
            .typed_for_test("Sun Jul 12 20:10:00 2026", TYPED);
        /* The click. In the main window this is `main::on_section` calling `show`; in the pop-out
         * it is the view row writing the index. Either way the next frame has another view on it. */
        screen.show(View::Fights);
        let _ = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.ui(ui, cx)
        });
        assert_eq!(
            settings
                .fight_notes
                .get("Sun Jul 12 20:10:00 2026")
                .map(String::as_str),
            Some(TYPED),
            "the reader typed a note and stepped to Fights, and it was dropped with the frame"
        );

        /* ---- and nothing is written while he is still on the fight he is annotating ---- */

        let mut screen = ParserScreen::default();
        let mut settings = crate::settings::Settings::default();
        screen.show(View::Analysis);
        screen.analysis.typed_for_test(&drawn, "still typing");
        let _ = painted_with(&mut ing, &mut settings, &mut screen, |ui, s, cx| {
            s.ui(ui, cx)
        });
        assert!(
            settings.fight_notes.is_empty(),
            "the Analysis view is on screen and the buffer belongs to the fight it is drawing, so \
             there is nothing to flush; flushing anyway is a settings file write per frame and a \
             field emptied under the reader's cursor: {:?}",
            settings.fight_notes
        );
    }

    /// DEFECT: TWO WINDOWS PRINTING DIFFERENT SESSION NUMBERS UNDER IDENTICAL WORDS.
    ///
    /// `Session` counts live events only, from the moment the reader that owns it started, and
    /// each window has a reader of its own: `main::App::new` builds one at launch and
    /// `windows::ChildCx::new` builds another the first time a tool window opens. The strip drew
    /// six figures under `active`, `kills`, `xp`, `kills/h`, `xp/h` and `loots` with nothing
    /// anywhere saying whose they were, so the owner with the parser popped out over the game had
    /// two kill counts for one night and no way to tell which was his.
    ///
    /// THE EVENTS ARE APPENDED TO A REAL LOG AND PUMPED THROUGH A REAL INGEST, because a `Session`
    /// is only ever filled by the live tail: handing the screen a hand built one would prove the
    /// strip can print a number, which is not the thing in doubt.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the lead-in label from `ParserScreen::session`, or
    /// rewording it to something that does not name the window.
    #[test]
    fn the_session_strip_says_whose_numbers_these_are() {
        let dir = crate::fights::probe::planted("screen-session", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        let log = dir.join(format!(
            "eqlog_{}_freeport.txt",
            crate::fights::probe::OWNER
        ));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log)
                .unwrap_or_else(|e| panic!("{}: {e}", log.display()));
            /* After the capture's last line, 23:46:18, so nothing here reaches back past a high
             * water mark. */
            writeln!(f, "[Wed Jul 15 23:50:00 2026] You have slain a gnoll pup!").unwrap();
            writeln!(f, "[Wed Jul 15 23:51:00 2026] You have slain a gnoll pup!").unwrap();
        }
        /* The pump reads at most once a second, so this waits for a poll rather than assuming
         * one. Ten seconds is a ceiling and not a measurement. */
        let began = std::time::Instant::now();
        while ing.session().is_empty() && began.elapsed() < Duration::from_secs(10) {
            let _ = ing.tail();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            ing.session().kills,
            2,
            "the two appended kills never reached the session, so this frame would be testing the \
             empty strip. Problem: {:?}",
            ing.active_problem()
        );

        let said = painted(&mut ing, |ui, s, cx| s.session(ui, cx));
        assert!(
            said.iter().any(|s| s == "2"),
            "the kill count is not on the strip: {said:?}"
        );
        assert!(
            said.concat().contains(ParserScreen::SESSION_SCOPE),
            "the strip prints six figures and never says whose they are. The main window and a \
             pop-out each count only what their own reader has seen, and they disagree: {said:?}"
        );
        assert!(
            ParserScreen::SESSION_SCOPE.contains("this window"),
            "the words have to name the window, because the other one is showing different numbers \
             under the same six labels"
        );
        assert!(
            ParserScreen::SESSION_SCOPE_WHY.contains("reader of its own"),
            "and the hover has to say why, or the lead-in reads as a note about layout"
        );
    }
}
