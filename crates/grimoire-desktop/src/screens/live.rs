//! Screen: LIVE. The fight you are in, in the main window.
//!
//! # This page said it was impossible, and that stopped being true
//!
//! Until today `nav::unbuilt_why` carried this sentence about this very page:
//!
//! > The engine folds a whole text at once and hands back finished fights; there is no way to read
//! > the fight you are IN while it is still open, because the aggregator is consumed to produce
//! > them. A live meter needs that door, and it does not exist yet.
//!
//! Every clause of that was true when it was written and the last one is now false.
//! `Fights::finish` does still take the aggregator by value, so the door was never opened that way;
//! what opened instead is `Ingest::current_fight`, which re-folds the END of the log on every poll
//! that brought new lines and hands back the newest fight, still running. `fold_text` closes the
//! final fight with `Ended::EndOfLog` because the text ran out, which from the end of a file that is
//! still being written is exactly what an open fight looks like, and `Ingest::fight_is_live` is what
//! tells that apart from a fight that really ended.
//!
//! A PAGE THAT SAYS A THING IS IMPOSSIBLE IS A CLAIM WITH A DATE ON IT. This one outlived its
//! subject by a day and told the owner his app could not do something it had been doing since that
//! morning, which is the same class of defect as an invented number pointed the other way.
//!
//! # It is the overlay's widgets at page density
//!
//! Nothing here computes anything. The three panels are `Widget`s from `overlay.rs`, drawn by the
//! same renderer the always-on-top windows use, which is the whole point of that vocabulary: the
//! number on this page and the number on the overlay beside it cannot disagree, because there is one
//! implementation and one config type between them.
use crate::overlay::{Cols, Metric, Ranked, Widget};
use crate::screens::dashboards::{live_subject, LiveSubject};
use crate::screens::parser::{no_fights_words, why_no_fights, NoFights};
use crate::screens::{Ask, Cx};
use crate::theme::*;
use egui::{RichText, Ui};
use std::time::Duration;

/// The three panels, in the order the mockup's live window puts them.
///
/// A CONST AND NOT A FIELD, because this page has no state and nothing about it is per-session. The
/// overlay windows are where a person arranges panels; this is the page that always shows all three.
/// THE PANELS, FOR A TEST IN ANOTHER MODULE.
///
/// `dps::a_table_of_rates_always_says_somewhere_that_they_are_rates` asserts an invariant over
/// every ranked configuration THE APP SHIPS, which is the only version of that assertion worth
/// having: checking the two that happened to be wrong would not stop a third being written.
/// It therefore has to be able to reach this list from over there.
#[cfg(test)]
pub fn panels_for_test() -> [(&'static str, Widget); 3] {
    panels()
}

fn panels() -> [(&'static str, Widget); 3] {
    let table = |metric: Metric| {
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
            cap: 12,
            fit: false,
            /* NO HEADLINE ON A PAGE. The overlay's big number exists because that window is glanced
             * at from across a room with one number on it; here the reader is looking at the page
             * on purpose and the heading above the table already says which metric this is. */
            headline: false,
        })
    };
    [
        ("DAMAGE", table(Metric::Dealt)),
        ("HEALING", table(Metric::Healed)),
        ("DAMAGE TAKEN", table(Metric::Taken)),
    ]
}

#[derive(Default)]
pub struct LiveScreen {
    /// The cast-on-other index, and how many spells it was built from.
    ///
    /// CACHED BECAUSE IT IS BUILT FROM TWO THOUSAND SPELLS. Rebuilding it every frame would be
    /// the most expensive thing on the page, and it would be rebuilding an identical answer
    /// sixty times a second.
    ///
    /// # THE DOC USED TO SAY THE SNAPSHOT DOES NOT CHANGE WHILE THE APP RUNS, AND IT DOES
    ///
    /// `Settings` can be pointed at a different data folder, and `App` reloads. The index was
    /// built once and never again, so after that the `seen landing on others` rows went on
    /// naming spells out of the PREVIOUS corpus for the rest of the session, beside `on you`
    /// rows read from the new one. Two halves of one panel, two different spell books.
    ///
    /// THE COUNT IS THE KEY AND NOT A FLAG, because a flag has to be set by whoever swaps the
    /// corpus and this page cannot make them remember. A different corpus is a different
    /// length in every case that matters, and the check costs one comparison a frame.
    cast: Option<(usize, crate::castmsg::Cast)>,
}

impl LiveScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE PUMP, as `screens::parser::ui` does it. In the main window the App has usually
         * pumped already and this returns 0 at once; the repaint request is what makes the poll
         * happen while nobody is touching the app. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        let ig = &*cx.ingest;
        let Some(fight) = ig.current_fight() else {
            let why = why_no_fights(
                ig.scanning(),
                ig.log_dir().dir.is_some(),
                ig.active_log().is_some(),
            );
            /* THE LINE THOSE WORDS POINT AT, WHICH THIS PAGE DID NOT DRAW.
             *
             * `no_fights_words` was written for the Fights section of the parser screen, which
             * carries a status line above its table, and two of its four sentences send the
             * reader to it: "the line above says what is being read" and "The line above says
             * why". Live borrowed the sentences and not the line, so on a machine with logging
             * off the page said the reason was above it and there was nothing above it at all.
             *
             * REUSING THE WORDS IS STILL RIGHT AND THE FIX IS THE LINE. Four screens answer "why
             * is there no fight" out of that one function precisely so they cannot come to
             * disagree about it; rewriting Live's own copy of the sentence would have traded a
             * missing line for a fifth answer. See `reading_line` for what is drawn and why it
             * can always say something.
             *
             * AND THE CONTROL UNDER THEM IS THE OTHER HALF OF THE SAME COMPLAINT, one window
             * further out; see `empty_state`. */
            let line = reading_line(ig);
            empty_state(ui, &line, why, &mut cx.ask);
            return;
        };
        let live = ig.fight_is_live();
        let fight = fight.clone();

        /* THE HIT POINT READING FOR WHATEVER IS BEING FOUGHT, off every kill this character has
         * stored. Looked up here rather than inside `header` so that function stays pure in
         * (fight, live) and can be drawn by a test with no `Ingest` at all. */
        let taking = fight
            .current_target()
            .and_then(|(who, _)| ig.hp_of(who).cloned());
        header(ui, &fight, live, taking.as_ref());
        ui.add_space(10.0);

        /* HOISTED OUT OF `self` BEFORE THE CLOSURE. `cx` is borrowed mutably inside it, so a
         * `self.cast` reached from in there would be a second borrow of this screen. */
        let cast = &mut self.cast;

        egui::ScrollArea::vertical()
            .id_salt("live_panels")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                /* EACH PANEL IS A CARD, and until now they were three headings and three tables
                 * stacked on one flat ground with nothing between them.
                 *
                 * `theme::card` is the box from the design: a one pixel edge, a ten pixel corner
                 * and a shadow. Those three are what make a panel read as an object sitting on the
                 * page rather than as a slightly different patch of it, and the app had none of
                 * them anywhere. */
                for (title, w) in panels() {
                    crate::theme::card(ui, |ui| {
                        crate::theme::card_head(ui, title);
                        crate::screens::dps::draw_widget(
                            ui,
                            &fight,
                            crate::fights::Pulse::from_live(live),
                            &w,
                        );
                    });
                    ui.add_space(10.0);
                }

                crate::theme::card(ui, |ui| {
                    crate::theme::card_head(ui, "Effects");
                    match cx.data {
                        Some(d) => {
                            let built = cast.as_ref().map(|(n, _)| *n);
                            if built != Some(d.spells.len()) {
                                *cast =
                                    Some((d.spells.len(), crate::castmsg::Cast::new(&d.spells)));
                            }
                            /* THE CORPUS IS BORROWED AND NOT COPIED, and it used to be
                             * `d.spells.clone()` on EVERY FRAME, six lines under a doc explaining
                             * that the index above is cached because rebuilding it from two thousand
                             * spells would be the most expensive thing on the page. The clone was the
                             * more expensive thing, and it ran sixty times a second.
                             *
                             * IT WAS THERE FOR A BORROW AND NOT FOR A REASON. `effects` takes `cx`
                             * mutably, so `d`, read out of `cx.data`, could not be held across the
                             * call. But `Cx::data` is `Option<&'a Snapshot>`: the reference outlives
                             * the borrow of `cx` itself, so copying the REFERENCE out first is all
                             * that was needed. */
                            let spells: &[crate::data::Spell] = &d.spells;
                            if let Some((_, c)) = cast.as_ref() {
                                effects(ui, cx, c, spells);
                            }
                        }
                        None => {
                            /* THE LINE IS SHORT AND THE REASON IS ON IT. An effects panel with no
                             * corpus behind it has nothing to draw, so it owes a line; what it owed
                             * and did not need was the second sentence explaining the first.
                             *
                             * AND THE HOVER'S "Set the data folder in Settings" IS A CONTROL NOW,
                             * for the reason `empty_state` sets out at length: this page is drawn
                             * in `windows::ParserWindow` too, which has no rail, no gear and no
                             * Settings page, so naming Settings there was naming a place the
                             * reader could not get to. Unlike the fights empty state this one has
                             * no branch to gate on: the data folder is the ONLY thing that fills
                             * the spell corpus, so Settings is the next step every time this arm
                             * is taken. */
                            ui.label(RichText::new("No spell corpus loaded.").color(TEXT_3))
                                .on_hover_text(
                                    "Every effect on this panel is named from the spell corpus, \
                                     which is read from the data folder.",
                                );
                            ui.add_space(6.0);
                            if crate::chrome::ghost_btn(ui, TO_SETTINGS, false)
                                .on_hover_text(
                                    "Opens Settings in the main window, bringing it forward if \
                                     this is a pop-out. The data folder is set there and nowhere \
                                     else.",
                                )
                                .clicked()
                            {
                                cx.ask = Ask::OpenSettings;
                            }
                        }
                    }
                });
            });
    }
}

/// WHAT IS BEING READ, OR WHY NOTHING IS: the line the empty state's own words point at.
///
/// # IT ALWAYS SAYS SOMETHING, AND THAT IS THE WHOLE REQUIREMENT
///
/// A sentence that says "the line above says why" is false the moment the line above is missing,
/// so a version of this that could return `None` would have fixed the empty state on the cases
/// somebody happened to try and left it lying on the rest. Every arm below is a fact the ingest
/// already holds: its own problem sentence, the folder resolver's problem sentence, the file it
/// has open, the folder it found no file in, and finally the state before any of that is known,
/// which is a real state on the first frame after launch.
///
/// THE INGEST'S OWN SENTENCE FIRST, AND NOT WORDS OF THIS PAGE'S. `Ingest::active_problem` is
/// what the Logs page prints for the same condition (see `logs::file_state`), and it is the one
/// that names the folder and the `/log` command. A second wording here would be a second answer.
fn reading_line(ig: &crate::ingest::Ingest) -> String {
    if let Some(p) = ig.active_problem() {
        return p.to_owned();
    }
    if let Some(p) = ig.log_dir().problem.as_deref() {
        return p.to_owned();
    }
    if let Some(f) = ig.active_log() {
        return format!("reading {}", f.name());
    }
    match ig.log_dir().dir.as_deref() {
        Some(d) => format!("no log open in {}", d.display()),
        None => String::from("no Logs folder resolved yet"),
    }
}

/// THE WORDS ON THE CONTROL THAT OPENS SETTINGS, and the only string this page uses for it.
///
/// TWO EMPTY STATES ON THIS PAGE POINT AT SETTINGS and both draw this exact label, because they
/// are one door: a reader who has met it under the fights list should recognise it under the
/// effects panel without reading it again. It is also what the tests below look for, and a second
/// spelling would be a control one of them could not find.
pub(crate) const TO_SETTINGS: &str = "Open Settings";

/// THE EMPTY STATE: what is being read, why there is no fight, and where the fix is IF THE FIX IS
/// SETTINGS. Reports whether the reader pressed the control.
///
/// # THE WORDS DESCRIBED A TRIP THE READER COULD NOT MAKE FROM HERE
///
/// `no_fights_words`' `NoFolder` arm says the Logs folder "is set in the main window's Settings,
/// which opens from the gear at the foot of that window's rail", and that sentence is as long as
/// it is because of exactly this problem: this page is drawn in the main window AND in
/// `windows::ParserWindow`, which has no rail, no gear and no Settings page. Settings is not a
/// `nav::ScreenId` at all. So in the one window the owner keeps over the game while he plays, the
/// empty state named a destination and then named the route to it, and the route did not exist
/// from where he was standing.
///
/// A CONTROL IS WHAT THAT SENTENCE WANTED TO BE, and `Ask::OpenSettings` is what made it possible:
/// the pop-out raises it, `Windows::show` hands it to the root and brings the main window forward,
/// and the reader arrives in front of the editor. It sets NOTHING; see that variant's own note for
/// why a control that set the folder from a pop-out would open a worse defect than it closed.
///
/// # ONLY FOR `NoFolder`, AND THE OTHER THREE ARE NOT AN OVERSIGHT
///
/// A control is a claim that pressing it helps. `Reading` resolves itself in a moment, `NoLog` is
/// usually `/log on` inside the GAME, and `NoCombat` means the log was read and nothing in it
/// attacked anything. Settings fixes none of those, and a button offered under all four would be
/// this page sending a reader to a screen that has nothing for him three times out of four. The
/// button goes where the words already point, and only there.
///
/// PURE IN `(line, why)` AND HANDED THE ASK RATHER THAN THE WHOLE `Cx`, for the same reason
/// `header` is handed its `Reading`: it can then be drawn, and CLICKED, by a test with no
/// `Ingest`, and a `NoFolder` ingest is not something a test on this machine can safely build
/// (`resolve_log_dir` with no setting walks the usual places and can land on the owner's real
/// EverQuest install).
fn empty_state(ui: &mut Ui, line: &str, why: NoFights, ask: &mut Ask) {
    ui.label(RichText::new(line).color(TEXT_3));
    ui.label(RichText::new(no_fights_words(why)).color(TEXT_2));
    if why != NoFights::NoFolder {
        return;
    }
    ui.add_space(8.0);
    if crate::chrome::ghost_btn(ui, TO_SETTINGS, false)
        .on_hover_text(
            "Opens Settings in the main window, bringing it forward if this is a pop-out. The \
             Logs folder is set there and nowhere else.",
        )
        .clicked()
    {
        *ask = Ask::OpenSettings;
    }
}

/// THE MEASURED DENOMINATOR AND THE KILLS THAT PRODUCED IT, WHICH HAVE TO BE THE SAME GROUP.
///
/// # THE HOVER PAIRED A COUNT FROM ONE STATISTIC WITH A FIGURE FROM ANOTHER
///
/// It read the figure from `Reading::expect` and the count from `Reading::modal`, and those are
/// two different answers whenever the samples are SETTLED: `expect` returns the median of every
/// sample there, while `modal` returns the densest agreeing window, which can be a subset of them.
/// Six kills that agree inside ten percent of the median print `5 kills of 6 agreed on ~111`,
/// where the 111 was measured off all six and the 5 belongs to a group the reader is not being
/// shown. Two statistics wearing one sentence, on the one line of this page whose entire job is to
/// say how much a reader should trust the figure beside it.
///
/// SO THE BRANCH IS TAKEN ONCE AND BOTH HALVES FALL OUT OF IT.
///
/// # AND THE BRANCH LIVES ON THE READING NOW, NOT HERE
///
/// This function used to spell out `Reading::expect`'s branch a second time, because `expect`
/// handed back the figure without the count. That was the right fix for the hover and the wrong
/// home for the rule: two call sites recovered the count separately (this page and the
/// dashboard's live card) and BOTH were wrong in the same way before this audit, which is what a
/// rule with no single owner does.
///
/// `hp::Reading::expect_with_count` IS THAT OWNER. It takes the branch once and hands out both
/// halves, and `Reading::expect` is now a one line delegate to it, so the figure and the count
/// that qualifies it cannot come from different statistics no matter who asks. This function is
/// kept as a named delegate rather than inlined at the call site because its doc above is the
/// account of the defect and belongs with the page that shipped it.
fn measured(r: &crate::hp::Reading) -> Option<(u64, usize)> {
    r.expect_with_count()
}

/// HOW MANY PLAYERS HAVE A ROW ON THIS PAGE, which is not how many are in the fight.
///
/// # THE HEADER COUNTED A POPULATION THE PANELS UNDER IT DO NOT DRAW
///
/// It read `FightRow::players`, which is every fighter the name rule calls a person, over three
/// tables built by `dps::ranked_dealers`, which drops anybody whose metric is zero and then stops
/// at `Ranked::cap` rows. A cleric who spent a pull mezzing, a bard who neither swung nor was
/// swung at, anybody who only took a heal: each is a player in the fight and none of them has a
/// row anywhere on the page. The header said `4 players` over three rows, and the missing one is
/// not a row that scrolled off, it is a row that does not exist.
///
/// THIS IS THE SAME DEFECT `FightRow::group_sum` WAS WRITTEN FOR, one population lower. That fixed
/// a header total that counted mobs the tables never list; this fixes a header COUNT that counts
/// people the tables never list.
///
/// ASKED OF `panels()` AND OF THE SAME FUNCTION THE TABLES ASK, so it cannot drift from what is
/// drawn: a fourth panel, a different metric or a different cap moves both together. It counts the
/// UNION of the rows on screen, so somebody in two tables is one player.
fn players_with_a_row(f: &crate::fights::FightRow) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for (_, w) in panels() {
        let Widget::Ranked(r) = w else { continue };
        for x in crate::screens::dps::ranked_dealers(f, r.metric)
            .into_iter()
            .take(r.cap.max(1))
        {
            let name = x.who.text();
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
    }
    seen.len()
}

/// WHAT IS BEING FOUGHT AND WHETHER IT IS STILL GOING.
///
/// NO HEALTH BAR AND NO PERCENTAGE, deliberately. The mockup's live window carries
/// `63% - 312,441 / 500,000`; the numerator is real damage dealt and the denominator is a mob's
/// maximum hit points, which the log never states. The damage is printed on its own instead.
fn header(
    ui: &mut Ui,
    f: &crate::fights::FightRow,
    live: bool,
    taking: Option<&crate::hp::Reading>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        /* THE MARK, AND IT IS THE ONLY ANIMATION ON THE PAGE. Whether the fight is still running is
         * the one fact a reader checks before believing any number here. */
        let (word, tint) = if live {
            ("IN COMBAT", SETTLED)
        } else {
            ("LAST FIGHT", TEXT_3)
        };
        ui.label(RichText::new(word).color(tint).strong());

        /* WHAT IS IN FRONT OF YOU, AND IT USED TO BE THE BIGGEST THING IN THE LAST EIGHT MINUTES.
         *
         * # THE OWNER'S SCREEN READ `IN COMBAT  A SPITE GOLEM  08:06  15 players named`
         *
         * Every figure on that line was correct and none of it was about now. A fight is a run of
         * combat with no `QUIET_SECONDS` gap in it; on a raid night combat never goes quiet for
         * thirty seconds, so an eight minute chain of pulls is ONE fight. `headline` is the named
         * thing that took the most damage across the whole of it, and its own doc calls itself "a
         * label for a list, not a claim about the encounter". Over a live meter it can name
         * something that died six minutes ago.
         *
         * REFRESHING FASTER WOULD HAVE FIXED NOTHING, which is why the fix is here and not in the
         * poll: the value was never stale, it was the wrong measurement. `FightRow::current_target`
         * is the newest thing hit, which is the question this line was already implying.
         *
         * AND THE CLOCK BESIDE IT IS THAT TARGET'S OWN. A clock next to a name has to be the clock
         * FOR that name; the chain's own duration is still on the stat row below, where it is
         * labelled and cannot be read as this mob's age.
         *
         * THAT LAST CLAIM WAS WRITTEN AS THOUGH A NAME WERE A MOB, AND IT IS NOT. The engine folds
         * a fighter by name because the log gives it nothing better, so `current_target`'s clock is
         * the span from the first hit on the FIRST mob of that name to the newest hit on the one
         * standing there now, and the damage beside it is the pair of them added up. On a single
         * pull those are the same number and the sentence above holds; on a second pull of one
         * name they are not, and neither figure is about the mob the reader is looking at. What is
         * printed in that case is below and the argument is in `FightRow::deaths_of`.
         *
         * # THE BRANCH ITSELF IS NOT SPELLED HERE ANY MORE, AND THAT IS A SECOND DEFECT'S FIX
         *
         * This page's rule is also the Dashboards live tile's rule, and for one build the two
         * files each held their own copy of it and disagreed: the card drew `FightRow::headline`
         * in its big slot with the CHAIN's clock beside it, under the same `IN COMBAT` mark, off
         * the same `FightRow`. Both numbers were correct and the two windows named different mobs
         * as the thing being fought, side by side, on a live stream.
         *
         * SO THE RULE IS ASKED OF [`crate::screens::dashboards::live_subject`], which is this
         * page's own answer written down once. It lives over there because that is where the
         * disagreement was found and repaired; which file holds it matters far less than that
         * there is ONE function to disagree with. Nothing about what this page PAINTS moved with
         * it: the fonts, the tints, the hovers and the damage block below are all still this
         * page's, at this page's density (16.0 here against the card's 15.0).
         *
         * WHAT WOULD HAVE HAPPENED WITHOUT THIS. A copy of a branch order is not wrong on the day
         * it is written, it is wrong on the day one of the two is edited, and there is nothing on
         * either screen to say the other exists. That is exactly how the defect arrived. */
        match live_subject(f) {
            LiveSubject::Fighting { who, secs } => {
                ui.label(
                    RichText::new(who)
                        .font(crate::fonts::display(16.0))
                        .color(GOLD_HI),
                );

                /* HOW MANY OF THIS NAME ARE ALREADY DEAD, AND IT DECIDES EVERYTHING AFTER THE
                 * NAME. See `FightRow::deaths_of`: the fold keys a fighter by NAME, so the second
                 * of two mobs called the same thing shares the first one's row, and the moment one
                 * of them dies both the clock and the damage on this line stop being about the
                 * thing in front of the reader. The name itself is still right: it IS what is
                 * being fought.
                 *
                 * IT WAS `already_buried`, A PRIVATE HELPER OF THIS FILE, AND IT IS ON THE ROW NOW.
                 * The Dashboards live tile draws the same subject off the same row and needs the
                 * same test, and two screens holding two copies of one rule is precisely how those
                 * two came to disagree about a mob in the first place. The body did not change; it
                 * moved to `fights.rs` beside `current_target`, whose "one of them is up again"
                 * test reads the very same `Mark::Death` timeline. */
                let buried = f.deaths_of(who);
                let into = f
                    .fighters
                    .iter()
                    .find(|x| !f.player(&x.who) && x.who.text() == who)
                    .map(|x| x.taken)
                    .unwrap_or(0);

                if buried == 0 {
                    ui.label(RichText::new(clock(secs)).color(TEXT_2).monospace())
                        .on_hover_text(
                            "How long this target has been under fire, from the first time it \
                             was hit to the last. The whole run of combat is on the duration \
                             below.",
                        );
                } else {
                    /* WHAT IS LEFT THAT THE LOG ACTUALLY STATES. The clock is gone because it
                     * would run from the first hit on a mob that is already looted, through the
                     * gap, to now; the kill count in its place is read straight off the death
                     * marks and is the fact that explains the absence.
                     *
                     * AND NO PER-INSTANCE NUMBER CAN BE RECOVERED TO PUT BACK HERE, which is the
                     * argument a later reader is most likely to try to overturn. The fold keeps
                     * totals and two timestamps, not a damage timeline, so the damage into the
                     * mob currently standing there is not in this process's memory at all. The
                     * honest move is therefore not a better numerator, it is refusing the
                     * comparison and the clock and saying what IS known: how many have gone
                     * down. This paragraph travelled here from `already_buried`, which became
                     * `FightRow::deaths_of` when the Dashboards card needed the same rule; the
                     * ROW knows how to count the deaths, and only this page knows what to do
                     * with the answer. */
                    ui.label(
                        RichText::new(format!("{buried} killed"))
                            .color(TEXT_3)
                            .monospace(),
                    )
                    .on_hover_text(format!(
                        "This run has already killed {buried} of these. Nothing in the log tells \
                         two mobs of one name apart, so they share one row: there is no clock for \
                         the one standing there now, and no way to say how much of this damage \
                         went into it.",
                    ));
                }

                /* HOW MUCH THIS ONE HAS TAKEN, AND WHAT ITS KIND HAS BEEN SEEN TO SURVIVE.
                 *
                 * # THE MOCK ASKED FOR `TARGET HEALTH 30.7%` AND THE LOG CANNOT SAY IT
                 *
                 * EverQuest prints no mob health: the damage is real and the denominator does not
                 * exist. What DOES exist is every previous kill of the same mob, and a mob that
                 * was engaged whole and died absorbed exactly what it could take.
                 *
                 * SO THE DENOMINATOR IS MEASURED RATHER THAN INVENTED, and it is only offered when
                 * enough kills agree: `hp::Reading::expect` refuses on a first meeting, on two
                 * kills, and on a mob whose kills disagree. On the owner's own night the princess
                 * settles at 20,016 from twelve of twenty-six kills.
                 *
                 * THE SAMPLE COUNT IS PRINTED WITH IT AND THAT IS NOT DECORATION. `20,016` alone
                 * is a claim about a mob; `of ~20,016 from 12 kills` is a claim about twelve
                 * measurements, which is what this actually is. A reader can tell how much to
                 * trust the bar without being told.
                 *
                 * AND THE DAMAGE STANDS ALONE WHEN THERE IS NO READING. What went in is always
                 * true; the comparison is the part that needs earning.
                 *
                 * # AND `buried > 0` IS THE OTHER WAY IT IS NOT EARNED
                 *
                 * The reading is one mob's worth by construction and `into` is the whole name's,
                 * so once one of them has died the two are not the same measurement and dividing
                 * one by the other is this page inventing a number. `FightRow::deaths_of`
                 * carries the argument; what the reader gets instead is the total, said plainly
                 * as a total. */
                let counted = if buried == 0 {
                    taking.and_then(|r| measured(r).map(|m| (m, r)))
                } else {
                    None
                };
                if into > 0 {
                    match counted {
                        Some(((expect, of), r)) => {
                            ui.label(
                                RichText::new(format!(
                                    "{} of ~{}",
                                    thousands(into),
                                    thousands(expect)
                                ))
                                .color(TEXT_2)
                                .monospace(),
                            )
                            .on_hover_text(format!(
                                "Damage into this target, against what this kind of mob has been \
                                 measured to absorb: {} kills of {} agreed on ~{}. The log never \
                                 states a mob's health; this is read from kills that finished.",
                                of,
                                r.n(),
                                thousands(expect)
                            ));
                        }
                        None if buried > 0 => {
                            ui.label(
                                RichText::new(thousands(into)).color(TEXT_3).monospace(),
                            )
                            /* NO CLAIM ABOUT A READING EITHER WAY. This arm is taken whether or
                             * not the book has a figure for this mob, so the sentence says why a
                             * figure could not be USED rather than whether one exists: a measured
                             * health is one mob's worth by construction (`hp::read` throws away
                             * any fight where the name died twice) and this numerator is not. */
                            .on_hover_text(format!(
                                "Damage into every {who} in this run, {} of them, because the log \
                                 gives two mobs of one name one row. No health figure beside it: \
                                 a measured one is what ONE of them absorbs, and this counts more \
                                 than one.",
                                buried + 1
                            ));
                        }
                        None => {
                            ui.label(
                                RichText::new(thousands(into)).color(TEXT_3).monospace(),
                            )
                            .on_hover_text(
                                "Damage into this target. No health figure: the log never states \
                                 one, and this mob has not been killed enough times for its \
                                 previous kills to agree on what it absorbs.",
                            );
                        }
                    }
                }
            }
            /* NOTHING ALIVE IS BEING FOUGHT. Two different real states, and they must not look
             * the same.
             *
             * BETWEEN PULLS is the common one: a fight stays open for `QUIET_SECONDS` after the
             * last blow, so for half a minute after a kill there is a live fight with nothing
             * alive in it. On the owner's own log that gap was sixty-nine seconds of looting
             * and chat. Falling straight through to `headline` printed the mob that had just
             * died, in the same gold, with nothing to say it was a corpse.
             *
             * SO THE KILL IS NAMED AS A KILL. `slain` in front of it, and the quieter tint the
             * page uses for a finished thing, because the reader's question here is not "what
             * am I fighting" but "did it go down". */
            LiveSubject::Slain { who } => {
                ui.label(RichText::new("slain").color(TEXT_3));
                ui.label(
                    RichText::new(who)
                        .font(crate::fonts::display(16.0))
                        .color(TEXT_2),
                )
                .on_hover_text(
                    "The last thing this run killed. Nothing is being fought right now; the \
                     fight stays open until combat has been quiet for a while.",
                );
            }
            /* AND NOTHING NAMED HAS BEEN HIT OR KILLED AT ALL: the reader can be standing in a
             * fight that is entirely somebody else's. The fight's own label is the honest
             * fallback, and it is named as a label. `live_subject` is what falls back to
             * `headline`, and to `dashboards::UNNAMED` when even that is missing, which is a real
             * state (a run that is nothing but the reader hurting himself) rather than an error. */
            LiveSubject::Label { text } => {
                ui.label(
                    RichText::new(text)
                        .font(crate::fonts::display(16.0))
                        .color(GOLD_HI),
                )
                .on_hover_text(
                    "Nothing named has been hit in this run, so this is the fight's own \
                     label: the biggest thing in it.",
                );
            }
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        /* THE WHOLE RUN, AND THE LABEL HAS TO SAY SO. Beside a mob's name this read as that
         * mob's age; it is every second of combat since the last thirty second lull. */
        stat(ui, "in combat for", &clock(f.secs));
        /* THREE OF THESE FOUR USED TO BE THE WHOLE FIGHT'S, MOBS INCLUDED, above three tables
         * that rank players only. See `FightRow::group_sum` for the measurement: the header
         * read 16,526 over a panel listing 12,976, with no row for the difference and a share
         * column adding to a hundred percent of the smaller figure.
         *
         * `deaths` WAS THE WORST OF THEM because it was not merely large, it was a different
         * quantity wearing the right word: `FightRow::deaths` counts every death in the fight,
         * so on a clean camp where nobody went down it printed the reader's KILL COUNT under
         * the word `deaths` on a combat meter. 33 against a real 1 on this capture.
         *
         * AND `took part` COUNTED EVERYTHING THAT WAS HIT: 22 over a four row table. */
        stat(ui, "group damage", &thousands(f.group_sum(|x| x.dealt)));
        stat(
            ui,
            "player deaths",
            &f.group_count(|x| x.deaths).to_string(),
        );
        /* AND `players` WAS A COUNT OF A POPULATION THE TABLES DO NOT DRAW, which is the same
         * defect as the three above it with a count instead of an amount: `FightRow::players` is
         * everybody the name rule calls a person, and a person who dealt, healed and took nothing
         * has no row in any of the three panels. See `players_with_a_row`. */
        stat(ui, "players", &players_with_a_row(f).to_string()).on_hover_text(
            "Players with a row in one of the three tables below. Somebody who dealt, healed and \
             took nothing in this fight is still in it and is not counted here, because there is \
             no row anywhere on the page to account for them.",
        );
        /* WHOSE ROWS THE THREE TABLES BELOW ARE, and so whose `group damage`, `player deaths` and
         * `players` beside it are. A roster the group filter narrowed and one it did not look the
         * same; this page's tables draw no heading of their own (`headline: false`), so this line
         * is the one place it can say which. The capture's last fight is the case: solo, and the
         * damage table empty while `Losumyda` dealt damage beside the reader. */
        ui.label(RichText::new(crate::screens::dps::whose(f)).color(TEXT_3));
        if let Some(z) = f.zone.as_deref() {
            stat(ui, "zone", z);
        }
    });
    if f.cut {
        /* A CHIP, LIKE EVERY OTHER WARNING IN THE APP. The page's own prose guard cannot see
         * this one: it draws the reference capture, which is never clipped, so `f.cut` is
         * false and the string never reaches the shapes it reads. */
        ui.label(RichText::new("opening not read: totals are a floor").color(WRONG));
    }
}

/// One figure and the word for it.
///
/// THE RESPONSE COMES BACK SO A STAT CAN CARRY ITS OWN CAVEAT. `players` counts the rows the
/// panels draw rather than the people in the fight, and a stat row that states a rule like that
/// and cannot say so is the shape a false number takes; the words for it go in `on_hover_text`,
/// which is not painted and so does not put a paragraph over a data page. The callers that have
/// nothing to add ignore the response.
fn stat(ui: &mut Ui, label: &str, value: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(value).color(TEXT).monospace().strong());
        ui.label(RichText::new(label).color(TEXT_3));
    })
    .response
}

/// `266` becomes `04:26`.
fn clock(secs: i64) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
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

/* ----------------------------------------------------------------- effects -- */

/// How far back an effects panel looks.
///
/// A FEW HUNDRED LINES AND NOT THE WHOLE WINDOW. The live buffer holds twenty thousand; walking all
/// of them once a frame would cost more than the fold that produced them, and an effect announced
/// that far back has either worn off or stopped being news. Four hundred is roughly the busiest
/// half minute in the reference capture.
const EFFECT_LINES: usize = 400;

/// WHAT IS ON WHOM, DRAWN FROM THE SENTENCES THE GAME PRINTED.
///
/// This is the panel `castmsg` exists for. The log never names a spell when it lands; it prints
/// flavour text, and the wiki's message strings are the only way back from that sentence to a spell
/// and a target. See that module for the whole argument.
///
/// TWO LISTS BECAUSE THE LOG GIVES TWO DIFFERENT AMOUNTS OF INFORMATION. What is on the READER can
/// be opened and closed, because his own landings and fades both have sentences. What is on anybody
/// ELSE can only be opened: there is no wears-off message for a third party anywhere in the corpus,
/// so those are shown as "seen landing" and never as "currently up". Saying that plainly is the
/// difference between a panel and a lie.
/// HOW MANY OTHER SPELLS SHARE THIS SENTENCE, in the singular when it is one.
///
/// Two spells with the same message printed "or 1 others", which is the page counting in the
/// plural at the one count where English does not, on a panel whose whole job is to be honest
/// about what the log does and does not distinguish.
fn others(candidates: usize) -> String {
    let n = candidates.saturating_sub(1);
    if n == 1 {
        String::from("or 1 other")
    } else {
        format!("or {n} others")
    }
}

fn effects(ui: &mut Ui, cx: &mut Cx, cast: &crate::castmsg::Cast, spells: &[crate::data::Spell]) {
    use grimoire_parse::combat::{parse, Reading};

    let mut book = crate::castmsg::Book::default();
    for line in cx.ingest.recent_tail(EFFECT_LINES) {
        let Some(entry) = parse(line) else { continue };
        let Reading::Flavour(f) = entry.reading else {
            continue;
        };
        let Some(body) = line.split_once("] ").map(|(_, b)| b.trim()) else {
            continue;
        };
        book.read(cast, body, f, spells);
    }

    let mine = book.mine();
    let theirs = book.theirs();
    if mine.is_empty() && theirs.is_empty() {
        /* SAME BLIND SPOT FROM THE OTHER SIDE: the guard's `Cx` has `data: None`, so every run
         * of it takes the no-corpus arm and this line is never in the shapes either. */
        ui.label(RichText::new("Nothing landed recently.").color(TEXT_3));
        return;
    }

    if !mine.is_empty() {
        ui.label(RichText::new("on you").color(TEXT_2));
        for e in mine {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(name_of(&e.candidates)).color(GOLD_HI));
                if e.candidates.len() > 1 {
                    ui.label(RichText::new(others(e.candidates.len())).color(TEXT_3));
                }
            });
        }
        ui.add_space(6.0);
    }

    if !theirs.is_empty() {
        /* THE WORDING IS LOAD BEARING. These are landings SEEN, not effects known to be up: nothing
         * in the log closes one for anybody but the reader. */
        ui.label(RichText::new("seen landing on others").color(TEXT_2));
        for e in theirs.iter().rev().take(8) {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(&e.who).color(TEXT).strong());
                ui.label(RichText::new(name_of(&e.candidates)).color(GOLD));
                if e.candidates.len() > 1 {
                    ui.label(RichText::new(others(e.candidates.len())).color(TEXT_3));
                }
            });
        }
    }
}

/// The first candidate, which is the corpus order and not a judgement.
///
/// THE COUNT IS SHOWN BESIDE IT WHENEVER THERE IS MORE THAN ONE, so a shared sentence never reads as
/// certainty. Four charm spells say `Someone blinks.`; naming one of them silently would be picking.
fn name_of(candidates: &[String]) -> &str {
    candidates.first().map(String::as_str).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEFECT: this page drawing a metric the overlay does not, or computing one of its own.
    ///
    /// The whole design rests on the page and the overlay sharing one renderer and one config type.
    /// A panel built here with its own arithmetic is the exact thing that lets the two disagree
    /// about a number in front of an audience.
    ///
    /// WHAT MUTATION MAKES THIS RED: any panel here that is not a `Widget`.
    #[test]
    fn every_panel_is_a_widget_from_the_shared_vocabulary() {
        let p = panels();
        assert_eq!(p.len(), 3);
        for (title, w) in p {
            assert!(!title.is_empty());
            match w {
                Widget::Ranked(r) => {
                    assert!(r.rate, "a live page shows rates");
                    assert!(!r.headline, "the page has its own heading");
                }
                other => panic!("{title} is not a ranked table: {other:?}"),
            }
        }
    }

    /// DEFECT: two panels showing the same metric, which would look like a rendering bug.
    #[test]
    fn the_three_panels_are_three_different_metrics() {
        let mut seen = Vec::new();
        for (_, w) in panels() {
            if let Widget::Ranked(r) = w {
                assert!(!seen.contains(&r.metric), "{:?} twice", r.metric);
                seen.push(r.metric);
            }
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn the_numbers_read_the_way_a_person_writes_them() {
        assert_eq!(clock(266), "04:26");
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(-5), "00:00");
        assert_eq!(thousands(16_526), "16,526");
        assert_eq!(thousands(0), "0");
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
    fn painted(ctx: &egui::Context, ing: &mut crate::ingest::Ingest) -> Vec<String> {
        drawn(ctx, ing, Vec::new())
            .0
            .into_iter()
            .map(|(w, _)| w)
            .collect()
    }

    /// Where a painted string is, so a test can press it. `None` when nothing painted it.
    ///
    /// THE TEXT'S RECTANGLE AND NOT THE CONTROL'S, which is the same thing for this purpose: a
    /// button's label is drawn inside the button, so the centre of the galley is inside the
    /// button's sense rect. `screens::videos`' click test works the same way and for the same
    /// reason: nothing in a `FullOutput` carries a widget's interaction rect.
    fn spot(runs: &[(String, egui::Rect)], s: &str) -> Option<egui::Pos2> {
        runs.iter().find(|(w, _)| w == s).map(|(_, r)| r.center())
    }

    /// A LEFT CLICK AT `at`, as the three events egui needs to see one.
    fn press(at: egui::Pos2) -> Vec<egui::Event> {
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
        ]
    }

    /// One whole frame of this page, with what it painted, where, and what it asked the App for.
    ///
    /// `Cx::data` IS `None` HERE AND THAT IS DELIBERATE, not a shortcut: it is the state the
    /// effects panel's no-corpus arm exists for, and the arm this file's own comments say the
    /// prose guard has always taken. Building a `Snapshot` would take that arm away from every
    /// test in this module that depends on it.
    fn drawn(
        ctx: &egui::Context,
        ing: &mut crate::ingest::Ingest,
        events: Vec<egui::Event>,
    ) -> (Vec<(String, egui::Rect)>, Ask) {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut screen = LiveScreen::default();
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
            events,
            ..Default::default()
        };
        let runs = runs_in(ctx.run_ui(input, |ui| screen.ui(ui, &mut cx)));
        (runs, cx.ask)
    }

    /// Every string one painted frame put on screen, whatever drew it.
    fn text_in(out: egui::FullOutput) -> Vec<String> {
        runs_in(out).into_iter().map(|(w, _)| w).collect()
    }

    /// The same, with each string's rectangle, so a control can be found and pressed.
    ///
    /// SHARED BY EVERY FRAME-READING HELPER HERE AND NOT COPIED INTO ANY OF THEM, because the one
    /// thing a reader of this has to get right is the nesting below, and two copies of that is two
    /// chances to get it wrong in the direction that passes.
    fn runs_in(mut out: egui::FullOutput) -> Vec<(String, egui::Rect)> {
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
                egui::Shape::Text(t) => said.push((
                    t.galley.text().to_owned(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => {}
            }
        }
        said
    }

    /// One frame of the empty state on its own, which is what that function is pure for.
    ///
    /// A `NoFolder` INGEST IS NOT SOMETHING A TEST ON THIS MACHINE MAY BUILD, which is the whole
    /// reason `empty_state` takes a `NoFights` and an `&mut Ask` rather than the `Cx`.
    /// `ingest::resolve_log_dir` with no setting walks the usual places and takes the first that
    /// exists, so an `Ingest` built with `log_dir: None` on the owner's own machine would resolve
    /// his real EverQuest folder and read his real log.
    fn empty_says(
        ctx: &egui::Context,
        why: NoFights,
        events: Vec<egui::Event>,
    ) -> (Vec<(String, egui::Rect)>, Ask) {
        let mut ask = Ask::None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 400.0),
            )),
            events,
            ..Default::default()
        };
        let runs = runs_in(ctx.run_ui(input, |ui| {
            empty_state(ui, "reading eqlog_Reviir_freeport.txt", why, &mut ask)
        }));
        (runs, ask)
    }

    /// One frame of `header` on its own, which is what that function is pure for.
    ///
    /// ITS DOC SAYS THE HIT POINT READING IS LOOKED UP BY THE CALLER SO IT "CAN BE DRAWN BY A TEST
    /// WITH NO `Ingest` AT ALL", and until now nothing took it up on that: every header rule was
    /// checked by driving `FightRow` methods and trusting that what the header did with the answer
    /// was right. The two invented-number defects on this page both lived in what the header did
    /// with the answer, not in the answer, so they were invisible from there.
    fn header_says(
        f: &crate::fights::FightRow,
        live: bool,
        hp: Option<&crate::hp::Reading>,
    ) -> Vec<String> {
        let ctx = prepared();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 400.0),
            )),
            ..Default::default()
        };
        text_in(ctx.run_ui(input, |ui| header(ui, f, live, hp)))
    }

    /// A hit point reading built straight from samples, as `hp::read` would have folded it.
    fn reading(samples: &[u64]) -> crate::hp::Reading {
        crate::hp::Reading {
            samples: samples.to_vec(),
            shown: String::from("a thunder spirit princess"),
            ..Default::default()
        }
    }

    /// DEFECT: THE LIVE PAGE HAVING NO TEST THAT EVER DREW IT.
    ///
    /// The tests above check the widget CONFIGS this page is built from, which is worth
    /// checking and is not the same claim. A page whose panels are configured correctly and
    /// whose `ui` is never reached is exactly as blank as one with no panels at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping a panel from `panels`, an early `return` in `ui`,
    /// or `draw_widget` refusing the config this page hands it.
    #[test]
    fn the_page_draws_the_capture_and_names_its_own_panels() {
        let ctx = prepared();
        let dir = crate::fights::probe::planted("screen-live", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        assert_eq!(
            ing.fights().len(),
            12,
            "the ingest found no fights, so this would be testing the empty state by accident. \
             Problem: {:?}",
            ing.active_problem()
        );

        let said = painted(&ctx, &mut ing);
        assert!(!said.is_empty(), "the Live page painted nothing at all");
        let tight = said.concat();

        /* EVERY PANEL'S OWN HEADING, TAKEN FROM THE VOCABULARY AND NOT RETYPED HERE. If a
         * panel stops being drawn its heading stops appearing, and no other panel's heading
         * can stand in for it: `the_three_panels_are_three_different_metrics` has already
         * established that the three are distinct. */
        for (heading, _) in panels() {
            assert!(
                tight.contains(heading),
                "the {heading} panel is configured but never reached the screen: {said:?}"
            );
        }

        /* AND THE ROWS UNDER THEM ARE THE ROSTER OF THE FIGHT THIS PAGE IS ON, ASKED OF THAT ROW.
         *
         * NOT THE CAPTURE OWNER, AND THE FIRST DRAFT OF THIS TEST GOT THAT WRONG. It asserted the
         * owner appears, which is true of the SESSION and false of the fight: Live is the newest
         * fight and nothing else, and the capture ends on a thirty six second pull in Nektulos
         * that she is not in. The name is taken from the same row the page reads, so the test
         * cannot drift from the page by naming somebody out of a different fight.
         *
         * AND THE PLAYER IN THAT PULL IS NOT ON HER ROSTER ANY MORE, which is the group filter
         * doing its job on real bytes. `You have been removed from the group.` at 23:21:54 proved
         * her solo, so `FightRow::group` is `Some(vec![])`, and `Losumyda` (whose only other lines
         * are NewPlayers chat) is a stranger fighting within range. This asserted that the page
         * named him, and went red the day the tables started asking `FightRow::ours`. So the
         * fight's own answer is asserted first, and then the page is held to it: the stranger is
         * nowhere on it, and the damage table says in its own words that nobody on the roster
         * dealt any. A page that ignored the group would name him and fail here. */
        let f = ing.current_fight().expect("the capture ends on a fight");
        assert_eq!(
            f.group,
            Some(Vec::new()),
            "the capture's last fight comes after the removal line, so the log proved the reader \
             solo in it and this test is no longer looking at the fight it was written for"
        );
        let stranger = f
            .fighters
            .iter()
            .find(|x| x.who.player() && x.dealt > 0)
            .map(|x| x.who.text().to_owned())
            .expect("the newest fight has a player who dealt damage");
        assert!(
            !f.ours(&crate::fights::Who::Named(stranger.clone())),
            "{stranger} is on a solo reader's roster"
        );
        assert!(
            !tight.contains(&stranger),
            "the page named {stranger}, a player the log proved was not in the reader's group, \
             so a table on it ignored `FightRow::group`: {said:?}"
        );
        /* AND IT SAYS SO IN THE ROSTER'S WORDS, NOT THE LOG'S. `Nobody has dealt any damage.` over
         * this fight is a sentence about the log, and the log has Losumyda dealing damage in it;
         * the reader, solo, is the one who has not. And the stat row says the roster is solo,
         * because this page's tables draw no heading and nothing else here tells a filtered table
         * from an unfiltered one. WHAT MUTATION MAKES THIS RED: the tables printing the old
         * `Nobody has` sentence; the stat row not drawing `dps::whose`. */
        let solo = format!("You have not {}.", crate::overlay::Metric::Dealt.nothing());
        assert!(
            said.contains(&solo),
            "the damage table did not say {solo:?} over a solo roster that dealt nothing: \
             {said:?}"
        );
        assert!(
            !said.iter().any(|s| s.starts_with("Nobody has")),
            "the page said nobody had done something over a fight the log names a player doing it \
             in: {said:?}"
        );
        assert!(
            said.iter().any(|s| s == "solo"),
            "the Live page does not say its roster is solo, so its empty tables read as a fight \
             nobody fought: {said:?}"
        );
    }

    /// AND WITH NOTHING TO READ IT SAYS SO RATHER THAN DRAWING THREE EMPTY TABLES.
    #[test]
    fn a_folder_with_no_log_in_it_still_gets_a_whole_page() {
        let ctx = prepared();
        let empty = crate::fights::probe::logs_dir("screen-live-empty");
        let mut none = crate::fights::probe::booted(&empty);
        assert!(none.fights().is_empty(), "there is no log in that folder");
        let said = painted(&ctx, &mut none);
        assert!(
            !said.is_empty(),
            "a Live page with no log to read painted nothing, so a reader is told nothing"
        );
        assert!(
            !said.concat().contains(crate::fights::probe::OWNER),
            "the page named a player it could not have read: {said:?}"
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
        let dir = crate::fights::probe::planted("prose-live", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "this must run with data in hand");
        let prose = crate::screens::prose(&painted(&ctx, &mut ing));
        assert!(
            prose.is_empty(),
            "the Live page paints {} sentence(s) over its panels: {prose:#?}",
            prose.len()
        );
    }

    /// DEFECT: `IN COMBAT  A SPITE GOLEM  08:06  15 players named`.
    ///
    /// # EVERY FIGURE ON THAT LINE WAS CORRECT AND NONE OF IT WAS ABOUT NOW
    ///
    /// A fight is a run of combat with no `QUIET_SECONDS` gap in it. On a raid night combat never
    /// goes quiet for thirty seconds, so an eight minute chain of pulls is ONE fight: the golem
    /// was the biggest damage sponge of the last eight minutes and may have died six minutes
    /// earlier, the clock was the chain's, and the player count was everyone seen in it.
    ///
    /// THE OWNER READ IT AS A REFRESH PROBLEM -- "update the current mob more often" -- and it was
    /// not one. The value was never stale; it was the wrong measurement, and polling faster would
    /// have redrawn the same wrong name. That is the whole reason this test exists: the defect is
    /// invisible to anything that checks the header is PRESENT and updating.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `f.headline` back in the big slot.
    #[test]
    fn the_header_names_what_is_being_fought_now_and_not_the_biggest_thing_in_the_chain() {
        use crate::fights::{FightRow, Fighter, Who};

        let mob = |name: &str, taken: u64, first: u32, last: u32| Fighter {
            who: Who::Named(name.into()),
            taken,
            first_taken_at: Some(first),
            last_taken_at: Some(last),
            ..Fighter::default()
        };

        /* THE OWNER'S OWN SHAPE, AND IT HAS TO DISCRIMINATE MORE THAN ONE WRONG RULE.
         *
         * The first draft of this fixture had the current target hit both LAST and most recently
         * FIRST, so ranking by `first_taken_at` gave the same answer and a mutation to that
         * effect stayed green. A fixture that cannot fail for the near-miss is not evidence.
         *
         * SO: the golem is the biggest and died early; the spawn is what is still being worked;
         * and an add arrived AFTER the spawn did and stopped before it. Newest-first-hit says the
         * add, biggest says the golem, and only newest-LAST-hit says the spawn. */
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
                    /* THE READER TOOK A HIT MORE RECENTLY THAN EITHER MOB, which is ordinary on a
                     * raid and must not make HIM the thing being fought. */
                    taken: 900,
                    first_taken_at: Some(10),
                    last_taken_at: Some(485),
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };

        let (who, secs) = f.current_target().expect("something was hit");
        assert_eq!(
            who, "a spite spawn",
            "the header named the biggest thing in the chain rather than the newest"
        );
        assert_eq!(
            secs, 380,
            "the clock beside the name is the chain's and not this target's"
        );
        assert_ne!(
            Some(who),
            f.headline.as_deref(),
            "this fixture no longer tells the two answers apart"
        );

        /* AND THE FIGHT'S OWN DURATION IS STILL THERE, because it is a real number and the stat
         * row is where it can be labelled. */
        assert_eq!(f.secs, 486);
    }

    /// DEFECT: `34,102 of ~20,016`, which is more damage into a mob than the mob can absorb.
    ///
    /// # THE NUMERATOR WAS PER NAME AND THE DENOMINATOR WAS PER MOB
    ///
    /// The engine folds a fighter by NAME because nothing in the log tells two mobs of one name
    /// apart, so pulling a second `a thunder spirit princess` inside one chain adds its damage to
    /// the dead one's row. `hp::read` is careful in the other direction: a fight where the name
    /// died twice is thrown away entirely, so a reading is one mob's worth. Divide the first by the
    /// second and the page states a comparison neither number supports, and it does it in gold on a
    /// live stream.
    ///
    /// THE CLOCK WENT THE SAME WAY and is the half that would have survived a narrower fix.
    /// `current_target` answers `last_taken_at - first_taken_at` over the folded row, which on a
    /// second pull runs from the first hit on a mob that is already looted, across the gap, to now.
    ///
    /// SO BOTH ARE REFUSED AND THE KILL COUNT IS PRINTED INSTEAD, because that is the part the log
    /// really states. There is no better numerator available: the fold keeps totals and two
    /// timestamps, not a damage timeline.
    ///
    /// WHAT MUTATION MAKES THIS RED: drawing the comparison or the target clock off a row that has
    /// already died in this run, which is `FightRow::deaths_of` answering 0, or the header
    /// ignoring it.
    #[test]
    fn a_name_pulled_twice_is_not_measured_against_one_mob_s_health() {
        use crate::fights::{FightRow, Fighter, Mark, Moment, Who};

        /* ONE NAME, TWO MOBS, ONE ROW. The first died at 140 seconds and the second has been
         * under fire since; 34,102 is what the pair of them absorbed between them. */
        let f = FightRow {
            secs: 486,
            fighters: vec![
                Fighter {
                    who: Who::Named("a thunder spirit princess".into()),
                    taken: 34_102,
                    first_taken_at: Some(0),
                    last_taken_at: Some(300),
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::You,
                    dealt: 34_102,
                    ..Fighter::default()
                },
            ],
            moments: vec![Moment {
                at: 140,
                what: Mark::Death {
                    killer: 1,
                    victim: 0,
                },
            }],
            ..FightRow::default()
        };
        let r = reading(&[20_011, 20_014, 20_016, 20_019, 20_025]);
        assert_eq!(
            r.expect(),
            Some(20_016),
            "the fixture's kills no longer settle on a figure, so this would pass with nothing to \
             print rather than with the fix"
        );
        assert_eq!(
            f.current_target().map(|(w, _)| w),
            Some("a thunder spirit princess"),
            "the fixture's mob is not what the header draws, so this tests nothing"
        );
        assert_eq!(f.deaths_of("a thunder spirit princess"), 1);

        let said = header_says(&f, true, Some(&r)).concat();
        assert!(
            said.contains("34,102"),
            "the damage the log DID state is gone from the line: {said}"
        );
        assert!(
            !said.contains("20,016"),
            "the header set two mobs' worth of damage against one mob's measured health: {said}"
        );
        assert!(
            !said.contains("05:00"),
            "the clock beside the name spans both mobs and the looting between them: {said}"
        );
        assert!(
            said.contains("1 killed"),
            "nothing on the line tells the reader the row holds more than one mob: {said}"
        );
    }

    /// AND THE COMPARISON IS NOT SIMPLY GONE: one mob of a name still earns it.
    ///
    /// THIS IS THE GUARD AGAINST THE CHEAP FIX for the test above, which is to stop printing the
    /// measured denominator at all. That would make the invented number go away by deleting the
    /// one figure on this page that is measured rather than assumed.
    ///
    /// WHAT MUTATION MAKES THIS RED: refusing the comparison when nothing of that name has died.
    #[test]
    fn one_mob_of_a_name_still_gets_its_measured_denominator() {
        use crate::fights::{FightRow, Fighter, Who};

        let f = FightRow {
            secs: 486,
            fighters: vec![Fighter {
                who: Who::Named("a thunder spirit princess".into()),
                taken: 14_086,
                first_taken_at: Some(0),
                last_taken_at: Some(300),
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        let r = reading(&[20_011, 20_014, 20_016, 20_019, 20_025]);
        let said = header_says(&f, true, Some(&r)).concat();
        assert!(
            said.contains("14,086 of ~20,016"),
            "a mob that has not died in this run lost the one denominator this app can measure: \
             {said}"
        );
        assert!(
            said.contains("05:00"),
            "the target's own clock is gone from a row that is one mob: {said}"
        );
    }

    /// DEFECT: `5 kills of 6 agreed on ~111`, where the 111 was measured off all six.
    ///
    /// The hover took its figure from `Reading::expect` and its count from `Reading::modal`, and
    /// those are different groups whenever the samples are SETTLED: `expect` is then the median of
    /// every sample, while `modal` is the densest agreeing window inside them. The count is on that
    /// line for one reason, to say how much of the history stands behind the figure, so a count
    /// that belongs to a different group than the figure is worse than no count at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: taking the count from `modal()` while the figure comes from
    /// `expect()`, which is exactly what the line did.
    #[test]
    fn the_count_beside_the_figure_was_measured_with_it() {
        /* SETTLED, BECAUSE `spread` DIVIDES BY THE MEDIAN AND `modal`'s WINDOW DIVIDES BY ITS OWN
         * LOW. 100 to 111 is 9 percent of the median 111 and 11 percent of the low 100, so the
         * whole set agrees and the densest window is only five of the six. */
        let r = reading(&[100, 110, 110, 111, 111, 111]);
        assert!(
            r.settled(),
            "the fixture is not in the branch this is about"
        );
        assert_eq!(
            r.modal(),
            Some((111, 5)),
            "the fixture no longer tells the two statistics apart"
        );
        let (v, of) = measured(&r).expect("a settled reading offers a figure");
        assert_eq!(
            Some(v),
            r.expect(),
            "the figure drifted from the one `Reading::expect` gives, so the page and the book \
             disagree about the same mob"
        );
        assert_eq!(
            of, 6,
            "the count came from the modal group and the figure from the median of everything"
        );

        /* AND THE OTHER BRANCH IS `modal`'s OWN COUNT AND NOT THE WHOLE SET. Three kills agree,
         * two more sit fifteen percent above them and one is a double pull. */
        let split = reading(&[20_011, 20_014, 20_016, 23_014, 23_015, 40_000]);
        assert!(!split.settled());
        let (v, of) = measured(&split).expect("three kills agree");
        assert_eq!(Some(v), split.expect());
        assert_eq!(
            of, 3,
            "the count is the whole history rather than the group"
        );
        assert_ne!(of, split.n());
    }

    /// DEFECT: `4 players` over three rows.
    ///
    /// The stat row counted `FightRow::players`, which is everybody the name rule calls a person;
    /// the three tables under it are `dps::ranked_dealers`, which keeps only rows whose metric is
    /// above zero. A player who dealt, healed and took nothing in a pull is in the first count and
    /// in none of the tables, so the header states a number the page cannot account for and a
    /// reader goes hunting for a row that was never going to be there.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `FightRow::players` back in the stat row.
    #[test]
    fn the_players_stat_counts_only_the_players_the_panels_can_account_for() {
        use crate::fights::{FightRow, Fighter, Who};

        let player = |n: &str| Fighter {
            who: Who::Named(n.into()),
            ..Fighter::default()
        };
        let f = FightRow {
            secs: 60,
            fighters: vec![
                Fighter {
                    dealt: 3_000,
                    ..player("Aelin")
                },
                Fighter {
                    healed: 500,
                    ..player("Brakk")
                },
                Fighter {
                    taken: 800,
                    ..player("Corin")
                },
                /* THE ONE EVERY PANEL DROPS. He was swung at and avoided every swing, which is a
                 * real row in the fold and no row in any table on this page. */
                Fighter {
                    avoided: 3,
                    ..player("Dain")
                },
                Fighter {
                    who: Who::Named("a spite golem".into()),
                    taken: 3_000,
                    first_taken_at: Some(0),
                    last_taken_at: Some(60),
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };
        assert_eq!(
            f.players(),
            4,
            "the fixture no longer holds a player the panels drop"
        );
        assert_eq!(players_with_a_row(&f), 3);

        let said = header_says(&f, true, None);
        assert!(
            said.iter().any(|s| s == "players"),
            "the stat row is not on the header at all: {said:?}"
        );
        assert!(
            said.iter().any(|s| s == "3"),
            "the header did not state the count the panels can account for: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s == "4"),
            "the header stated a player count with no row anywhere under it: {said:?}"
        );
    }

    /// DEFECT: an empty state that sends the reader to a line the page does not draw.
    ///
    /// # THE WORDS ARE SHARED AND THE LINE THEY ASSUME WAS NOT
    ///
    /// `no_fights_words` was written for the Fights section of the parser screen, which carries a
    /// status line above its table, and two of its four sentences point at it: "the line above says
    /// what is being read", and "The line above says why, and it is usually that logging is off".
    /// Live borrowed the sentences and drew no line, so a reader whose game is not logging was told
    /// the reason was above him and given nothing above him.
    ///
    /// THE FIX IS THE LINE AND NOT NEW WORDS. Four screens answer "why is there no fight" out of
    /// that one function precisely so they cannot drift apart; a private sentence here would have
    /// traded a missing line for a fifth answer to one question.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `reading_line` label above the empty state.
    #[test]
    fn the_empty_state_draws_the_line_its_own_words_send_the_reader_to() {
        let ctx = prepared();
        let empty = crate::fights::probe::logs_dir("screen-live-no-line");
        let mut none = crate::fights::probe::booted(&empty);
        assert!(none.fights().is_empty(), "there is no log in that folder");

        let why = why_no_fights(
            none.scanning(),
            none.log_dir().dir.is_some(),
            none.active_log().is_some(),
        );
        assert_eq!(
            why,
            crate::screens::parser::NoFights::NoLog,
            "a folder with no eqlog in it is the case whose words point at a line above"
        );
        let words = no_fights_words(why);
        assert!(
            words.contains("line above"),
            "the shared words stopped pointing at a line, so this test is guarding nothing: \
             {words}"
        );
        let line = reading_line(&none);

        let said = painted(&ctx, &mut none);
        assert!(
            said.iter().any(|s| s == words),
            "this is not the empty state the test is about: {said:?}"
        );
        assert!(
            said.contains(&line),
            "the words send the reader to the line above and the page drew none: {said:?}"
        );
    }

    /// AND NOTHING NAMED HIT MEANS NO CLAIM, not the reader's own name in the slot.
    ///
    /// A fight the reader only stood near has no mob row of its own; putting a player there would
    /// say he is what is being fought.
    #[test]
    fn a_fight_where_only_players_were_hit_names_no_current_target() {
        use crate::fights::{FightRow, Fighter, Who};
        let f = FightRow {
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
            f.current_target(),
            None,
            "the reader was named as the thing being fought"
        );
    }

    /// DEFECT: THE HEADER HANDED TO THE ALPHABET.
    ///
    /// # `IN COMBAT  A REVULTANT RAT  02:02  9 players named`
    ///
    /// The log stamps to the SECOND and the reference capture puts up to thirty-two lines inside
    /// one, so in a raid the thing being killed and every add, pet and stray around it share the
    /// newest second constantly. The tie-break is not a rare case: it decides the header almost
    /// every frame.
    ///
    /// AND IT WAS THE NAME, ASCENDING. `'A'` is 65 in ASCII and `'a'` is 97, so a mob the log
    /// capitalises beats every lowercase-named one outright. A rat that got clipped once by a
    /// cleave took the header off the thing nine people were killing, and kept it.
    ///
    /// THE FIRST FIX FOR THIS HEADER SHIPPED WITH THIS BUG IN IT. Replacing `headline` with the
    /// newest target was right; leaving the tie on the name meant the new rule was decided by
    /// spelling in exactly the case it was written for.
    ///
    /// WHAT MUTATION MAKES THIS RED: breaking the tie on the name instead of on damage.
    #[test]
    fn a_stray_hit_in_the_newest_second_does_not_take_the_header() {
        use crate::fights::{FightRow, Fighter, Who};

        let mob = |name: &str, taken: u64, first: u32, last: u32| Fighter {
            who: Who::Named(name.into()),
            taken,
            first_taken_at: Some(first),
            last_taken_at: Some(last),
            ..Fighter::default()
        };

        /* BOTH HIT IN THE SAME NEWEST SECOND, which is the ordinary case and not a corner. The
         * rat's name sorts first on every byte-order rule there is; it has taken almost nothing. */
        let f = FightRow {
            secs: 122,
            fighters: vec![
                mob("a spite golem", 240_000, 0, 122),
                mob("A revultant rat", 60, 120, 122),
            ],
            ..FightRow::default()
        };

        let (who, _) = f.current_target().expect("something was hit");
        assert_eq!(
            who, "a spite golem",
            "a rat that took 60 damage holds the header over a golem that took 240,000, because \
             its name starts with a capital letter"
        );

        /* AND THE ORDER THE FOLD LISTED THEM IN CHANGES NOTHING. */
        let mut swapped = f.clone();
        swapped.fighters.reverse();
        assert_eq!(
            swapped.current_target().map(|(w, _)| w),
            Some("a spite golem")
        );
    }

    /* ------------------------------------------- the empty state's way out -- */

    /// DEFECT: AN EMPTY STATE THAT NAMES A PLACE THE READER CANNOT GET TO FROM WHERE HE IS.
    ///
    /// # THE WORDS ARE RIGHT AND IN THE POP-OUT THEY ARE A DEAD END
    ///
    /// `no_fights_words`' `NoFolder` arm says the Logs folder "is set in the main window's
    /// Settings, which opens from the gear at the foot of that window's rail". This page is drawn
    /// in the main window AND in `windows::ParserWindow`, and the pop-out has no rail, no gear and
    /// no Settings page at all: Settings is not a `nav::ScreenId`, it is a `Body`. So in the one
    /// window the owner keeps over the game while he plays, the page named a destination, named
    /// the route to it, and the route was not there.
    ///
    /// THE CLICK IS THE ASSERTION AND THE LABEL ALONE WOULD NOT BE. A disabled `egui::Button`
    /// paints its text exactly the same, and a button wired to nothing paints it too; only
    /// pressing it tells those apart from one that reaches the App. `screens::videos` makes the
    /// same argument about its reload control and for the same reason.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the control from the `NoFolder` arm, disabling it,
    /// or raising any ask but `Ask::OpenSettings` from it.
    #[test]
    fn the_no_folder_empty_state_carries_the_door_and_not_just_its_address() {
        let ctx = prepared();
        let (runs, ask) = empty_says(&ctx, NoFights::NoFolder, Vec::new());
        assert_eq!(ask, Ask::None, "nothing was pressed yet");
        let words = no_fights_words(NoFights::NoFolder);
        assert!(
            words.contains("Settings"),
            "the shared words stopped naming Settings, so this control answers a complaint \
             nobody is making any more: {words}"
        );
        assert!(
            runs.iter().any(|(w, _)| w == words),
            "this is not the empty state the test is about: {:?}",
            runs.iter().map(|(w, _)| w).collect::<Vec<_>>()
        );
        let at = spot(&runs, TO_SETTINGS).unwrap_or_else(|| {
            panic!(
                "the empty state drew no {TO_SETTINGS:?} control; it drew {:?}",
                runs.iter().map(|(w, _)| w).collect::<Vec<_>>()
            )
        });

        let (_, ask) = empty_says(&ctx, NoFights::NoFolder, press(at));
        assert_eq!(
            ask,
            Ask::OpenSettings,
            "the control did not reach the App, so it is a button that does nothing in the one \
             window where the words beside it are a dead end"
        );
    }

    /// AND IT IS NOT OFFERED WHERE SETTINGS IS NOT THE ANSWER.
    ///
    /// A control is a claim that pressing it helps. The bootstrap finishes on its own, a folder
    /// with no eqlog in it is usually logging turned off inside the GAME, and a log that was read
    /// with no combat in it is a quiet night. Settings fixes none of the three, and a door offered
    /// under all four causes would be this page sending a reader somewhere useless three times out
    /// of four, which is worse than the sentence it replaced.
    ///
    /// WHAT MUTATION MAKES THIS RED: drawing the control unconditionally, or gating it on
    /// anything wider than `NoFights::NoFolder`.
    #[test]
    fn the_settings_door_is_only_offered_for_the_one_cause_settings_fixes() {
        let ctx = prepared();
        for why in [NoFights::Reading, NoFights::NoLog, NoFights::NoCombat] {
            let (runs, ask) = empty_says(&ctx, why, Vec::new());
            assert!(
                runs.iter().any(|(w, _)| w == no_fights_words(why)),
                "{why:?} did not draw its own words, so this frame proves nothing"
            );
            assert!(
                spot(&runs, TO_SETTINGS).is_none(),
                "{why:?} offered a trip to Settings, which does not fix it"
            );
            assert_eq!(ask, Ask::None);
        }
    }

    /// AND THE SAME DOOR IS UNDER THE EFFECTS PANEL, PRESSED THROUGH THE WHOLE PAGE.
    ///
    /// # THIS IS THE REACHABILITY HALF, AND IT IS THE HALF THIS TREE LOSES
    ///
    /// `empty_says` drives `empty_state` directly, which proves the control works and proves
    /// nothing at all about whether `LiveScreen::ui` ever draws one. This drives `ui` itself, over
    /// the reference capture, with `Cx::data` at `None` (the no-corpus arm this file's own
    /// comments say every guard here takes), finds the control in the shapes the page produced and
    /// presses it. What comes back is the ask the App would read after the frame.
    ///
    /// THE EFFECTS PANEL NEEDS NO CAUSE GATE, unlike the fights empty state: the data folder is
    /// the only thing that fills the spell corpus, so Settings is the next step every time this
    /// arm is taken.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the control from the no-corpus arm, wiring it to
    /// anything but `Ask::OpenSettings`, or an early return in `ui` that stops the effects card
    /// being drawn at all.
    #[test]
    fn the_effects_panel_with_no_corpus_offers_the_same_door_through_the_page() {
        let ctx = prepared();
        let logs = crate::fights::probe::planted(
            "screen-live-settings-door",
            crate::fights::probe::CAPTURE,
        );
        let mut ing = crate::fights::probe::booted(&logs);
        assert!(
            !ing.fights().is_empty(),
            "the capture folded no fights, so the page would take its empty state instead"
        );

        let (runs, ask) = drawn(&ctx, &mut ing, Vec::new());
        assert_eq!(ask, Ask::None, "nothing was pressed yet");
        assert!(
            runs.iter().any(|(w, _)| w == "No spell corpus loaded."),
            "the page is not drawing the no-corpus arm, so this test is about nothing"
        );
        let at = spot(&runs, TO_SETTINGS).unwrap_or_else(|| {
            panic!(
                "the page drew no {TO_SETTINGS:?} control beside the no-corpus line; it drew {:?}",
                runs.iter().map(|(w, _)| w).collect::<Vec<_>>()
            )
        });

        let (_, ask) = drawn(&ctx, &mut ing, press(at));
        assert_eq!(
            ask,
            Ask::OpenSettings,
            "the whole page draws the control and nothing reaches the App, which is a door \
             painted on a wall"
        );
    }
}
