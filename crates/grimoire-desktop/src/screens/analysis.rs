//! Screen: Analysis. One fight, read as deeply as the log supports.
//!
//! # It is the same widgets, drawn large
//!
//! A tab here is a list of [`Widget`]s over one [`FightRow`], which is exactly what an overlay is.
//! The page draws them with room for a heading and the overlay draws them small over the game, and
//! both call `screens::dps::draw_widget`. That is the whole reason the two surfaces cannot end up
//! disagreeing about what a crit rate is: there is one renderer and no second opinion.
//!
//! # What it deliberately does not show
//!
//! The mockup this is built from carries four things the log cannot support, and each is absent
//! rather than empty, because an empty panel tells a reader the app looked and found nothing:
//!
//!   * A THREAT tab. Nothing in `combat::Event` carries threat and no line prints it.
//!   * The encounter HEALTH BAR. Damage dealt is real; a mob's maximum is not in the log.
//!   * `Pull: <name>`. First-appearance order is not the puller, and the first actor can be the mob.
//!   * A character PORTRAIT. There is no character art in the tree and no class field to pick one
//!     with; inferring class from a spell in the log shows the wrong class the first time a group
//!     member's spell folds in.
//!
//! # Which fight it reads
//!
//! `Ingest::fights()` is the BOOTSTRAP history: written once when the app starts and not rewritten
//! while it runs. `Ingest::current_fight()` is the live end, re-folded every poll. Both are offered,
//! and the difference is stated on screen, because a page that silently mixed them would show a
//! reader a fight that stopped updating without saying when.
use crate::fights::{FightRow, Mark, Who};
use crate::overlay::{Cols, Detail, Metric, Ranked, Subject, Timeline, Widget};
use crate::screens::Cx;
use crate::theme::*;
use egui::{RichText, Ui};
use std::collections::BTreeMap;

/// The tabs, in the order the mockup puts them.
///
/// A TAB IS A LIST OF WIDGETS AND NOTHING ELSE. Adding one is naming it here; there is no per-tab
/// renderer to write, which is what the shared vocabulary bought.
pub const TABS: [&str; 6] = [
    "Summary", "Damage", "Healing", "Tanking", "Timeline", "Deaths",
];

/// THE TAB'S WIDGETS, FOR THE UNIT INVARIANT IN `dps`. See
/// `dps::a_table_of_rates_always_says_somewhere_that_they_are_rates`: it walks every ranked config
/// the app ships, and it can only do that if it can reach them.
#[cfg(test)]
pub fn panels_for_test(tab: usize) -> Vec<Widget> {
    panels(tab)
}

/// What each tab draws. See [`TABS`].
fn panels(tab: usize) -> Vec<Widget> {
    let table = |metric: Metric| {
        Widget::Ranked(Ranked {
            foot: false,
            metric,
            rate: true,
            headline: false,
            cap: 24,
            fit: false,
            /* THE COLUMN HEADER, WHICH IS WHERE THE UNIT LIVES ON A PAGE.
             *
             * `headline: false` skips `dps::header`, and that is the only other place a unit is
             * drawn beside a figure. So `head: true` on the line below is the only thing standing
             * between this page and a column of dps under a heading that names a total.
             *
             * IT USED TO SAY `false` HERE, AND THIS COMMENT USED TO DESCRIBE THAT IN THE PRESENT
             * TENSE, which is worth correcting rather than deleting: a reader who found the old
             * words believed the shipped page was still printing rates with no unit anywhere on
             * screen. It was, for one build. This page was left out of the wave that gave Live,
             * Dashboards and Reports their headers, purely because it builds its config with
             * `..Cols::default()` rather than a literal `Cols`, so the search that found the
             * others missed it. The header is on now and the tables say `dps` and `hps`.
             *
             * AND THE INVARIANT DID NOT COVER IT EITHER, which is the part worth keeping:
             * `dps::a_table_of_rates_always_says_somewhere_that_they_are_rates` walked Live,
             * Dashboards and the shipped overlay, and stopped. A guard over "every shipped
             * config" that enumerates the pages by hand is a guard over the pages somebody
             * remembered. It reaches this page now, through `panels_for_test` above, so a return
             * to `head: false` here is a red test rather than a discovery on stream. */
            cols: Cols {
                head: true,
                ..Cols::default()
            },
        })
    };
    /* THIS PAGE SCROLLS AND IS NOT A GRID CELL, so a panel may run as long as it likes. */
    let mine = Detail {
        who: Subject::You,
        cap: 12,
        head: true,
        fit: false,
    };
    match tab {
        0 => vec![
            Widget::Timeline(Timeline {
                height: 150.0,
                cap: 5,
                marks: true,
            }),
            table(Metric::Dealt),
        ],
        1 => vec![
            table(Metric::Dealt),
            Widget::Abilities(mine),
            Widget::Targets(mine),
            Widget::Outcomes(mine),
            Widget::Elements(mine),
        ],
        2 => vec![table(Metric::Healed), table(Metric::Received)],
        3 => vec![table(Metric::Taken)],
        4 => vec![Widget::Timeline(Timeline {
            height: 260.0,
            cap: 8,
            marks: true,
        })],
        _ => Vec::new(),
    }
}

/// The Analysis screen's own state: which fight and which tab.
#[derive(Default)]
pub struct AnalysisScreen {
    tab: usize,
    /// WHICH FIGHT, AS AN INDEX FROM THE END. Counting from the END and not the start is what keeps
    /// a selection meaningful: the history list grows at its end, so an index from the front would
    /// slide onto a different fight every time the log was rescanned.
    back: usize,
    /// Follow the live fight rather than the history. The default, because somebody who opens this
    /// mid-pull wants the pull.
    live: bool,
    /// What is being typed into the note field right now, which is NOT yet in settings.
    note: String,
    /// WHICH FIGHT THE BUFFER ABOVE BELONGS TO, by that fight's start stamp, or `None` before the
    /// page has drawn any fight's notes.
    ///
    /// AN `Option` AND NOT AN EMPTY STRING STANDING IN FOR ONE. The two fields move together and
    /// [`AnalysisScreen::notes`] writes the buffer out under this key before repointing the pair;
    /// with a sentinel it could not tell "nothing loaded yet, there is nothing to save" from "the
    /// fight whose start stamp is empty", and a `FightRow::default` has exactly that stamp.
    note_for: Option<String>,
}

impl AnalysisScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* The pump, exactly as the other log-reading screens do it. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1000));

        let history: Vec<FightRow> = cx.ingest.fights().to_vec();
        let current = cx.ingest.current_fight().cloned();
        let is_live = cx.ingest.fight_is_live();

        if history.is_empty() && current.is_none() {
            let why = crate::screens::parser::why_no_fights(
                cx.ingest.scanning(),
                cx.ingest.log_dir().dir.is_some(),
                cx.ingest.active_log().is_some(),
            );
            ui.label(RichText::new(crate::screens::parser::no_fights_words(why)).color(TEXT_2));
            return;
        }

        /* Default to the live fight the first time there is one. */
        if current.is_some() && !self.live && self.back == 0 && self.note_for.is_none() {
            self.live = true;
        }

        self.picker(ui, &history, current.is_some());

        let shown: Option<&FightRow> = if self.live {
            current.as_ref()
        } else {
            self.back = self.back.min(history.len().saturating_sub(1));
            history.get(history.len().saturating_sub(1) - self.back)
        };
        let Some(f) = shown else {
            /* WHAT THIS MEANS NOW, WHICH IS NOT WHAT IT USED TO MEAN. It was reachable by pressing
             * `<` with an empty bootstrap history: the arrow moved the reader onto row 0 of a list
             * with no rows and the page said his fight had scrolled out of the read window, about a
             * fight that had never been in it. `picker` no longer offers a step that lands nowhere,
             * so what is left here is the honest case: a rescan adopted a shorter list (or a
             * different log) under a selection that was valid when it was made. */
            ui.label(
                RichText::new("That fight is no longer in the window that was read.").color(TEXT_3),
            );
            return;
        };

        header(ui, f, self.live && is_live);
        ui.add_space(8.0);
        tiles(ui, f);
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            for (i, name) in TABS.iter().enumerate() {
                if ui.selectable_label(self.tab == i, *name).clicked() {
                    self.tab = i;
                }
            }
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt("analysis_body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.tab == 5 {
                    deaths(ui, f);
                } else {
                    for w in panels(self.tab) {
                        crate::screens::dps::draw_widget(
                            ui,
                            f,
                            crate::fights::Pulse::from_live(is_live),
                            &w,
                        );
                        ui.add_space(10.0);
                    }
                }
                self.notes(ui, cx, f);
            });
    }

    /// WHICH FIGHT: live, or one out of the history, with arrows either side.
    ///
    /// # AN ARROW WITH NOWHERE TO GO IS GREYED OUT, IT DOES NOT MOVE THE READER NOWHERE
    ///
    /// `<` was a plain button that always took the click. With a live fight and an EMPTY bootstrap
    /// history (launch mid-pull, or a tail holding one open fight and nothing finished) it set
    /// `live = false` and `back = 0`; `ui` then indexed an empty list, found nothing, and printed
    /// "That fight is no longer in the window that was read." about a fight that never existed.
    /// The reader pressed an arrow while standing in a pull and the page went blank.
    ///
    /// SO THE TEST THAT ENABLES THE ARROW IS THE DESTINATION ITSELF. [`older`] and [`newer`]
    /// answer with WHERE the step lands, `None` disables the control and says why on hover, and
    /// the click applies exactly the answer that enabled it. A separate "can I" predicate beside a
    /// "do it" branch is two rules that drift; this is one.
    fn picker(&mut self, ui: &mut Ui, history: &[FightRow], has_live: bool) {
        /* CLAMPED ONCE, HERE. `ui` clamps `back` too, but only after this runs and only on the
         * branch that reads the history, so a rescan that shortened the list would have had this
         * row asking about a row number that is no longer there. */
        let back = self.back.min(history.len().saturating_sub(1));
        let now = if self.live {
            Stand::Live
        } else {
            Stand::Back(back)
        };
        let go_older = older(now, history.len());
        let go_newer = newer(now, has_live);
        ui.horizontal(|ui| {
            if has_live && ui.selectable_label(self.live, "Live").clicked() {
                self.stand(Stand::Live);
            }
            let no_older = if history.is_empty() {
                "no finished fight was in the log when the app started"
            } else {
                "this is the oldest fight that was read"
            };
            let back_arrow = ui
                .add_enabled(go_older.is_some(), egui::Button::new("<"))
                .on_hover_text("older fight")
                .on_disabled_hover_text(no_older);
            if let (true, Some(s)) = (back_arrow.clicked(), go_older) {
                self.stand(s);
            }
            let label = if self.live {
                "the fight you are in".to_owned()
            } else {
                history
                    .get(history.len().saturating_sub(1) - back)
                    .map_or_else(
                        || "no fight".to_owned(),
                        |f| {
                            format!(
                                "{}  ({})",
                                f.headline.as_deref().unwrap_or("an unnamed fight"),
                                clock_of(&f.end)
                            )
                        },
                    )
            };
            ui.label(RichText::new(label).color(TEXT).strong());
            let no_newer = if self.live {
                "you are already on the fight you are in"
            } else {
                "this is the newest fight that was read"
            };
            let fwd_arrow = ui
                .add_enabled(go_newer.is_some(), egui::Button::new(">"))
                .on_hover_text("newer fight")
                .on_disabled_hover_text(no_newer);
            if let (true, Some(s)) = (fwd_arrow.clicked(), go_newer) {
                self.stand(s);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                /* THE TWO LISTS ARE NOT THE SAME LIST, AND THE PAGE SAYS SO. `fights()` is written
                 * once at startup; only the live end keeps moving. A reader flicking between them
                 * must know which one has stopped. */
                ui.label(
                    RichText::new(if self.live {
                        "re-read from the log every second".to_owned()
                    } else {
                        format!("{} fights read when the app started", history.len())
                    })
                    .color(TEXT_3),
                );
            });
        });
    }

    /// Move to a standing point. The ONE place `live` and `back` are written together, so the two
    /// fields cannot end up disagreeing about which of the two lists is being read.
    fn stand(&mut self, s: Stand) {
        match s {
            Stand::Live => self.live = true,
            Stand::Back(b) => {
                self.live = false;
                self.back = b;
            }
        }
    }

    /// A note a person leaves on a fight, kept in settings and keyed by the fight's own start stamp.
    ///
    /// THE STAMP AND NOT AN INDEX, because the history list is rebuilt on every rescan and an index
    /// would move a note onto a different fight. The stamp is the log's own text and is stable.
    ///
    /// # THE BUFFER IS EMPTIED OUT BEFORE IT IS REFILLED, AND THAT IS THE WHOLE OF IT
    ///
    /// `ui` calls `picker` and then this, in that order, and it has to: the picker is what decides
    /// which fight the frame is drawing. So on the very frame a reader clicks `<`, this function is
    /// handed the NEW fight while `self.note` still holds what was typed about the old one. It used
    /// to notice the key had changed and overwrite the buffer on the spot, and the only writer to
    /// settings was `lost_focus` on the field BELOW, which by then was the new fight's field
    /// holding the new fight's text. A note typed and not blurred was gone the instant the reader
    /// clicked the next fight: no warning, no way back, and the click that destroyed it is the most
    /// natural one on the page.
    ///
    /// REORDERING `ui` IS NOT THE FIX. This function needs the fight the picker chose, so it cannot
    /// run first, and saving on every keystroke is a settings file per character typed. What is
    /// actually true is that the buffer and `note_for` are ONE value: whatever is in the buffer
    /// belongs to `note_for` and is written there before the pair is repointed, whatever the
    /// repointing turns out to be (an arrow today, a fight list tomorrow).
    ///
    /// THE WRITE-OUT IS [`AnalysisScreen::flush_notes`] AND IS NO LONGER SPELLED OUT HERE. It was
    /// four lines inline, and it had to grow a second caller outside this page (see that function
    /// for the half of the defect this one cannot reach); two spellings of "the buffer belongs to
    /// `note_for`" is two rules that drift, and the one that drifts is the one nobody is editing.
    fn notes(&mut self, ui: &mut Ui, cx: &mut Cx, f: &FightRow) {
        let key = f.start.clone();
        if self.note_for.as_deref() != Some(key.as_str()) {
            self.flush_notes(cx);
            self.note = cx
                .settings
                .fight_notes
                .get(&key)
                .cloned()
                .unwrap_or_default();
            self.note_for = Some(key.clone());
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Fight notes").color(TEXT_3));
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.note)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text("What happened here?"),
        );
        /* WRITTEN WHEN THE FIELD IS LEFT, not on every keystroke: each write is a settings file. */
        if r.lost_focus() {
            let typed = self.note.clone();
            save_note(cx, &key, &typed);
        }
    }

    /// PUT WHAT IS IN THE BUFFER UNDER THE FIGHT IT WAS TYPED ABOUT, AND POINT AT NOTHING.
    ///
    /// # THE HALF OF THE LOST-NOTE DEFECT THAT `notes` CANNOT REACH
    ///
    /// Both of this page's writers only ever run while this page is DRAWING: `lost_focus` on the
    /// field needs the widget to be laid out again to observe the focus leaving, and
    /// [`AnalysisScreen::notes`] itself only runs from [`AnalysisScreen::ui`]. So the repointings
    /// that happen INSIDE the page
    /// (the arrows, the Live label) were answered and the ones that happen by leaving it were not.
    /// A reader who typed a note and then stepped to Fights, Kills, Loot or Overlays took the
    /// buffer with him: the widget was never drawn again, nothing observed a blur, and the text was
    /// dropped with the frame. No warning, and the step is the ordinary one.
    ///
    /// # WHY THIS IS PUBLIC AND WHO CALLS IT
    ///
    /// It was deliberately NOT added on the round that fixed the in-page half, because at that
    /// moment nothing outside this file could have called it and a method with no production caller
    /// is this tree's signature defect. `ParserScreen` owns this screen (Analysis is a VIEW of the
    /// Log Parser destination, which is why the state is a field on it), so the caller and the
    /// method landed together: `ParserScreen::ui` calls this on every frame it draws one of its
    /// OTHER views, which is exactly the frame after the step that used to lose the note.
    ///
    /// IDEMPOTENT, WHICH IS WHAT MAKES A PER-FRAME CALLER AFFORDABLE. It TAKES `note_for`, so the
    /// second call and every one after it returns before touching anything, and [`store_note`]
    /// answers whether the table actually moved so a flush of a note nobody changed costs no file
    /// write at all.
    ///
    /// WHAT IT STILL DOES NOT COVER, SAID PLAINLY: closing the app, or closing the pop-out Parser
    /// window, with a note typed and the Analysis view on screen. Nothing draws a frame after
    /// either, so no caller in this file or in `screens::parser` can see it; that flush belongs
    /// where the screens are owned and shut down (`main.rs`, `windows.rs`).
    /// PUT A NOTE IN THE BUFFER AS IF IT HAD JUST BEEN TYPED. See the forwarder in
    /// `screens::parser` for who needs this and why it cannot drive the page instead.
    #[cfg(test)]
    pub fn seed_note_for_test(&mut self, key: &str, typed: &str) {
        self.note_for = Some(key.to_owned());
        self.note = typed.to_owned();
    }

    pub fn flush_notes(&mut self, cx: &mut Cx) {
        self.flush_notes_into(cx.settings);
    }

    /// THE SAME WRITE-OUT, ASKED FOR WITH ONLY THE SETTINGS.
    ///
    /// # THE LAST PLACE A NOTE COULD STILL BE LOST WAS THE CLOSE
    ///
    /// [`AnalysisScreen::flush_notes`] closes the case the finding named, changing fight with an
    /// unblurred note, because `notes` calls it before it reloads the buffer. It could not close
    /// the other one: quitting the app, or closing the Parser tool window, with a note typed and
    /// never blurred. Nothing draws a frame after either event, so no caller inside this page can
    /// see them, and the doc on `flush_notes` recorded that gap rather than pretending it was
    /// covered.
    ///
    /// `eframe::App::on_exit` IS THE CALLER, and it cannot build a `Cx`: that borrows the player,
    /// the chat handle, the auth view and the watcher's status, none of which exist as a group at
    /// teardown. It does hold `&mut Settings`, and settings are all this write ever needed:
    /// `save_note` puts the text in `Settings::fight_notes` and saves the file, and touches
    /// nothing else on the context.
    ///
    /// SO THE NARROWER ARGUMENT IS THE REAL ONE and `flush_notes` is the convenience over it.
    /// Written the other way round, the one caller that matters most could not call it.
    pub fn flush_notes_into(&mut self, settings: &mut crate::settings::Settings) {
        let Some(leaving) = self.note_for.take() else {
            return;
        };
        let typed = std::mem::take(&mut self.note);
        if !store_note(&mut settings.fight_notes, &leaving, &typed) {
            return;
        }
        if let Err(e) = settings.save() {
            log::warn!("the fight note was not saved: {e}");
        }
    }

    /// WHAT THE READER HAS TYPED AND NOT YET LEFT, PUT THERE WITHOUT A KEYBOARD. Test only.
    ///
    /// The state being stood up is a `TextEdit` that has been typed into and not blurred, and in a
    /// headless frame the only other way to reach it is to land a synthetic pointer on a widget
    /// rectangle and then feed key events, which tests egui's hit testing rather than this rule.
    /// The tests in THIS module reach it by assignment because they can see the fields; the caller
    /// that [`AnalysisScreen::flush_notes`] exists for lives in `screens::parser`, which cannot,
    /// and a test of that caller that could not stand the state up would be a test of nothing.
    #[cfg(test)]
    pub fn typed_for_test(&mut self, fight: &str, typed: &str) {
        self.note_for = Some(fight.to_owned());
        self.note = typed.to_owned();
    }
}

/// WHERE THE READER IS STANDING, as one value rather than a bool and an index that can disagree.
///
/// `Live` is `Ingest::current_fight`, the end that keeps moving. `Back(n)` is `n` rows back from
/// the newest of `Ingest::fights`, the list written once at startup. COUNTING FROM THE END is what
/// keeps a selection meaningful: that list grows at its end, so an index from the front would slide
/// onto a different fight every time the log was rescanned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stand {
    Live,
    Back(usize),
}

/// One step older, or `None` when there is no older fight to stand on.
///
/// `None` IS WHAT GREYS THE ARROW OUT, and it is the whole reason this is a function: the page used
/// to move first and discover afterwards that it had landed on nothing.
///
/// FROM LIVE THE STEP IS TWO ROWS AND NOT ONE, which is deliberate and not an off by one.
/// `Ingest::fights` and `Ingest::current_fight` are two folds of the SAME file, so the newest
/// history row is nearly always the live fight over again, frozen at the moment the app started;
/// landing on it would show the same pull twice and call the second one older. The clamp still
/// allows it when the history holds exactly one row, because refusing there would put a one fight
/// history out of reach altogether.
fn older(now: Stand, len: usize) -> Option<Stand> {
    if len == 0 {
        return None;
    }
    let to = match now {
        Stand::Live => 1.min(len - 1),
        Stand::Back(b) => (b + 1).min(len - 1),
    };
    if now == Stand::Back(to) {
        None
    } else {
        Some(Stand::Back(to))
    }
}

/// One step newer, or `None` when the reader is already on the newest thing this page can show.
///
/// There is nothing newer than the live fight, and nothing newer than the newest history row while
/// no fight is live. Both of those were clicks that did nothing at all.
fn newer(now: Stand, has_live: bool) -> Option<Stand> {
    match now {
        Stand::Live => None,
        Stand::Back(0) if has_live => Some(Stand::Live),
        Stand::Back(0) => None,
        Stand::Back(b) => Some(Stand::Back(b - 1)),
    }
}

/// Put what was typed under `key`, and write the settings file only if that changed anything.
///
/// THE "ONLY IF" IS LOAD-BEARING NOW THAT THERE ARE TWO CALLERS. Leaving the field writes, and so
/// does changing fight, and a reader arrowing through twenty fights having typed nothing would
/// otherwise rewrite settings.json twenty times.
fn save_note(cx: &mut Cx, key: &str, typed: &str) {
    if !store_note(&mut cx.settings.fight_notes, key, typed) {
        return;
    }
    if let Err(e) = cx.settings.save() {
        log::warn!("the fight note was not saved: {e}");
    }
}

/// The table half of [`save_note`], WITHOUT THE FILE, so a test can drive the rule without going
/// anywhere near the owner's real settings. Answers whether the table moved, which is what gives
/// the caller licence to write.
///
/// A BLANK NOTE REMOVES THE ENTRY rather than storing an empty string: settings.json skips an empty
/// map, and a note a person cleared should leave nothing behind. STORED TRIMMED, so that the same
/// text typed with a stray newline is not a second, different note that provokes a second file
/// write every time the reader passes through the fight.
fn store_note(notes: &mut BTreeMap<String, String>, key: &str, typed: &str) -> bool {
    let text = typed.trim();
    if text.is_empty() {
        return notes.remove(key).is_some();
    }
    if notes.get(key).map(String::as_str) == Some(text) {
        return false;
    }
    notes.insert(key.to_owned(), text.to_owned());
    true
}

/// What is being fought, where, and how it ended.
fn header(ui: &mut Ui, f: &FightRow, live: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if live {
            ui.label(RichText::new("\u{25cf}").color(SETTLED));
        }
        ui.label(
            RichText::new(f.headline.as_deref().unwrap_or("an unnamed fight"))
                .color(GOLD_HI)
                .strong()
                .size(18.0),
        );
        if let Some(z) = &f.zone {
            ui.label(RichText::new(z).color(TEXT_2));
        }
        ui.label(RichText::new(clock_of(&f.start)).color(TEXT_3).monospace());
    });
    if f.cut {
        ui.label(
            RichText::new(
                "This fight opened before the part of the log that was read, so its numbers are a \
                 floor.",
            )
            .color(WRONG),
        );
    }
}

/// WHAT THE GROUP DID, SUMMED OVER THE POPULATION THE TABLES RANK.
///
/// THE SAME FILTER `dps::ranked_dealers` USES, and that is the whole of it: a tile summed without
/// `Who::player` counts the pull, so the tile and the table beneath it are about different sets of
/// people. A FUNCTION rather than a line inside `tiles` so a test can drive it without a `Ui`,
/// which is the only reason the worst defect on this page went unnoticed: nothing could reach the
/// arithmetic to check it.
///
/// BOTH THIS AND [`group_count`] ARE NOW ONE LINE ONTO `FightRow`, and the move is the point rather
/// than the tidiness: while this arithmetic was private to this page, `screens::live` printed the
/// raw fight totals in a header above the same widgets and disagreed with this page about the same
/// fight.
///
/// (THIS DOC WAS ATTACHED TO THE COUNTING FUNCTION BELOW for a build, with the amount function
/// carrying none at all. It is recorded rather than quietly moved because of what it did to a
/// reader: every word of it is about summing an amount, and a reader who trusted where it sat took
/// `group_count` for a total and would have read "Player deaths" as a quantity of something.)
fn group_total(f: &FightRow, pick: fn(&crate::fights::Fighter) -> u64) -> u64 {
    f.group_sum(pick)
}

/// The same as [`group_total`], for a COUNT rather than an amount: how many of a thing happened to
/// the population the tables rank. `Player deaths` is the one tile that uses it.
fn group_count(f: &FightRow, pick: fn(&crate::fights::Fighter) -> u32) -> u32 {
    f.group_count(pick)
}

/// THE RATE THE `Group dps` TILE PRINTS, as a function so a test can drive the tile's own
/// arithmetic without a `Ui`.
///
/// IT IS THE GROUP'S TOTAL OVER THE FIGHT'S SPAN, NEVER `FightRow::damage` over it, which is the
/// defect the tile shipped with and which the test named for this rule did not actually check: see
/// `the_group_rate_is_the_group_over_the_clock_the_log_can_time`.
///
/// `None` MEANS THE LOG CANNOT EXPRESS ONE, on the overlay's own floor rather than a second copy of
/// the rule: the log stamps to the second, so a fight younger than `dps::MIN_RATE_SECS` has no rate
/// the file can support, and the two surfaces would disagree the day that floor moved.
fn group_rate(f: &FightRow) -> Option<u64> {
    crate::screens::dps::dps(group_total(f, |x| x.dealt), f.secs)
}

/// The row of figures across the top: the mockup's stat tiles.
///
/// # `Raid dps` MEANT THE WHOLE FIGHT, AND THAT WAS A LIE IN SIXTEEN POINT TEXT
///
/// This tile divided `FightRow::damage` by the span. That field's own doc says what it is: "Every
/// point of damage anybody dealt to anybody inside the fight", the pull included. So the tile
/// counted the mobs as part of the raid.
///
/// MEASURED ON THE CAPTURE'S FIRST FIGHT: 266 seconds, 16,526 total damage, of which the PLAYERS
/// dealt 12,976. The tile printed 62 where the honest figure is 48, twenty-eight percent high, and
/// the surplus was the mummy and the skeletons hitting the group.
///
/// AND IT CONTRADICTED THE PAGE IT SAT ON. The capture's third fight is the Qeynos guards killing
/// the reader: every dealer is a guard and the only player row dealt nothing. The tile read 27 for
/// a group that dealt zero, eight points above a Damage tab printing "Nobody named has any dmg."
/// Two numbers on one screen disagreeing, with the invented one in the larger font.
///
/// SO EVERY TOTAL HERE IS THE POPULATION THE TABLES RANK, which is `Who::player`. What the pull
/// dealt is a real number and it is not the raid's; it has no tile because no panel on this page
/// ranks it.
fn tiles(ui: &mut Ui, f: &FightRow) {
    /* `FightRow::players`, WHICH IS THE ROSTER'S RULE, so the tile counts the rows the tabs
     * under it rank: the reader and his group when the log knows it, every player when not. */
    let players: usize = f.players();
    let ours = group_total(f, |x| x.dealt);
    let healed = group_total(f, |x| x.healed);
    let rate = group_rate(f);
    let ended = match f.zone_gap {
        Some(g) => format!("{} ({}s later)", f.ended, g),
        None => f.ended.clone(),
    };

    ui.horizontal_wrapped(|ui| {
        tile(ui, "Duration", &span(f.secs));
        tile(ui, "Group damage", &thousands(ours));
        tile(
            ui,
            "Group dps",
            &rate.map_or_else(|| "-".to_owned(), thousands),
        );
        tile(ui, "Group healing", &thousands(healed));
        /* DEATHS ON YOUR SIDE, NOT EVERY DEATH IN THE FIGHT. `FightRow::deaths` counts every
         * `Event::Death` whoever died, so on a clean camp it is a second copy of the kill count:
         * the reports page measured 33 against a real 1, with 31 of them being the reader's own
         * kills. The label did not say which population it counted and the number was the wrong
         * one, which is the damage tile's defect in a second place. */
        tile(
            ui,
            "Player deaths",
            &group_count(f, |x| x.deaths).to_string(),
        );
        tile(ui, "Players", &players.to_string());
        tile(ui, "End state", &ended);
    });
}

fn tile(ui: &mut Ui, label: &str, value: &str) {
    egui::Frame::NONE
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .corner_radius(3)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).color(TEXT).strong().size(16.0));
                ui.label(RichText::new(label).color(TEXT_3).size(10.5));
            });
        });
}

/// EVERY DEATH IN THE FIGHT, WITH WHO GOT THE CREDIT.
///
/// `ingest::KillEvent` CANNOT DO THIS: it has no killer field at all, so a screen built on it can
/// say what died and never who killed it. These come off `FightRow::moments`, which carries both
/// slots.
fn deaths(ui: &mut Ui, f: &FightRow) {
    let name = |slot: usize| -> String {
        f.fighters.get(slot).map_or_else(
            || "(no such fighter)".to_owned(),
            |x| x.who.text().to_owned(),
        )
    };
    let rows: Vec<&crate::fights::Moment> = f
        .moments
        .iter()
        .filter(|m| matches!(m.what, Mark::Death { .. }))
        .collect();
    if rows.is_empty() {
        ui.label(RichText::new("Nobody died.").color(TEXT_3));
        return;
    }
    ui.spacing_mut().item_spacing.y = 2.0;
    for m in rows {
        if let Mark::Death { killer, victim } = &m.what {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(span(i64::from(m.at)))
                        .color(TEXT_3)
                        .monospace(),
                );
                let tint = if f.fighters.get(*victim).is_some_and(|x| x.who == Who::You) {
                    WRONG
                } else {
                    TEXT
                };
                ui.label(RichText::new(name(*victim)).color(tint));
                ui.label(RichText::new("killed by").color(TEXT_3));
                ui.label(RichText::new(name(*killer)).color(TEXT_2));
            });
        }
    }
}

/// `266` becomes `4:26`.
fn span(secs: i64) -> String {
    let s = secs.max(0);
    format!("{}:{:02}", s / 60, s % 60)
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

/// The clock out of a log stamp: `Wed Jul 15 23:21:16 2026` becomes `23:21:16`.
///
/// BY SHAPE AND NOT BY OFFSET. The stamp carries no zone, which is why `FightRow::start` is text;
/// turning it into an instant to format it straight back out would be a timezone claim these bytes
/// cannot support. A stamp this does not recognise comes back whole rather than sliced into
/// something that looks like a time and is not.
fn clock_of(stamp: &str) -> &str {
    stamp
        .split_whitespace()
        .find(|w| w.len() == 8 && w.as_bytes()[2] == b':' && w.as_bytes()[5] == b':')
        .unwrap_or(stamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{Fighter, Who};

    fn fighter(who: Who, dealt: u64, healed: u64) -> Fighter {
        Fighter {
            who,
            pet: false,
            class: None,
            first_taken_at: None,
            last_taken_at: None,
            dealt,
            taken: 0,
            healed,
            received: 0,
            swings: 0,
            landed: 0,
            avoided: 0,
            kills: 0,
            deaths: 0,
            abilities: Vec::new(),
            targets: Vec::new(),
            schools: Vec::new(),
            outcomes: Default::default(),
            melee_crits: 0,
            series: Vec::new(),
        }
    }

    /// DEFECT: THE STAT TILE COUNTING THE THING BEING FOUGHT AS PART OF THE GROUP.
    ///
    /// This shipped. `Raid dps` divided `FightRow::damage`, whose own doc says it is "every point
    /// of damage anybody dealt to anybody", by the span. Measured on the capture's first fight it
    /// printed 62 where the players' own figure is 48: twenty-eight percent high, and the surplus
    /// was the mummy and the skeletons hitting the group.
    ///
    /// AND IT CONTRADICTED ITS OWN PAGE. On the fight where the Qeynos guards kill the reader, every
    /// dealer is a guard and the tile read 27 for a group that dealt nothing, directly above a
    /// Damage tab printing that nobody named had any.
    ///
    /// WHAT MUTATION MAKES THIS RED: summing `f.damage`, or dropping the `who.player()` filter from
    /// `group_total`.
    #[test]
    fn a_group_total_counts_the_group_and_not_the_pull() {
        let mut f = FightRow::default();
        f.secs = 100;
        f.fighters = vec![
            fighter(Who::You, 1_000, 40),
            fighter(Who::Named("Tanefilo".into()), 500, 60),
            /* The pull. A space in the name is what makes it not a player. */
            {
                let mut m = fighter(Who::Named("a dry bone skeleton".into()), 9_000, 0);
                /* The pull died. `FightRow::deaths` counts it; the group lost nobody. */
                m.deaths = 27;
                m
            },
            /* And an NPC healer, which the healing tile used to count too. */
            fighter(Who::Named("Sir Edwin Motte".into()), 0, 900),
        ];
        f.damage = f.fighters.iter().map(|x| x.dealt).sum();

        assert_eq!(f.damage, 10_500, "the fight total does include the pull");
        assert_eq!(
            group_total(&f, |x| x.dealt),
            1_500,
            "the skeleton's 9,000 is not the group's damage"
        );
        assert_eq!(
            group_total(&f, |x| x.healed),
            100,
            "an NPC healer's output is not the group's healing"
        );
        assert_eq!(
            group_count(&f, |x| x.deaths),
            0,
            "the mobs the group killed are not deaths on your side"
        );

        /* AND THE RATE FOLLOWS. Off the fight total this reads 105; off the group's it reads 15. */
        assert_eq!(
            crate::screens::dps::dps(group_total(&f, |x| x.dealt), f.secs),
            Some(15)
        );
    }

    /// DEFECT: a tile printing a rate for a fight too short for the log to express one.
    ///
    /// The page must use the overlay's own floor rather than a second copy of the rule, or the two
    /// surfaces disagree the day it moves. It asks `group_rate` and not `dps::dps` directly,
    /// because a test that assembles the pipeline itself proves only that the parts exist: that is
    /// the mistake `the_group_rate_is_the_group_over_the_clock_the_log_can_time` records.
    ///
    /// WHAT MUTATION MAKES THIS RED: `group_rate` dividing by `f.secs` itself instead of calling
    /// `dps::dps`. It is written in terms of `MIN_RATE_SECS` rather than the 3 that constant happens
    /// to hold today, so that raising the floor moves this test with it rather than leaving it
    /// asserting the old rule under the new name.
    #[test]
    fn the_page_withholds_a_rate_on_the_same_floor_the_overlay_uses() {
        let mut f = FightRow {
            fighters: vec![fighter(Who::You, 400, 0)],
            secs: crate::screens::dps::MIN_RATE_SECS - 1,
            ..Default::default()
        };
        assert_eq!(group_rate(&f), None);
        f.secs = crate::screens::dps::MIN_RATE_SECS;
        assert!(group_rate(&f).is_some());
    }

    /// DEFECT: a tab that selects nothing, which `nav.rs` refuses elsewhere and which teaches a
    /// reader that a control on this page might do nothing.
    ///
    /// The Deaths tab is the one exception and it is handled outside `panels`; every other tab must
    /// name at least one widget.
    #[test]
    fn every_tab_but_deaths_draws_something() {
        for (i, name) in TABS.iter().enumerate() {
            if *name == "Deaths" {
                assert!(panels(i).is_empty(), "Deaths is drawn by its own function");
                continue;
            }
            assert!(
                !panels(i).is_empty(),
                "the {name} tab draws no widget, so pressing it does nothing"
            );
        }
    }

    /// DEFECT: a THREAT tab, which is the one the mockup has and the log cannot support.
    ///
    /// WHAT MUTATION MAKES THIS RED: adding "Threat" to `TABS`. Nothing in `combat::Event` carries
    /// threat; a tab for it could only ever be empty, and an empty tab tells a reader the app
    /// looked and found none rather than that it cannot know.
    #[test]
    fn there_is_no_threat_tab() {
        assert!(
            !TABS.iter().any(|t| t.eq_ignore_ascii_case("threat")),
            "threat is not in the log: see the goal doc. An absent tab beats an empty one"
        );
    }

    /// DEFECT: A TEST NAMED FOR THIS PAGE'S RATE RULE THAT TESTED A CLOSURE IT WROTE ITSELF.
    ///
    /// What stood here built `|f| if f.secs >= 3 { Some(f.damage / f.secs) } else { None }` inside
    /// its own body and asserted things about that. It was green, it carried the rule's name, and
    /// it touched no line this app ships. Worse than useless: its arithmetic divided
    /// `FightRow::damage`, which is every point ANYBODY dealt including the pull, so the guard was
    /// shaped exactly like the defect the tile above it had already been fixed for. A test that
    /// reimplements the bug and passes is a certificate of the bug.
    ///
    /// SO THIS CALLS `group_rate`, which is the function the `Group dps` tile calls, on the
    /// capture's own first fight: 266 seconds, 16,526 dealt in total, 12,976 of it by the players.
    /// The honest tile reads 48. 62 is the mummy and the skeletons counted as part of the raid.
    ///
    /// WHAT MUTATION MAKES THIS RED: `group_rate` summing `f.damage` instead of `group_total`, or
    /// dividing by `f.secs` itself instead of calling `dps::dps` and so losing the floor.
    #[test]
    fn the_group_rate_is_the_group_over_the_clock_the_log_can_time() {
        let mut f = FightRow {
            secs: 266,
            ..FightRow::default()
        };
        f.fighters = vec![
            fighter(Who::You, 12_000, 0),
            fighter(Who::Named("Tanefilo".into()), 976, 0),
            /* The pull. A space in the name is what makes it not a player. */
            fighter(Who::Named("a lurking mummy".into()), 3_550, 0),
        ];
        f.damage = f.fighters.iter().map(|x| x.dealt).sum();

        assert_eq!(f.damage, 16_526, "the fight total does include the pull");
        assert_eq!(
            group_rate(&f),
            Some(48),
            "62 is this tile dividing the whole fight's damage, mummy included"
        );

        /* AND THE FLOOR, THROUGH THE SAME FUNCTION. The log stamps to the second, so a fight one
         * printed second old has no rate the file can express. */
        f.secs = crate::screens::dps::MIN_RATE_SECS - 1;
        assert_eq!(
            group_rate(&f),
            None,
            "damage inside one printed second is not a rate"
        );
        f.secs = crate::screens::dps::MIN_RATE_SECS;
        assert!(group_rate(&f).is_some());
    }

    /// DEFECT: THE `<` ARROW OFFERING A STEP THAT LANDS ON NOTHING.
    ///
    /// With a live fight and an EMPTY bootstrap history the arrow set `live = false` and `back = 0`,
    /// and `ui` then indexed an empty list and printed "That fight is no longer in the window that
    /// was read." about a fight that never existed. A reader standing in a pull pressed an arrow and
    /// got a blank page and a sentence about a read window. The other three no-op clicks (`>` while
    /// live, `>` on the newest history row with nothing live, `<` on the oldest) were quieter but
    /// the same thing: a control that answers nothing.
    ///
    /// WHAT MUTATION MAKES THIS RED: letting `older` return `Some(Stand::Back(0))` when `len` is 0,
    /// or answering `newer(Stand::Live, ..)` with `Some(Stand::Live)` as the old branch effectively
    /// did.
    #[test]
    fn an_arrow_with_nowhere_to_go_is_not_offered() {
        assert_eq!(
            older(Stand::Live, 0),
            None,
            "there is no older fight when the bootstrap read none: this is the blank page"
        );
        assert_eq!(newer(Stand::Live, true), None, "nothing is newer than live");
        assert_eq!(
            newer(Stand::Back(0), false),
            None,
            "the newest row that was read, with no live fight to step onto"
        );
        assert_eq!(newer(Stand::Back(0), true), Some(Stand::Live));

        /* A ONE ROW HISTORY IS STILL REACHABLE, which is why the step from Live is clamped rather
         * than fixed at two. */
        assert_eq!(older(Stand::Live, 1), Some(Stand::Back(0)));
        assert_eq!(older(Stand::Back(0), 1), None);

        /* AND THE ORDINARY CASE. From Live the step is two rows: `fights()` and `current_fight()`
         * are two folds of one file and the newest row is nearly always the live fight again. */
        assert_eq!(older(Stand::Live, 5), Some(Stand::Back(1)));
        assert_eq!(older(Stand::Back(1), 5), Some(Stand::Back(2)));
        assert_eq!(older(Stand::Back(4), 5), None);
        assert_eq!(newer(Stand::Back(4), true), Some(Stand::Back(3)));
    }

    /// The rule behind the test above, said once instead of case by case: EVERY STEP THE PAGE
    /// OFFERS LANDS ON SOMETHING THE PAGE CAN DRAW.
    ///
    /// `ui` reads `history[len - 1 - back]`, so a `Back(b)` destination is only real while `b` is
    /// under `len`, and a `Live` destination is only real while there is a live fight. A step onto
    /// where the reader already stands is not a destination either: that is the dead click.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `len == 0` guard in `older`, removing its
    /// `.min(len - 1)` clamp, or letting `newer` hand back `Stand::Live` without `has_live`.
    #[test]
    fn every_step_the_arrows_offer_lands_on_a_fight_that_exists() {
        for len in 0..6usize {
            for has_live in [false, true] {
                let mut here = vec![Stand::Live];
                here.extend((0..len).map(Stand::Back));
                for now in here {
                    if now == Stand::Live && !has_live {
                        continue;
                    }
                    for step in [older(now, len), newer(now, has_live)]
                        .into_iter()
                        .flatten()
                    {
                        match step {
                            Stand::Live => assert!(
                                has_live,
                                "from {now:?} over {len} fights the page offered the live fight \
                                 while there is not one"
                            ),
                            Stand::Back(b) => assert!(
                                b < len,
                                "from {now:?} the page offered history row {b} of {len}, which is \
                                 the blank page with the read window sentence on it"
                            ),
                        }
                        assert_ne!(
                            step, now,
                            "from {now:?} over {len} fights an arrow was offered that lands where \
                             the reader already stands"
                        );
                    }
                }
            }
        }
    }

    /* ------------------------------------------------- the notes, in a real frame -- */

    /// An `Ingest` pointed at a folder this test made, for the one `Cx` field that needs a real one.
    ///
    /// NOTHING HERE GOES NEAR `%APPDATA%/eql-grimoire`. `probe::logs_dir` makes a scratch folder
    /// under the system temp directory and refuses to hand two tests the same tag, `Settings` is
    /// constructed rather than loaded, `Settings::save` refuses outright under `cfg(test)`, and a
    /// fresh `Ingest` has no fight store until somebody hands it one.
    fn scratch_ingest(tag: &str) -> crate::ingest::Ingest {
        let dir = crate::fights::probe::logs_dir(tag);
        crate::ingest::Ingest::new(&crate::settings::Settings {
            log_dir: Some(dir.clone()),
            data_root: Some(dir),
            ..crate::settings::Settings::default()
        })
    }

    /// Run `body` inside ONE real egui frame with a real `Cx`.
    ///
    /// A REAL FRAME AND NOT A STUB, because what is being tested is a `TextEdit` and the order of
    /// events inside a single frame. Faking the `Ui` would test the fake.
    fn framed(
        ing: &mut crate::ingest::Ingest,
        settings: &mut crate::settings::Settings,
        body: impl FnOnce(&mut Ui, &mut Cx),
    ) {
        let ctx = egui::Context::default();
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut cx = Cx {
            data: None,
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
            ..Default::default()
        };
        /* `run_ui` takes an `FnMut` because egui can repeat a pass when a widget asks the frame to
         * be discarded. Nothing here asks, so the body runs exactly once and the `take` is what
         * lets it be an `FnOnce` that owns what it borrows. */
        let mut body = Some(body);
        let out = ctx.run_ui(input, |ui| {
            if let Some(run) = body.take() {
                run(ui, &mut cx);
            }
        });
        /* Headless: no renderer takes the font atlas, and epaint panics on a dropped delta unless
         * the drop is declared deliberate. */
        out.drop_without_applying_deltas();
    }

    /// DEFECT: THE `Players` TILE COUNTING PLAYERS THE TABS UNDER IT DO NOT RANK.
    ///
    /// The tile is `FightRow::players`, the roster's count, so over a solo fight with a stranger
    /// dealing damage it is the reader alone. A copy of the old `Who::player` filter in the tile
    /// counted the stranger too, and nothing drew the tile to notice.
    ///
    /// WHAT MUTATION MAKES THIS RED: `tiles` counting `f.fighters` filtered on `Who::player`.
    #[test]
    fn the_players_tile_counts_the_roster_it_sits_over() {
        let f = FightRow {
            secs: 30,
            group: Some(Vec::new()),
            fighters: vec![
                fighter(Who::You, 100, 0),
                fighter(Who::Named("Losumyda".into()), 200, 0),
            ],
            ..FightRow::default()
        };
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| tiles(ui, &f));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        /* IN PAINT ORDER, so the figure a tile draws comes straight before its label. */
        let mut said: Vec<String> = Vec::new();
        let mut stack: Vec<egui::Shape> = shapes.into_iter().rev().map(|c| c.shape).collect();
        while let Some(sh) = stack.pop() {
            match sh {
                egui::Shape::Vec(v) => stack.extend(v.into_iter().rev()),
                egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        let at = said
            .iter()
            .position(|s| s == "Players")
            .expect("the tiles draw a Players tile");
        assert_eq!(
            said.get(at.wrapping_sub(1)).map(String::as_str),
            Some("1"),
            "the Players tile over a solo fight counted a player the log proved was not with the \
             reader: {said:?}"
        );
    }

    fn fight_at(stamp: &str) -> FightRow {
        FightRow {
            start: stamp.to_owned(),
            ..FightRow::default()
        }
    }

    /// DEFECT: A NOTE TYPED AND NOT BLURRED, THROWN AWAY BY CLICKING THE NEXT FIGHT.
    ///
    /// `ui` draws the picker and then the notes, in that order, and it must: the picker decides
    /// which fight the frame draws. So the frame in which a reader clicks `<` hands `notes` the NEW
    /// fight while `self.note` still holds what was typed about the old one. The old code saw the
    /// key change, refilled the buffer from settings on the spot, and the typed text was gone. The
    /// only writer was `lost_focus` on the field below, which by then belonged to the new fight, so
    /// nothing was ever written and nothing said so.
    ///
    /// THIS DRIVES `notes` ITSELF, twice in one frame, with the two fights the click puts either
    /// side of it. That is precisely the state transition the click produces and it does not depend
    /// on landing a synthetic pointer on a button rectangle.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the `self.flush_notes(cx)` call from `notes`, or
    /// moving it below the line that refills the buffer from settings. (That flush was four lines
    /// spelled out inline here when this test was written; it is a function now because a second
    /// caller outside this page needed the same rule.)
    #[test]
    fn a_note_typed_and_not_blurred_survives_changing_fight() {
        const FIRST: &str = "Wed Jul 15 23:16:50 2026";
        const SECOND: &str = "Wed Jul 15 23:21:16 2026";
        let mut ing = scratch_ingest("analysis-note-flush");
        let mut settings = crate::settings::Settings::default();
        let mut screen = AnalysisScreen::default();

        framed(&mut ing, &mut settings, |ui, cx| {
            screen.notes(ui, cx, &fight_at(FIRST));
            /* The reader types. The text lives in the screen's buffer, not in settings, until the
             * field is left, and clicking an arrow is not leaving the field. */
            screen.note = "adds on the third pull".to_owned();
            screen.notes(ui, cx, &fight_at(SECOND));
        });

        assert_eq!(
            settings.fight_notes.get(FIRST).map(String::as_str),
            Some("adds on the third pull"),
            "the note was typed about the first fight and changing fight threw it away"
        );
        assert!(
            screen.note.is_empty(),
            "the second fight has no note of its own and must not inherit the first one's"
        );
        assert_eq!(screen.note_for.as_deref(), Some(SECOND));

        /* AND IT COMES BACK. The stamp is the key, so returning to the fight returns the note. */
        framed(&mut ing, &mut settings, |ui, cx| {
            screen.notes(ui, cx, &fight_at(FIRST));
        });
        assert_eq!(screen.note, "adds on the third pull");
    }

    /// DEFECT: A NOTE TYPED AND NOT BLURRED, THROWN AWAY BY LEAVING THE PAGE ALTOGETHER.
    ///
    /// The round that fixed the arrows fixed only the repointings that happen INSIDE this page.
    /// Both writers need this page to be drawing: `lost_focus` needs the field laid out again to
    /// see the focus go, and `notes` runs only from `ui`. So a reader who typed a note and stepped
    /// to Fights, Kills, Loot or Overlays lost it, because no frame ever drew the field again.
    ///
    /// THE PROOF THAT THE CALLER IS WIRED IS IN `screens::parser`, deliberately, because that is
    /// where the caller is: see `a_note_typed_on_analysis_is_written_when_the_reader_steps_to_
    /// another_view`. This one pins the RULE, which is that the flush writes the buffer out under
    /// the fight it belongs to and then points at nothing, and that calling it again costs nothing.
    ///
    /// WHAT MUTATION MAKES THIS RED: `flush_notes` reading `self.note_for` instead of TAKING it
    /// (the second flush would then write the buffer out a second time, and the buffer is empty by
    /// then, so the note would be REMOVED from settings by its own flush); or the body being
    /// emptied out.
    #[test]
    fn flushing_writes_the_buffer_out_under_its_own_fight_and_then_holds_nothing() {
        const STAMP: &str = "Wed Jul 15 23:16:50 2026";
        let mut ing = scratch_ingest("analysis-note-leave");
        let mut settings = crate::settings::Settings::default();
        let mut screen = AnalysisScreen::default();

        framed(&mut ing, &mut settings, |_ui, cx| {
            /* The reader typed and did not blur, which is every keystroke up to the moment he
             * clicks something else. */
            screen.typed_for_test(STAMP, "adds on the third pull");
            screen.flush_notes(cx);
        });
        assert_eq!(
            settings.fight_notes.get(STAMP).map(String::as_str),
            Some("adds on the third pull"),
            "leaving the page threw the note away"
        );
        assert_eq!(
            screen.note_for, None,
            "the buffer belongs to nothing now, so the next fight drawn refills it from settings \
             rather than inheriting this text"
        );
        assert!(screen.note.is_empty());

        /* AND AGAIN, WHICH IS WHAT A PER-FRAME CALLER DOES. A flush that wrote the (now empty)
         * buffer out a second time would clear the note it had just saved. */
        framed(&mut ing, &mut settings, |_ui, cx| {
            screen.flush_notes(cx);
            screen.flush_notes(cx);
        });
        assert_eq!(
            settings.fight_notes.get(STAMP).map(String::as_str),
            Some("adds on the third pull"),
            "a second flush erased the note the first one saved"
        );
    }

    /// A NOTE CLEARED LEAVES NOTHING BEHIND, and an unchanged one does not rewrite the file.
    ///
    /// The second half is not tidiness. `save_note` now has two callers, one of them "the reader
    /// changed fight", so without the change test a reader arrowing through twenty fights having
    /// typed nothing would write settings.json twenty times.
    ///
    /// WHAT MUTATION MAKES THIS RED: having `store_note` return `true` unconditionally, or
    /// inserting an empty string instead of removing the entry.
    #[test]
    fn a_note_is_stored_only_when_it_actually_moved() {
        let mut notes = BTreeMap::new();
        assert!(store_note(&mut notes, "a", "  mummy adds  "));
        assert_eq!(notes.get("a").map(String::as_str), Some("mummy adds"));
        assert!(
            !store_note(&mut notes, "a", "mummy adds\n"),
            "the same text again is not a change and must not cost a file write"
        );
        assert!(
            store_note(&mut notes, "a", "   "),
            "clearing the field is a change"
        );
        assert!(
            !notes.contains_key("a"),
            "a cleared note is removed, not stored as an empty string"
        );
        assert!(
            !store_note(&mut notes, "a", ""),
            "clearing what was already gone is not a change"
        );
    }

    #[test]
    fn the_clock_comes_out_of_the_stamp_by_shape() {
        assert_eq!(clock_of("Wed Jul 15 23:21:16 2026"), "23:21:16");
        assert_eq!(clock_of("not a stamp"), "not a stamp");
        assert_eq!(span(266), "4:26");
        assert_eq!(span(0), "0:00");
        assert_eq!(thousands(16_526), "16,526");
    }
}
