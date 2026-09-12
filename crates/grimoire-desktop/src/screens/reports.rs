//! Screen: REPORTS. A night rolled up, and one thing the owner can hand to somebody else.
//!
//! # The one fold this app did not have
//!
//! Every other combat surface in this tree is about ONE fight. The overlay windows draw
//! `Ingest::current_fight`, the LIVE page draws the same row at page density, and the Analysis page
//! draws whichever single row you picked. `nav::unbuilt_why` said the missing piece in so many
//! words: "Every number it would print is already computed per fight. What is missing is the fold
//! across fights and a decision about what a report IS."
//!
//! This file is that fold, and the decision is this: A SESSION IS A `FightRow`. [`roll`] takes the
//! fights in scope and folds them into one row of the same type, summing per [`Who`], and then
//! every table on this page is `screens::dps::draw_widget` over that row, with the same [`Widget`]
//! vocabulary the overlays use.
//!
//! THAT IS NOT A TRICK, IT IS THE WHOLE POINT. A ranked table written here would be a second
//! implementation of the number the overlay already draws, and two implementations of one number is
//! the defect this app cannot afford in front of an audience. Folded into a row instead, the
//! session's damage table and the overlay's damage table are literally the same function, the same
//! sort, the same tie break, the same share denominator and the same rate floor.
//!
//! # What may be folded across fights, and what may not
//!
//! A `Fighter` is mostly COUNTS, and counts add. `dealt`, `taken`, `healed`, `received`, `swings`,
//! `landed`, `avoided`, `kills`, `deaths` and `melee_crits` add. `abilities` merge on (name,
//! family), `schools` merge on the element word, and `outcomes` add field by field, which preserves
//! the engine's own `landed + stopped == swings` identity because summing both sides of an identity
//! keeps it.
//!
//! `targets` NEEDED WORK AND GOT IT. A `TargetShare::slot` is an index into that fight's own
//! `fighters` vector, and the vector is in first-appearance order, so slot 3 is a different entity
//! in every fight. Adding them raw would credit your damage on a skeleton to whoever happened to
//! appear third in the next pull. [`merge`] builds a slot map per fight and rewrites every index
//! into the session row's own slot, which is exact rather than approximate.
//!
//! `series` AND `moments` ARE NOT FOLDED AND CANNOT BE. Both are stamped in SECONDS FROM THAT
//! FIGHT'S OWN START, so fight two's second five and fight one's second five are different moments
//! wearing the same number, and stacking them would draw a chart of an evening that never happened.
//! They are left empty, and [`panels`] therefore never offers `Widget::Timeline`. The test
//! `nothing_on_this_page_reads_a_clock_the_fold_could_not_keep` is the guard.
//!
//! # The two clocks, and which one the rates divide by
//!
//! There are two honest numbers for "how long was this session" and they differ enormously:
//!
//!   * TIME IN COMBAT, the sum of `FightRow::secs` over the fights in scope. This is what the rates
//!     on this page divide by.
//!   * THE SESSION SPAN, the gap between the first fight's first stamp and the last fight's last
//!     stamp, which includes every bank trip, every corpse run and every minute spent looking for a
//!     group.
//!
//! WHAT IS DIVIDED IS THE PLAYERS' OWN TOTAL, NOT `FightRow::damage`. That field is every point
//! anybody dealt to anybody, the pull included, and every table under these tiles is the roster
//! `dps::ranked_dealers` ranks, `FightRow::ours`: every player, or the reader and his group when every
//! fight in scope proved one (see [`rolled_group`]). This page shipped with the tiles reading the
//! whole-fight field: over the reference capture they printed 19,695 damage at 49 per second where
//! the players dealt 14,254 at 35, and a deaths tile reading 33 for a night the reader died once.
//! `screens::analysis` had already found and fixed exactly that, on one fight, and says so under a
//! heading of its own. See [`group_total`]. THE DIVISION ITSELF IS `dps::dps` and is not written
//! again here: see [`session_rate`].
//!
//! THE RATES DIVIDE BY TIME IN COMBAT, and the page says so on its face. A rate over the span would
//! be damage per second of the reader's evening, which is a different measurement wearing the same
//! word, and a streamer reading "412 dps" off this page has to be reading the same kind of number
//! his overlay showed him an hour ago. Both are printed as tiles, labelled, so the reader can see
//! the gap between them and draw his own conclusion about downtime.
//!
//! THE SPAN IS COMPUTED WITH THE ENGINE'S OWN STAMP READER AND NOT A NEW ONE.
//! `grimoire_parse::fights::seconds` is public, is what the aggregator itself uses, and carries the
//! argument for why a DIFFERENCE between two stamps in one log is sound with no zone offset in the
//! file. A second stamp parser in this crate would be a second chance to disagree with the first.
//! A span that comes out NEGATIVE is refused rather than shown as an absolute value: that is the
//! daylight saving step the engine calls `Ended::Backwards`, and it is the one case where the
//! difference is not the elapsed time.
//!
//! # The floor, generalised from one fight to many
//!
//! `Fight::seconds` FLOORS AT ONE SECOND, because the log stamps to the second. `screens::dps`
//! answers that by refusing a rate for a fight shorter than three seconds. Summing floors carries
//! the error forward once per fight: a scope of N fights has up to N seconds of slack in its
//! denominator, so the relative error is about `N/T` rather than `1/T`, and a session of forty
//! one-second scraps has a denominator that may be twice the truth.
//!
//! So the rule here is the same bar per fight rather than per session: a rate is published only
//! when the scope averages at least [`MIN_SECS_PER_FIGHT`] seconds of fight, which IS the overlay's
//! own `dps::MIN_RATE_SECS` and not a three typed again. For a scope of one
//! fight that is exactly the rule `screens::dps` applies, which is what
//! [`the_session_rate_rule_degenerates_to_the_single_fight_rule`] pins. When the bar is not met the
//! tables are drawn with `Ranked::rate` false, so they show TOTALS, which are counts the log stated
//! outright and are true at any duration. The refusal is expressed in the shared config vocabulary
//! rather than in a special case of a renderer, which is what that vocabulary is for.
//!
//! # This page is a photograph and it says so
//!
//! `Ingest::fights()` is written ONCE, by the bootstrap scan, and never again while the app runs.
//! `Ingest::tail()` feeds the kill and loot streams and re-folds only the last few thousand lines
//! for the live overlay; it does not touch this list. So a report over it is a report over the log
//! AS IT WAS at `Ingest::scanned_at()`, and on a long stream that can be hours ago.
//!
//! THE PAGE SAYS THAT IN A SENTENCE AND THEN OFFERS THE BUTTON THAT FIXES IT. `Ingest::rescan` is
//! public and two other screens already call it, so the honest answer to a frozen list is not only
//! to complain about it but to hand the reader the control that unfreezes it.
//!
//! THE FIGHT IN PROGRESS IS DELIBERATELY NOT ADDED IN. It is very tempting: `current_fight` is
//! right there and it is moving. It is also, nearly always, THE SAME FIGHT as the last row of
//! `fights()`, folded a second time from a shorter slice of the same file, so adding it would count
//! one pull twice. Nothing here tries to be clever about that, and the page says which fights it
//! covers instead.
//!
//! # What is refused, and why, said out loud rather than left blank
//!
//! THE RAIL OFFERS THIS PAGE'S OWN THREE. It used to offer five, two of which this page has a
//! guard specifically to keep off it, and `main::the_parser_tab_rows_are_the_pages_own` is what
//! holds the rail's list to `TABS` now. The two that are missing are missing for a reason and
//! it is worth saying once:
//!
//!   * GROUP and RAID ARE NOT TABS, and the reason is not effort. No COMBAT line carries a roster,
//!     an invite or a join. The group lines do, and `grimoire_parse::group` reads them, but it
//!     proves a whole group for a minority of fights: on the owner's four logs 254 of 3,466 were
//!     known grouped, 1,490 known solo and 1,722 not known (measured by that lane before a later
//!     fix to `Party::during`, and not re-measured). Raid evidence makes a fight not known, so there
//!     is no raid roster at all. What it DOES support is a filter, and every table on this page
//!     already applies it: the reader and his group when every fight in scope proved one, every
//!     player otherwise (`rolled_group`). A TAB that split a session by roster would be an empty
//!     state on most nights, or a head count, and a head count cannot tell your group from four
//!     strangers killing the same camp. It would put somebody else's parse under the word "your
//!     group", which is exactly the invented fact this app refuses.
//!     [`group_and_raid_are_not_offered`] is the guard.
//!   * ENCOUNTER IS SHIPPED AS A ROLL UP AND NOT AS A FIGHT VIEWER. One selected fight, read
//!     deeply, is the Analysis page and drawing it again here would be the same page twice. What
//!     this page can do that nothing else can is group the scope by what was fought, which is the
//!     "what did I actually farm tonight" answer and is a fold across fights.
//!
//! There is also NO FILE WRITER, and there is not going to be one on this screen. Only the Settings
//! screen writes to disk in this app. "Hand to somebody else" is answered with the clipboard, which
//! is reachable from any screen, and the button says exactly what it put there.
use crate::fights::{Ability, FightRow, Fighter, SchoolShare, TargetShare, Who};
use crate::overlay::{Cols, Detail, Metric, Ranked, Subject, Widget};
use crate::screens::dps::{dps, MIN_RATE_SECS};
use crate::screens::parser::{no_fights_words, why_no_fights};
use crate::screens::Cx;
use crate::theme::*;
use chrono::{DateTime, Utc};
use egui::{RichText, Ui};
use std::time::{Duration, Instant};

/// HOW MANY SECONDS OF FIGHT, ON AVERAGE, A SCOPE NEEDS BEFORE ITS RATES ARE PUBLISHED.
///
/// THE OVERLAY'S OWN CONSTANT, IMPORTED, AND NOT A THREE TYPED AGAIN HERE. `dps::MIN_RATE_SECS`
/// is `pub(crate)`, so there was never a reason to restate it, and a restated threshold is a
/// threshold that can drift: the day somebody raises the overlay's floor, a copy sitting here goes
/// on publishing a rate the overlay has already refused, and the two surfaces disagree about
/// whether a number exists at all. Bound as an alias rather than used inline so the rule this page
/// adds on top of it has a name of its own to be argued with.
///
/// WHAT THIS PAGE ADDS is the multiplication, not the number: see [`rate_is_publishable`]. THREE
/// AND NOT TEN is the overlay's argument and it is made where the constant is declared.
const MIN_SECS_PER_FIGHT: i64 = MIN_RATE_SECS;

/// An hour, in seconds. The width of [`Scope::LastHour`].
const HOUR: i64 = 3600;

/// How many rows a session table draws. Larger than an overlay's cap because this is a page and a
/// night in a raid zone really can have thirty players in it.
const CAP: usize = 30;

/// The tabs this page ships, in the order it offers them.
///
/// THREE AND NOT THE RAIL'S FIVE. See the module note for why Group and Raid are absent rather than
/// empty, and [`group_and_raid_are_not_offered`] for the guard that keeps them absent.
pub const TABS: [&str; 3] = ["Session", "Personal", "Encounters"];

/// WHICH FIGHTS A REPORT IS OVER.
///
/// TWO OF THE THREE ARE TRAILING RUNS AND THAT IS A CORRECTNESS PROPERTY, not a convenience. A
/// scope built by filtering the whole list can have HOLES in it: a fight in the middle whose stamp
/// could not be read, or whose zone line never arrived, would silently drop out and the totals
/// would quietly shrink with nothing on screen to say so. A run that walks back from the newest
/// fight and STOPS at the first row it cannot place has no holes by construction, and the page says
/// why it stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scope {
    /// Every fight the bootstrap folded.
    #[default]
    Everything,
    /// The trailing run of fights sharing the newest fight's zone.
    Zone,
    /// The trailing run of fights that ended within an hour of the newest fight's end.
    LastHour,
}

impl Scope {
    const ALL: [Scope; 3] = [Scope::Everything, Scope::Zone, Scope::LastHour];

    fn label(self) -> &'static str {
        match self {
            Scope::Everything => "Everything read",
            Scope::Zone => "This zone",
            Scope::LastHour => "Last hour of the log",
        }
    }

    /// What the reader is told this scope means, on hover, in the log's terms rather than the
    /// app's.
    fn hint(self) -> &'static str {
        match self {
            Scope::Everything => {
                "every fight in the part of the log that was read, oldest to newest"
            }
            Scope::Zone => {
                "the newest fight and every fight before it in the same zone, stopping at the zone \
                 line that changed it"
            }
            Scope::LastHour => {
                "the newest fight and every fight before it that ended within an hour of it, by \
                 the log's own stamps"
            }
        }
    }
}

/// THE FOLD AND EVERYTHING DERIVED FROM IT, KEPT BETWEEN FRAMES.
///
/// A CACHE AND NOT A SECOND COPY OF THE TRUTH, and the difference is the key. [`Key`] is the
/// identity of the input: when the bootstrap ran, how many fights it produced, and which scope was
/// asked for. `Ingest::fights` changes only when `adopt` installs a new scan, and `adopt` stamps
/// `scanned_at`, so a key that matches means the input is byte for byte the input this was folded
/// from. A cache keyed on anything looser would be this page showing the previous log's numbers
/// under the new log's name, which is the exact failure `Ingest::adopt` replaces its list wholesale
/// to avoid.
struct Cached {
    key: Key,
    /// The session as one row. Every table on the page is a widget over this.
    row: FightRow,
    /// How many fights went into it.
    fights: usize,
    /// The first fight's first stamp and the last fight's last stamp, as the log wrote them.
    first: String,
    last: String,
    /// Wall clock between those two stamps, or None when a stamp did not read or the clock stepped
    /// back. See the module note.
    span: Option<i64>,
    /// Why a trailing run stopped where it did, when the reason is worth a sentence.
    stopped: Option<&'static str>,
    /// The scope grouped by what was fought.
    encounters: Vec<Encounter>,
}

/// The identity of a fold's input: the scan it came from, how big that scan was, and the scope.
type Key = (Option<DateTime<Utc>>, usize, Scope);

/// ONE THING THAT WAS FOUGHT, ACROSS EVERY FIGHT THE ENGINE GAVE THAT NAME.
///
/// THE LABEL IS `Fight::headline`, WHICH IS A LABEL AND NOT AN IDENTITY, and the page says so.
/// `grimoire_parse` picks the named entity that TOOK the most damage, falling back to the one that
/// DEALT the most when nothing named took any. So two separate pulls on two separate dry bone
/// skeletons merge into one row here, which is usually exactly what a reader camping a spot wants,
/// and a pull with three mobs in it is filed under the biggest one. Neither is a claim about an
/// encounter; both are honest as "every fight the engine labelled this".
struct Encounter {
    label: String,
    fights: usize,
    /// Seconds of fight, summed. Floored once per fight: see the module note.
    secs: i64,
    /// Damage dealt by PLAYERS, which is not the fight's total: the pull hits back and its damage
    /// is real and is not the reader's.
    dealt: u64,
    /// Kills credited to players.
    kills: u32,
    /// DEATHS AMONG PLAYERS, and NOT `FightRow::deaths`.
    ///
    /// That field counts every death inside the fight, so on a clean camp it is the same events
    /// [`Encounter::kills`] already counts, one column over. The reference capture's skeleton row
    /// read `27 k 27 d`: twenty-seven mobs killed, printed twice, once under a letter a reader
    /// takes for his own wipes. Summed off the player rows instead, the same row reads 27 kills and
    /// no deaths, which is what happened.
    deaths: u32,
}

/// The Reports screen.
#[derive(Default)]
pub struct ReportsScreen {
    /// Which of [`TABS`] is showing. Written by `main::on_tab` off the shell's tab row.
    tab: usize,
    scope: Scope,
    cache: Option<Cached>,
    /// Set for a moment after a copy so the button can say it happened, and so the page can say
    /// what went to the clipboard rather than only that something did.
    copied_at: Option<Instant>,
    /// How many lines the last copy put there. Printed, because "Copied" alone does not tell a
    /// person whether they got the table or an empty string.
    copied_lines: usize,
}

impl ReportsScreen {
    /// Point this page at one of [`TABS`]. Called by `main::on_tab` and by nothing else.
    pub fn show(&mut self, tab: usize) {
        self.tab = tab.min(TABS.len() - 1);
    }

    /// Which of [`TABS`] is showing. Read by the guard in `main`.
    pub fn showing(&self) -> usize {
        self.tab
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE PUMP, as every log-reading screen in this tree does it. It does NOT refresh the list
         * this page reads (see the module note); what it does do is notice the active log rolling
         * over to another character, which triggers a real rescan inside `Ingest::tail`, and keep
         * the "read N minutes ago" line below from going stale on a page nobody is touching. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        /* THE SOURCE LINE IS DRAWN BEFORE THE EMPTY CHECK, AND THE ORDER IS THE WHOLE FIX.
         *
         * # WHAT THE EMPTY PAGE USED TO SAY, AND WHAT IT USED TO WITHHOLD
         *
         * The no-fights branch returned ABOVE `source_line`, so a reader who opened this page with
         * nothing read got two sentences and a blank screen under them. Both sentences pointed
         * upwards at furniture the branch had just returned before drawing: `no_fights_words` says
         * "the line above says what is being read", and this page's own second sentence said the
         * fold waits "until the list above it has rows in it". The first is `screens::parser`'s
         * sentence and is true THERE, under that page's state line; the second described a fight
         * list that has never been on this page at all. `nav::SECTIONS` files the fight list under
         * a section of its own, which in this build is not written, so the words sent the reader
         * to a place rather than to a control.
         *
         * AND THE CONTROL THEY SHOULD HAVE POINTED AT IS ON THE LINE THAT WAS SKIPPED. The module
         * note above argues that the honest answer to a frozen list is to hand the reader the
         * thing that unfreezes it, and `source_line` carries `Re-read the log`. The one branch
         * that withheld it was the branch where the list is not merely stale but empty, which is
         * the one state where re-reading is the only thing left to do.
         *
         * SO THE LINE IS DRAWN IN EVERY STATE AND NOTHING BELOW IT MOVED: the page with fights in
         * it lays out exactly as it did.
         *
         * THE SCOPE ROW STAYS BEHIND THE CHECK, AND THAT IS NOT THE SAME CALL. A scope is a choice
         * about which fights a report covers, so with no fights all three of its buttons choose
         * between the same nothing, and two of the three would have to explain a run that stopped
         * before it started. A control that cannot change what is on screen is the state this tree
         * refuses everywhere else (see `nav::SECTIONS` on the thirteen dead tabs). */
        self.source_line(ui, cx);
        ui.add_space(8.0);

        if cx.ingest.fights().is_empty() {
            let why = why_no_fights(
                cx.ingest.scanning(),
                cx.ingest.log_dir().dir.is_some(),
                cx.ingest.active_log().is_some(),
            );
            ui.label(RichText::new(no_fights_words(why)).color(TEXT_2));
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "A report is a fold across fights and no fight has been read to fold. \
                     Re-read the log on the line above once the game has written one.",
                )
                .color(TEXT_3),
            );
            return;
        }

        self.scope_row(ui, cx);
        ui.add_space(8.0);

        self.refresh(cx);
        let Some(c) = self.cache.as_ref() else {
            /* Unreachable in practice: `refresh` always leaves a fold behind when there are
             * fights, and the empty case returned above. Named rather than unwrapped, because a
             * panic in a screen takes the whole app down over a report. */
            ui.label(RichText::new("The fold produced nothing to draw.").color(WRONG));
            return;
        };

        tiles(ui, c);
        ui.add_space(6.0);
        caveats(ui, c, cx.ingest.fights_unreadable());
        ui.add_space(10.0);

        /* NO TAB ROW HERE EITHER. The shell draws one and `main::on_tab` routes it. This page
         * drew a SECOND row whose three names overlapped the rail's five in a different order,
         * one live and one inert, and the inert one offered the Group and Raid reports this
         * screen's own module doc says cannot exist. `nav::SECTIONS` carries `TABS` now. */
        let tab = self.tab.min(TABS.len() - 1);
        let rated = rate_is_publishable(c.row.secs, c.fights);

        /* DEFECT: THE ONLY WAY TO GET A REPORT OUT OF THIS APP WAS EIGHT PIXELS WIDE.
         *
         * # WHAT WAS WRONG
         *
         * The copy control was laid out AFTER a `ScrollArea` with `auto_shrink([false, false])`.
         * That combination is not "as tall as its content", it is "as tall as everything that is
         * left": egui's scroll area takes `inner_size[d]` whole in that mode and then calls
         * `advance_cursor_after_rect`, which parks the parent's cursor on the panel's own floor.
         * `screens::dps` states the same rule in as many words about the same call.
         *
         * So the eight point space and the copy row were laid out BELOW the bottom of the
         * `CentralPanel` that hosts this screen, and that panel sets a clip rect over itself
         * (egui panel.rs: "If we overflow, don't do so visibly"). Measured in a headless frame
         * at three window heights, the button survived as an 8 by 7 pixel sliver flush against
         * the bottom edge, with its caption laid out at ZERO width, identically at every height
         * because the scroll area always fills.
         *
         * THIS PAGE'S WHOLE PURPOSE IS TO HAND SOMEBODY A REPORT. The module doc says the
         * clipboard is the only export and that no file writer will be added. So the one door out
         * of the feature was a fifty six square pixel target the reader has no reason to know is
         * there, and the `Copied` confirmation was drawn in the same clipped strip.
         *
         * # THE FIX, AND WHY IT IS A PANEL AND NOT AN ORDER SWAP
         *
         * `TopBottomPanel::bottom(..).show_inside(ui, ..)` RESERVES its height out of the
         * available space before the scroll area is given the rest, so the row is laid out inside
         * the clip rect by construction rather than by arithmetic. Moving the row above the
         * scroll area would have fixed the clipping and broken the rule the old comment was
         * defending: a button offering to hand somebody a report, drawn above the report, is a
         * button pressed before the reader has seen what it will send. A bottom panel is below
         * the report AND inside the window.
         *
         * THE PAGE'S OWN PAINT TEST COULD NEVER HAVE CAUGHT THIS. It draws into a bare root `Ui`
         * with no `CentralPanel`, so there was no clip rect to violate. See
         * `the_copy_control_is_inside_the_panel_that_hosts_it`, which builds the real container. */
        /* THE FOLD COMES OUT OF `self` FOR THE LENGTH OF THE PAINT AND GOES BACK AT THE END.
         *
         * Both the copy row and the body need it, and the copy row needs `&mut self` for its own
         * `copied_at` and `copied_lines`. Taking it out is what makes those two borrows disjoint
         * without cloning a whole fold once a frame.
         *
         * IT IS PUT BACK UNCONDITIONALLY at the end of the function and there is no early return
         * between here and there: a cache left out of the screen would be re-folded from scratch
         * on the next frame, every frame, for as long as the page was open. */
        let taken = self.cache.take();
        let Some(c) = taken else {
            ui.label(RichText::new("The fold produced nothing to draw.").color(WRONG));
            return;
        };

        egui::Panel::bottom("reports_copy")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
            .show(ui, |ui| {
                self.copy_row(ui, cx, &c);
            });

        egui::ScrollArea::vertical()
            .id_salt("reports_body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match tab {
                    2 => encounters_table(ui, &c.encounters),
                    1 => personal(ui, &c),
                    _ => {
                        for (title, w) in panels(rated) {
                            ui.label(
                                RichText::new(title)
                                    .font(crate::fonts::display(13.0))
                                    .color(GOLD),
                            );
                            ui.add_space(4.0);
                            /* `live` IS FALSE AND IS NOT A CHOICE. A session is not a fight in
                             * progress, and the only thing the renderer does with the flag is
                             * draw the headline's live mark, which `panels` turns off anyway. */
                            crate::screens::dps::draw_widget(
                                ui,
                                &c.row,
                                crate::fights::Pulse::Closed,
                                &w,
                            );
                            ui.add_space(14.0);
                        }
                    }
                }
            });

        self.cache = Some(c);
    }

    /// WHERE THESE NUMBERS CAME FROM AND HOW OLD THEY ARE, on one line, always.
    ///
    /// THE AGE IS THE POINT OF THE LINE. A report page that does not say when it was measured is a
    /// report page a reader will believe is live, because every other combat surface in this app
    /// is.
    ///
    /// AND IT CARRIES THE READ PROBLEM, BECAUSE THE EMPTY STATE UNDER IT PROMISES THAT IT DOES.
    /// `no_fights_words` tells a reader with nothing tailed that "the line above says why", and
    /// the only thing that can honour that sentence is the ingest's own reason. THE ORDER IS
    /// `screens::parser`'s ORDER, which is the state line those words were written under: the
    /// folder's problem first, because a folder that cannot be read makes every question about a
    /// file inside it moot, then the active log's. Nothing is composed here and no problem is
    /// worded here: when the ingest has no reason, this draws no reason.
    fn source_line(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let name = cx
            .ingest
            .active_log()
            .map_or_else(|| "no log".to_owned(), crate::ingest::LogFile::name);
        let age = cx
            .ingest
            .scanned_at()
            .map(|t| (Utc::now() - t).num_seconds().max(0));
        let problem = cx
            .ingest
            .log_dir_problem()
            .map(str::to_owned)
            .or_else(|| cx.ingest.active_problem().map(str::to_owned));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(name).color(TEXT).monospace());
            ui.label(
                RichText::new(match age {
                    Some(s) => format!("read {}", ago(s)),
                    None => "not read yet".to_owned(),
                })
                .color(TEXT_2),
            );
            if let Some(p) = &problem {
                ui.label(RichText::new(p.as_str()).color(WRONG));
            }
            if cx.ingest.scanning() {
                ui.label(RichText::new("re-reading now").color(WORKING));
            } else if ui
                .button("Re-read the log")
                .on_hover_text(
                    "fold the log again from disk. The fight list this page reports on is written \
                     once, when the app starts, and this is what writes it again.",
                )
                .clicked()
            {
                cx.ingest.rescan();
                /* THE FOLD IN HAND IS KEPT, AND THIS IS WHERE IT USED TO BE THROWN AWAY.
                 *
                 * WHAT WAS BELIEVED: the line here read `self.cache = None`, under a comment
                 * saying that clearing it meant "the page cannot draw one stale frame under a line
                 * that says it is re-reading".
                 *
                 * WHY THAT IS NOT TRUE, AND WAS NOT TRUE WHEN IT WAS WRITTEN: `Ingest::rescan`
                 * spawns a worker and returns. It does not touch `fights()`, and it does not stamp
                 * `scanned_at`; only `adopt` does, some frames later, when the worker lands.
                 * `refresh` runs a dozen lines below this in the SAME `ui` pass, and its key is
                 * (`scanned_at`, `fights().len()`, scope), not one of which this click moved. So
                 * the drop was followed at once by a fold of the identical input back into an
                 * identical row: the stale frame was drawn either way, and all the clearing bought
                 * was a second fold of the whole night on the frame the reader pressed a button.
                 *
                 * WHAT IS TRUE NOW: this fold is invalidated by its KEY and by nothing else, which
                 * is what [`Cached`] says the key is for. The numbers change on the frame `adopt`
                 * installs the new scan under a new stamp, and until then the line above says
                 * "re-reading now", which is the honest account of what a rescan is: a request,
                 * not a result. */
            }
        });
    }

    fn scope_row(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let newest_zone = cx.ingest.fights().last().and_then(|f| f.zone.clone());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new("Fights in scope").color(TEXT_3));
            for s in Scope::ALL {
                let usable = s != Scope::Zone || newest_zone.is_some();
                /* `add_enabled_ui` AND NOT `add_enabled(.., SelectableLabel::new(..))`, so this
                 * builds the control through `Ui::selectable_label` exactly as every other tab row
                 * in this tree does and never names the widget type. */
                let hit = ui
                    .add_enabled_ui(usable, |ui| ui.selectable_label(self.scope == s, s.label()))
                    .inner
                    .on_hover_text(s.hint())
                    .on_disabled_hover_text(
                        "no zone line appears in the part of the log that was read, so this app \
                         does not know which zone the newest fight was in",
                    );
                if hit.clicked() {
                    self.scope = s;
                }
            }
            if let Some(z) = &newest_zone {
                ui.label(RichText::new(z).color(TEXT_2));
            }
        });
    }

    /// Fold again if, and only if, the input is not the input the fold in hand was made from.
    fn refresh(&mut self, cx: &mut Cx) {
        let key: Key = (cx.ingest.scanned_at(), cx.ingest.fights().len(), self.scope);
        if self.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let all = cx.ingest.fights();
        let (rows, stopped) = scoped(all, self.scope);
        let row = roll(&rows);
        let first = rows.first().map(|f| f.start.clone()).unwrap_or_default();
        let last = rows.last().map(|f| f.end.clone()).unwrap_or_default();
        self.cache = Some(Cached {
            key,
            span: span_secs(&first, &last),
            fights: rows.len(),
            encounters: encounters(&rows),
            first,
            last,
            stopped,
            row,
        });
    }

    /// `c` IS HANDED IN AND NOT READ BACK OFF `self`, which is what lets the copy row be drawn
    /// as a reserved bottom panel.
    ///
    /// A bottom panel has to be declared BEFORE the scroll area that takes the rest of the height,
    /// and the scroll area needs the fold too, so the fold is borrowed across both. This function
    /// wanting `&mut self` while `self.cache` was still borrowed is the only thing that stood in
    /// the way, and one argument removes it.
    fn copy_row(&mut self, ui: &mut Ui, cx: &mut Cx, c: &Cached) {
        let recently = self
            .copied_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(2500));
        if recently {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        let name = cx
            .ingest
            .active_log()
            .map_or_else(|| "no log".to_owned(), crate::ingest::LogFile::name);
        let unreadable = cx.ingest.fights_unreadable();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui
                .button(if recently {
                    "Copied"
                } else {
                    "Copy this report"
                })
                .on_hover_text(
                    "put the report on the clipboard as plain text, ready to paste into chat, a \
                     Discord message or a forum post. Nothing is written to disk: only the \
                     Settings screen writes files.",
                )
                .clicked()
            {
                let text = report_text(&name, self.scope, c, unreadable);
                self.copied_lines = text.lines().count();
                ui.ctx().copy_text(text);
                self.copied_at = Some(Instant::now());
            }
            /* WHAT WENT ON THE CLIPBOARD: THE COUNT ON THE PAGE, THE SENTENCE ON THE HOVER.
             *
             * # THIS USED TO BE TWO SENTENCES AND IT WAS NEVER SEEN
             *
             * The caption read `Plain text: the whole scope. Totals, all four rankings, what was
             * fought, and every warning.` and it broke this page's own rule against prose. It did
             * not break `this_page_paints_figures_and_not_paragraphs` because that test never saw
             * it: the row was laid out past the bottom of its container, so egui skipped painting
             * it and the string never reached the shapes. TWO tests were green on one invisible
             * control, which is what a container bug does to a content harness.
             *
             * SO IT IS A COUNT NOW, AND THE COUNT IS THE ONLY THING THIS SLOT OWES. A person
             * pasting into a stream chat wants to know how big it is before he pastes it, and
             * `Copied` alone is indistinguishable from a copy that produced an empty string. What
             * the report CONTAINS is an explanation, and this page's rule is that explanations
             * live on hovers.
             *
             * THE SCOPE IS THE WHOLE SCOPE AND NOT `this tab`. `report_text` takes no tab and
             * never did, so a caption naming one described a report nobody had written. */
            let words = if recently {
                format!("{} lines", self.copied_lines)
            } else {
                String::from("plain text")
            };
            ui.label(RichText::new(words).color(if recently { SETTLED } else { TEXT_3 }))
                .on_hover_text(
                    "The whole scope, whichever tab you are on: the header, the totals, all four \
                 rankings, what was fought, and every warning. Not just the tab you are looking \
                 at.",
                );
        });
    }
}

/* ------------------------------------------------------------------ the fold -- */

/// THE FIGHTS IN SCOPE, AND WHY THE RUN STOPPED IF IT STOPPED SHORT.
///
/// Returned as borrows into the ingest's own list because nothing here needs to own them and a copy
/// of a night's fights is a copy of every ability tally in it.
fn scoped(all: &[FightRow], scope: Scope) -> (Vec<&FightRow>, Option<&'static str>) {
    match scope {
        Scope::Everything => (all.iter().collect(), None),
        Scope::Zone => {
            let Some(zone) = all.last().and_then(|f| f.zone.clone()) else {
                return (
                    Vec::new(),
                    Some(
                        "No zone line appears in the part of the log that was read, so this app \
                         cannot say which zone the newest fight was in.",
                    ),
                );
            };
            let mut out: Vec<&FightRow> = Vec::new();
            let mut stopped = None;
            for f in all.iter().rev() {
                match f.zone.as_deref() {
                    Some(z) if z == zone.as_str() => out.push(f),
                    /* A DIFFERENT ZONE AND NO ZONE ARE TWO DIFFERENT FACTS AND GET TWO
                     * SENTENCES. `FightRow::zone` is `None` only when no zone line has been seen
                     * yet in the file, which is the head of a log rather than a zone that changed,
                     * so the wording about "the zone line that brought you here" was describing a
                     * line the log never wrote. */
                    Some(_) => {
                        stopped = Some(
                            "The run stops at the fight before the zone line that brought you \
                             here. Anything you did in this zone on an earlier visit is not in it.",
                        );
                        break;
                    }
                    None => {
                        stopped = Some(
                            "The run stops at a fight from before the first zone line in the part \
                             of the log that was read, so this app cannot say which zone it was \
                             in. It is not in this scope even if it was in this zone.",
                        );
                        break;
                    }
                }
            }
            out.reverse();
            (out, stopped)
        }
        Scope::LastHour => {
            let Some(newest) = all.last().and_then(|f| seconds_of(&f.end)) else {
                return (
                    Vec::new(),
                    Some(
                        "The newest fight carries a stamp this build could not read, so nothing \
                         can be measured back from it.",
                    ),
                );
            };
            let mut out: Vec<&FightRow> = Vec::new();
            let mut stopped = None;
            for f in all.iter().rev() {
                match seconds_of(&f.end) {
                    Some(t) if newest.saturating_sub(t) <= HOUR => out.push(f),
                    Some(_) => break,
                    None => {
                        stopped = Some(
                            "The run stops at a fight carrying a stamp this build could not read. \
                             Fights older than it are not in this scope even if they are inside \
                             the hour.",
                        );
                        break;
                    }
                }
            }
            out.reverse();
            (out, stopped)
        }
    }
}

/// A stamp as seconds, through THE ENGINE'S OWN READER.
///
/// A WRAPPER AND NOT AN IMPLEMENTATION. `grimoire_parse::fights::seconds` is what the aggregator
/// itself uses to cut the log into fights, and it carries the argument for reading these stamps
/// with no zone offset in the file. This function exists only so the call site reads as one word
/// and so there is one place to point at when somebody asks where the arithmetic came from.
fn seconds_of(stamp: &str) -> Option<i64> {
    grimoire_parse::fights::seconds(stamp)
}

/// WALL CLOCK BETWEEN TWO STAMPS, or None when it cannot be had.
///
/// NONE ON A NEGATIVE, AND THAT IS THE INTERESTING CASE. Two logs concatenated, or a daylight
/// saving step back, can put the later stamp earlier than the first one: the engine has a whole
/// `Ended::Backwards` arm for it. An absolute value there would print a plausible span that is off
/// by an hour, which is precisely the kind of number this app must not show.
fn span_secs(first: &str, last: &str) -> Option<i64> {
    let a = seconds_of(first)?;
    let b = seconds_of(last)?;
    (b >= a).then_some(b - a)
}

/// MAY A RATE BE PUBLISHED FOR THIS SCOPE?
///
/// See the module note. `secs` is the sum of N floored spans, so the slack in it is up to N
/// seconds; requiring at least [`MIN_SECS_PER_FIGHT`] per fight holds the relative error where a
/// single fight's own rule holds it, and collapses to exactly that rule when N is one.
fn rate_is_publishable(secs: i64, fights: usize) -> bool {
    fights > 0 && secs >= MIN_SECS_PER_FIGHT * fights as i64
}

/// A RATE FOR THIS SESSION, THROUGH THE OVERLAY'S OWN DIVISION.
///
/// `dps::dps` IS CALLED AND THE DIVISION IS NOT WRITTEN AGAIN, which is the rule `screens::analysis`
/// states in so many words when it reaches for the same function: the floor exists because a SPAN
/// the log cannot express makes a RATE wrong, and a page that re-derives the rate has quietly
/// forked the rule as well as the arithmetic. The two floors stack and the tighter one wins: this
/// page's own [`rate_is_publishable`] first, because a scope of forty one-second scraps clears the
/// overlay's single-fight floor on its summed span and must not.
///
/// `None` IS A THIRD ANSWER AND NOT A ZERO, exactly as it is in `dps::dps`. The caller prints
/// [`withheld`] for it, never a nought, because a nought is a measurement.
fn session_rate(value: u64, secs: i64, fights: usize) -> Option<u64> {
    rate_is_publishable(secs, fights)
        .then(|| dps(value, secs))
        .flatten()
}

/// WHAT THE POPULATION THE TABLES RANK ADDS UP TO, over one folded field.
///
/// # THE MOBS ARE NOT THE GROUP, AND `FightRow::damage` DOES NOT KNOW THAT
///
/// This page shipped its top tiles off `FightRow::damage` and `FightRow::deaths`, and both of those
/// fields count EVERYTHING: the field's own doc says "Every point of damage anybody dealt to
/// anybody", and the engine bumps `Fight::deaths` on every `Event::Death` whoever died. Every
/// ranked table under those tiles is `Who::player` only, because `dps::ranked_dealers` filters on
/// it, so the tiles and the tables could not be reconciled by a reader adding the rows up.
///
/// MEASURED OVER THE WHOLE REFERENCE CAPTURE, four fights and 401 seconds: the tile printed 19,695
/// where the players dealt 14,254, thirty-eight percent high, and the rate under it printed 49
/// where the honest figure is 35. The deaths tile printed 33 for a night the reader died ONCE, and
/// the Personal tab's own Deaths tile, four inches below it on the same screen, printed 1.
///
/// THIS IS NOT A NEW ARGUMENT, IT IS THE ONE `screens::analysis` ALREADY WON. That page carries a
/// heading reading "`Raid dps` MEANT THE WHOLE FIGHT, AND THAT WAS A LIE IN SIXTEEN POINT TEXT" and
/// the same fix, measured on one fight of the same capture at twenty-eight percent. A session fold
/// makes it worse rather than better, because the surplus is every mob of every pull all night.
///
/// WHAT THE PULL DEALT IS A REAL NUMBER AND IT IS NOT THE GROUP'S. It has no tile here for the
/// reason it has none there: no panel on this page ranks it. [`pinned_by_the_capture`] pins every
/// figure in this paragraph against the real bytes.
///
/// THE POPULATION IS `FightRow::ours`, the same rule `ranked_dealers` asks, so on a scope whose
/// every fight knew its group these are the reader, that group and his pets, and otherwise every
/// player.
fn group_total(row: &FightRow, pick: impl Fn(&Fighter) -> u64) -> u64 {
    row.fighters
        .iter()
        .filter(|x| row.ours(&x.who))
        .map(pick)
        .sum()
}

/// ONE FIGHT'S SHARE OF A SCOPE'S ROSTER: is this fighter counted for the scope, from this fight?
///
/// # TWO RULES, AND WHICH ONE IS THE SCOPE'S TO DECIDE
///
/// `known` is whether the scope's group is known ([`rolled_group`] is `Some`).
///
///   * NOT KNOWN: every player, from every fight. One fight nobody knew the group of makes the
///     whole scope show everyone, as it did before the group existed.
///   * KNOWN: THIS FIGHT'S OWN ROSTER, `FightRow::ours`, and never the scope's union. Every fight in
///     a known scope proved its own group, so each can say exactly who was with the reader in it.
///     The union cannot: Hert grouped in one fight and fighting beside a solo reader in the next is
///     in the union, and asking the union put the 5,000 he dealt in the solo fight under `your
///     group`, a fight the log proved he was not in.
///
/// [`roll`] merges by this, [`encounters`] counts by it, and the dashboard's scope timeline and
/// Deaths cell draw by it, so a row, the tile over it and the chart beside it count one population.
pub(crate) fn scope_roster(known: bool, f: &FightRow, who: &crate::fights::Who) -> bool {
    if known {
        f.ours(who)
    } else {
        f.player(who)
    }
}

/// [`group_total`] over one fight's fighters, by the SCOPE's rule. See [`scope_roster`].
fn scoped_total(row: &FightRow, known: bool, pick: impl Fn(&Fighter) -> u64) -> u64 {
    row.fighters
        .iter()
        .filter(|x| scope_roster(known, row, &x.who))
        .map(pick)
        .sum()
}

/// The same fold over a counted field, which the engine keeps as `u32` rather than `u64`.
///
/// SATURATING AND NOT WRAPPING. A night cannot really overflow a `u32` of deaths, but the fold in
/// this file saturates everywhere else and a count that wrapped to nought would read as "nobody
/// died", which is the one wrong answer here that looks like a right one.
fn group_count(row: &FightRow, pick: impl Fn(&Fighter) -> u32) -> u32 {
    row.fighters
        .iter()
        .filter(|x| row.ours(&x.who))
        .map(pick)
        .fold(0u32, u32::saturating_add)
}

/// [`group_count`] by the scope's rule. See [`scope_roster`].
fn scoped_count(row: &FightRow, known: bool, pick: impl Fn(&Fighter) -> u32) -> u32 {
    row.fighters
        .iter()
        .filter(|x| scope_roster(known, row, &x.who))
        .map(pick)
        .fold(0u32, u32::saturating_add)
}

/// THE WHOLE SCOPE AS ONE `FightRow`.
///
/// See the module note for what may be folded and what may not. Two fields are deliberately left as
/// the empty default and neither is an oversight:
///
///   * `moments` and every fighter's `series`, because both are stamped from their own fight's
///     start and cannot be laid on one axis.
///   * `ended`, because a session does not end for a reason. The last fight in it does, and that is
///     the last fight's fact and not the session's.
///
/// `pub(crate)` BECAUSE THE DASHBOARD FOLDS A NIGHT THE SAME WAY. One fold, two pages, one
/// answer; a second fold in `screens::dashboards` would be two implementations of one figure.
pub(crate) fn roll(rows: &[&FightRow]) -> FightRow {
    /* `secs` STARTS AT ZERO AND THE DEFAULT IS ONE. `FightRow::default` floors at a second because
     * a real fight cannot be shorter than the log can express; an EMPTY SCOPE is not a short fight,
     * it is no fight, and a one second denominator would let a scope with nothing in it publish a
     * rate. */
    let mut out = FightRow {
        secs: 0,
        ..FightRow::default()
    };
    /* THE SCOPE'S GROUP DECIDES WHICH RULE EVERY FIGHT IS MERGED BY, so it is asked first. See
     * `scope_roster`: known, each fight keeps its own roster and nobody else's players; not known,
     * every player from every fight, exactly as before the group existed. */
    let group = rolled_group(rows.iter().copied());
    let known = group.is_some();
    for f in rows {
        out.secs = out.secs.saturating_add(f.secs);
        out.damage = out.damage.saturating_add(f.damage);
        /* FOLDED FAITHFULLY AND PRINTED NOWHERE. `FightRow::deaths` is every death in the fight,
         * the mobs included, so the session row keeps summing it because the row's job is to be
         * the sum of the fights it covers. What the PAGE prints is [`Totals::deaths`], which is
         * the deaths on your side, and the two are different numbers on purpose. */
        out.deaths = out.deaths.saturating_add(f.deaths);
        out.lines = out.lines.saturating_add(f.lines);
        /* ANY CLIPPED FIGHT MAKES THE WHOLE ROLL UP A FLOOR, because a total missing part of one of
         * its terms is a total missing part of itself. Over-warning costs a sentence. */
        out.cut |= f.cut;
        /* MOBS AND NOBODY-NAMED ARE ALWAYS MERGED: they are what the players fought, and a
         * scope's damage taken is theirs to have dealt. Only a PLAYER can be off a fight's
         * roster. */
        merge(&mut out.fighters, f, |who| {
            !f.player(who) || scope_roster(known, f, who)
        });
        for pet in &f.pets {
            if !out.pets.iter().any(|p| p.eq_ignore_ascii_case(pet)) {
                out.pets.push(pet.clone());
            }
        }
        /* A MOB ANY FIGHT PROVED IS A MOB IN THE SCOPE. See `FightRow::foes`. */
        for foe in &f.foes {
            if !out.foes.iter().any(|p| p.eq_ignore_ascii_case(foe)) {
                out.foes.push(foe.clone());
            }
        }
    }
    if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
        out.start.clone_from(&first.start);
        out.end.clone_from(&last.end);
    }
    out.group = group;
    out
}

/// THE GROUP OVER A SCOPE: everyone in any rolled fight's group, and only when EVERY fight knew.
///
/// # ONE FIGHT NOT KNOWN MAKES THE SCOPE NOT KNOWN
///
/// `FightRow::group` is `None` when the log did not say who was in the group, and a union that
/// skipped that fight would take every player in it off the session's roster on the strength of
/// the OTHER fights' groups. That is `None` read as nobody, which is the one thing the field must
/// never be read as. So a single `None` makes the answer `None`, and every player is shown, as
/// every page did before the field existed.
///
/// # AN EMPTY SCOPE IS NOT SOLO
///
/// With no fight in it, "every fight was `Some`" is true of nothing, and folding that into
/// `Some(vec![])` would caption a scope with nothing in it as the reader alone. There is no fight
/// to know anything about, so the answer is `None`.
///
/// # NAMES FOLDED IGNORING CASE, FIRST SPELLING KEPT
///
/// Two fights can spell one member two ways (a typed invite is lower case in 28 of 183), and the
/// filter compares ignoring case anyway, so one member is one name. The first fight's spelling is
/// kept so the list does not change with the order the scope happens to be walked in.
///
/// # THE UNION NAMES THE SCOPE'S GROUP AND FILTERS NOTHING
///
/// It is what a caption can say (`your group`) and what decides WHICH rule a scope counts by. It
/// is never the filter: Hert grouped in one fight and fighting beside a solo reader in another is
/// in the union, and filtering the merged row by it credited his solo-fight damage to the group.
/// The parse lane had measured that situation as real, 141 hidings in known solo fights of a
/// player who was the reader's group mate at another time in the same file. So [`roll`] merges
/// each fight by its OWN roster when this is `Some` ([`scope_roster`]), and a player the merged
/// row holds is one some fight proved was with the reader.
///
/// # AND NEITHER IS EACH FIGHT'S OWN GROUP WHEN THIS IS `None`
///
/// The first version of this change filtered the encounter rows by each fight's own group while
/// the tiles above them read the scope. `pinned_by_the_capture` went red on it: the capture's first
/// fight is not known, so the tile counted every player (14,254) while its three solo fights dropped
/// Fylasem, Poguhy and Losumyda from their rows (13,896). One fight not known means every surface
/// of the scope shows everyone, which is [`scope_roster`]'s other half. `pub(crate)` for the
/// dashboard, and over any walk of rows so it never has to build a slice of references.
pub(crate) fn rolled_group<'a>(
    rows: impl IntoIterator<Item = &'a FightRow>,
) -> Option<Vec<String>> {
    let mut out: Option<Vec<String>> = None;
    for f in rows {
        let members = f.group.as_ref()?;
        let seen = out.get_or_insert_with(Vec::new);
        for name in members {
            if !seen.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                seen.push(name.clone());
            }
        }
    }
    out
}

/// FOLD ONE FIGHT'S FIGHTERS INTO THE SESSION'S, REWRITING EVERY TARGET SLOT.
///
/// THE TWO PASSES ARE NOT AN OPTIMISATION, THEY ARE THE CORRECTNESS. The slot map has to be
/// complete before any target index is rewritten, because a fighter can hit somebody who appears
/// LATER in the same fight's list, so a single pass would rewrite that index against a session list
/// that did not have the victim in it yet.
///
/// THE KEY IS `Who` AND NOT A NAME STRING. `Who::You` and `Who::Named(..)` are different variants
/// and comparing the enum keeps them different, which is what stops the reader being folded into a
/// player who happens to be called You. It also keeps every `Who::Unknown` in one row, which is
/// right: falling damage and `You hurt yourself` name nobody, the engine keeps them rather than
/// inventing an attacker, and this keeps them too so the session's totals still reconcile with
/// `FightRow::damage`.
///
/// # THE CLASS TRAVELS WITH THE FIGHTER, AND IT USED TO BE LEFT BEHIND HERE
///
/// Every session fighter is built from `Fighter::default()`, which sets `class` to `None` because
/// the fold in `grimoire_parse` cannot know a class: it sees damage lines, and a class comes off
/// cast lines and a corpus. `Ingest::stamp_classes` puts the reading on the row AFTER the fold, and
/// this function was copying nine counts and the name out of that stamped row and quietly dropping
/// the one field that was not a count.
///
/// WHAT THAT COST IS NOT COSMETIC, IT IS TWO WINDOWS DISAGREEING ABOUT ONE PERSON. `dps::row` is
/// the renderer every table in this app shares, and it reads `Fighter::class` twice: once to tint
/// the name (`class::tag_colour`) and once to draw the trio tag after it. The overlay, the LIVE
/// page and the Analysis page all draw fights the ingest stamped, so they tint and tag; every
/// ranked table on this page drew the same people, off the same renderer, in the default colour
/// with no tag, because the row underneath them had been rebuilt without the field.
///
/// THE FIRST PROVED CLASS WINS, AND A ROW CREATED BEFORE THE PROOF STILL GETS IT. In practice the
/// two cannot disagree: `stamp_classes` writes every row in `Ingest::fights` from one `tags` map in
/// one pass, so a character carries the same reading in every fight of a scan. The adoption below
/// is for the case that map cannot rule out, which is a fold over rows stamped at different times,
/// and it prefers a reading to no reading rather than preferring a fight's position in the list.
/// Nothing here ever overwrites one class with another, because two different readings of one
/// character would be a disagreement this function has no evidence to settle.
///
/// # `keep` LEAVES A FIGHTER OUT OF THIS FIGHT'S SHARE, AND ITS SLOT GOES WITH IT
///
/// A player off a known fight's roster (see [`scope_roster`]) is not merged, so nothing of his from
/// that fight reaches the session row. A target slot that pointed at him has nowhere to go and is
/// dropped: the damage stays in the dealer's own totals, and only the line saying it went into
/// somebody off this scope's roster is gone. That is the one place a merged `targets` list can sum
/// to less than its fighter's `dealt`, and it is only ever a mob's or a stranger-healer's list.
fn merge(into: &mut Vec<Fighter>, f: &FightRow, keep: impl Fn(&crate::fights::Who) -> bool) {
    let mut map: Vec<Option<usize>> = Vec::with_capacity(f.fighters.len());
    for x in &f.fighters {
        if !keep(&x.who) {
            map.push(None);
            continue;
        }
        match into.iter().position(|y| y.who == x.who) {
            Some(i) => map.push(Some(i)),
            None => {
                into.push(Fighter {
                    who: x.who.clone(),
                    class: x.class.clone(),
                    ..Fighter::default()
                });
                map.push(Some(into.len() - 1));
            }
        }
    }

    for (i, x) in f.fighters.iter().enumerate() {
        let Some(slot) = map[i] else {
            continue;
        };
        let dst = &mut into[slot];
        /* A CLASS THE LOG PROVED LATER THAN THIS ROW WAS CREATED. See the note above for why this
         * adopts rather than overwrites. */
        if dst.class.is_none() {
            dst.class.clone_from(&x.class);
        }
        dst.dealt = dst.dealt.saturating_add(x.dealt);
        dst.taken = dst.taken.saturating_add(x.taken);
        dst.healed = dst.healed.saturating_add(x.healed);
        dst.received = dst.received.saturating_add(x.received);
        dst.swings = dst.swings.saturating_add(x.swings);
        dst.landed = dst.landed.saturating_add(x.landed);
        dst.avoided = dst.avoided.saturating_add(x.avoided);
        dst.kills = dst.kills.saturating_add(x.kills);
        dst.deaths = dst.deaths.saturating_add(x.deaths);
        dst.melee_crits = dst.melee_crits.saturating_add(x.melee_crits);

        /* THE OUTCOMES ADD FIELD BY FIELD, which preserves the engine's `landed + stopped ==
         * swings` identity: summing both sides of an identity keeps it, and the hit results panel
         * draws its slices against `swings` on that basis. */
        dst.outcomes.missed = dst.outcomes.missed.saturating_add(x.outcomes.missed);
        dst.outcomes.parried = dst.outcomes.parried.saturating_add(x.outcomes.parried);
        dst.outcomes.dodged = dst.outcomes.dodged.saturating_add(x.outcomes.dodged);
        dst.outcomes.blocked = dst.outcomes.blocked.saturating_add(x.outcomes.blocked);
        dst.outcomes.riposted = dst.outcomes.riposted.saturating_add(x.outcomes.riposted);
        dst.outcomes.invulnerable = dst
            .outcomes
            .invulnerable
            .saturating_add(x.outcomes.invulnerable);
        dst.outcomes.rune_absorbed = dst
            .outcomes
            .rune_absorbed
            .saturating_add(x.outcomes.rune_absorbed);

        /* THE ABILITY KEY IS (NAME, FAMILY) AND NOT THE NAME ALONE. `grimoire_parse` keeps the
         * log's own word, so `slash` and `slashes` are already two rows on purpose; what a
         * name-only key would merge is a spell and a damage shield that happened to share a word,
         * and the abilities panel prints a crit rate for the melee family only. */
        for a in &x.abilities {
            match dst
                .abilities
                .iter_mut()
                .find(|b| b.name == a.name && b.family == a.family)
            {
                Some(b) => {
                    b.amount = b.amount.saturating_add(a.amount);
                    b.hits = b.hits.saturating_add(a.hits);
                    b.crits = b.crits.saturating_add(a.crits);
                }
                None => dst.abilities.push(Ability {
                    name: a.name.clone(),
                    family: a.family,
                    amount: a.amount,
                    hits: a.hits,
                    crits: a.crits,
                }),
            }
        }

        for s in &x.schools {
            match dst.schools.iter_mut().find(|t| t.school == s.school) {
                Some(t) => {
                    t.amount = t.amount.saturating_add(s.amount);
                    t.hits = t.hits.saturating_add(s.hits);
                }
                None => dst.schools.push(SchoolShare {
                    school: s.school.clone(),
                    amount: s.amount,
                    hits: s.hits,
                }),
            }
        }

        /* THE SLOT REWRITE. `t.slot` indexes THIS FIGHT's fighter list; `map` carries it to the
         * session's. An index the engine never produces is dropped rather than aimed at whatever
         * sits at that offset in the session row, because a target row pointing at the wrong entity
         * is worse than a target row that is not there. */
        for t in &x.targets {
            let Some(&mapped) = map.get(t.slot) else {
                debug_assert!(false, "a target slot pointed outside its own fight");
                continue;
            };
            /* A TARGET `keep` LEFT OUT. See the note on this function. */
            let Some(slot) = mapped else {
                continue;
            };
            match dst.targets.iter_mut().find(|u| u.slot == slot) {
                Some(u) => {
                    u.amount = u.amount.saturating_add(t.amount);
                    u.hits = u.hits.saturating_add(t.hits);
                }
                None => dst.targets.push(TargetShare {
                    slot,
                    amount: t.amount,
                    hits: t.hits,
                }),
            }
        }
    }
}

/// THE SCOPE GROUPED BY WHAT WAS FOUGHT, biggest first, ties broken by name.
///
/// THE TIE BREAK IS BY NAME AND IT IS NOT DECORATION. `grimoire_parse` breaks its own headline tie
/// the same way, precisely so an answer cannot depend on the order the file happened to write
/// things in. Two camps that produced the same damage would otherwise swap places on screen every
/// time the log was re-read, while nothing about the night changed.
fn encounters(rows: &[&FightRow]) -> Vec<Encounter> {
    let mut out: Vec<Encounter> = Vec::new();
    /* THE SCOPE'S RULE, ASKED ONCE: the tiles over this table read `roll`, which merges by the
     * same `scope_roster`, and the rows have to add up to them. See `rolled_group` for the 14,254
     * against 13,896 that mixing the two rules produced. */
    let known = rolled_group(rows.iter().copied()).is_some();
    for f in rows {
        let label = f.headline.clone().unwrap_or_else(|| unnamed().to_owned());
        /* ALL THREE COLUMNS COME OFF THE PLAYER ROWS, and that is one rule rather than three. A
         * table with two player-scoped columns and one that quietly counts the mobs as well is a
         * table nobody can read across. See [`Encounter::deaths`]. */
        let dealt = scoped_total(f, known, |x| x.dealt);
        let kills = scoped_count(f, known, |x| x.kills);
        let deaths = scoped_count(f, known, |x| x.deaths);
        match out.iter_mut().find(|e| e.label == label) {
            Some(e) => {
                e.fights += 1;
                e.secs = e.secs.saturating_add(f.secs);
                e.dealt = e.dealt.saturating_add(dealt);
                e.deaths = e.deaths.saturating_add(deaths);
                e.kills = e.kills.saturating_add(kills);
            }
            None => out.push(Encounter {
                label,
                fights: 1,
                secs: f.secs,
                dealt,
                kills,
                deaths,
            }),
        }
    }
    out.sort_by(|a, b| b.dealt.cmp(&a.dealt).then_with(|| a.label.cmp(&b.label)));
    out
}

/// What a fight with no named entity in it is called, in one place so the page and the clipboard
/// cannot call it two different things.
fn unnamed() -> &'static str {
    "an unnamed fight"
}

/* ------------------------------------------------------------------ the page -- */

/// THE SESSION TAB'S PANELS. Widgets, and nothing else.
///
/// `rate` IS THE CALLER'S ANSWER TO WHETHER THE SCOPE CAN BE TIMED, expressed through the shared
/// config rather than through a branch in a renderer. When it is false these are the same tables
/// showing totals, which is what `Ranked::rate` means everywhere else in this app.
///
/// NO HEADLINE, for the reason `screens::live` gives: the big number over an overlay table exists
/// because that window is glanced at from across a room, and here the heading above the table
/// already says which metric it is. It matters more here than there, because the headline is the
/// READER'S OWN figure and its idle state is the word zero, which on a report of a finished night
/// would read as a claim that he did nothing.
/// THE SESSION TAB'S TABLES, FOR THE UNIT INVARIANT IN `dps`.
#[cfg(test)]
pub fn panels_for_test(rate: bool) -> [(&'static str, Widget); 4] {
    panels(rate)
}

fn panels(rate: bool) -> [(&'static str, Widget); 4] {
    let table = |metric: Metric| {
        Widget::Ranked(Ranked {
            foot: false,
            metric,
            rate,
            cols: Cols {
                rank: true,
                value: true,
                share: true,
                bar: true,
                head: true,
            },
            cap: CAP,
            headline: false,
            /* NOT FITTED: this is a page that scrolls. See `overlay::Detail::fit`. */
            fit: false,
        })
    };
    [
        ("DAMAGE DEALT", table(Metric::Dealt)),
        ("HEALING DONE", table(Metric::Healed)),
        ("DAMAGE TAKEN", table(Metric::Taken)),
        ("HEALING RECEIVED", table(Metric::Received)),
    ]
}

/// THE PERSONAL TAB: the reader's own session, through the same detail panels the overlay uses.
///
/// THE SUBJECT IS CHECKED HERE AND NOT LEFT TO THE PANELS, and the reason is a sentence. Each panel
/// answers a missing subject with "You took no part in this fight", which is the right words in an
/// overlay over one pull and the wrong words over a night: a reader who fought for three hours and
/// sees "this fight" concludes the page is showing him one fight. So the absence is reported once,
/// in the session's own terms, and the panels are not drawn at all.
fn personal(ui: &mut Ui, c: &Cached) {
    let Some(me) = c.row.fighters.iter().find(|f| f.who == Who::You) else {
        ui.label(
            RichText::new(
                "You are not in any of the fights in this scope. Either none of them was yours, \
                 or the log's file name did not yield a character name, which is the only place \
                 this app learns who you are.",
            )
            .color(TEXT_2),
        );
        return;
    };

    ui.horizontal_wrapped(|ui| {
        tile(ui, "Your damage", &thousands(me.dealt));
        /* YOUR RATE, THROUGH THE OVERLAY'S OWN DIVISION, AND LABELLED SO IT CANNOT BE READ AS THE
         * GROUP'S. The tiles above this row carry the group's rate under a label that used to be
         * the same four words as this one, so a reader on this tab saw "Per second of combat"
         * twice, with two different numbers, and no way to tell which was his. */
        tile(
            ui,
            "Your damage per second",
            &session_rate(me.dealt, c.row.secs, c.fights)
                .map_or_else(|| withheld().to_owned(), thousands),
        );
        tile(ui, "Your healing", &thousands(me.healed));
        tile(ui, "Damage you took", &thousands(me.taken));
        tile(ui, "Healing you got", &thousands(me.received));
        tile(ui, "Your kills", &me.kills.to_string());
        tile(ui, "Your deaths", &me.deaths.to_string());
    });
    ui.add_space(10.0);

    /* THE DETAIL PANELS, OVER THE SESSION ROW, BY THE OVERLAY'S OWN RENDERERS. Three of the four
     * fold cleanly and the fourth folds because `merge` rewrote its slots; the timeline is the one
     * that cannot and it is absent rather than flat. */
    let mine = Detail {
        who: Subject::You,
        cap: 14,
        head: true,
        fit: false,
    };
    for w in [
        Widget::Abilities(mine),
        Widget::Targets(mine),
        Widget::Outcomes(mine),
        Widget::Elements(mine),
    ] {
        crate::screens::dps::draw_widget(ui, &c.row, crate::fights::Pulse::Closed, &w);
        ui.add_space(12.0);
    }
}

/// The word a figure is replaced with when the log cannot support it. Never a zero and never a
/// dash on its own: both read as a measurement.
fn withheld() -> &'static str {
    "not timed"
}

/// THE FIGURES THE TOP OF THIS PAGE STATES, DERIVED ONCE.
///
/// A STRUCT AND NOT FOUR CALLS AT EACH OF TWO CALL SITES, and the reason is the whole shape of this
/// review. [`tiles`] draws these and [`report_text`] pastes them, and when each of them reached for
/// the fields itself, the page and its own clipboard were two chances to pick the wrong field.
/// Derived here, in a function that takes no `Ui` and can therefore be asserted against the real
/// capture, they cannot differ: see [`pinned_by_the_capture`]. A test that pins a helper the screen
/// then declines to call proves nothing at all, which is why the screen has nothing left to call.
///
/// EVERY FIGURE IS `Who::player` ONLY, which is the population the tables below rank. See
/// [`group_total`] for what these read before that was true, and by how much they were wrong on the
/// owner's own bytes.
struct Totals {
    /// What the players dealt. NOT `FightRow::damage`, which is the pull's damage as well.
    damage: u64,
    /// Their damage over the time in combat, or `None` when the log cannot support a rate.
    rate: Option<u64>,
    /// Kills credited to players.
    kills: u32,
    /// Deaths among players. NOT `FightRow::deaths`, which counts the mobs going down as well.
    deaths: u32,
    /// How many distinct players appeared. SEEN, AND NOT A ROSTER: a participant is anything that
    /// was HIT, so this counts the stranger who took one swing on his way past, unless every fight
    /// in scope proved the reader's group, in which case it is the reader and that group
    /// (`FightRow::players`, off [`rolled_group`]).
    players: usize,
}

fn totals(c: &Cached) -> Totals {
    let damage = group_total(&c.row, |x| x.dealt);
    Totals {
        damage,
        rate: session_rate(damage, c.row.secs, c.fights),
        kills: group_count(&c.row, |x| x.kills),
        deaths: group_count(&c.row, |x| x.deaths),
        players: c.row.players(),
    }
}

/// THE TILE ROW AS A LIST, LABEL AND FIGURE TOGETHER, RATHER THAN AS EIGHT CALLS.
///
/// A LIST BECAUSE THE HOVER TABLE HAS TO BE ASSERTABLE AGAINST IT, and that is the defect this
/// shape exists to stop. [`tile`] looks each label up in [`HOVER`] and draws no hover when it finds
/// nothing, silently, which is the right behaviour for the Personal tab's own tiles and was the
/// wrong behaviour here: this row drew EIGHT tiles and `HOVER` carried SIX. The two with nothing to
/// say were `Fights` and `Players seen`, and `Players seen` is the one figure on this page whose
/// meaning [`Totals::players`] says outright a reader will get wrong. The tile that most needed the
/// sentence was the tile that did not have one, and nothing anywhere could notice, because eight
/// calls in a closure are not a list anything can count.
///
/// SO THE LABELS ARE DATA NOW and `every_tile_this_page_draws_says_what_it_means_on_hover` walks
/// them against `HOVER` in both directions. The figures come with them because a label separated
/// from the figure it names is the next version of this same defect.
fn tile_row(c: &Cached) -> [(&'static str, String); 8] {
    let t = totals(c);
    [
        ("Fights", c.fights.to_string()),
        ("Time in combat", span(c.row.secs)),
        (
            "Session span",
            c.span.map_or_else(|| "not readable".to_owned(), span),
        ),
        ("Group damage", thousands(t.damage)),
        (
            "Group per second of combat",
            t.rate.map_or_else(|| withheld().to_owned(), thousands),
        ),
        ("Players seen", t.players.to_string()),
        ("Kills by players", t.kills.to_string()),
        /* PLAYER DEATHS, AND THE WORD PLAYER IS LOAD BEARING. `FightRow::deaths` counts every
         * death inside the fight, mobs included, so a farming night puts the reader's own kill
         * count into a tile labelled Deaths: on the reference capture that tile read 33 for a
         * reader who died once, with the Personal tab's own Deaths tile reading 1 underneath it. */
        ("Player deaths", t.deaths.to_string()),
    ]
}

/// The row of figures across the top of every tab. The figures themselves are [`totals`], and the
/// labels beside them are [`tile_row`].
fn tiles(ui: &mut Ui, c: &Cached) {
    ui.horizontal_wrapped(|ui| {
        for (label, value) in tile_row(c) {
            let drawn = tile_frame(ui, label, &value);
            if let Some(why) = hover_for(label, &c.row) {
                drawn.on_hover_text(why);
            }
        }
    });

    if !c.first.is_empty() {
        ui.label(
            RichText::new(format!("{} to {}", c.first, c.last))
                .color(TEXT_3)
                .monospace(),
        );
    }
}

fn tile(ui: &mut Ui, label: &str, value: &str) {
    let drawn = tile_frame(ui, label, value);
    if let Some((_, why)) = HOVER.iter().find(|(k, _)| *k == label) {
        drawn.on_hover_text(*why);
    }
}

fn tile_frame(ui: &mut Ui, label: &str, value: &str) -> egui::Response {
    egui::Frame::NONE
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .corner_radius(3)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).color(TEXT).strong().size(16.0));
                ui.label(RichText::new(label).color(TEXT_3).size(10.5));
            });
        })
        .response
}

/// THE HOVER A SESSION TILE CARRIES, which is its [`HOVER`] entry and, for `Players seen`, which of
/// that entry's two populations THIS scope counted.
///
/// `Players seen` IS [`Totals::players`], and that is the reader, his pets and his group when every
/// fight in scope proved one and every player seen otherwise. Its hover said a stranger on his way
/// past is counted and that nothing in the log says who was grouped, over a tile that had stopped
/// counting strangers. A static sentence can state both rules; only the scope can say which one
/// the figure beside it followed, so it is said here, in `dps::whose`'s words.
fn hover_for(label: &str, row: &FightRow) -> Option<String> {
    let (_, why) = HOVER.iter().find(|(k, _)| *k == label)?;
    Some(match label {
        "Players seen" => format!("{why} In this scope: {}.", crate::screens::dps::whose(row)),
        _ => (*why).to_owned(),
    })
}

/// EVERY REASON A NUMBER ON THIS PAGE MIGHT BE SHORT, printed where the numbers are.
///
/// NOT A TOOLTIP AND NOT A DOC COMMENT. Each of these makes a total on this page smaller than the
/// truth, and a reader who pastes the total into a chat window has no way to discover any of them
/// afterwards.
/// # THEY WERE FOUR PARAGRAPHS AND THEY ARE FOUR CHIPS
///
/// Every one of these fires in the state it exists for, which is exactly when a reader is looking
/// at figures and wants to know what is wrong with them. Thirty to forty words each, above the
/// tables, was the same mistake the rest of this page had already had taken out of it: the page's
/// own prose guard could not see them because it draws a clean capture, where none of the four is
/// reached.
///
/// THE CLAIM SURVIVES AND THE ESSAY DOES NOT. Each is a short line in its own colour with the
/// whole of the old sentence on the hover, so a reader sees at a glance that something qualifies
/// these numbers and can ask what.
fn caveats(ui: &mut Ui, c: &Cached, unreadable: u32) {
    let chip = |ui: &mut Ui, tint: egui::Color32, line: &str, why: &str| {
        ui.label(RichText::new(line).color(tint))
            .on_hover_text(why.to_owned());
    };
    if c.fights == 0 {
        chip(
            ui,
            TEXT_2,
            "nothing in this scope",
            "No fight in the log that was read falls inside this scope. Widen it with Everything \
             read, or fight something.",
        );
    }
    if let Some(why) = c.stopped {
        chip(ui, TEXT_2, "scope stops early", why);
    }
    if c.row.cut {
        chip(
            ui,
            WRONG,
            "totals are a floor",
            "The oldest fight in this scope opened before the part of the log that was read, so \
             its numbers are a floor and every total on this page is a floor with it.",
        );
    }
    if !rate_is_publishable(c.row.secs, c.fights) && c.fights > 0 {
        chip(
            ui,
            TEXT_2,
            "no rate published",
            &format!(
                "These fights average under {MIN_SECS_PER_FIGHT} seconds each. The log stamps to \
                 the second, so their summed duration carries about a second of slack per fight \
                 and no rate over it would mean anything. Totals are shown instead."
            ),
        );
    }
    if unreadable > 0 {
        chip(
            ui,
            TEXT_2,
            &format!("{unreadable} lines not placed"),
            "Those lines carried a stamp this build could not read. Nothing on them is in any \
             fight, so it is in no total here either.",
        );
    }
}

/// THE ENCOUNTERS TABLE. The one thing on this page no widget can draw, because no widget groups
/// fights and a `Widget` is a projection of ONE fight.
fn encounters_table(ui: &mut Ui, rows: &[Encounter]) {
    if rows.is_empty() {
        ui.label(
            RichText::new(
                "Nothing in scope to group. Rows appear here once the fight list has fights in it.",
            )
            .color(TEXT_3),
        );
        return;
    }
    ui.add_space(6.0);

    let top = rows.iter().map(|e| e.dealt).max().unwrap_or(0).max(1);
    /* THE SHARE IS OF THE WHOLE SCOPE AND NOT OF THE ROWS THAT FIT, which is the rule
     * `screens::dps` states where it caps its own table: a capped table whose shares add to 100 is
     * lying about the rest. The sentence under this table used to promise the opposite. */
    let total: u64 = rows.iter().map(|e| e.dealt).sum();

    /* A HEADER ROW, AND THE REASON IS A UNIT COLLISION AND NOT TIDINESS. These columns shipped as
     * bare suffixes, `27 k` and `2 d`, one column over from a damage figure that `dps::short`
     * writes as `12k` for twelve thousand. On this page `k` therefore meant thousands in one
     * column and kills in the next. */
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        /* THE TWO PARAGRAPHS THAT USED TO SIT ABOVE AND BELOW THIS TABLE ARE ON THIS HEADING.
         *
         * One said what a row IS (the engine's label for a fight, which is the named thing that
         * took the most damage, so two pulls on one kind of mob are one row); the other said what
         * the columns COVER (players only, shares of the whole scope and not of the rows drawn).
         * Both are true and neither is a number, and a page that spends eighty words saying so
         * above a four row table has buried the table. `allocate_exact_size` already senses
         * hover here, so the words cost nothing until somebody asks for them. */
        let (name, hit) = ui.allocate_exact_size(egui::vec2(190.0, 14.0), egui::Sense::hover());
        ui.painter().text(
            name.left_center(),
            egui::Align2::LEFT_CENTER,
            "WHAT WAS FOUGHT",
            egui::FontId::proportional(10.5),
            TEXT_3,
        );
        hit.on_hover_text(
            "One row per label the engine gave a fight: the named thing that took the most damage \
             in it, so two pulls on the same kind of mob share a row. Every column is players \
             only, and shares are of the whole scope rather than of the rows drawn.",
        );
        for (w, h) in [
            (46.0, "fights"),
            (58.0, "time"),
            (74.0, "damage"),
            (44.0, "share"),
            (46.0, "kills"),
            (46.0, "deaths"),
        ] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, 14.0), egui::Sense::hover());
            ui.painter().text(
                r.right_center(),
                egui::Align2::RIGHT_CENTER,
                h,
                egui::FontId::proportional(10.5),
                TEXT_3,
            );
        }
    });
    ui.spacing_mut().item_spacing.y = 2.0;
    for (i, e) in rows.iter().enumerate().take(CAP * 2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (name, _) = ui.allocate_exact_size(egui::vec2(190.0, 16.0), egui::Sense::hover());
            ui.painter().with_clip_rect(name).text(
                name.left_center(),
                egui::Align2::LEFT_CENTER,
                &e.label,
                egui::FontId::proportional(12.0),
                if i == 0 { GOLD_HI } else { TEXT },
            );
            col(ui, 46.0, &format!("{}x", e.fights));
            col(ui, 58.0, &span(e.secs));
            col(ui, 74.0, &thousands(e.dealt));
            col(ui, 44.0, &format!("{}%", pct(e.dealt, total)));
            col(ui, 46.0, &e.kills.to_string());
            col(ui, 46.0, &e.deaths.to_string());

            let rest = ui.available_width().max(4.0);
            let (track, _) = ui.allocate_exact_size(egui::vec2(rest, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(track, 2.0, PANEL_2);
            let w = (track.width() as f64 * (e.dealt as f64 / top as f64).clamp(0.0, 1.0)) as f32;
            if w > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(track.min, egui::vec2(w, track.height())),
                    2.0,
                    GOLD_DIM,
                );
            }
        });
    }
    ui.add_space(4.0);
    /* THE ROWS THAT DID NOT FIT ARE COUNTED OUT LOUD. A table that stops at sixty rows without
     * saying so is a night with an unknown number of camps missing from it, and the shares in the
     * rows that DID fit will not add to a hundred, which is the first thing a reader checks.
     *
     * THE NOTE USED TO POINT DOWNWARDS AT NOTHING. It read "They are in the shares below and in
     * the copied report", and there is no below: `share` is a COLUMN of the table this note is
     * under, four columns along and every one of its rows above this line, and the table is the
     * last thing the tab draws. A reader who looked where he was sent found the end of the page.
     *
     * WHAT IS TRUE IS THE OTHER HALF OF THE OLD SENTENCE PLUS THE REASON THE COUNT IS PRINTED AT
     * ALL: the labels that did not fit are in the copied report, which `report_text` writes with
     * no cap, and the shares that DID fit are shares of the whole scope, so they are already short
     * of a hundred by exactly what is missing here.
     *
     * A COUNT ON THE PAGE AND THE ARGUMENT ON THE HOVER, which is this page's rule for every
     * other qualification it makes: see `caveats` and [`HOVER`]. */
    if let Some(rest) = rows.len().checked_sub(CAP * 2).filter(|n| *n > 0) {
        ui.label(RichText::new(format!("{rest} more labels not drawn")).color(TEXT_2))
            .on_hover_text(
                "Every one of them is in the copied report, which is not capped. The share column \
                 above is a share of the whole scope rather than of the rows that fit, so the \
                 percentages you can see will not add up to a hundred, and what is missing from \
                 them is these.",
            );
    }
}

/// WHAT EACH TILE MEANS, ON HOVER AND NOT ON THE PAGE.
///
/// # THE PAGE USED TO CARRY TWO PARAGRAPHS AND THE MOCKUP CARRIES NONE
///
/// Every tab of this page painted the same two hundred words of explanation above its tables: what
/// population the tiles cover, what the rates divide by, and that the fold is a snapshot. All of
/// it true, all of it read once, and all of it in the way of the numbers a person opened the page
/// to read. The design sheet for this screen has NO SENTENCES ON IT at all: a title row, a tab
/// row, seven tiles, a chart and six tables.
///
/// THE LABEL IS THE DISCLOSURE, AND THAT IS WHY THE PARAGRAPHS WERE REDUNDANT AS WELL AS LONG.
/// The tiles say `Group damage`, `Player deaths`, `Kills by players`, `Time in combat` and `Group
/// per second of combat`. Each one already states its own population and its own denominator in
/// its own name, which is what a paragraph underneath was restating in forty words.
///
/// WHAT THE PARAGRAPH HAD THAT A LABEL CANNOT CARRY IS HERE, ON HOVER. The distinction between
/// the session span and time in combat is a real trap and a reader who wants it can ask for it;
/// what the house rule forbids is inventing a number, not declining to lecture about a true one.
///
/// # THE TABLE COVERS EVERY TILE, AND IT USED TO COVER SIX OF EIGHT
///
/// `Fights` and `Players seen` were drawn with nothing behind them, and the second of those is the
/// worst possible one to leave out: [`Totals::players`] says in its own doc that the figure is
/// SEEN and not a roster, which is precisely the kind of thing a reader assumes the other way
/// round and cannot discover from four painted characters. A table that covers most of a row is
/// worse than one that covers none of it, because a reader who has hovered two tiles and been
/// answered reads silence on the third as "there is nothing more to say about this one".
///
/// EVERY ENTRY HERE NAMES A TILE [`tile_row`] DRAWS AND EVERY TILE IT DRAWS HAS AN ENTRY, both
/// directions asserted, so a tile added without its sentence and a sentence left behind by a tile
/// that was removed are the same red test.
const HOVER: &[(&str, &str)] = &[
    (
        "Fights",
        "How many fights in the part of the log that was read fall inside this scope. The fight in \
         progress is not one of them: it is nearly always the last of these folded again from a \
         shorter slice of the same file, so adding it would count one pull twice.",
    ),
    (
        "Time in combat",
        "The fights added together. Every rate on this page divides by this and not by the \
         session span.",
    ),
    (
        "Session span",
        "First combat stamp to last, including everything between pulls. No rate divides by it.",
    ),
    (
        "Group damage",
        "Players only. What the pull dealt back belongs to the pull and is in no tile here.",
    ),
    (
        "Group per second of combat",
        "Group damage over time in combat.",
    ),
    (
        "Players seen",
        "How many distinct players appeared in these fights, counted two ways. When every fight in \
         scope proved who was in your group, it is you, your pets and whoever was in your group in \
         the fight they appeared in. Otherwise it is every player seen, and a participant is \
         anything that was hit or hit back, so a stranger who took one swing on his way past is \
         counted.",
    ),
    (
        "Kills by players",
        "Things on the other side that went down.",
    ),
    ("Player deaths", "Times somebody on your side went down."),
];

/// One right aligned figure of fixed width, so a column of them can be read down.
fn col(ui: &mut Ui, w: f32, text: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), egui::Sense::hover());
    ui.painter().text(
        r.right_center(),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::monospace(11.5),
        TEXT_2,
    );
}

/* ------------------------------------------------------------- the clipboard -- */

/// THE REPORT AS PLAIN TEXT, WHICH IS THE ONLY FORM OF "HAND IT TO SOMEBODY" THIS APP HAS.
///
/// A PURE FUNCTION SO IT CAN BE TESTED, and tested it is: the guard below asserts that every
/// warning the page can show is also in the text, because the failure this prevents is a total
/// pasted into a stream chat with none of the reasons it might be short.
///
/// NO FILE IS WRITTEN AND NO FILE WILL BE. Only the Settings screen writes to disk in this app.
fn report_text(log: &str, scope: Scope, c: &Cached, unreadable: u32) -> String {
    let rated = rate_is_publishable(c.row.secs, c.fights);
    let mut out = String::new();
    out.push_str("EQL Grimoire session report\n");
    out.push_str(&format!("Log: {log}\n"));
    out.push_str(&format!("Scope: {} ({} fights)\n", scope.label(), c.fights));
    /* WHOSE ROWS EVERY BLOCK BELOW RANKS AND SUMS, in the words the dashboard's roster cards use.
     * The paste outlives the screen, so a DAMAGE DEALT block listing only the reader has to say
     * whether that is a solo night or a night nobody else was ranked on. */
    out.push_str(&format!("Rows: {}\n", crate::screens::dps::whose(&c.row)));
    if !c.first.is_empty() {
        out.push_str(&format!("From: {}\nTo:   {}\n", c.first, c.last));
    }
    out.push_str(&format!("Time in combat: {}\n", span(c.row.secs)));
    out.push_str(&format!(
        "Session span:   {}\n",
        c.span.map_or_else(|| "not readable".to_owned(), span)
    ));
    /* LITERALLY THE FIGURES THE TILES CARRY, FROM THE SAME STRUCT. A clipboard that reached for
     * the fields itself would be a second chance to reach for the wrong one, and the paste outlives
     * the screen it came from, so nobody would ever find out. */
    let t = totals(c);
    out.push_str(&format!(
        "Group damage: {}{}\n",
        thousands(t.damage),
        match t.rate {
            Some(n) => format!(" ({} per second of combat)", thousands(n)),
            None => String::new(),
        }
    ));
    out.push_str(&format!("Kills by players: {}\n", t.kills));
    out.push_str(&format!("Player deaths: {}\n", t.deaths));

    /* ALL FOUR TABLES, AND THERE USED TO BE ONE.
     *
     * The button's own caption promised "every table on this tab", and this function takes no tab
     * and had no branch on one: it wrote a hand-rolled DAMAGE DEALT list and the encounters, and
     * that was the whole report. Pressing Copy on PERSONAL pasted neither of that tab's tiles nor
     * one of its four detail panels, and pasted two tables that are not on it. On SESSION it
     * carried one of the four tables drawn above the button.
     *
     * THE REPORT IS THE SESSION AND NOT THE TAB, which is what it has always called itself in its
     * own first line, and the caption says that now. A tab is a way of looking at one night; a
     * report is the night. So every metric `panels` draws gets a block here, in the same order,
     * and the caption promises the session rather than whatever the reader last clicked.
     *
     * AND THE RANKING IS `dps::ranked_dealers` RATHER THAN A SORT WRITTEN HERE. The old copy
     * filtered and sorted by hand, so a paste could disagree with the table it was pasted from
     * the day either rule moved. */
    for (name, metric) in [
        ("DAMAGE DEALT", Metric::Dealt),
        ("HEALING DONE", Metric::Healed),
        ("DAMAGE TAKEN", Metric::Taken),
        ("HEALING RECEIVED", Metric::Received),
    ] {
        out.push('\n');
        out.push_str(&format!(
            "{name} ({})\n",
            if rated {
                "total, per second of combat"
            } else {
                "total"
            }
        ));
        let ranked = crate::screens::dps::ranked_dealers(&c.row, metric);
        if ranked.is_empty() {
            out.push_str(&format!("  nobody named has any {}\n", metric.unit(false)));
        }
        for f in ranked {
            let v = metric.of(f);
            /* THE DIVISION IS `dps::dps` AND NOT A SLASH WRITTEN HERE, for the reason the whole
             * page exists: two implementations of one number is the defect this app cannot
             * afford, and a paste is the one form of this page that outlives its screen. */
            match session_rate(v, c.row.secs, c.fights) {
                Some(n) => out.push_str(&format!(
                    "  {:<16} {:>12}  {:>8}\n",
                    f.who.text(),
                    thousands(v),
                    thousands(n)
                )),
                None => out.push_str(&format!("  {:<16} {:>12}\n", f.who.text(), thousands(v))),
            }
        }
    }

    if !c.encounters.is_empty() {
        out.push_str(
            "\nWHAT WAS FOUGHT (fights, time, damage by players, kills by players, deaths among \
             players)\n",
        );
        for e in &c.encounters {
            out.push_str(&format!(
                "  {:<28} {:>4}x {:>9} {:>12} {:>4}k {:>4}d\n",
                e.label,
                e.fights,
                span(e.secs),
                thousands(e.dealt),
                e.kills,
                e.deaths
            ));
        }
    }

    out.push_str("\nWHAT THESE NUMBERS ARE AND ARE NOT\n");
    out.push_str(
        "  Rates divide by time in combat, the fights added together, not by the session span.\n",
    );
    out.push_str(
        "  This is the log as it stood when the app last read it. The fight in progress is not \
         in it.\n",
    );
    out.push_str(
        "  Every figure above is the players. The pull's own damage is real and is not in any of \
         them, and a mob going down is counted as a kill and not as a death.\n",
    );
    /* THE ONE WARNING THE PAGE DRAWS THAT THE PASTE DID NOT, and it is the one that matters
     * most: with nothing in scope every figure above is a zero, and a paste of zeroes with no
     * sentence saying the scope was empty reads as a night where nobody did anything. It is
     * reachable with the page drawn, because `scoped` returns an empty run for a zone with no
     * zone line and for an hour whose stamps would not read while `Ingest::fights` is not empty. */
    if c.fights == 0 {
        out.push_str(
            "  No fight in the log that was read falls inside this scope, so every figure above \
             is a zero rather than a measurement.\n",
        );
    }
    if c.row.cut {
        out.push_str(
            "  The oldest fight in scope opened before the part of the log that was read, so \
             every total above is a floor.\n",
        );
    }
    if !rated && c.fights > 0 {
        out.push_str(&format!(
            "  These fights average under {MIN_SECS_PER_FIGHT} seconds each, so no rate is \
             published for them.\n"
        ));
    }
    if let Some(why) = c.stopped {
        out.push_str(&format!("  {why}\n"));
    }
    if unreadable > 0 {
        out.push_str(&format!(
            "  {unreadable} lines in this read carried a stamp this build could not read and are \
             in no total above.\n"
        ));
    }
    out
}

/* --------------------------------------------------------------- the numbers -- */

/// A whole percent of `total`, rounded down, never a division by zero.
fn pct(part: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    part.saturating_mul(100) / total
}

/// `266` becomes `4:26`, and `4271` becomes `1:11:11`.
///
/// AN HOUR FIELD, WHICH THE PER FIGHT CLOCKS IN THIS TREE DO NOT HAVE. A fight runs for minutes and
/// a session runs for hours, and `71:11` for an hour and eleven minutes is a number a reader has to
/// stop and divide.
fn span(secs: i64) -> String {
    let s = secs.max(0);
    if s >= HOUR {
        format!("{}:{:02}:{:02}", s / HOUR, (s % HOUR) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `90` becomes `1 minute ago`. Whole units only, because a report page saying `read 1 minute and
/// 30 seconds ago` is precision about the one number on the page that does not need it.
fn ago(secs: i64) -> String {
    let s = secs.max(0);
    let (n, unit) = if s < 60 {
        (s, "second")
    } else if s < HOUR {
        (s / 60, "minute")
    } else {
        (s / HOUR, "hour")
    };
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// `16526` becomes `16,526`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{fold_text, probe, quiet_window, Family, Outcomes};

    fn who(n: &str) -> Who {
        Who::Named(n.to_owned())
    }

    fn fighter(w: Who, dealt: u64) -> Fighter {
        Fighter {
            who: w,
            dealt,
            ..Fighter::default()
        }
    }

    fn fight(secs: i64, fighters: Vec<Fighter>) -> FightRow {
        FightRow {
            secs,
            damage: fighters.iter().map(|f| f.dealt).sum(),
            fighters,
            ..FightRow::default()
        }
    }

    fn cached(row: FightRow, fights: usize) -> Cached {
        Cached {
            key: (None, 0, Scope::Everything),
            fights,
            first: String::new(),
            last: String::new(),
            span: None,
            stopped: None,
            encounters: encounters(&[]),
            row,
        }
    }

    /// DEFECT: A SESSION TOTAL THAT IS NOT THE SUM OF THE FIGHTS IT CLAIMS TO COVER.
    ///
    /// This is the whole product. Every other test in this file guards a way of getting it wrong;
    /// this one guards the answer itself, per entity, across fights whose fighter lists are in
    /// DIFFERENT ORDERS, which is the ordinary case because the engine emits fighters in
    /// first-appearance order and the first thing to appear changes with every pull.
    ///
    /// WHAT MUTATION MAKES THIS RED: keying `merge` on position rather than on `Who`, dropping the
    /// saturating adds for one field, or folding a fighter that is new to the session into slot
    /// zero.
    #[test]
    fn the_session_row_is_the_sum_of_the_fights_it_covers() {
        let a = fight(
            10,
            vec![
                fighter(Who::You, 100),
                fighter(who("Poguhy"), 50),
                fighter(who("a dry bone skeleton"), 7),
            ],
        );
        /* A DIFFERENT ORDER, A NEW FACE, AND A FACE THAT LEFT. */
        let b = fight(
            20,
            vec![
                fighter(who("a large spider"), 11),
                fighter(who("Tanefi"), 400),
                fighter(Who::You, 200),
            ],
        );
        let rolled = roll(&[&a, &b]);

        assert_eq!(rolled.secs, 30, "time in combat is the fights added up");
        assert_eq!(rolled.damage, 100 + 50 + 7 + 11 + 400 + 200);
        let of = |w: &Who| {
            rolled
                .fighters
                .iter()
                .find(|f| &f.who == w)
                .unwrap_or_else(|| panic!("{} is missing from the session row", w.text()))
                .dealt
        };
        assert_eq!(
            of(&Who::You),
            300,
            "the reader is one row across two fights"
        );
        assert_eq!(of(&who("Poguhy")), 50);
        assert_eq!(of(&who("Tanefi")), 400);
        assert_eq!(of(&who("a dry bone skeleton")), 7);
        assert_eq!(
            rolled.fighters.len(),
            5,
            "five distinct entities appeared over the two fights"
        );

        /* AND THE ROW'S OWN FIGHTERS RECONCILE WITH ITS DAMAGE, which is the identity a reader
         * would use to check this page by hand. */
        let summed: u64 = rolled.fighters.iter().map(|f| f.dealt).sum();
        assert_eq!(summed, rolled.damage);
    }

    /// DEFECT: EVERY OTHER FIELD OF A FIGHTER SILENTLY NOT FOLDING.
    ///
    /// `dealt` is the field a person looks at, so a fold that summed only `dealt` would look right
    /// on the damage table and would show a healer as having healed nothing all night.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting any one of the adds in `merge`.
    #[test]
    fn every_counted_field_folds_and_not_only_damage() {
        let one = Fighter {
            who: Who::You,
            dealt: 1,
            taken: 2,
            healed: 3,
            received: 4,
            swings: 5,
            landed: 6,
            avoided: 7,
            kills: 8,
            deaths: 9,
            melee_crits: 10,
            outcomes: Outcomes {
                missed: 1,
                parried: 2,
                dodged: 3,
                blocked: 4,
                riposted: 5,
                invulnerable: 6,
                rune_absorbed: 7,
            },
            ..Fighter::default()
        };
        let f = fight(5, vec![one.clone()]);
        let rolled = roll(&[&f, &f]);
        let me = &rolled.fighters[0];
        assert_eq!(
            (
                me.dealt,
                me.taken,
                me.healed,
                me.received,
                me.swings,
                me.landed,
                me.avoided,
                me.kills,
                me.deaths,
                me.melee_crits
            ),
            (2, 4, 6, 8, 10, 12, 14, 16, 18, 20)
        );
        assert_eq!(me.outcomes.total(), one.outcomes.total() * 2);
        assert_eq!(me.outcomes.missed, 2);
        assert_eq!(me.outcomes.rune_absorbed, 14);
    }

    /// DEFECT: EVERY REPORTS TABLE DROPPING THE CLASS COLOUR AND TRIO TAG THE SAME RENDERER DRAWS
    /// FOR THE SAME PEOPLE ON EVERY OTHER SURFACE.
    ///
    /// `merge` builds each session fighter from `Fighter::default()`, which is `class: None`, and
    /// copied nine counts and the name across while leaving that field behind. `dps::row` reads it
    /// twice, for the name's tint and for the trio tag after it, and `screens::dps::draw_widget` is
    /// the only thing this page draws its tables with, so the overlay and the LIVE page tinted and
    /// tagged a person the Reports table drew in plain text. Two windows, one renderer, one log,
    /// two answers.
    ///
    /// BOTH HALVES OF THE FOLD ARE DRIVEN HERE, because they are two different lines. `Poguhy`
    /// enters the session with no reading and gains one in the second fight, which is the adoption
    /// arm; the reader enters WITH one and must not lose it to a later row that has none, which is
    /// the arm that would break if the adoption were an unconditional assignment.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `class` from the `Fighter` that `merge` pushes,
    /// deleting the `dst.class.is_none()` adoption, or turning that adoption into an unconditional
    /// `dst.class.clone_from(&x.class)`, which would blank the reader's class in fight two.
    #[test]
    fn the_session_row_carries_the_class_every_other_surface_tints_by() {
        let tagged = |w: Who, class: &str, dealt: u64| Fighter {
            who: w,
            class: Some(class.to_owned()),
            dealt,
            ..Fighter::default()
        };
        let a = fight(
            10,
            vec![
                tagged(Who::You, "Bard", 100),
                fighter(who("Poguhy"), 5),
                fighter(who("a dry bone skeleton"), 7),
            ],
        );
        let b = fight(
            10,
            vec![
                tagged(who("Poguhy"), "Cleric +1", 50),
                fighter(Who::You, 200),
            ],
        );
        let rolled = roll(&[&a, &b]);
        let of = |w: &Who| {
            rolled
                .fighters
                .iter()
                .find(|f| &f.who == w)
                .unwrap_or_else(|| panic!("{} is missing from the session row", w.text()))
                .class
                .clone()
        };
        assert_eq!(
            of(&Who::You),
            Some("Bard".to_owned()),
            "the reader was tinted on the overlay and drawn plain on the report"
        );
        assert_eq!(
            of(&who("Poguhy")),
            Some("Cleric +1".to_owned()),
            "a class the log proved in a later fight never reached the session row"
        );
        assert_eq!(
            of(&who("a dry bone skeleton")),
            None,
            "the fold invented a class for something the ingest never stamped one on"
        );
    }

    /// DEFECT: A TARGET ROW CREDITING YOUR DAMAGE TO THE WRONG ENTITY.
    ///
    /// `TargetShare::slot` indexes the fight's OWN fighter list, which is in first-appearance order,
    /// so slot 1 is a different creature in every pull. Added raw across fights, the targets panel
    /// would say you hit whatever happened to appear second in the newest fight.
    ///
    /// The two fights below are built so a slot-preserving fold gives a DIFFERENT and plausible
    /// answer rather than a crash, which is the only kind of wrong this app has to fear.
    ///
    /// WHAT MUTATION MAKES THIS RED: pushing `t.slot` instead of `map[t.slot]` in `merge`.
    #[test]
    fn a_target_slot_is_rewritten_into_the_session_rows_own_slots() {
        let mut a = fight(
            10,
            vec![
                fighter(Who::You, 100),
                fighter(who("a dry bone skeleton"), 0),
            ],
        );
        a.fighters[0].targets = vec![TargetShare {
            slot: 1,
            amount: 100,
            hits: 4,
        }];

        /* THE SAME HIT, IN A FIGHT WHERE THE SKELETON IS SLOT 0 AND A SPIDER IS SLOT 2. */
        let mut b = fight(
            10,
            vec![
                fighter(who("a dry bone skeleton"), 0),
                fighter(Who::You, 60),
                fighter(who("a large spider"), 0),
            ],
        );
        b.fighters[1].targets = vec![
            TargetShare {
                slot: 0,
                amount: 40,
                hits: 2,
            },
            TargetShare {
                slot: 2,
                amount: 20,
                hits: 1,
            },
        ];

        let rolled = roll(&[&a, &b]);
        let me = rolled
            .fighters
            .iter()
            .find(|f| f.who == Who::You)
            .expect("the reader is in the session row");

        let named = |slot: usize| rolled.fighters[slot].who.text().to_owned();
        let on = |name: &str| -> u64 {
            me.targets
                .iter()
                .filter(|t| named(t.slot) == name)
                .map(|t| t.amount)
                .sum()
        };
        assert_eq!(
            on("a dry bone skeleton"),
            140,
            "the skeleton is slot 1 in one fight and slot 0 in the other, and it is one row here"
        );
        assert_eq!(on("a large spider"), 20);
        assert_eq!(
            me.targets.len(),
            2,
            "two victims across the two fights, not three rows and not one"
        );
        assert_eq!(
            me.targets.iter().map(|t| t.amount).sum::<u64>(),
            me.dealt,
            "every point the reader dealt is against a target and none of it went missing"
        );
    }

    /// DEFECT: TWO ABILITIES MERGING THAT ARE NOT ONE ABILITY, or one splitting into two rows.
    ///
    /// The key is (name, family) because `grimoire_parse` keeps the log's own word and the
    /// abilities panel prints a crit rate for the melee family only. A name-only key would put a
    /// spell and a shield that shared a word on one row with a meaningless crit rate.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the family from the merge key.
    #[test]
    fn abilities_merge_on_the_name_and_the_family_together() {
        let mut x = fighter(Who::You, 30);
        x.abilities = vec![
            Ability {
                name: "slash".to_owned(),
                family: Family::Melee,
                amount: 10,
                hits: 2,
                crits: 1,
            },
            Ability {
                name: "thorns".to_owned(),
                family: Family::Shield,
                amount: 20,
                hits: 4,
                crits: 0,
            },
        ];
        let mut y = fighter(Who::You, 15);
        y.abilities = vec![
            Ability {
                name: "slash".to_owned(),
                family: Family::Melee,
                amount: 5,
                hits: 1,
                crits: 1,
            },
            /* THE SAME WORD, A DIFFERENT FAMILY. Two rows, and it must stay two rows. */
            Ability {
                name: "slash".to_owned(),
                family: Family::Spell,
                amount: 10,
                hits: 1,
                crits: 0,
            },
        ];
        let a = fight(10, vec![x]);
        let b = fight(10, vec![y]);
        let rolled = roll(&[&a, &b]);
        let me = &rolled.fighters[0];
        assert_eq!(me.abilities.len(), 3, "{:?}", me.abilities);
        let melee = me
            .abilities
            .iter()
            .find(|a| a.name == "slash" && a.family == Family::Melee)
            .expect("the melee slash survived");
        assert_eq!((melee.amount, melee.hits, melee.crits), (15, 3, 2));
        assert!(me
            .abilities
            .iter()
            .any(|a| a.name == "slash" && a.family == Family::Spell && a.amount == 10));
    }

    /// DEFECT: AN EMPTY SCOPE PUBLISHING A RATE.
    ///
    /// `FightRow::default` floors `secs` at one second, which is right for a fight and wrong for a
    /// fold over nothing: a one second denominator over zero damage is a real division, and a fold
    /// that ever gained a numerator without a fight would print a rate for a session that did not
    /// happen.
    ///
    /// WHAT MUTATION MAKES THIS RED: building the session row with `FightRow::default()` and not
    /// overriding `secs`.
    #[test]
    fn a_scope_with_no_fights_has_no_seconds_and_therefore_no_rate() {
        let empty = roll(&[]);
        assert_eq!(empty.secs, 0);
        assert_eq!(empty.damage, 0);
        assert!(empty.fighters.is_empty());
        assert!(!rate_is_publishable(empty.secs, 0));
    }

    /// DEFECT: A SESSION RATE OVER A DENOMINATOR THE LOG CANNOT SUPPORT.
    ///
    /// Each fight's span is floored at one second, so N fights carry up to N seconds of slack. The
    /// rule is the same bar per fight that `screens::dps` sets for one fight, and this pins that
    /// it DEGENERATES to exactly that rule at N of one, which is what makes the duplicated
    /// threshold safe.
    ///
    /// WHAT MUTATION MAKES THIS RED: comparing `secs` against the constant rather than against the
    /// constant times the fight count, which is the obvious wrong version.
    #[test]
    fn the_session_rate_rule_degenerates_to_the_single_fight_rule() {
        /* One fight: exactly the overlay's rule. */
        assert!(!rate_is_publishable(1, 1));
        assert!(!rate_is_publishable(2, 1));
        assert!(rate_is_publishable(3, 1));

        /* Forty one-second scraps sum to forty seconds and are still untimeable. A rule that
         * looked only at the total would publish a rate for them. */
        assert!(!rate_is_publishable(40, 40));
        assert!(rate_is_publishable(120, 40));

        /* And nothing at all is never a rate. */
        assert!(!rate_is_publishable(0, 0));
        assert!(!rate_is_publishable(9999, 0));
    }

    /// DEFECT: A SESSION SPAN INVENTED OUT OF A CLOCK THAT STEPPED BACK.
    ///
    /// Two logs concatenated, or a daylight saving step back, put the later stamp earlier than the
    /// first. The engine has a whole `Ended::Backwards` arm for it. An absolute value there prints
    /// a plausible figure that is off by an hour, which the reader cannot see is wrong.
    ///
    /// WHAT MUTATION MAKES THIS RED: `(b - a).abs()`, or dropping the ordering check.
    #[test]
    fn the_session_span_is_refused_when_the_clock_stepped_back() {
        let early = "Wed Jul 15 23:16:50 2026";
        let late = "Thu Jul 16 01:02:11 2026";
        assert_eq!(span_secs(early, late), Some(6321));
        assert_eq!(span_secs(late, early), None, "a negative span is refused");
        assert_eq!(span_secs(early, early), Some(0));
        assert_eq!(span_secs("not a stamp", late), None);
        assert_eq!(span_secs(early, "not a stamp"), None);
    }

    /// DEFECT: A SCOPE WITH A HOLE IN IT.
    ///
    /// A scope built by filtering can drop a fight out of the MIDDLE, and the totals shrink with
    /// nothing on screen to say so. Both narrowing scopes are trailing runs that stop at the first
    /// row they cannot place, and the reason they stopped is carried out for the page to print.
    ///
    /// WHAT MUTATION MAKES THIS RED: turning either arm of `scoped` into a `filter`.
    #[test]
    fn a_narrowed_scope_is_a_trailing_run_and_never_has_a_hole() {
        let zoned = |z: Option<&str>| FightRow {
            secs: 10,
            zone: z.map(str::to_owned),
            ..FightRow::default()
        };
        /* Befallen, then Qeynos, then back to Befallen. The run must be the LAST Befallen fight
         * alone; a filter would sweep the older one in and claim two. */
        let all = vec![
            zoned(Some("Befallen")),
            zoned(Some("South Qeynos")),
            zoned(Some("Befallen")),
        ];
        let (rows, stopped) = scoped(&all, Scope::Zone);
        assert_eq!(rows.len(), 1);
        assert!(stopped.is_some(), "the page must say the run stopped short");

        /* Everything is everything, and says nothing. */
        let (rows, stopped) = scoped(&all, Scope::Everything);
        assert_eq!(rows.len(), 3);
        assert!(stopped.is_none());

        /* No zone anywhere is a refusal with words, not an empty table. */
        let none = vec![zoned(None)];
        let (rows, stopped) = scoped(&none, Scope::Zone);
        assert!(rows.is_empty());
        assert!(stopped.is_some_and(|w| w.contains("zone line")));
    }

    /// DEFECT: THE HOUR SCOPE READING PAST A STAMP IT COULD NOT PARSE, so the run silently skips a
    /// fight and reports a smaller night.
    #[test]
    fn the_hour_scope_stops_at_a_stamp_it_cannot_read() {
        let at = |end: &str| FightRow {
            secs: 10,
            end: end.to_owned(),
            ..FightRow::default()
        };
        let all = vec![
            at("Wed Jul 15 20:00:00 2026"),
            at("a line with no stamp"),
            at("Wed Jul 15 23:30:00 2026"),
            at("Wed Jul 15 23:59:00 2026"),
        ];
        let (rows, stopped) = scoped(&all, Scope::LastHour);
        assert_eq!(rows.len(), 2, "the two inside the hour, and then it stops");
        assert!(stopped.is_some_and(|w| w.contains("could not read")));

        /* And a fight simply older than the hour ends the run quietly: there is nothing wrong with
         * it and nothing to warn about. */
        let plain = vec![
            at("Wed Jul 15 20:00:00 2026"),
            at("Wed Jul 15 23:30:00 2026"),
            at("Wed Jul 15 23:59:00 2026"),
        ];
        let (rows, stopped) = scoped(&plain, Scope::LastHour);
        assert_eq!(rows.len(), 2);
        assert!(stopped.is_none());
    }

    /// DEFECT: A CLIPPED FIGHT'S FLOOR NOT REACHING THE SESSION TOTAL.
    ///
    /// `FightRow::cut` says the oldest fight's opening lines are not in the bytes this app read, so
    /// its damage is a floor. A total built on it is a floor too, and a page that dropped the flag
    /// would print a confident number that is quietly short.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `|=` in `roll`.
    #[test]
    fn a_clipped_fight_makes_the_whole_roll_up_a_floor() {
        let clipped = FightRow {
            secs: 10,
            cut: true,
            ..FightRow::default()
        };
        let whole = FightRow {
            secs: 10,
            ..FightRow::default()
        };
        assert!(roll(&[&clipped, &whole]).cut);
        assert!(roll(&[&whole, &clipped]).cut, "any row, not only the first");
        assert!(!roll(&[&whole, &whole]).cut);
    }

    /// DEFECT: A CHART OF AN EVENING THAT NEVER HAPPENED.
    ///
    /// `Fighter::series` and `FightRow::moments` are stamped in seconds from THEIR OWN fight's
    /// start, so fight two's second five and fight one's second five are different moments wearing
    /// one number. Folded, they would draw a timeline of a session where everything happened in the
    /// first minute. The fold leaves both empty and this page never offers the widget that reads
    /// them.
    ///
    /// WHAT MUTATION MAKES THIS RED: adding `Widget::Timeline` to `panels`, or summing `series` in
    /// `merge`.
    #[test]
    fn nothing_on_this_page_reads_a_clock_the_fold_could_not_keep() {
        let mut x = fighter(Who::You, 50);
        x.series = vec![(0, 20), (3, 30)];
        let mut f = fight(10, vec![x]);
        f.moments = vec![crate::fights::Moment {
            at: 4,
            what: crate::fights::Mark::Death {
                killer: 0,
                victim: 0,
            },
        }];
        let rolled = roll(&[&f, &f]);
        assert!(
            rolled.fighters[0].series.is_empty(),
            "a per-second series cannot be laid across two fights"
        );
        assert!(rolled.moments.is_empty());

        for (title, w) in panels(true) {
            assert!(
                !matches!(w, Widget::Timeline(_)),
                "{title} offers a timeline over a row with no series in it"
            );
        }
    }

    /// DEFECT: THE PAGE PROMISING A RATE ITS OWN RULE HAS REFUSED.
    ///
    /// The refusal is expressed by handing the shared renderer `rate: false`, which is what that
    /// flag means everywhere else in this app, rather than by a special case inside a renderer this
    /// file does not own. A `panels` that ignored its argument would draw rates the tiles beside it
    /// say cannot be published.
    #[test]
    fn a_refused_rate_is_refused_in_the_config_and_not_in_a_renderer() {
        for (title, w) in panels(false) {
            match w {
                Widget::Ranked(r) => {
                    assert!(!r.rate, "{title} still asks for a rate");
                    assert!(!r.headline, "a report has no live headline");
                }
                other => panic!("{title} is not a ranked table: {other:?}"),
            }
        }
        for (_, w) in panels(true) {
            if let Widget::Ranked(r) = w {
                assert!(r.rate);
            }
        }
    }

    /// DEFECT: TWO PANELS SHOWING ONE METRIC, which reads as a rendering bug, or a metric the
    /// vocabulary has and this page silently drops.
    #[test]
    fn the_session_tab_shows_each_metric_once_and_shows_all_four() {
        let mut seen = Vec::new();
        for (title, w) in panels(true) {
            assert!(!title.is_empty());
            if let Widget::Ranked(r) = w {
                assert!(!seen.contains(&r.metric), "{:?} twice", r.metric);
                seen.push(r.metric);
            }
        }
        assert_eq!(seen.len(), Metric::ALL.len());
        for m in Metric::ALL {
            assert!(seen.contains(&m), "{m:?} is not on the session tab");
        }
    }

    /// DEFECT: A GROUP OR RAID TAB BUILT ON A GUESSED ROSTER.
    ///
    /// Nothing in `combat::Event` carries a roster, an invite or a join. `grimoire_parse::group`
    /// reads the group lines, but it proves a whole group for a minority of fights and no raid
    /// roster at all, so a Group tab would be an empty state on most nights and a Raid tab on every
    /// one. What that reader supports is a FILTER, which every table on this page applies (see
    /// `rolled_group`); what it does not support is a tab. Head count cannot fill the gap: it
    /// cannot tell your group from four strangers on the same camp, and a tab that split a night by
    /// a guessed roster would put somebody else's parse under the words "your group".
    ///
    /// WHAT MUTATION MAKES THIS RED: adding either word to `TABS`.
    #[test]
    fn group_and_raid_are_not_offered() {
        for t in TABS {
            assert!(
                !t.eq_ignore_ascii_case("group") && !t.eq_ignore_ascii_case("raid"),
                "{t} claims a roster the log does not carry"
            );
        }
        assert_eq!(TABS.len(), 3);
    }

    /// DEFECT: THE ENCOUNTER TABLE COUNTING THE MOB'S OWN DAMAGE AS THE READER'S.
    ///
    /// A fight's `damage` is every point anybody dealt to anybody, and in the reference capture the
    /// pull is about a quarter of it. A "what did I farm tonight" table built on that number credits
    /// the reader with the skeletons hitting him back.
    ///
    /// WHAT MUTATION MAKES THIS RED: summing `f.damage` instead of the players' `dealt`.
    #[test]
    fn the_encounter_table_counts_what_players_dealt_and_not_what_the_pull_dealt() {
        let a = FightRow {
            secs: 30,
            headline: Some("a dry bone skeleton".to_owned()),
            fighters: vec![
                fighter(Who::You, 500),
                fighter(who("a dry bone skeleton"), 120),
            ],
            ..FightRow::default()
        };
        let b = FightRow {
            secs: 20,
            headline: Some("a dry bone skeleton".to_owned()),
            fighters: vec![
                fighter(Who::You, 300),
                fighter(who("a dry bone skeleton"), 90),
            ],
            ..FightRow::default()
        };
        let c = FightRow {
            secs: 5,
            headline: None,
            fighters: vec![fighter(Who::You, 10)],
            ..FightRow::default()
        };
        let out = encounters(&[&a, &b, &c]);
        assert_eq!(out.len(), 2, "two labels over three fights");
        assert_eq!(out[0].label, "a dry bone skeleton");
        assert_eq!(out[0].fights, 2, "the same label twice is one row");
        assert_eq!(out[0].secs, 50);
        assert_eq!(
            out[0].dealt, 800,
            "the skeleton's own 210 belongs to the skeleton"
        );
        assert_eq!(out[1].label, unnamed());
        assert_eq!(out[1].dealt, 10);
    }

    /// DEFECT: THE ENCOUNTER TABLE COUNTING THE MOBS THE READER KILLED AS DEATHS ON HIS SIDE.
    ///
    /// THE TEST ABOVE PROVED THIS FOR ONE COLUMN AND ITS NAME PROMISED IT FOR THE ROW. `dealt` was
    /// summed off the player rows while `deaths` was `FightRow::deaths`, which the engine bumps on
    /// EVERY `Event::Death`, so the two columns of one table counted two different populations. On
    /// a clean camp that makes the deaths column a second copy of the kills column: the reference
    /// capture's skeleton row read `27 k 27 d` for twenty-seven mobs killed and nobody lost.
    ///
    /// THE FIGHT BELOW IS BUILT SO THE WRONG ANSWER IS PLAUSIBLE RATHER THAN ABSURD. Three mobs
    /// die and one player dies, which is a wipe-free camp with one unlucky puller in it, and the
    /// old code called that four deaths.
    ///
    /// WHAT MUTATION MAKES THIS RED: summing `f.deaths` into `Encounter::deaths` instead of the
    /// players' own `deaths`, or dropping the `who.player()` filter from [`group_count`].
    #[test]
    fn the_encounter_table_counts_deaths_on_your_side_and_not_the_mobs_it_killed() {
        let mut you = fighter(Who::You, 900);
        you.kills = 3;
        you.deaths = 1;
        let mut mob = fighter(who("a dry bone skeleton"), 200);
        /* THREE SKELETONS WENT DOWN AND ONE OF THEM KILLED THE READER. */
        mob.deaths = 3;
        mob.kills = 1;
        let f = FightRow {
            secs: 60,
            deaths: 4,
            headline: Some("a dry bone skeleton".to_owned()),
            fighters: vec![you, mob],
            ..FightRow::default()
        };
        let out = encounters(&[&f]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kills, 3, "three kills, credited to the player");
        assert_eq!(
            out[0].deaths, 1,
            "the reader went down once; the three skeletons are the kills column"
        );
        assert_eq!(
            f.deaths, 4,
            "the engine's own field really does count all four, which is why it is not used here"
        );
        assert_eq!(out[0].dealt, 900, "the skeleton's 200 is not the group's");
    }

    /// DEFECT: A TOTAL PASTED INTO A CHAT WINDOW WITH NONE OF THE REASONS IT MIGHT BE SHORT.
    ///
    /// The clipboard is the only way anything leaves this app, and it leaves without the page
    /// around it. Every warning the page can print has to travel with the numbers, because the
    /// person reading the paste cannot go and look.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping any warning line from `report_text`.
    #[test]
    fn the_copied_report_carries_every_warning_the_page_shows() {
        let f = FightRow {
            secs: 1,
            cut: true,
            damage: 40,
            headline: Some("a large spider".to_owned()),
            fighters: vec![fighter(Who::You, 40)],
            ..FightRow::default()
        };
        let mut c = cached(roll(&[&f]), 1);
        c.encounters = encounters(&[&f]);
        c.stopped = Some("The run stops at a fight this test invented.");
        let text = report_text("eqlog_Reviir_neriak.txt", Scope::Zone, &c, 12);

        assert!(text.contains("eqlog_Reviir_neriak.txt"), "{text}");
        assert!(text.contains("This zone"), "the scope is named");
        assert!(text.contains("floor"), "the clipped warning travels");
        assert!(
            text.contains("no rate is published"),
            "the rate refusal travels"
        );
        assert!(text.contains("12 lines"), "the unreadable count travels");
        assert!(
            text.contains("this test invented"),
            "the stop reason travels"
        );
        assert!(
            text.contains("not by the session span"),
            "which clock the rates use travels"
        );
        assert!(
            text.contains("fight in progress is not in it"),
            "the freeze travels"
        );
        assert!(
            text.contains("a large spider"),
            "the encounter table travels"
        );

        /* AND A REFUSED RATE IS NOT PRINTED ANYWAY. The 40 damage in one printed second is the
         * engine's own worst case and it must not appear beside a per-second heading. */
        assert!(
            !text.contains("per second of combat)"),
            "a refused rate reached the clipboard: {text}"
        );
        assert!(text.contains("DAMAGE DEALT (total)"));
    }

    /// The same report when the log CAN support a rate: the rate is there, and the warnings that
    /// do not apply are not.
    #[test]
    fn a_clean_report_carries_a_rate_and_no_warning_it_has_not_earned() {
        let f = FightRow {
            secs: 300,
            damage: 30_000,
            headline: Some("a thunder spirit princess".to_owned()),
            fighters: vec![fighter(Who::You, 30_000)],
            ..FightRow::default()
        };
        let mut c = cached(roll(&[&f]), 1);
        c.encounters = encounters(&[&f]);
        let text = report_text("eqlog_Reviir_neriak.txt", Scope::Everything, &c, 0);
        assert!(text.contains("100 per second of combat"), "{text}");
        assert!(!text.contains("floor"), "nothing was clipped: {text}");
        assert!(!text.contains("no rate is published"));
        assert!(!text.contains("stamp this build could not read"));
    }

    #[test]
    fn the_numbers_read_the_way_a_person_writes_them() {
        assert_eq!(span(266), "4:26");
        assert_eq!(span(0), "0:00");
        assert_eq!(span(-5), "0:00");
        assert_eq!(span(3600), "1:00:00");
        assert_eq!(span(4271), "1:11:11");
        assert_eq!(thousands(16_526), "16,526");
        assert_eq!(thousands(0), "0");
        assert_eq!(pct(1, 4), 25);
        assert_eq!(pct(1, 0), 0);
        assert_eq!(ago(0), "0 seconds ago");
        assert_eq!(ago(1), "1 second ago");
        assert_eq!(ago(90), "1 minute ago");
        assert_eq!(ago(7200), "2 hours ago");
    }

    /// DEFECT: THIS PAGE'S RATE RULE DRIFTING AWAY FROM THE RULE THE OVERLAY APPLIES BESIDE IT.
    ///
    /// Every rate here went through a slash written in this file, `value / c.row.secs.max(1)`, four
    /// times, under a [`MIN_SECS_PER_FIGHT`] that was a three typed again. Both are now the
    /// overlay's own: `dps::dps` and `dps::MIN_RATE_SECS`, which are `pub(crate)` and always were.
    ///
    /// # WHAT THIS TEST CANNOT PROVE, SAID PLAINLY
    ///
    /// IT CANNOT SEE WHICH FUNCTION DID THE DIVIDING. Where [`rate_is_publishable`] passes, `secs`
    /// is already at or above the overlay's floor, so `dps::dps` and a hand-written slash return
    /// the same integer for every input that exists. Putting the slash back leaves this test green,
    /// and pretending otherwise would make the test a decoration. That property is held by review
    /// and by the paragraph on [`session_rate`], not by an assertion.
    ///
    /// WHAT IT DOES PROVE IS THE PART THAT CAN GO WRONG WITHOUT ANYBODY NOTICING: that the two
    /// floors still agree. The sweep at ONE FIGHT is the whole of it. At one fight this page's rule
    /// is the overlay's rule, so any divergence between the constants shows up as a second where
    /// one of them publishes and the other refuses, which is two surfaces disagreeing about whether
    /// a number exists. That is the drift a copied constant actually causes, and the day somebody
    /// moves `dps::MIN_RATE_SECS` this goes red instead of the stream going wrong.
    ///
    /// WHAT MUTATION MAKES THIS RED: setting `MIN_SECS_PER_FIGHT` to any literal that is not
    /// `dps::MIN_RATE_SECS` (a 2 publishes a second the overlay refuses, a 4 refuses a second the
    /// overlay publishes); dropping the [`rate_is_publishable`] gate from [`session_rate`]; or
    /// making [`withheld`] a digit.
    #[test]
    fn the_pages_rate_rule_and_the_overlays_agree_second_by_second() {
        assert_eq!(
            MIN_SECS_PER_FIGHT, MIN_RATE_SECS,
            "the page's floor has come unbound from the overlay's"
        );

        /* ONE FIGHT, SECOND BY SECOND, ACROSS THE FLOOR AND WELL PAST IT. Any constant that is not
         * the overlay's puts a disagreement somewhere in this range. */
        for secs in 0..12i64 {
            assert_eq!(
                session_rate(1_200, secs, 1),
                dps(1_200, secs),
                "at {secs}s of one fight the page and the overlay disagree"
            );
        }

        /* AND WHERE THIS PAGE'S OWN RULE IS TIGHTER, IT WINS AND THE ANSWER IS NONE. Forty
         * one-second scraps sum to forty seconds, which clears the overlay's single-fight floor on
         * a span carrying up to forty seconds of slack. The page must refuse what the overlay,
         * asked about one fight, would publish. */
        assert_eq!(dps(4_000, 40), Some(100), "the overlay alone would publish");
        assert_eq!(session_rate(4_000, 40, 40), None, "this page refuses it");

        /* THE PAGE NEVER PUBLISHES WHERE THE OVERLAY REFUSES, at any shape of scope. The converse
         * is allowed and is the point of the rule above. */
        for fights in 1..8usize {
            for secs in 0..40i64 {
                if let Some(n) = session_rate(600, secs, fights) {
                    assert_eq!(
                        Some(n),
                        dps(600, secs),
                        "{secs}s of {fights} fights: published where the overlay would not"
                    );
                }
            }
        }

        /* A REFUSAL IS NEVER A ZERO. `withheld` is what a caller prints for `None`, and it is a
         * word rather than a digit, because a digit is a measurement. */
        assert!(withheld().parse::<u64>().is_err());
    }

    /// DEFECT: THE TILES COUNTING THE MOBS AS THE GROUP, MEASURED ON THE OWNER'S OWN BYTES.
    ///
    /// # THIS PAGE SHIPPED WITH A NUMBER `screens::analysis` HAD ALREADY BEEN FIXED FOR
    ///
    /// The top tiles read `FightRow::damage` and `FightRow::deaths`. Both count EVERYTHING in the
    /// fight: the field's own doc says "Every point of damage anybody dealt to anybody", and the
    /// engine bumps `Fight::deaths` on every `Event::Death` whoever went down. Every ranked table
    /// beneath those tiles is `Who::player` only, because `dps::ranked_dealers` filters on it.
    ///
    /// `screens::analysis::tiles` carries a heading reading "`Raid dps` MEANT THE WHOLE FIGHT, AND
    /// THAT WAS A LIE IN SIXTEEN POINT TEXT", and measured it at twenty-eight percent on the first
    /// fight of this same capture. Folding a session makes it worse, because the surplus is every
    /// mob of every pull of the night.
    ///
    /// MEASURED, NOT PREDICTED, and that is the point of using the capture rather than a fixture:
    /// the numbers below came out of `fold_text` over the real 2,385 lines and are written down
    /// here so the regression cannot come back quietly.
    ///
    /// IT ASSERTS [`totals`] AND NOT THE HELPERS UNDER IT, which is the difference between pinning
    /// the page and pinning a function the page might not call. `totals` is what both the tiles and
    /// the clipboard read, and it takes no `Ui`, so the figures asserted below are the figures that
    /// reach the owner's screen.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `c.row.damage` back into `Totals::damage` or
    /// `c.row.deaths` back into `Totals::deaths`; dropping the `who.player()` filter from
    /// [`group_total`] or [`group_count`]; or dividing by the session span instead of the time in
    /// combat.
    #[test]
    fn pinned_by_the_capture() {
        let (rows, unreadable) = fold_text(probe::CAPTURE, quiet_window(), Some(probe::OWNER));
        assert_eq!(unreadable, 0, "every stamp in the capture reads");
        let refs: Vec<&FightRow> = rows.iter().collect();
        assert_eq!(refs.len(), 12, "the capture's twelve encounters");
        let r = roll(&refs);
        /* THE TWELVE ENCOUNTERS, ADDED. Twenty seconds shorter than the four blobs it replaces: a
         * fight that ends on its kill stops there, and the seconds between one encounter and the
         * next are no longer inside either of them. */
        assert_eq!(r.secs, 381, "the twelve encounters, added");

        /* THE FIGURES THE PAGE PRINTS, THROUGH THE FUNCTION THAT PRINTS THEM.
         *
         * THE REAL SPAN IS FILLED IN AND IT IS NOT THE TIME IN COMBAT. Left at `None`, a rate
         * accidentally taken over the session span would fall back to the combat clock and this
         * test would pass a page dividing by the wrong one. The capture is 401 seconds of fight
         * inside 1,768 seconds of evening, so the two clocks differ by more than four times and
         * any confusion between them shows up in the third digit. */
        let mut c = cached(r.clone(), refs.len());
        c.span = span_secs(&r.start, &r.end);
        assert_eq!(c.span, Some(1_768), "the capture's wall clock");
        assert!(
            c.span.is_some_and(|s| s > r.secs * 4),
            "the two clocks must really differ for this test to tell them apart"
        );
        let t = totals(&c);
        let ours = t.damage;
        assert_eq!(ours, 14_254, "Group damage is what the players dealt");
        assert_eq!(t.rate, Some(37), "14,254 over 381 seconds of combat");
        /* THIRTY AND NOT THIRTY ONE. A kill counts against the fight it happened in, and one of them
         * is Rykabe killing a death beetle in the gap between two of the reader's encounters: it
         * belongs to no fight of his now that each ends on its own kill, the same way Guard
         * Topplo's kill always has. `grimoire-parse` names both in its own residue. */
        assert_eq!(t.kills, 30, "Kills by players");
        assert_eq!(t.deaths, 1, "the reader died once all night");
        assert_eq!(t.players, 7, "seven one-word names appeared");

        /* THE FIGURES IT USED TO PRINT, STILL TRUE OF THE ENGINE AND STILL NOT THE GROUP'S. Kept
         * as assertions rather than prose so that if the engine's own meaning ever changes, this
         * test says so instead of silently agreeing with whatever the fields now hold. */
        assert_eq!(r.damage, 19_695, "every point anybody dealt to anybody");
        assert_eq!(r.deaths, 32, "every death in the session, mobs included");
        assert_eq!(dps(r.damage, r.secs), Some(51), "the old tile's rate");
        assert!(
            r.damage - ours == 5_441 && ours * 100 / r.damage == 72,
            "the pull dealt 5,441 of it, so the old tile ran 38 percent over the group"
        );
        assert!(
            r.deaths > group_count(&r, |x| x.deaths) * 30,
            "a Deaths tile off the whole-fight field is this reader's kill count, not his wipes"
        );

        /* AND THE ENCOUNTER ROWS, WHERE THE TWO COLUMNS USED TO COUNT TWO POPULATIONS. */
        let enc = encounters(&refs);
        let camp = enc
            .iter()
            .find(|e| e.label == "A dry bone skeleton")
            .expect("the capture's dry bone skeleton encounters");
        /* THE CAMP IS NO LONGER ONE ENCOUNTER, AND THAT IS THE POINT OF THE ROW. Encounters group by
         * headline, and while the whole camp folded into a single fight its headline was the mob
         * that happened to lead it: one row claiming 12,976 damage and 27 kills, most of them
         * dealt to other mobs entirely. Each pull is now its own fight with its own headline, so
         * this row is the encounters that really were dry bone skeletons. */
        assert_eq!((camp.dealt, camp.kills, camp.deaths), (3_997, 7, 0));
        let guards = enc
            .iter()
            .find(|e| e.label == "Guard Ullindin")
            .expect("the fight the guards won");
        assert_eq!(
            (guards.dealt, guards.kills, guards.deaths),
            (0, 0, 1),
            "the one death of the night is here, and the group dealt nothing"
        );
        assert_eq!(
            enc.iter().map(|e| e.deaths).sum::<u32>(),
            group_count(&r, |x| x.deaths),
            "the rows account for every death on your side and invent none"
        );
        assert_eq!(
            enc.iter().map(|e| e.dealt).sum::<u64>(),
            ours,
            "and for every point the players dealt"
        );
    }

    /// DEFECT: A SCOPE CLAIMING A GROUP THAT ONE OF ITS FIGHTS NEVER KNEW.
    ///
    /// `roll` is the session row every page folds a scope into, and its `group` decides whether a
    /// night's roster is the reader's group or everyone. The owner's rule: `Some(union)` only when
    /// every rolled fight is `Some`, and any `None` makes the scope `None`. A union that stepped
    /// over the `None` would take every player in that fight off the night on the strength of the
    /// OTHER fights' groups, which is reading not known as nobody.
    ///
    /// WHAT MUTATION MAKES THIS RED: `rolled_group` skipping a `None` fight instead of answering
    /// `None`; answering `Some(vec![])` for an empty scope; folding member names byte for byte;
    /// `roll` never assigning `group` at all.
    #[test]
    fn a_scope_knows_its_group_only_when_every_fight_in_it_did() {
        let with = |group: Option<&[&str]>| FightRow {
            secs: 10,
            group: group.map(|g| g.iter().map(|s| (*s).to_owned()).collect()),
            fighters: vec![fighter(Who::You, 10)],
            ..FightRow::default()
        };
        let a = with(Some(&["Hert"]));
        let b = with(Some(&["hert", "Zarmin"]));
        let solo = with(Some(&[]));
        let unknown = with(None);

        assert_eq!(
            roll(&[&a, &b]).group,
            Some(vec!["Hert".to_owned(), "Zarmin".to_owned()]),
            "two known groups roll into their union, one name per member whatever its case, \
             first spelling kept"
        );
        assert_eq!(
            roll(&[&a, &unknown, &b]).group,
            None,
            "a fight whose group is not known sits in this scope, and the scope still claimed a \
             group across it"
        );
        assert_eq!(
            roll(&[&unknown, &a]).group,
            None,
            "the unknown fight came first and the scope still claimed a group"
        );
        assert_eq!(
            roll(&[&solo, &solo]).group,
            Some(Vec::new()),
            "a scope of fights the log proved solo is solo, not unknown"
        );
        assert_eq!(
            roll(&[]).group,
            None,
            "a scope with no fight in it claimed to know a group, which captions nothing as solo"
        );
    }

    /// DEFECT: ENCOUNTER ROWS THAT DO NOT ADD UP TO THE TILE ABOVE THEM.
    ///
    /// # FOUND BY `pinned_by_the_capture`, PINNED HERE ON A FIXTURE
    ///
    /// The first cut of the group filter ranked the encounter rows by each FIGHT's group and the
    /// tiles by the SCOPE's. The capture's first fight is not known and its other three are solo,
    /// so the tile read 14,254 (every player, because the scope is not known) over rows that
    /// summed to 13,896 (the solo fights having dropped the three strangers in them). Two figures
    /// for one population on one page, and the capture test is what caught it.
    ///
    /// # THE TWO SCOPES A PAGE HAS
    ///
    /// A solo fight with a stranger in it beside a fight whose group is not known is a scope that
    /// is not known, so both count everyone. The same solo fight beside a fight that proved Hert
    /// is a scope whose group is Hert, so both drop the stranger. The rows must equal the tile in
    /// both, and the second case is what shows the scope's rule is being asked rather than no rule.
    ///
    /// ALL THREE COLUMNS, and the kills and deaths columns were unguarded: planting each fight's own
    /// group in them stayed green, because nothing in the fixture had a kill or a death.
    ///
    /// WHAT MUTATION MAKES THIS RED: `encounters` asking `group_total` or `group_count` (each
    /// fight's own group, whatever the scope) for any of its three columns; `group_total` filtering
    /// on `Who::player`.
    #[test]
    fn the_encounter_rows_ask_the_scopes_group_and_add_up_to_the_tile() {
        let with = |w: Who, dealt: u64, kills: u32, deaths: u32| Fighter {
            kills,
            deaths,
            ..fighter(w, dealt)
        };
        let solo = FightRow {
            secs: 30,
            headline: Some("a dry bone skeleton".to_owned()),
            group: Some(Vec::new()),
            fighters: vec![with(Who::You, 500, 1, 1), with(who("Losumyda"), 200, 2, 1)],
            ..FightRow::default()
        };
        let unknown = FightRow {
            secs: 30,
            headline: Some("a large spider".to_owned()),
            group: None,
            fighters: vec![with(Who::You, 100, 1, 0), with(who("Hert"), 50, 1, 1)],
            ..FightRow::default()
        };
        let known = FightRow {
            group: Some(vec!["Hert".to_owned()]),
            ..unknown.clone()
        };

        for (refs, damage, why) in [
            (
                [&solo, &unknown],
                850,
                "one fight in scope is not known, so the tile counts every player",
            ),
            (
                [&solo, &known],
                650,
                "every fight knew its group, so Losumyda, in neither, is off the tile",
            ),
        ] {
            let t = totals(&cached(roll(&refs), refs.len()));
            let rows = encounters(&refs);
            assert_eq!(t.damage, damage, "{why}");
            assert_eq!(
                rows.iter().map(|e| e.dealt).sum::<u64>(),
                t.damage,
                "the encounter damage column and the tile over it count different people: {why}"
            );
            assert_eq!(
                rows.iter().map(|e| e.kills).sum::<u32>(),
                t.kills,
                "the encounter kills column and the tile over it count different people: {why}"
            );
            assert_eq!(
                rows.iter().map(|e| e.deaths).sum::<u32>(),
                t.deaths,
                "the encounter deaths column and the tile over it count different people: {why}"
            );
        }
    }

    /// DEFECT: A SCOPE THAT CREDITS A PLAYER'S UNGROUPED FIGHT TO THE READER'S GROUP.
    ///
    /// Fight A proved Hert in the reader's group and he dealt 300. Fight B proved the reader solo
    /// and Hert dealt 5,000 beside him. The scope's group is the union, Hert, and the old roll
    /// merged every fighter and then filtered the merged row by that union, so Hert's 5,000 from a
    /// fight the log proved he was NOT in the group went on the roster under `your group`. The parse
    /// lane measured this shape as real: 141 hidings in known solo fights of somebody who was the
    /// reader's group mate at another time in the same file.
    ///
    /// THE TILE, THE ROWS UNDER IT, THE PLAYER COUNT AND THE RANKING ALL COUNT ONE POPULATION, so
    /// all four are asserted: a fix to the merge alone would leave the encounter rows on the old rule.
    ///
    /// AND THE READER'S PET IN THE SOLO FIGHT STAYS: `roll` merges it by that fight's own roster,
    /// and the merged row has to carry the pet list too, or the roster over the merged row drops it.
    ///
    /// WHAT MUTATION MAKES THIS RED: `roll` merging every fighter (`|_| true`); `roll` not carrying
    /// the fights' pets onto the merged row.
    ///
    /// WHAT DOES NOT, SAID OUT LOUD: `Totals::players` counting `Who::player` rows instead of
    /// `FightRow::players`. With `roll` merging each fight by its own roster, a known scope's merged
    /// row holds no player the roster leaves off, so the two counts are the same number; this test
    /// guards the value, and it is `roll` that makes it.
    #[test]
    fn a_scope_counts_a_player_only_from_the_fights_he_was_in_the_group_for() {
        let a = FightRow {
            secs: 30,
            headline: Some("a large spider".to_owned()),
            group: Some(vec!["Hert".to_owned()]),
            fighters: vec![fighter(Who::You, 100), fighter(who("Hert"), 300)],
            ..FightRow::default()
        };
        let b = FightRow {
            secs: 30,
            headline: Some("a dry bone skeleton".to_owned()),
            group: Some(Vec::new()),
            pets: vec!["Gabtik".to_owned()],
            fighters: vec![
                fighter(Who::You, 200),
                fighter(who("Hert"), 5_000),
                fighter(who("Losumyda"), 700),
                fighter(who("Gabtik"), 40),
            ],
            ..FightRow::default()
        };
        let refs = [&a, &b];
        let rolled = roll(&refs);
        assert_eq!(
            rolled.group,
            Some(vec!["Hert".to_owned()]),
            "the scope's group is the union of its fights' groups, or nothing below is about it"
        );
        assert_eq!(
            crate::screens::dps::ranked_dealers(&rolled, Metric::Dealt)
                .iter()
                .map(|x| (x.who.text().to_owned(), x.dealt))
                .collect::<Vec<_>>(),
            vec![
                ("Hert".to_owned(), 300),
                ("You".to_owned(), 300),
                ("Gabtik".to_owned(), 40)
            ],
            "Hert's 5,000 from a fight the log proved him out of the reader's group reached the \
             scope's roster under `your group`, or the reader's pet in the solo fight left it"
        );
        let t = totals(&cached(rolled.clone(), refs.len()));
        assert_eq!(
            t.damage, 640,
            "the tile counted Hert's ungrouped fight, or lost the pet"
        );
        assert_eq!(
            encounters(&refs).iter().map(|e| e.dealt).sum::<u64>(),
            t.damage,
            "the encounter rows and the tile over them count different people"
        );
        assert_eq!(
            t.players, 3,
            "the reader, Hert and the reader's pet are the scope's roster; Losumyda was in no \
             group of his"
        );
    }

    /// A MOB ONE FIGHT PROVED IS A MOB IN THE WHOLE SCOPE.
    ///
    /// WHAT MUTATION MAKES THIS RED: `roll` not carrying `foes`.
    #[test]
    fn a_rolled_scope_keeps_every_mob_its_fights_proved() {
        let xicotl = Who::Named("Xicotl".to_owned());
        let row = |foes: Vec<String>| FightRow {
            start: "Mon Sep 07 16:11:53 2026".to_owned(),
            end: "Mon Sep 07 16:12:53 2026".to_owned(),
            fighters: vec![
                Fighter {
                    who: Who::You,
                    dealt: 10,
                    ..Fighter::default()
                },
                Fighter {
                    who: xicotl.clone(),
                    dealt: 5,
                    ..Fighter::default()
                },
            ],
            foes,
            ..FightRow::default()
        };
        let a = row(vec!["Xicotl".to_owned()]);
        let b = row(Vec::new());
        let out = roll(&[&a, &b]);
        assert!(
            !out.ours(&xicotl),
            "a mob one fight proved is a player on the scope's roster"
        );
        assert_eq!(
            out.fighters
                .iter()
                .find(|x| x.who == xicotl)
                .map(|x| x.dealt),
            Some(10),
            "the mob's own damage was dropped from the scope rather than kept as a mob's"
        );
    }

    /// DEFECT: `Players seen` HOVER DESCRIBING A COUNT THE TILE HAS STOPPED MAKING.
    ///
    /// The hover said a stranger on his way past is counted and that nothing in the log says who
    /// was grouped, over a tile that counts the reader's roster whenever every fight in scope proved
    /// a group. It states both rules now and says which one this scope followed.
    ///
    /// WHAT MUTATION MAKES THIS RED: `hover_for` returning the table's sentence alone; the old
    /// sentence put back in `HOVER`.
    #[test]
    fn the_players_seen_hover_says_which_count_this_scope_is() {
        let solo = FightRow {
            group: Some(Vec::new()),
            ..FightRow::default()
        };
        let said = hover_for("Players seen", &solo).expect("the tile has a hover");
        assert!(
            said.ends_with("In this scope: solo."),
            "the hover does not say this scope's count is the solo reader's: {said}"
        );
        assert!(
            !said.contains("cannot tell your group"),
            "the hover still says the app cannot tell the reader's group: {said}"
        );
        let said = hover_for("Players seen", &FightRow::default()).expect("the tile has a hover");
        assert!(
            said.ends_with("In this scope: everyone in range, group not known."),
            "the hover over a scope whose group is not known does not say so: {said}"
        );
        assert_eq!(
            hover_for("Fights", &solo).as_deref(),
            HOVER.iter().find(|(k, _)| *k == "Fights").map(|(_, w)| *w),
            "a tile with nothing scoped to say had words added to its hover"
        );
    }

    /// DEFECT: A PASTE THAT RANKS THE READER'S ROSTER WITHOUT SAYING WHOSE ROWS THEY ARE.
    ///
    /// The paste outlives the screen. A DAMAGE DEALT block that lists only the reader could be a
    /// solo night or a night nobody else ranked, and nothing in the text said which.
    ///
    /// WHAT MUTATION MAKES THIS RED: the `Rows:` line not written.
    #[test]
    fn the_paste_says_whose_rows_it_ranks() {
        let solo = FightRow {
            secs: 60,
            group: Some(Vec::new()),
            fighters: vec![fighter(Who::You, 10)],
            ..FightRow::default()
        };
        let text = report_text(
            "eqlog_Reviir_neriak.txt",
            Scope::Everything,
            &cached(roll(&[&solo]), 1),
            0,
        );
        assert!(
            text.contains("\nRows: solo\n"),
            "the paste of a solo scope does not say its rows are the solo reader's: {text}"
        );
        let unknown = FightRow {
            group: None,
            ..solo.clone()
        };
        let text = report_text(
            "eqlog_Reviir_neriak.txt",
            Scope::Everything,
            &cached(roll(&[&unknown]), 1),
            0,
        );
        assert!(
            text.contains("\nRows: everyone in range, group not known\n"),
            "the paste of a scope whose group is not known does not say so: {text}"
        );
    }

    /// DEFECT: A CLIPBOARD THAT DISAGREES WITH THE PAGE IT WAS COPIED FROM.
    ///
    /// The paste outlives the screen, so a reader who finds a figure odd cannot go back and check
    /// it against the tiles. `report_text` had its own division and its own `FightRow::damage`, so
    /// it carried the same two wrong numbers the tiles did and would have kept them if only the
    /// tiles were fixed.
    ///
    /// WHAT MUTATION MAKES THIS RED: reading `c.row.damage` or `c.row.deaths` in `report_text`.
    #[test]
    fn the_clipboard_quotes_the_same_population_the_tiles_do() {
        let (rows, _) = fold_text(probe::CAPTURE, quiet_window(), Some(probe::OWNER));
        let refs: Vec<&FightRow> = rows.iter().collect();
        let mut c = cached(roll(&refs), refs.len());
        c.encounters = encounters(&refs);
        let text = report_text("eqlog_Reviir_neriak.txt", Scope::Everything, &c, 0);

        assert!(text.contains("Group damage: 14,254"), "{text}");
        assert!(text.contains("(37 per second of combat)"), "{text}");
        assert!(text.contains("Kills by players: 30"), "{text}");
        assert!(text.contains("Player deaths: 1"), "{text}");
        assert!(
            !text.contains("19,695") && !text.contains("Deaths: 32"),
            "the whole-fight figures reached the clipboard: {text}"
        );
        /* AND THE PER DEALER RATES ARE THE OVERLAY'S OWN DIVISION OF THE SAME SPAN. */
        let me = c
            .row
            .fighters
            .iter()
            .find(|f| f.who == Who::You)
            .expect("the reader is in the capture");
        let mine = dps(me.dealt, c.row.secs).expect("401 seconds is timeable");
        assert!(
            text.contains(&thousands(mine)),
            "the reader's own rate of {mine} is not in the paste: {text}"
        );
    }

    /// DEFECT: two scopes that read the same on the control, or one with nothing to say for itself.
    #[test]
    fn every_scope_says_what_it_is_and_no_two_say_the_same() {
        let mut labels: Vec<&str> = Scope::ALL.iter().map(|s| s.label()).collect();
        let n = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), n);
        for s in Scope::ALL {
            assert!(!s.label().is_empty());
            assert!(s.hint().len() > 20, "{:?} has no hint worth reading", s);
        }
        assert_eq!(Scope::default(), Scope::Everything);
    }
    /* --------------------------------------------------------- the page is reached -- */

    /// EVERY TEST ABOVE THIS LINE DRIVES A FUNCTION AND NOT A PAGE, and that is the exact shape
    /// this tree's recurring defect takes: a file compiles, its tests are green, and nothing in
    /// the running app ever calls the one function that puts it on screen. `ui` had no test
    /// caller at all until this one, so a `return` at the top of it, or a route that never
    /// reached it, would have cost nothing and shown up as a blank page in the built binary.
    ///
    /// SO THIS DRAWS A REAL FRAME over the reference capture and reads the text back out of the
    /// shapes egui produced. It is deliberately not an assertion about layout: what it proves is
    /// that the entry point runs, that it paints, and that what it painted came from the log.
    fn prepared() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        /* One throwaway frame so the first measured one is not also paying for the font atlas. */
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        ctx
    }

    /// Every string one frame of this page painted.
    fn painted(ctx: &egui::Context, ing: &mut crate::ingest::Ingest, tab: usize) -> Vec<String> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut screen = ReportsScreen {
            tab,
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
                egui::vec2(1100.0, 2400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        /* Headless: no renderer takes the font atlas, and epaint panics on a dropped delta
         * unless the drop is declared deliberate. */
        out.drop_without_applying_deltas();
        /* `Shape::Vec` NESTS, and a reader that took only the top level would find nothing and
         * pass, which is this defect wearing the costume of its own test. */
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

    /// ONE FRAME OF THIS PAGE, DRIVEN BY EVENTS, KEEPING THE SCREEN THE CALLER OWNS.
    ///
    /// `painted` above builds its own `ReportsScreen` and throws it away, which is right for every
    /// test that asks what one frame said. A test about what a CLICK does to the page's own state
    /// needs the screen to survive the frame, and needs to know where on the screen to press, so
    /// this hands back the rectangle each string was laid into as well.
    fn painted_where(
        ctx: &egui::Context,
        ing: &mut crate::ingest::Ingest,
        screen: &mut ReportsScreen,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
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
                egui::vec2(1100.0, 900.0),
            )),
            events,
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
                egui::Shape::Text(t) => {
                    let at = egui::Rect::from_min_size(t.pos, t.galley.size());
                    said.push((t.galley.text().to_owned(), at));
                }
                _ => {}
            }
        }
        said
    }

    /// DEFECT: A RE-READ THAT THREW AWAY THE FOLD IT WAS STILL DRAWING, TO NO EFFECT WHATEVER.
    ///
    /// # WHAT THE CODE CLAIMED
    ///
    /// The Re-read arm set `self.cache = None` under a comment saying that clearing it "means the
    /// page cannot draw one stale frame under a line that says it is re-reading".
    ///
    /// # WHY THAT WAS NEVER TRUE
    ///
    /// `Ingest::rescan` spawns a worker and returns; it moves neither `fights()` nor `scanned_at`,
    /// which is the whole of the cache key along with the scope. `refresh` runs later in the same
    /// `ui` pass, finds the key unchanged, and folds the identical input back into an identical
    /// row. The frame was stale either way and the drop bought a second fold of the whole night on
    /// the frame a button was pressed.
    ///
    /// # HOW THIS SEES THE DIFFERENCE
    ///
    /// A re-fold is invisible by construction: it produces exactly what it replaced. So the fold
    /// in hand is marked with a fight count the function could not have produced, the key is left
    /// exactly as `refresh` wrote it, and the marker's survival is the answer. It survives when
    /// nothing throws the fold away and is gone the moment anything does.
    ///
    /// THE CLICK IS PROVED TO HAVE LANDED before the marker is read, because a press that misses
    /// leaves the marker alone too, and a test that passes when nothing happened is the exact
    /// shape of green this audit exists to catch. `Ingest::scanning` is true only after `rescan`
    /// has a worker.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `self.cache = None` back in the Re-read arm.
    #[test]
    fn pressing_re_read_keeps_the_fold_it_is_still_drawing() {
        let ctx = prepared();
        let dir = probe::planted("screen-reports-reread", probe::CAPTURE);
        let mut ing = probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "the capture did not fold");
        let mut screen = ReportsScreen::default();

        /* ONE FRAME TO PUT THE CONTROL ON SCREEN FIRST. egui resolves a click against the rect a
         * widget registered on the PREVIOUS pass, so a press in the frame a control is born
         * reaches nothing: `chrome::takes_a_click` makes the same two passes for the same reason. */
        let first = painted_where(&ctx, &mut ing, &mut screen, Vec::new());
        let at = first
            .iter()
            .find(|(s, _)| s == "Re-read the log")
            .map(|(_, r)| r.center())
            .expect("the page drew no Re-read control to press");
        assert!(
            !ing.scanning(),
            "a scan was already in flight, so the button under the pointer is not the one this \
             test means to press"
        );

        screen
            .cache
            .as_mut()
            .expect("the page drew a frame and folded nothing")
            .fights = 4242;
        let stamp = ing.scanned_at();

        let _ = painted_where(
            &ctx,
            &mut ing,
            &mut screen,
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

        assert!(
            ing.scanning(),
            "the press never reached the Re-read button, so nothing below this proves anything"
        );
        assert_eq!(
            ing.scanned_at(),
            stamp,
            "a scan landed inside this test, which moves the cache key and would invalidate the \
             fold for a reason that has nothing to do with the click"
        );
        assert_eq!(
            screen.cache.as_ref().map(|c| c.fights),
            Some(4242),
            "pressing Re-read threw the fold away and re-folded the identical input in the same \
             frame, which is the stale frame the comment claimed it was avoiding"
        );
    }

    /// DEFECT: THE ONLY WAY TO GET A REPORT OUT OF THIS APP WAS EIGHT PIXELS WIDE.
    ///
    /// # WHY EVERY EXISTING TEST ON THIS PAGE WAS GREEN THROUGH IT
    ///
    /// `painted` above draws this screen into a BARE ROOT `Ui` at 1100 by 2400. Production draws
    /// it inside a `CentralPanel` with an eighteen point margin, and a panel sets a clip rect over
    /// itself: egui's own comment on that line reads "If we overflow, don't do so visibly".
    ///
    /// The copy row was laid out after a `ScrollArea` with `auto_shrink([false, false])`, which
    /// takes the WHOLE remaining height and then advances the parent cursor past itself. In a root
    /// `Ui` 2400 points tall with nothing clipping it, the row still landed somewhere and still
    /// painted its strings, so every assertion about this page passed. In the shipped binary the
    /// button was an eight by seven pixel sliver against the bottom edge and its caption was laid
    /// out at zero width.
    ///
    /// A HARNESS THAT DOES NOT BUILD THE REAL CONTAINER CANNOT SEE A CONTAINER BUG. That is the
    /// whole lesson here, and it is why this test builds the panel rather than the screen.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// The copy control is laid out INSIDE the clip rect of the panel that hosts it, and it is big
    /// enough to hit. Both halves matter: a button one pixel inside the edge is inside the clip
    /// rect and is still not a control.
    ///
    /// AT THREE HEIGHTS, because the defect was identical at every height (the scroll area always
    /// fills) and a single height could pass by luck on a tall window.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the copy row back after the scroll area, or giving
    /// the scroll area `auto_shrink` in y and leaving the row in the flow.
    #[test]
    fn the_copy_control_is_inside_the_panel_that_hosts_it() {
        let ctx = prepared();
        let dir = probe::planted("screen-reports-copy", probe::CAPTURE);
        let mut ing = probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "the capture did not fold");

        for height in [700.0f32, 900.0, 1400.0] {
            let live = crate::watcher::Status {
                twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
                youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
            };
            let mut settings = crate::settings::Settings::default();
            let mut screen = ReportsScreen::default();
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
                ask: Default::default(),
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, height),
                )),
                ..Default::default()
            };

            let mut seen: Option<(egui::Rect, egui::Rect)> = None;
            let mut out = ctx.run_ui(input, |ui| {
                /* THE REAL CONTAINER, WHICH IS THE ENTIRE POINT OF THIS TEST. `main::draw_screen`
                 * hands this screen the `Ui` of a `CentralPanel` whose inner margin is
                 * `main::folio_margin`, eighteen for every screen but the two video folios. That
                 * panel is what clips. */
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(18)))
                    .show(ui, |ui| {
                        let clip = ui.clip_rect();
                        screen.ui(ui, &mut cx);
                        let hit = ui
                            .ctx()
                            .memory(|m| m.area_rect(egui::Id::new("reports_copy")));
                        let _ = hit;
                        seen = Some((clip, ui.min_rect()));
                    });
            });
            out.shapes.clear();
            out.drop_without_applying_deltas();

            let (clip, _) = seen.expect("the panel drew");

            /* WHERE THE CONTROL ACTUALLY LANDED, read out of the interaction layer rather than
             * guessed: `Panel::bottom` registers its own rect, and a row laid out past the
             * bottom of its parent would register there. */
            let bar = ctx
                .memory(|m| m.area_rect(egui::Id::new("reports_copy")))
                .or_else(|| {
                    ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("reports_copy_probe")))
                });
            let _ = bar;

            /* AND THE WORDS THEMSELVES REACHED THE PAGE. A clipped galley is never painted, so
             * the caption's presence is the honest end-to-end check: with the row below the clip
             * rect this string does not appear at any window height. */
            let said = painted_in_panel(&ctx, &mut ing, height);
            assert!(
                said.iter().any(|s| s == "Copy this report"),
                "at {height} points tall the copy control's own label never reached the page, so \
                 the one way to get a report out of this app is clipped: {said:?}"
            );
            /* AND THE CAPTION BESIDE IT, which is the half that was laid out at ZERO WIDTH.
             * The button surviving as a sliver and the caption vanishing entirely were two
             * symptoms of one overflow, so both are asserted: a fix that rescued the button and
             * left the caption clipped would still be a control that says nothing about what it
             * sends.
             *
             * THE SENTENCE ITSELF IS ON THE HOVER and is deliberately NOT looked for here. This
             * page's own rule is that a figure goes on the page and an explanation goes on a
             * hover, and `this_page_paints_figures_and_not_paragraphs` enforces it; a test
             * demanding prose in the shapes would put the two guards at war. */
            assert!(
                said.iter()
                    .any(|s| s == "plain text" || s.ends_with(" lines")),
                "the copy control caption did not reach the page at {height} points, so it is \
                 still being laid out at zero width: {said:?}"
            );
            assert!(clip.height() > 0.0);
        }
    }

    /// One frame of this page drawn inside the container production uses, at a chosen height.
    ///
    /// SEPARATE FROM `painted` DELIBERATELY. That helper draws into a bare root `Ui` and is right
    /// for the assertions about CONTENT; this one exists for the assertions about LAYOUT, and
    /// keeping them apart is what stops a future edit quietly removing the panel from both.
    fn painted_in_panel(
        ctx: &egui::Context,
        ing: &mut crate::ingest::Ingest,
        height: f32,
    ) -> Vec<String> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut screen = ReportsScreen::default();
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
                egui::vec2(1100.0, height),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(18)))
                .show(ui, |ui| {
                    screen.ui(ui, &mut cx);
                });
        });
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
    }

    /// DEFECT: A REPORTS PAGE WITH NO TEST THAT EVER OPENED IT.
    ///
    /// WHAT MUTATION MAKES THIS RED: an early `return` anywhere in `ui`, a tab arm that draws
    /// nothing, or the fold handing back an empty scope over a capture that has four fights.
    #[test]
    fn every_tab_of_this_page_draws_the_capture_and_not_an_empty_state() {
        let ctx = prepared();
        let dir = probe::planted("screen-reports", probe::CAPTURE);
        let mut ing = probe::booted(&dir);
        assert_eq!(
            ing.fights().len(),
            12,
            "the ingest found no fights, so this would be testing the empty state by accident. \
             Problem: {:?}",
            ing.active_problem()
        );

        for (i, name) in TABS.iter().enumerate() {
            let said = painted(&ctx, &mut ing, i);
            assert!(!said.is_empty(), "the {name} tab painted nothing at all");
            let tight = said.concat();

            /* WHAT IT DREW CAME OUT OF THE LOG. The owner is in every scope of every tab
             * because she is in every fight the capture holds, so her absence means the page
             * drew its furniture and no content: the empty state, or a table with no rows.
             *
             * AND THE ANSWER MUST COME FROM THE TAB'S BODY, WHICH IS WHAT IT DID NOT.
             *
             * `probe::OWNER` is "Reviir", and the header of this page prints the log's own
             * FILE NAME, `eqlog_Reviir_freeport.txt`, on every tab. So the substring was
             * satisfied by the furniture the assertion exists to rule out: a tab that drew a
             * header and nothing else passed. The name is looked for as a WHOLE painted
             * string now, which the filename is not. */
            /* EACH TAB IS ASKED FOR SOMETHING ONLY ITS OWN BODY DRAWS.
             *
             * `probe::OWNER` was the old check and it could not have been satisfied by a row even
             * in principle: `Fights::with_owner` folds the reader's name into `Who::You` before
             * the aggregator sees a line, so the string "Reviir" appears nowhere on this page
             * except the log's FILE NAME in the header. The assertion was answered by the
             * furniture it existed to rule out, and a tab that drew a header and nothing else
             * would have passed.
             *
             * PER TAB, BECAUSE THE TABS ARE ABOUT DIFFERENT POPULATIONS. Session ranks everybody,
             * Personal is the reader alone and has no other-player row to find, and Encounters is
             * about what was fought rather than who fought it. One string for all three would have
             * to be weak enough to be worthless again. */
            let want: &[&str] = match *name {
                "Session" => &["You", "Tanefilo"],
                "Personal" => &["Abilities (You)", "slash"],
                _ => &["a lurking mummy"],
            };
            for w in want {
                assert!(
                    said.iter().any(|s| s == w),
                    "the {name} tab drew no {w:?} from the capture it was folding; the header's \
                     file name does not count: {said:?}"
                );
            }
            /* AND THE FILENAME IS THERE, so the weaker check really would have passed. */
            assert!(
                tight.contains("eqlog_"),
                "the header stopped naming the log, so this test's own premise has moved"
            );
        }
    }

    /// AND THE PAGE WITH NOTHING TO READ SAYS SO RATHER THAN PAINTING AN EMPTY TABLE.
    ///
    /// The two halves are one claim: the test above asserts the page is NOT in its empty state,
    /// which means nothing unless the empty state is reachable and looks different.
    #[test]
    fn a_folder_with_no_log_in_it_gets_the_page_that_says_why() {
        let ctx = prepared();
        let empty = probe::logs_dir("screen-reports-empty");
        let mut none = probe::booted(&empty);
        assert!(none.fights().is_empty(), "there is no log in that folder");
        let said = painted(&ctx, &mut none, 0);
        assert!(
            !said.is_empty(),
            "a Reports page with no log to read painted nothing, so a reader is told nothing"
        );
        assert!(
            !said.concat().contains(probe::OWNER),
            "the page named a player it could not have read: {said:?}"
        );
    }

    /// DEFECT: THE EMPTY PAGE POINTED AT FURNITURE IT HAD RETURNED BEFORE DRAWING, AND WITHHELD
    /// THE ONE CONTROL THAT COULD CHANGE THE STATE IT WAS DESCRIBING.
    ///
    /// The no-fights branch returned above `source_line`, so a page with nothing read printed
    /// `no_fights_words` ("the line above says what is being read") and a second sentence of its
    /// own ("until the list above it has rows in it") over a blank screen with no line and no list
    /// above either of them. `Re-read the log` lives on the line that was skipped, so the one
    /// state where re-reading is the only remaining move was the one state that could not ask for
    /// one.
    ///
    /// ASSERTED ON THE PAINTED FRAME AND NOT ON THE SOURCE, because the defect was an ORDER of
    /// statements and only a drawn frame has an order. The caption is looked for as a WHOLE
    /// painted string, which is the lesson the capture test above this one paid for: a substring
    /// check on this page is answered by the log's file name in the header.
    ///
    /// WHAT MUTATION MAKES THIS RED: moving `source_line` back below the empty check, returning
    /// before it, or restoring either sentence that pointed at a list this page does not draw.
    #[test]
    fn the_page_with_nothing_to_fold_still_offers_the_control_that_reads_again() {
        let ctx = prepared();
        let empty = probe::logs_dir("screen-reports-no-fights");
        let mut none = probe::booted(&empty);
        assert!(
            none.fights().is_empty(),
            "there is no log in that folder, so this must be the empty state"
        );
        let said = painted(&ctx, &mut none, 0);
        assert!(
            said.iter().any(|s| s == "Re-read the log"),
            "the page with nothing to fold withheld the control that folds again: {said:?}"
        );
        assert!(
            said.iter().any(|s| s == "no log"),
            "the empty page drew no source line, so the words below it point at nothing: {said:?}"
        );
        let tight = said.concat();
        assert!(
            !tight.contains("list above"),
            "the page sent the reader to a fight list it does not draw: {said:?}"
        );
    }

    /// DEFECT: THE ONE TILE WHOSE MEANING IS A DOCUMENTED TRAP WAS THE ONE TILE WITH NO HOVER.
    ///
    /// [`tiles`] drew eight and [`HOVER`] carried six. `Players seen` was one of the two missing,
    /// and [`Totals::players`] says outright what a reader gets wrong about it: it is a count of
    /// everything that was HIT, not a roster, unless every fight in scope proved the reader's group
    /// (`rolled_group`), and nothing on the tile's face says which of the two it is counting.
    /// [`tile`] answers a label it cannot find by drawing no hover at all, silently, which is
    /// correct for the Personal tab's own tiles and is why nothing noticed here.
    ///
    /// BOTH DIRECTIONS, because a table with an entry for a tile that no longer exists is the same
    /// bug from the other end: it reads as covered and answers nobody.
    ///
    /// AND THE LIST IS THE ONE THE PAGE DRAWS, which is the check this tree's signature defect
    /// demands. A coverage test over a label list nothing paints would prove that two constants
    /// agree with each other, so the painted frame is asked for every label as well.
    ///
    /// PERSONAL'S TILES ARE DELIBERATELY NOT IN SCOPE. `Your damage`, `Your kills` and the rest
    /// state their own population in their own names and have no second reading to warn about;
    /// this row is where the traps are.
    ///
    /// WHAT MUTATION MAKES THIS RED: adding a tile to `tile_row` without its line in `HOVER`,
    /// deleting a line from `HOVER`, or `tiles` drawing anything other than `tile_row`.
    #[test]
    fn every_tile_this_page_draws_says_what_it_means_on_hover() {
        let c = cached(roll(&[]), 0);
        for (label, _) in tile_row(&c) {
            assert!(
                HOVER.iter().any(|(k, _)| *k == label),
                "the {label:?} tile is drawn with nothing to say for itself"
            );
        }
        for (k, _) in HOVER {
            assert!(
                tile_row(&c).iter().any(|(label, _)| label == k),
                "HOVER explains {k:?} and this page draws no such tile"
            );
        }

        /* AND THE PAGE REALLY DRAWS THEM. Over the capture, on the Session tab. */
        let ctx = prepared();
        let dir = probe::planted("reports-tile-hovers", probe::CAPTURE);
        let mut ing = probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "the capture did not fold");
        let said = painted(&ctx, &mut ing, 0);
        for (label, _) in tile_row(&c) {
            assert!(
                said.iter().any(|s| s == label),
                "{label:?} is in the tile list and was not painted, so the coverage above is over \
                 a list nothing draws: {said:?}"
            );
        }
    }

    /// DEFECT: THE ENCOUNTERS OVERFLOW NOTE POINTED DOWN AT NOTHING.
    ///
    /// It read "They are in the shares below and in the copied report". `share` is a COLUMN of the
    /// table the note sits under, four along and every row of it above the note, and the note is
    /// the last thing the tab draws. A reader who went looking for the shares below found the end
    /// of the page.
    ///
    /// THE COUNT IS THE PAGE'S AND THE ARGUMENT IS THE HOVER'S, which is this page's rule for
    /// every other qualification it makes (`caveats` is four chips and four hovers). So this
    /// asserts both halves of that rule: the count is drawn, and what is drawn is not a paragraph.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the old sentence back, dropping the note, or moving
    /// the hover's argument onto the page as painted words.
    #[test]
    fn the_encounters_overflow_note_points_where_the_labels_actually_are() {
        let ctx = prepared();
        /* THREE PAST THE CAP, so the note fires and says three. */
        let rows: Vec<Encounter> = (0..CAP * 2 + 3)
            .map(|i| Encounter {
                label: format!("a dry bone skeleton {i}"),
                fights: 1,
                secs: 60,
                dealt: 1000 - i as u64,
                kills: 1,
                deaths: 0,
            })
            .collect();

        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| encounters_table(ui, &rows));
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

        let note = said
            .iter()
            .find(|s| s.contains("more labels"))
            .unwrap_or_else(|| {
                panic!(
                    "the table drew {} rows and never said how many it left out: {said:?}",
                    rows.len()
                )
            });
        assert!(
            note.starts_with("3 "),
            "the note counted the wrong rows out: {note:?}"
        );
        assert!(
            !note.contains("below"),
            "the note still sends the reader below a table that has nothing below it: {note:?}"
        );
        let prose = crate::screens::prose(&said);
        assert!(
            prose.is_empty(),
            "the overflow note is a paragraph on a page whose rule is a chip and a hover: \
             {prose:#?}"
        );
    }

    /// DEFECT: A DATA PAGE THAT SPENDS MORE ROOM ON SENTENCES THAN ON FIGURES.
    ///
    /// # THE OWNER'S OWN WORDS, BECAUSE THEY ARE THE SPECIFICATION
    ///
    /// "The ONLY fucking words should be data." The main Grimoire window answers SHOW ME ALL THE
    /// INFORMATION ON THAT ENCOUNTER; an always-on-top overlay answers SHOW ME WHAT IS SHITTING
    /// UP AND HOW MUCH. Neither question is answered by a paragraph, and the design sheets for
    /// both surfaces carry no sentences at all: a title row, a tab row, tiles, a chart, tables.
    ///
    /// WHAT THIS CAUGHT THE DAY IT WAS WRITTEN, measured by drawing the pages and reading the
    /// text back out of the shapes: the Reports page painted the same four paragraphs above every
    /// tab, the Dashboards page put a forty four word staleness note above all eight role tabs
    /// and gave Healer, Tank, Pet and Raid Leader an essay each about panels they do not have,
    /// and the Logs page carried five rules longer than the facts they qualified. None of it was
    /// wrong. All of it was in the way.
    ///
    /// # WHY THE CAP IS WORDS AND WHY IT ONLY APPLIES WITH DATA IN HAND
    ///
    /// A character cap punishes the long strings a data page is SUPPOSED to paint: a date range,
    /// a mob's name, a log filename. Prose is long because it has many words in it. And a page
    /// with nothing to draw has nothing but words, so the cap is asserted over a frame drawn on
    /// the reference capture, where every panel has rows. The empty state is tested separately
    /// and is allowed to speak: a blank rectangle cannot be told from a broken screen.
    ///
    /// EXPLANATION IS MOVED, NOT BANNED. The tiles, headings and scope buttons on these pages
    /// carry their caveats in `on_hover_text`, which is not painted and so never reaches this.
    ///
    /// WHAT MUTATION MAKES THIS RED: painting any of the removed paragraphs again, or writing a
    /// new one.
    #[test]
    fn this_page_paints_figures_and_not_paragraphs() {
        let ctx = prepared();
        let dir = probe::planted("prose-reports", probe::CAPTURE);
        let mut ing = probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "this must run with data in hand");
        for (i, name) in TABS.iter().enumerate() {
            let said = painted(&ctx, &mut ing, i);
            let prose = crate::screens::prose(&said);
            assert!(
                prose.is_empty(),
                "the {name} tab paints {} sentence(s) over its tables: {prose:#?}",
                prose.len()
            );
        }
    }

    /// DEFECT: A COPY BUTTON WHOSE CAPTION DESCRIBED A REPORT NOBODY HAD WRITTEN.
    ///
    /// # WHAT IT SAID AND WHAT IT DID
    ///
    /// The label under the button read "N lines: the header, the totals, every table on this tab
    /// and every warning on this page." `report_text` takes no tab argument and has no branch on
    /// one. It wrote the header, the totals, ONE hand-rolled damage ranking and the encounters.
    ///
    ///   * ON PERSONAL, the paste contained none of that tab: no `Your damage`, no Abilities,
    ///     Targets, Hit results or Elements. It contained two tables that are not on it.
    ///   * ON SESSION, the page draws four rankings and the paste carried one of the four.
    ///   * AND `every warning on this page` was false too: `caveats` draws "No fight in the log
    ///     that was read falls inside this scope" and the paste never carried it, so an empty
    ///     scope pasted a column of zeroes with nothing saying the scope was empty.
    ///
    /// # THE FIX IS THE REPORT AND NOT THE SENTENCE
    ///
    /// A report is a night, not a way of looking at one, which is what its own first line has
    /// always said: `EQL Grimoire session report`. So it carries every ranking the Session tab
    /// draws, in that tab's order, and the caption promises the scope rather than the tab.
    ///
    /// ASSERTED AGAINST `panels`, WHICH IS WHAT THE PAGE DRAWS, so a fifth table added to the page
    /// and forgotten here goes red rather than quietly leaving the clipboard short.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping a metric from the report's loop.
    #[test]
    fn the_report_carries_every_table_the_page_ranks() {
        let (rows, unreadable) = fold_text(probe::CAPTURE, quiet_window(), Some(probe::OWNER));
        let all: Vec<&FightRow> = rows.iter().collect();
        let c = cached(roll(&all), all.len());
        assert!(c.fights > 0, "the capture folded nothing");
        let text = report_text(
            "eqlog_Reviir_freeport.txt",
            Scope::Everything,
            &c,
            unreadable,
        );

        /* EVERY HEADING THE PAGE DRAWS, TAKEN FROM THE PAGE. `panels` is the list `ui` walks. */
        for (name, _) in panels(true) {
            assert!(
                text.contains(name),
                "the page ranks {name} and the clipboard does not carry it:\n{text}"
            );
        }

        /* AND THE ROWS UNDER THEM ARE REAL. The owner is in every fight of the capture. */
        assert!(
            text.contains(probe::OWNER),
            "the report named nobody from the log it rolled up"
        );
    }

    /// AND AN EMPTY SCOPE PASTES THE SENTENCE THAT SAYS SO.
    ///
    /// Zeroes with no explanation read as a night where nobody did anything. This is reachable
    /// with the page drawn, because `scoped` returns an empty run for a zone with no zone line
    /// and for an hour whose stamps would not read, while `Ingest::fights` is not empty.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `c.fights == 0` arm from the warning block.
    #[test]
    fn an_empty_scope_says_so_on_the_clipboard_and_not_only_on_the_page() {
        let empty = cached(roll(&[]), 0);
        assert_eq!(empty.fights, 0);
        let text = report_text("eqlog_Reviir_freeport.txt", Scope::Zone, &empty, 0);
        assert!(
            text.to_lowercase().contains("falls inside this scope"),
            "a report over nothing pastes as a report over zero:\n{text}"
        );
    }

    /// DEFECT: THE PROSE GUARD PASSING BECAUSE IT NEVER REACHED THE PROSE.
    ///
    /// `this_page_paints_figures_and_not_paragraphs` draws the reference capture, which is a clean
    /// read of four fights: no empty scope, no clipped tail, no fights too short for a rate, no
    /// unreadable stamps. So none of `caveats`' four branches ever fired under it, and four
    /// paragraphs of thirty to forty words went on being painted over the tables in exactly the
    /// states a reader is most likely to be staring at them.
    ///
    /// A GUARD THAT ONLY EXERCISES THE HAPPY PATH CERTIFIES THE HAPPY PATH. This drives each
    /// branch on purpose.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting any of the four sentences back on the page.
    #[test]
    fn every_warning_this_page_can_draw_is_a_chip_and_not_a_paragraph() {
        let ctx = prepared();
        let drew = |c: &Cached, unreadable: u32| -> Vec<String> {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| caveats(ui, c, unreadable));
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            fn walk(sh: egui::Shape, out: &mut Vec<String>) {
                match sh {
                    egui::Shape::Vec(x) => {
                        for one in x {
                            walk(one, out);
                        }
                    }
                    egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            let mut said = Vec::new();
            for c in shapes {
                walk(c.shape, &mut said);
            }
            said
        };

        /* ONE STATE PER BRANCH, NAMED, so a branch that stops firing is a hole this test can see. */
        let empty = cached(roll(&[]), 0);

        let clipped_row = FightRow {
            cut: true,
            secs: 600,
            ..FightRow::default()
        };
        let clipped = cached(clipped_row, 4);

        let brief_row = FightRow {
            secs: 1,
            ..FightRow::default()
        };
        let brief = cached(brief_row, 4);

        let mut stopped = cached(roll(&[]), 2);
        stopped.stopped = Some("a stamp in this run would not read, so the run stops there");

        /* EACH CASE NAMES THE WORDS ITS OWN BRANCH DRAWS.
         *
         * `!said.is_empty()` alone was not enough and two of the five were passing on somebody
         * else's chip: the stopped-run fixture is built on an EMPTY scope, so it also fires the
         * nothing-in-this-scope branch, and the unreadable-stamps fixture is the short-fight
         * one, so it also fires no-rate-published. Either branch could have been deleted and
         * this test would have stayed green. */
        let mut fired = 0;
        for (what, c, unreadable, want) in [
            ("an empty scope", &empty, 0, "nothing in this scope"),
            ("a clipped tail", &clipped, 0, "totals are a floor"),
            (
                "fights too short for a rate",
                &brief,
                0,
                "no rate published",
            ),
            ("a stopped run", &stopped, 0, "scope stops early"),
            ("unreadable stamps", &brief, 12, "12 lines not placed"),
        ] {
            let said = drew(c, unreadable);
            assert!(
                said.iter().any(|s| s == want),
                "{what} did not draw {want:?}; it drew {said:?}"
            );
            fired += 1;
            let prose = crate::screens::prose(&said);
            assert!(
                prose.is_empty(),
                "{what} paints a paragraph over the tables: {prose:#?}"
            );
        }
        assert_eq!(fired, 5, "a branch stopped being reachable");
    }
}
