//! The detail panels: abilities, targets, elements, hit results and the timeline.
//!
//! # Every one of these is pure in (config, fight)
//!
//! Nothing here reads window state, a settings key or a clock. That is what lets the same call draw
//! a panel large on the Analysis page, small in an overlay over the game, and into a framed
//! rectangle as the builder's live preview: three callers, one renderer, and no third opinion about
//! what a crit rate is.
//!
//! # Every number traces to a field the engine measured
//!
//! None of these panels computes anything the log did not state. In particular:
//!
//!   * The hit results donut reads [`Outcomes`], which sits on the ATTACKER and names each reason.
//!     It is NOT `swings - landed`: measured over the reference capture, that subtraction is 110 for
//!     the reader and a fifth of it is the target parrying or dodging, so a Miss slice built that
//!     way credits the defender's skill to the attacker's aim.
//!   * The targets panel resolves a SLOT INDEX against the fight's own fighter list, never a name.
//!     The engine folds `a dry bone skeleton` (121 lines) and `A dry bone skeleton` (168) into one
//!     participant; a name key would split one mob into two rows with every percentage wrong.
//!   * The elements panel shows only what `DamageKind::Spell`'s school field named. Melee carries no
//!     element word at all, so it is the stated REMAINDER and never a silent "physical" slice.
//!   * The timeline's x axis is seconds from the fight's own start, because the log carries no zone
//!     offset and a duration is the only honest axis.
use crate::fights::{Ability, Family, FightRow, Fighter, Who};
use crate::overlay::{Detail, Subject, Timeline};
use crate::theme::*;
use egui::{RichText, Ui};

/// The widest a label column gets before it is clipped.
const NAME_W: f32 = 132.0;
/// The figure column, right aligned so a reader can run an eye down it.
const NUM_W: f32 = 62.0;
/// The share column.
const PCT_W: f32 = 42.0;
/// One row's height, which is also a bar's.
const ROW_H: f32 = 16.0;

/// THE ROOM A `+N more` LINE NEEDS, kept clear when [`overlay::Detail::fit`] is set.
///
/// THE SAME 18 POINTS `dashboards::MORE_H` RESERVES, because it is the same line on the same
/// page. A panel that filled its card to the last point would leave that line nowhere to go and
/// the reader would be told nothing was missing.
const MORE_ROOM: f32 = 18.0;

/// HOW MANY ROWS A PANEL DREW, AND HOW MANY THERE WERE TO DRAW.
///
/// RETURNED SO THE SURFACE CAN SAY WHAT WAS LEFT OUT. The panel cannot say it: those words are
/// a control that opens the page holding the rest, which is the dashboard's business and means
/// nothing over a game.
pub struct Drew {
    /// Rows painted.
    pub shown: usize,
    /// Rows there were to paint, after the config's own cap.
    pub total: usize,
}

impl Drew {
    /// Rows there was no room for.
    pub fn hidden(&self) -> usize {
        self.total.saturating_sub(self.shown)
    }
}

/// IS THERE ROOM FOR ANOTHER ROW AND FOR THE LINE THAT SAYS WHAT DID NOT FIT?
///
/// ALWAYS TRUE WHEN THE PANEL IS NOT FITTING, so an overlay is untouched: it clips, on purpose.
fn room(ui: &Ui, d: &Detail) -> bool {
    !d.fit || ui.available_height() >= ROW_H + MORE_ROOM
}

/// WHICH FIGHTER A DETAIL PANEL IS ABOUT, or `None` when the fight has nobody to be about.
///
/// `Subject::You` RESOLVES THROUGH `Who::You` AND NOT A NAME. `Fights::with_owner` folds the
/// reader's character name into that variant before the aggregator sees a line, so this is the
/// aggregator's own answer to who he is rather than a string this screen guessed at.
///
/// `None` IS A REAL ANSWER. A fight the reader only watched has no `You` row at all, and a panel
/// that fell back to somebody else would put another person's numbers under a heading with his name
/// on it.
///
/// # `TopDealer` IS THE RANKING RULE THE TABLES USE, AND IT USED TO BE A THIRD ONE
///
/// It read `filter(who.player()).max_by_key(dealt)`, which differs from [`dps::ranked_dealers`] in
/// two ways, and both of them put a name on screen the log does not support.
///
/// NO FLOOR. `max_by_key` over an empty-handed group still returns somebody, because the largest
/// of several zeroes is a zero. The capture's third fight is the Qeynos guards killing the reader:
/// every player row in it dealt nothing, and this headed a Top dealer panel with one of their names
/// over a column of zeroes. `ranked_dealers` keeps `metric.of(f) > 0` for exactly this reason, and
/// [`no_subject`] already had the right words waiting for the case that could not happen.
///
/// NO TIE BREAK. `Iterator::max_by_key` returns the LAST maximum, so on a tie the winner was
/// whichever of the two the aggregator happened to enrol later. That order is not stable across a
/// live fold: `Fights::slot` pushes a participant the first time it sees them act, so two players
/// level on damage swap places the moment either one is seen again, and a heading with a person's
/// name in it flipped back and forth once a second while neither of them did anything. The tables
/// break ties on the name, and this now breaks them the same way, which is also why it is written
/// as a sort of the same shape rather than as a cleverer `max_by`.
fn subject(f: &FightRow, who: Subject) -> Option<&Fighter> {
    match who {
        Subject::You => f.fighters.iter().find(|x| x.who == Who::You),
        /* `FightRow::ours` AND NOT `Who::player`, because the heading has to name the row
         * `ranked_dealers` puts first, and that ranks the reader's group when the log knows it. */
        Subject::TopDealer => f
            .fighters
            .iter()
            .filter(|x| x.dealt > 0 && f.ours(&x.who))
            .max_by(|a, b| {
                /* `max_by` keeps the LAST of equals, so the name comparison is REVERSED: the
                 * later-is-greater rule then keeps the name that sorts first, which is the row
                 * `ranked_dealers` puts at the top. */
                a.dealt
                    .cmp(&b.dealt)
                    .then_with(|| b.who.text().cmp(a.who.text()))
            }),
    }
}

/// What a detail panel says when its subject is not in this fight.
///
/// `TopDealer` SAYS THE ROSTER'S WORDS, `dps::nobody`. [`subject`] picks the top dealer off the
/// roster, so over a fight whose group is known a player outside it may well have dealt damage,
/// and "Nobody dealt damage in this fight" is then a sentence about the log that the log
/// contradicts.
fn no_subject(ui: &mut Ui, f: &FightRow, who: Subject) {
    ui.label(RichText::new(no_subject_words(f, who)).color(TEXT_3));
}

/// [`no_subject`]'s words, apart from the paint so a test can read them.
fn no_subject_words(f: &FightRow, who: Subject) -> String {
    match who {
        Subject::You => String::from("You took no part in this fight."),
        Subject::TopDealer => crate::screens::dps::nobody(f, crate::overlay::Metric::Dealt),
    }
}

/// THE FIGHTERS A PER-SECOND CHART OF ONE FIGHT DRAWS A LINE FOR: the roster's, with a series,
/// most damage first, ties by name, at most `cap` of them.
///
/// ONE FUNCTION FOR BOTH CHARTS THAT DRAW ONE FIGHT, this panel's [`timeline`] and the dashboard's
/// one-fight timeline, and it asks `FightRow::ours`: a line on either is a row on the meter beside
/// it, and never a player the meter left off because the log proved he was not in the group. Each
/// chart held its own copy of this filter, and each copy could go back to `Who::player` with no
/// test noticing.
pub(crate) fn charted(f: &FightRow, cap: usize) -> Vec<&Fighter> {
    let mut who: Vec<&Fighter> = f
        .fighters
        .iter()
        .filter(|x| f.ours(&x.who) && !x.series.is_empty())
        .collect();
    who.sort_by(|a, b| {
        b.dealt
            .cmp(&a.dealt)
            .then_with(|| a.who.text().cmp(b.who.text()))
    });
    who.truncate(cap.max(1));
    who
}

/// A whole percent of `total`, rounded down, and never a division by zero.
fn share(part: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    part.saturating_mul(100) / total
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

/// The heading every detail panel wears, naming its subject so two panels about two people are
/// never confused for one panel about the group.
fn heading(ui: &mut Ui, what: &str, f: &Fighter) {
    ui.label(
        RichText::new(format!("{what} ({})", f.who.text()))
            .color(GOLD)
            .strong(),
    );
}

/// One label, one figure, one share, one bar. The shape every detail panel's rows take.
fn detail_row(ui: &mut Ui, label: &str, tint: egui::Color32, amount: u64, total: u64, top: u64) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        let (name, _) = ui.allocate_exact_size(egui::vec2(NAME_W, ROW_H), egui::Sense::hover());
        ui.painter().with_clip_rect(name).text(
            name.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            TEXT,
        );

        let (num, _) = ui.allocate_exact_size(egui::vec2(NUM_W, ROW_H), egui::Sense::hover());
        ui.painter().text(
            num.right_center(),
            egui::Align2::RIGHT_CENTER,
            thousands(amount),
            egui::FontId::monospace(12.0),
            TEXT,
        );

        let (pct, _) = ui.allocate_exact_size(egui::vec2(PCT_W, ROW_H), egui::Sense::hover());
        ui.painter().text(
            pct.right_center(),
            egui::Align2::RIGHT_CENTER,
            format!("{}%", share(amount, total)),
            egui::FontId::monospace(12.0),
            TEXT_2,
        );

        let rest = ui.available_width().max(4.0);
        let (track, _) = ui.allocate_exact_size(egui::vec2(rest, ROW_H), egui::Sense::hover());
        ui.painter().rect_filled(track, 2.0, PANEL_2);
        let w = if top == 0 {
            0.0
        } else {
            (track.width() as f64 * (amount as f64 / top as f64).clamp(0.0, 1.0)) as f32
        };
        if w > 0.0 {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(track.min, egui::vec2(w, track.height())),
                2.0,
                tint,
            );
        }
    });
}

/// ONE FIGHTER'S DAMAGE BY WHAT THEY USED. The mockup's Ability Breakdown.
///
/// THE CRIT COLUMN IS OF THIS ABILITY'S OWN HITS. A crit rate over the fighter's whole output would
/// be the same number on every row, which is not a breakdown of anything.
pub fn abilities(ui: &mut Ui, f: &FightRow, d: &Detail) -> Drew {
    let Some(me) = subject(f, d.who) else {
        no_subject(ui, f, d.who);
        return Drew { shown: 0, total: 0 };
    };
    if d.head {
        heading(ui, "Abilities", me);
    }
    if me.abilities.is_empty() {
        ui.label(RichText::new("Nothing the log gave a name to.").color(TEXT_3));
        return Drew { shown: 0, total: 0 };
    }

    let mut rows: Vec<&Ability> = me.abilities.iter().collect();
    rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.name.cmp(&b.name)));
    let total: u64 = rows.iter().map(|a| a.amount).sum();
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut shown = 0usize;
    for a in rows.iter().take(d.cap.max(1)) {
        if !room(ui, d) {
            break;
        }
        shown += 1;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (name, _) = ui.allocate_exact_size(egui::vec2(NAME_W, ROW_H), egui::Sense::hover());
            ui.painter().with_clip_rect(name).text(
                name.left_center(),
                egui::Align2::LEFT_CENTER,
                &a.name,
                egui::FontId::proportional(12.0),
                TEXT,
            );
            let (num, _) = ui.allocate_exact_size(egui::vec2(NUM_W, ROW_H), egui::Sense::hover());
            ui.painter().text(
                num.right_center(),
                egui::Align2::RIGHT_CENTER,
                thousands(a.amount),
                egui::FontId::monospace(12.0),
                TEXT,
            );
            let (pct, _) = ui.allocate_exact_size(egui::vec2(PCT_W, ROW_H), egui::Sense::hover());
            ui.painter().text(
                pct.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("{}%", share(a.amount, total)),
                egui::FontId::monospace(12.0),
                TEXT_2,
            );
            ui.label(
                RichText::new(format!("{} hits", a.hits))
                    .color(TEXT_3)
                    .monospace(),
            );
            /* THE CRIT RATE IS OMITTED WHERE IT CANNOT MEAN ANYTHING. A shield tick and a DoT tick
             * do not crit, so a `0%` beside one reads as a bad roll rather than as an inapplicable
             * column. */
            if a.family == crate::fights::Family::Melee {
                ui.label(
                    RichText::new(format!(
                        "{}% crit",
                        share(u64::from(a.crits), u64::from(a.hits))
                    ))
                    .color(TEXT_3)
                    .monospace(),
                );
            } else {
                ui.label(RichText::new(a.family.label()).color(TEXT_3));
            }
        });
    }
    Drew {
        shown,
        total: rows.len().min(d.cap.max(1)),
    }
}

/// ONE FIGHTER'S DAMAGE BY WHAT THEY HIT. The mockup's Targets panel.
pub fn targets(ui: &mut Ui, f: &FightRow, d: &Detail) -> Drew {
    let Some(me) = subject(f, d.who) else {
        no_subject(ui, f, d.who);
        return Drew { shown: 0, total: 0 };
    };
    if d.head {
        heading(ui, "Targets", me);
    }
    if me.targets.is_empty() {
        ui.label(RichText::new("Hit nothing.").color(TEXT_3));
        return Drew { shown: 0, total: 0 };
    }

    /* TIES BREAK ON THE RESOLVED NAME, AND THIS USED TO BE A BARE `sort_by_key`.
     *
     * Rust's sort is STABLE, so equal amounts kept the order of `me.targets`, which is the order
     * `Participant::tally_target` first saw each victim. During a live fold that order is not
     * fixed: a target is appended the first time it is hit, so two mobs level on damage swap rows
     * whenever either is hit again, and a table nobody is changing reorders itself under the
     * reader's eye. It is the same defect `subject` had for its heading, and it takes the same
     * rule: `dps::ranked_dealers` breaks ties on the name ascending, so this does too.
     *
     * ON THE RESOLVED NAME AND NOT ON THE SLOT, because the slot is an index into this fight's
     * participants and sorting by it would be sorting by arrival under another spelling. */
    let name_of = |slot: usize| {
        f.fighters
            .get(slot)
            .map_or("(no such fighter)", |x| x.who.text())
    };
    let mut rows = me.targets.clone();
    rows.sort_by(|a, b| {
        b.amount
            .cmp(&a.amount)
            .then_with(|| name_of(a.slot).cmp(name_of(b.slot)))
    });
    let total: u64 = rows.iter().map(|t| t.amount).sum();
    let top = rows.first().map_or(0, |t| t.amount);

    ui.spacing_mut().item_spacing.y = 2.0;
    let mut shown = 0usize;
    for t in rows.iter().take(d.cap.max(1)) {
        if !room(ui, d) {
            break;
        }
        shown += 1;
        /* THE SLOT RESOLVES AGAINST THIS FIGHT'S OWN LIST. A slot from another fight, or an index
         * past the end, is a bug rather than a row: named as such instead of drawn as a blank. */
        detail_row(ui, name_of(t.slot), GOLD_DIM, t.amount, total, top);
    }
    Drew {
        shown,
        total: rows.len().min(d.cap.max(1)),
    }
}

/// ONE FIGHTER'S SPELL DAMAGE BY ELEMENT.
///
/// MELEE IS THE STATED REMAINDER AND NOT A SLICE. The log gives a melee line no element word, so
/// bucketing it as "physical" would be this app adding a fact the file does not carry.
pub fn elements(ui: &mut Ui, f: &FightRow, d: &Detail) {
    let Some(me) = subject(f, d.who) else {
        return no_subject(ui, f, d.who);
    };
    if d.head {
        heading(ui, "Elements", me);
    }
    if me.schools.is_empty() {
        ui.label(RichText::new("Cast nothing that named an element.").color(TEXT_3));
        return;
    }

    let mut rows = me.schools.clone();
    rows.sort_by(|a, b| {
        b.amount
            .cmp(&a.amount)
            .then_with(|| a.school.cmp(&b.school))
    });
    let spelled: u64 = rows.iter().map(|s| s.amount).sum();
    let top = rows.first().map_or(0, |s| s.amount);

    ui.spacing_mut().item_spacing.y = 2.0;
    for s in rows.iter().take(d.cap.max(1)) {
        detail_row(ui, &s.school, WORKING, s.amount, spelled, top);
    }

    /* THE REST, NAMED, AND THE NAMING IS NOW A MEASUREMENT.
     *
     * IT USED TO END "which is melee and shields", AND THAT WAS A GUESS DRESSED AS A READING.
     * `Participant::tally_school` is called from ONE place and only under `DamageKind::Spell`, so
     * the remainder is everything that is not a direct spell: melee and shields, yes, and ALSO
     * every damage-over-time tick, which this app has a whole `NameKind::Dot` family for and which
     * is the largest part of some casters' output. A fire wizard's DoT ticks landing in a bucket
     * captioned "melee and shields" is the house rule broken in a sentence rather than in a
     * number, and it is the same defect: a fact on screen that the file does not carry.
     *
     * SO THE FAMILIES ARE COUNTED. `by_name` carries a `Family` per key, taken off the arm of
     * `DamageKind` the grammar matched, so what the remainder consists of is something the fold
     * already knows and this only has to ask. A family with nothing in it is not named.
     *
     * AND WHAT IS LEFT AFTER THE FAMILIES IS LEFT UNNAMED. `SelfInflicted` and `Environmental`
     * take the `None` arm in that same match and so appear in no family at all; falling down a
     * lift shaft is damage you dealt to yourself and it has no ability name to print. The sentence
     * says the number and stops rather than dividing it among families it was never in. */
    let rest = me.dealt.saturating_sub(spelled);
    if rest > 0 {
        let named: Vec<&'static str> = [Family::Melee, Family::Shield, Family::Dot]
            .into_iter()
            .filter(|fam| {
                me.abilities
                    .iter()
                    .any(|a| a.family == *fam && a.amount > 0)
            })
            .map(|fam| fam.label())
            .collect();
        let what = match named.len() {
            0 => String::new(),
            1 => format!(", which is {}", named[0]),
            _ => format!(
                ", which is {} and {}",
                named[..named.len() - 1].join(", "),
                named[named.len() - 1]
            ),
        };
        ui.label(
            RichText::new(format!(
                "{} more with no element named{what}.",
                thousands(rest)
            ))
            .color(TEXT_3),
        );
    }
}

/// ONE FIGHTER'S SWINGS, BY WHAT STOPPED THEM. The mockup's Hit Results.
///
/// EVERY SLICE IS DRAWN EVEN AT ZERO. A reader can tell "none of these happened" from "this app did
/// not look" only if the row is there; a donut that silently omits its empty slices is the same
/// shape whether the parser checked or not.
pub fn outcomes(ui: &mut Ui, f: &FightRow, d: &Detail) {
    let Some(me) = subject(f, d.who) else {
        return no_subject(ui, f, d.who);
    };
    if d.head {
        heading(ui, "Hit results", me);
    }
    if me.swings == 0 {
        ui.label(RichText::new("Threw no melee swings.").color(TEXT_3));
        return;
    }

    /* THE DENOMINATOR IS SWINGS, AND LANDED IS A SLICE LIKE ANY OTHER. `landed + stopped == swings`
     * is an identity the engine's own test pins, so this column adds to 100 and cannot be made to
     * by fudging. */
    let swings = u64::from(me.swings);
    ui.spacing_mut().item_spacing.y = 2.0;
    detail_row(ui, "landed", SETTLED, u64::from(me.landed), swings, swings);
    /* THE FIVE MEASURED OUTCOMES, THEN ANY HYPOTHESISED ONE THAT ACTUALLY FIRED. See
     * `Outcomes::hypothesised`: a permanent zero row for a line the grammar has never once seen is
     * the Immune-slice mistake under another word. */
    let rows: Vec<(&str, u32)> = me
        .outcomes
        .slices()
        .into_iter()
        .chain(me.outcomes.hypothesised())
        .collect();
    for (word, n) in rows {
        detail_row(ui, word, WRONG, u64::from(n), swings, swings);
    }

    ui.label(
        RichText::new(format!(
            "{} swings, {}% landed, {} of them critical",
            me.swings,
            share(u64::from(me.landed), swings),
            me.melee_crits
        ))
        .color(TEXT_2),
    );
}

/// DAMAGE OVER THE FIGHT, A LINE PER FIGHTER. The mockup's DPS Timeline.
///
/// # The x axis is seconds and the grain is one second, because that is all the file has
///
/// The log stamps to the second and nothing finer, and the engine accumulates per second for
/// exactly that reason. A smoother curve would be this app inventing intermediate values.
///
/// # A gap is drawn as a gap
///
/// A second in which a fighter dealt nothing has no entry, and the line drops to zero there rather
/// than being interpolated across. Joining two points a minute apart with a straight line would
/// draw sustained damage through a minute of nothing.
pub fn timeline(ui: &mut Ui, f: &FightRow, t: &Timeline) {
    /* THE ROSTER'S POPULATION, off `charted`, so a line on this chart is a row on the meter beside
     * it and never a player the meter left off because he was not in the group. */
    let who = charted(f, t.cap);

    if who.is_empty() {
        /* THE ROSTER'S WORDS: the lines above are the roster's, so is the claim. See `dps::nobody`. */
        ui.label(
            RichText::new(crate::screens::dps::nobody(
                f,
                crate::overlay::Metric::Dealt,
            ))
            .color(TEXT_3),
        );
        return;
    }

    let span = u32::try_from(f.secs.max(1)).unwrap_or(1);
    let peak = who
        .iter()
        .flat_map(|x| x.series.iter().map(|(_, a)| *a))
        .max()
        .unwrap_or(1)
        .max(1);

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(40.0), t.height.max(40.0)),
        egui::Sense::hover(),
    );
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 2.0, PANEL_2);

    let x_of = |sec: u32| rect.left() + rect.width() * (sec as f32 / span as f32).clamp(0.0, 1.0);
    let y_of =
        |amount: u64| rect.bottom() - rect.height() * (amount as f32 / peak as f32).clamp(0.0, 1.0);

    /* THE MARKS FIRST, so a line is never hidden behind a tick. */
    if t.marks {
        for m in &f.moments {
            let x = x_of(m.at);
            let tint = match m.what {
                crate::fights::Mark::Death { .. } => WRONG,
                crate::fights::Mark::Crit { .. } => GOLD,
                _ => RULE,
            };
            p.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, tint.gamma_multiply(0.5)),
            );
        }
    }

    for (i, x) in who.iter().enumerate() {
        /* THE TABLE'S OWN COLOUR FOR THIS RANK. See `dps::rank_tint`: this file used to keep a
         * duplicate of the four colours and wrap past the fourth, so a chart and the list
         * beside it gave the fifth person two different colours. */
        let tint = crate::screens::dps::rank_tint(i);
        /* ONE POINT PER SECOND OF THE FIGHT, zero where the fighter dealt nothing. Walking the
         * span rather than the series is what puts the gaps in: a polyline over only the seconds
         * that HAVE entries would join across a lull and draw damage that never happened. */
        let mut pts: Vec<egui::Pos2> = Vec::with_capacity(span as usize + 1);
        let mut at = 0usize;
        for sec in 0..=span {
            let amount = match x.series.get(at) {
                Some((s, a)) if *s == sec => {
                    at += 1;
                    *a
                }
                _ => 0,
            };
            pts.push(egui::pos2(x_of(sec), y_of(amount)));
        }
        p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, tint)));
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (i, x) in who.iter().enumerate() {
            let tint = crate::screens::dps::rank_tint(i);
            ui.label(RichText::new("\u{25cf}").color(tint));
            ui.label(RichText::new(x.who.text()).color(TEXT_2));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("peak {}/s", thousands(peak)))
                    .color(TEXT_3)
                    .monospace(),
            );
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{Family, Outcomes, SchoolShare, TargetShare};

    fn me(dealt: u64) -> Fighter {
        Fighter {
            who: Who::You,
            dealt,
            ..Fighter::default()
        }
    }

    /// DEFECT: a detail panel falling back to somebody else when its subject is absent.
    ///
    /// A fight the reader only watched has no `You` row. A panel that quietly showed the top dealer
    /// instead would put another person's numbers under a heading with his name on it, which is
    /// worse than an empty panel because it is plausible.
    #[test]
    fn a_detail_panel_about_a_missing_person_finds_nobody() {
        let f = FightRow {
            fighters: vec![Fighter {
                who: Who::Named("Poguhy".into()),
                dealt: 100,
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        assert!(subject(&f, Subject::You).is_none(), "there is no You here");
        assert!(subject(&f, Subject::TopDealer).is_some());
    }

    /// DEFECT: the top dealer being a mob.
    ///
    /// The thing being fought hits back hard enough to outrank half a group, and a panel headed
    /// "Abilities (a lurking mummy)" is not what anybody opened.
    #[test]
    fn the_top_dealer_is_a_player_and_never_the_pull() {
        let f = FightRow {
            fighters: vec![
                Fighter {
                    who: Who::Named("a lurking mummy".into()),
                    dealt: 9_999,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("Poguhy".into()),
                    dealt: 100,
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };
        assert_eq!(
            subject(&f, Subject::TopDealer).map(|x| x.who.text()),
            Some("Poguhy"),
            "the mob out-dealt everyone and is still not the subject"
        );
    }

    /// DEFECT: a target row resolving a slot against the wrong fight, or past the end.
    #[test]
    fn a_target_slot_that_names_nobody_says_so() {
        let f = FightRow {
            fighters: vec![Fighter {
                who: Who::You,
                dealt: 10,
                targets: vec![TargetShare {
                    slot: 7,
                    amount: 10,
                    hits: 1,
                }],
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        let name = f
            .fighters
            .get(f.fighters[0].targets[0].slot)
            .map_or("(no such fighter)", |x| x.who.text());
        assert_eq!(name, "(no such fighter)");
    }

    /// DEFECT: an element chart that silently buckets melee as "physical".
    ///
    /// The log gives a melee line no element word at all. The remainder is named instead, so a
    /// reader can see that the elements do not add up to the fighter's whole output and why.
    #[test]
    fn the_element_remainder_is_stated_and_not_absorbed() {
        let f = Fighter {
            dealt: 1_000,
            schools: vec![SchoolShare {
                school: "fire".into(),
                amount: 300,
                hits: 3,
            }],
            ..me(1_000)
        };
        let spelled: u64 = f.schools.iter().map(|s| s.amount).sum();
        assert_eq!(spelled, 300);
        assert_eq!(f.dealt - spelled, 700, "the remainder is real and is named");
    }

    /// DEFECT: a hit-results column that does not add to a hundred.
    ///
    /// `landed + stopped == swings` is an identity the engine pins, and the denominator here is
    /// swings, so the slices cannot be made to add up by fudging.
    #[test]
    fn every_swing_is_in_exactly_one_slice() {
        let f = Fighter {
            swings: 100,
            landed: 60,
            outcomes: Outcomes {
                missed: 30,
                parried: 6,
                dodged: 4,
                ..Outcomes::default()
            },
            ..me(0)
        };
        assert_eq!(f.landed + f.outcomes.total(), f.swings);
        let pct: u64 = std::iter::once(share(u64::from(f.landed), 100))
            .chain(
                f.outcomes
                    .slices()
                    .iter()
                    .map(|(_, n)| share(u64::from(*n), 100)),
            )
            .sum();
        assert_eq!(pct, 100, "the slices add to {pct} percent");
    }

    /// DEFECT: a crit rate printed beside a damage shield or a DoT tick.
    ///
    /// Neither crits, so a `0%` there reads as a bad roll rather than as a column that does not
    /// apply. Only the melee family gets one.
    #[test]
    fn only_melee_carries_a_crit_rate() {
        for (family, wants_crit) in [
            (Family::Melee, true),
            (Family::Shield, false),
            (Family::Dot, false),
            (Family::Spell, false),
        ] {
            assert_eq!(
                family == Family::Melee,
                wants_crit,
                "{family:?} disagrees with the rule the renderer applies"
            );
            assert!(!family.label().is_empty());
        }
    }

    #[test]
    fn a_share_never_divides_by_zero() {
        assert_eq!(share(5, 0), 0);
        assert_eq!(share(1, 3), 33);
        assert_eq!(share(0, 10), 0);
        assert_eq!(thousands(16_526), "16,526");
    }

    /// DEFECT: A `Top dealer` PANEL NAMING SOMEBODY WHO DEALT NOTHING.
    ///
    /// `max_by_key` over a group that all dealt zero still returns a row, because the largest of
    /// several zeroes is a zero. THE CAPTURE HAS THIS FIGHT: the Qeynos guards killing the reader,
    /// where every player row is a guard's victim and dealt nothing at all. The panel headed
    /// itself with one of their names and printed a column of zeroes under it, which reads as a
    /// measurement of that person rather than as the absence of one.
    ///
    /// AND THE RIGHT WORDS WERE ALREADY WRITTEN. `no_subject` says nobody dealt damage (in the
    /// roster's words now, `dps::nobody`), which was unreachable for `TopDealer` until this floor
    /// existed: the message described the behaviour the resolver did not have.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `x.dealt > 0` from `subject`.
    #[test]
    fn a_group_that_dealt_nothing_has_no_top_dealer() {
        let f = FightRow {
            fighters: vec![
                Fighter {
                    who: Who::Named("Poguhy".into()),
                    dealt: 0,
                    taken: 400,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::You,
                    dealt: 0,
                    taken: 900,
                    ..Fighter::default()
                },
                /* The guards dealt plenty. They are not players and never were candidates. */
                Fighter {
                    who: Who::Named("Guard Ullindin".into()),
                    dealt: 1_300,
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };
        assert!(
            subject(&f, Subject::TopDealer).is_none(),
            "a fight the group dealt nothing in named a top dealer anyway"
        );
        /* AND THE READER IS STILL THERE, so the You panel is not empty on the same fight: the
         * two subjects answer differently and the floor applies to one of them. */
        assert!(subject(&f, Subject::You).is_some());
    }

    /// DEFECT: THE HEADING FLIPPING BETWEEN TWO NAMES WHILE NEITHER PLAYER DID ANYTHING.
    ///
    /// `Iterator::max_by_key` returns the LAST maximum, so on a tie the winner was whichever of
    /// the two `Fights::slot` enrolled later. That order is not stable during a live fold: a
    /// participant is pushed the first time they are seen acting, so a fight where two people are
    /// level on damage re-decides the heading every time either one is seen again, and the panel
    /// title changed once a second with no number under it changing at all.
    ///
    /// THE RULE IS THE TABLES' RULE AND NOT A NEW ONE. `dps::ranked_dealers` breaks ties on the
    /// name, ascending, so the row it puts at the top is the one this heading has to name; the two
    /// disagreeing is a page whose heading and whose first row are about different people.
    ///
    /// WHAT MUTATION MAKES THIS RED: `max_by_key(|x| x.dealt)`, or comparing the names the same
    /// way round as the damage.
    #[test]
    fn a_tie_is_broken_the_way_the_tables_break_it_and_not_by_arrival() {
        let tied = |first: &str, second: &str| FightRow {
            fighters: vec![
                Fighter {
                    who: Who::Named(first.into()),
                    dealt: 500,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named(second.into()),
                    dealt: 500,
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };

        /* THE SAME ANSWER FROM BOTH ARRIVAL ORDERS. That is the whole property: enrolment order
         * is what used to decide it. */
        for f in [tied("Amarine", "Poguhy"), tied("Poguhy", "Amarine")] {
            assert_eq!(
                subject(&f, Subject::TopDealer).map(|x| x.who.text()),
                Some("Amarine"),
                "the tie went to whoever was enrolled last"
            );
        }

        /* AND IT IS THE ROW THE TABLE PUTS FIRST. Asked of the ranking function rather than
         * restated, so the two cannot drift. */
        let f = tied("Poguhy", "Amarine");
        assert_eq!(
            subject(&f, Subject::TopDealer).map(|x| x.who.text()),
            crate::screens::dps::ranked_dealers(&f, crate::overlay::Metric::Dealt)
                .first()
                .map(|x| x.who.text()),
            "the heading names one person and the table's top row names another"
        );
    }

    /// DEFECT: THE HEADING NAMING A STRANGER OVER A TABLE THE GROUP FILTER LEFT HIM OFF.
    ///
    /// The tie test above asks both rules over a fight whose group is not known, where `Who::player`
    /// and the roster agree. Over the capture's last fight (solo, `Losumyda` the only dealer) the
    /// heading would name Losumyda over a table reading that nobody on the roster dealt anything.
    /// So the parity is asked over all three answers, with a stranger out-dealing everyone.
    ///
    /// WHAT MUTATION MAKES THIS RED: `subject` filtering `TopDealer` on `Who::player`.
    #[test]
    fn the_top_dealer_heading_is_the_first_row_of_the_filtered_table() {
        let with = |group: Option<Vec<String>>, you: u64| FightRow {
            group,
            fighters: vec![
                Fighter {
                    who: Who::You,
                    dealt: you,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("Hert".into()),
                    dealt: 50,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("Losumyda".into()),
                    dealt: 900,
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };
        for (f, over) in [
            (with(Some(vec!["hert".into()]), 100), "a known group"),
            (with(Some(Vec::new()), 100), "a solo reader"),
            (with(Some(Vec::new()), 0), "a solo reader who dealt nothing"),
            (with(None, 100), "a group that is not known"),
        ] {
            assert_eq!(
                subject(&f, Subject::TopDealer).map(|x| x.who.text()),
                crate::screens::dps::ranked_dealers(&f, crate::overlay::Metric::Dealt)
                    .first()
                    .map(|x| x.who.text()),
                "over {over}, the heading names one person and the table's top row names another"
            );
        }
    }

    /// DEFECT: AN EMPTY TOP DEALER PANEL SAYING NOBODY DEALT DAMAGE OVER A FIGHT A STRANGER DEALT IN.
    ///
    /// `subject` picks the top dealer off the roster, so over a solo fight with only a stranger
    /// dealing damage the panel is empty, and `Nobody dealt damage in this fight.` is contradicted
    /// by the log. It says the roster's words.
    ///
    /// WHAT MUTATION MAKES THIS RED: `no_subject_words` answering the old fixed sentence.
    #[test]
    fn an_empty_top_dealer_panel_says_whose_roster_dealt_nothing() {
        let f = FightRow {
            group: Some(Vec::new()),
            fighters: vec![Fighter {
                who: Who::Named("Losumyda".into()),
                dealt: 900,
                ..Fighter::default()
            }],
            ..FightRow::default()
        };
        assert!(
            subject(&f, Subject::TopDealer).is_none(),
            "the panel is not empty"
        );
        assert_eq!(
            no_subject_words(&f, Subject::TopDealer),
            "You have not dealt any damage.",
            "the empty Top dealer panel over a solo fight said nobody dealt damage, and a stranger did"
        );
    }

    /// DEFECT: A CHART OF ONE FIGHT DRAWING A PLAYER THE METER BESIDE IT LEAVES OFF.
    ///
    /// `charted` is what both one-fight charts draw lines for. Over a solo fight with the reader's
    /// own pet and a stranger in it, the lines are the reader and the pet; over the same fight with
    /// the group not known, every player; a mob never.
    ///
    /// WHAT MUTATION MAKES THIS RED: `charted` filtering on `Who::player` again.
    #[test]
    fn a_chart_of_one_fight_draws_the_roster_and_not_the_players_beside_it() {
        let lined = |who: Who, dealt: u64| Fighter {
            who,
            dealt,
            series: vec![(0, dealt)],
            ..Fighter::default()
        };
        let f = FightRow {
            group: Some(Vec::new()),
            pets: vec!["Gabtik".into()],
            fighters: vec![
                lined(Who::You, 100),
                lined(Who::Named("Gabtik".into()), 60),
                lined(Who::Named("Losumyda".into()), 900),
                lined(Who::Named("a dry bone skeleton".into()), 40),
            ],
            ..FightRow::default()
        };
        let names = |f: &FightRow| -> Vec<String> {
            charted(f, 6)
                .iter()
                .map(|x| x.who.text().to_owned())
                .collect()
        };
        assert_eq!(
            names(&f),
            vec!["You", "Gabtik"],
            "a chart over a solo fight drew a player the log proved was not with the reader"
        );
        let everyone = FightRow {
            group: None,
            ..f.clone()
        };
        assert_eq!(
            names(&everyone),
            vec!["Losumyda", "You", "Gabtik"],
            "a chart over a fight whose group is not known left a player off"
        );
    }

    /* --------------------------------------------------- what these panels say -- */

    /// EVERY TEST ABOVE THIS LINE CHECKS ARITHMETIC, AND THE TWO WORST DEFECTS THIS FILE HAS HAD
    /// WERE NOT ARITHMETIC.
    ///
    /// The elements caption claimed the remainder was "melee and shields" while the fold puts
    /// every damage-over-time tick in it too, and the targets table reordered itself under a
    /// reader who was not touching it. Neither is visible from a function that returns a number:
    /// one is a SENTENCE and the other is an ORDER, and both only exist once something is drawn.
    /// `the_element_remainder_is_stated_and_not_absorbed` asserts `1000 - 300 == 700`, which was
    /// true the whole time the caption beside it was wrong.
    ///
    /// SO THIS DRAWS THE PANELS AND READS THE TEXT BACK. No `Cx` is needed: these are free
    /// functions over a `FightRow`, which is most of why they are worth having as free functions.
    fn drew(build: impl FnMut(&mut Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, build);
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        /* `Shape::Vec` NESTS, and a reader of the top level alone finds nothing and passes.
         *
         * IN PAINT ORDER, AND THAT IS NOT A DETAIL HERE. A stack drained with `pop` hands the
         * shapes back reversed. That is invisible to a test asking only whether some word appears,
         * and it is the whole answer to a test about which of two rows is on top: the first draft
         * of `a_tie_between_two_targets_is_drawn_the_same_way_round_whichever_was_hit_first` read
         * the table upside down and reported the tie break as broken while it was working. */
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
    }

    fn ability(name: &str, family: Family, amount: u64) -> Ability {
        Ability {
            name: name.to_owned(),
            family,
            amount,
            hits: 1,
            crits: 0,
        }
    }

    fn detail(cap: usize) -> Detail {
        Detail {
            fit: false,
            who: Subject::You,
            head: true,
            cap,
        }
    }

    /// DEFECT: THE ELEMENTS REMAINDER NAMING FAMILIES NOBODY COUNTED.
    ///
    /// The caption read "which is melee and shields" unconditionally. `tally_school` is called
    /// from one place in the fold and only under `DamageKind::Spell`, so the remainder is
    /// everything that is NOT a direct spell, and that includes every DoT tick. A caster whose
    /// output is mostly dots had them captioned as melee, which is the house rule broken in a
    /// sentence instead of in a number.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the literal back, or dropping `Family::Dot` from the
    /// list the caption walks.
    #[test]
    fn the_remainder_names_the_families_it_counted_and_no_others() {
        let dotty = FightRow {
            fighters: vec![Fighter {
                dealt: 1_000,
                schools: vec![SchoolShare {
                    school: "fire".into(),
                    amount: 300,
                    hits: 3,
                }],
                /* The other 700 is all ticks. There is no melee here at all. */
                abilities: vec![
                    ability("Burning Aura", Family::Spell, 300),
                    ability("Ignite Blood", Family::Dot, 700),
                ],
                ..me(1_000)
            }],
            ..FightRow::default()
        };
        let said = drew(|ui| elements(ui, &dotty, &detail(8))).concat();
        assert!(
            said.contains("700"),
            "the remainder itself went missing: {said:?}"
        );
        assert!(
            said.contains("dot"),
            "700 points of dot ticks were left out of the sentence naming them: {said:?}"
        );
        assert!(
            !said.contains("melee"),
            "the caption named melee in a fight with no melee line in it: {said:?}"
        );

        /* AND A FIGHTER WHO REALLY DID SWING GETS THE WORD. Otherwise the assertion above could
         * be passed by a caption that names nothing ever. */
        let swinging = FightRow {
            fighters: vec![Fighter {
                dealt: 1_000,
                schools: vec![SchoolShare {
                    school: "fire".into(),
                    amount: 300,
                    hits: 3,
                }],
                abilities: vec![
                    ability("Burning Aura", Family::Spell, 300),
                    ability("slash", Family::Melee, 700),
                ],
                ..me(1_000)
            }],
            ..FightRow::default()
        };
        let said = drew(|ui| elements(ui, &swinging, &detail(8))).concat();
        assert!(
            said.contains("melee"),
            "a swinger got no melee word: {said:?}"
        );
        assert!(
            !said.contains("dot"),
            "the caption named dots in a fight with no tick in it: {said:?}"
        );
    }

    /// DEFECT: THE TARGETS TABLE REORDERING ITSELF WHILE NOBODY TOUCHED IT.
    ///
    /// `sort_by_key(Reverse(amount))` is STABLE, so two targets level on damage kept the order
    /// `tally_target` first saw them in, and during a live fold that order changes: a victim is
    /// appended the first time it is hit. Two mobs on the same total swapped rows every time
    /// either was hit again.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `then_with` on the resolved name.
    #[test]
    fn a_tie_between_two_targets_is_drawn_the_same_way_round_whichever_was_hit_first() {
        let built = |first: usize, second: usize| FightRow {
            fighters: vec![
                Fighter {
                    dealt: 1_000,
                    targets: vec![
                        TargetShare {
                            slot: first,
                            amount: 500,
                            hits: 5,
                        },
                        TargetShare {
                            slot: second,
                            amount: 500,
                            hits: 5,
                        },
                    ],
                    ..me(1_000)
                },
                Fighter {
                    who: Who::Named("a lurking mummy".into()),
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("a dry bone skeleton".into()),
                    ..Fighter::default()
                },
            ],
            ..FightRow::default()
        };

        /* The mummy is slot 1 and the skeleton slot 2. Whichever was hit first, the row that
         * comes out on top is the one whose NAME sorts first, which is the skeleton. */
        let order = |f: &FightRow| -> Vec<String> {
            drew(|ui| {
                targets(ui, f, &detail(8));
            })
            .into_iter()
            .filter(|s| s.contains("skeleton") || s.contains("mummy"))
            .collect()
        };
        let a = order(&built(1, 2));
        let b = order(&built(2, 1));
        assert_eq!(a.len(), 2, "both targets should be drawn: {a:?}");
        assert_eq!(
            a, b,
            "the same two targets on the same two totals drew in two different orders"
        );
        assert!(
            a[0].contains("skeleton"),
            "the tie did not go to the name that sorts first: {a:?}"
        );
    }

    /// DEFECT: THE SAME PERSON IN TWO COLOURS, IN A CHART AND IN THE LIST BESIDE IT.
    ///
    /// This file kept its own copy of the four rank colours, byte for byte the same as
    /// `dps::RAMP`, and wrapped past the fourth with `RAMP[i % 4]` while the table fell through to
    /// `RAMP_REST`. So the fifth line on a timeline came back GOLD, the colour a reader has spent
    /// the whole page learning means "top row", next to a legend whose table row for that person
    /// was grey.
    ///
    /// AND THE DOC ON THE DUPLICATE CLAIMED THE OPPOSITE: "Shared with the ranked table so one
    /// person is the same colour in a chart and in the list beside it." True for four people.
    ///
    /// THE ASSERTION IS OVER THE TAIL AND NOT ONLY THE FOUR, because a test that checked ranks 0
    /// to 3 passes on both rules and would have caught nothing. Sixteen is arbitrary and past any
    /// real cap; what matters is that it is well beyond four.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving this file back a ramp of its own, or `rank_tint`
    /// wrapping instead of falling through.
    #[test]
    fn one_rank_is_one_colour_everywhere_it_is_drawn() {
        let tint = crate::screens::dps::rank_tint;

        /* The four are distinct, or a ramp is not a ramp. */
        let top: Vec<egui::Color32> = (0..4).map(tint).collect();
        let mut uniq = top.clone();
        uniq.sort_by_key(|c| c.to_array());
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "two of the first four ranks share a colour");

        /* AND NOTHING PAST THEM REUSES ONE. This is the half the old rule failed. */
        for i in 4..16 {
            assert!(
                !top.contains(&tint(i)),
                "rank {i} came back in the colour of a top-four row"
            );
        }

        /* The tail is ONE tint and not a second ramp: rows nobody is comparing look alike on
         * purpose, and that is the claim `RAMP_REST` makes. */
        assert_eq!(tint(4), tint(15), "the tail is not a single quiet tint");
    }
}
