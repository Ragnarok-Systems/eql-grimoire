//! Screen: DPS. Decision D11. The first of the combat overlays.
//!
//! A ranked table of WHO DID THE DAMAGE in the newest fight the engine has folded, drawn to sit in
//! an always-on-top window over a full screen game. It is the first thing in this app that reads
//! `grimoire_parse`'s per-participant numbers rather than a count of them.
//!
//! # It is a RATE now, and it is published behind a floor
//!
//! The owner asked for it in so many words: real time DPS, yours, zero out of combat. This screen
//! showed damage instead for one build, and the reason was real: `Fight::seconds()` floors a span
//! at one second because the log stamps to the second and up to thirty-two lines in the reference
//! capture share a single printed second, so `Fight::dps()` reports 40.0 for a fight that opened
//! and closed inside one of those.
//!
//! THE FLOOR IS WHAT MAKES THE RATE HONEST RATHER THAN THE ABSENCE OF THE RATE. A span printed as
//! N seconds is truly somewhere between N minus one and N plus one, so the rate carries about
//! `1/N` of relative error and nothing can remove it: the information is not in the file. Below
//! [`MIN_RATE_SECS`] that error is worse than the number is worth and the damage figure is shown
//! instead. Above it the error shrinks as the pull goes on, which is exactly the direction a live
//! meter needs. [`a_rate_is_only_published_when_the_log_can_support_it`] is the guard.
//!
//! # What counts as YOUR damage, and the one thing that cannot
//!
//! `grimoire_parse::combat::DamageKind` already separates the five the owner named, and
//! `Participant::dealt` already sums them per actor, so the headline is one division:
//!
//!   * MELEE, which is `Melee { verb }` and already covers ripostes and flurries: both are ordinary
//!     swings that connected, and the verb is kept as the log wrote it.
//!   * DIRECT SPELLS, `Spell { spell, resist }`, whose caster the log names.
//!   * DOTS, `Dot { spell }`, whose caster comes from the last `by` clause on the tick.
//!   * YOUR OWN DAMAGE SHIELD, `Shield { effect }`, credited to the WEARER.
//!
//! A DAMAGE SHIELD YOU CAST ON SOMEBODY ELSE CANNOT BE CREDITED TO YOU, and that is a fact about
//! the log rather than a decision taken here. The line reads `a skeleton is pierced by Tanefilo's
//! thorns`, and the possessive is the WEARER: the engine proved that from the capture (`Poguhy
//! begins casting Shield of Barbs` at 23:17:00, zero `pierced by Poguhy's thorns` before it and 61
//! after). Nothing on that line names who cast the shield. Nor can it be recovered from the cast:
//! `Event::CastStart` carries a caster and a spell and NO TARGET, because `You begin casting Shield
//! of Barbs.` names none. So the damage from a shield you put on the tank is counted as the tank's,
//! and inventing an attribution for it would be this app making up a number.
//!
//! # It follows the fight you are in, and it did not always
//!
//! It reads `Ingest::current_fight`, which is re-folded from the end of the log on every poll that
//! brought new lines. It used to read `Ingest::fights().last()`, and that list is written ONCE, by
//! the bootstrap scan, and never again while the app runs: `Ingest::tail` fed only the kill and
//! loot stream. So this overlay showed whichever fight happened to be last when the app started
//! and never changed for the whole session. Measured 2026-09-05 and reported by the owner in one
//! sentence: it should be showing each fight and updating when a new one starts.
//!
//! THE FIGHT IN PROGRESS IS A REAL ROW. `fold_text` closes the final fight with `Ended::EndOfLog`
//! because the text ran out, which from the end of a log that is still being written is exactly
//! what an open fight looks like. `Ingest::fight_is_live` tells the two apart, and the header shows
//! a mark rather than a word for it.
//!
//! # What is not here, so the window is not read as a promise
//!
//!   * NO THREAT. Nothing in `combat::Event` carries it and nothing in the log prints it.
//!   * NO ENCOUNTER HEALTH BAR. Damage dealt to a mob is real; the mob's maximum is not in the log,
//!     and a bar with an invented denominator is an invented number.
//!   * NO HEALING OR TANK VIEW YET. [`fights::Fighter`] carries `healed`, `received` and `taken`,
//!     so both are a projection of data this screen already has in hand, and both are their own
//!     window rather than a mode of this one.
//!   * NO DELTA AGAINST A PRIOR FIGHT, and this one is a refusal rather than a gap. See below.
//!
//! # The mock's delta column, and why this build does not ship it
//!
//! The owner's dashboard mock, which is not in this tree and whose grid is transcribed in
//! `screens::dashgrid`, gives every
//! roster row a `.delta` reading `▲ 8.4%` and the roster foot the line
//! `At playhead: +8.4% vs prior comparable`. Neither is drawn here or in `screens::dashboards`,
//! and the reason is not that the data is missing. It is that the CLAIM is missing.
//!
//! THE FIGHTS ARE THERE. `crate::store` keeps every finished fight on disk, per character and
//! server, with every participant's totals in it; `Ingest::adopt` writes them and `hp::read` walks
//! the whole history already. Reading a player's numbers out of an earlier night is a solved
//! problem in this crate.
//!
//! WHAT IS NOT THERE IS AN ENCOUNTER. Nothing in an EverQuest Legends log line says that one pull
//! is the same pull as another. A "fight" in this app is not an encounter the game named: it is a
//! run of combat with no `QUIET_SECONDS` gap in it, which is a rule this app applies to a clock. On
//! a raid night combat never goes quiet for thirty seconds, so an eight minute chain of pulls is
//! ONE fight, and `Fight::headline` is whatever took the most damage inside that window. Its own
//! doc calls itself "a label for a list, not a claim about the encounter". Two fights that share a
//! headline can be the golem alone and the golem plus twelve adds and a wipe. A percentage on a
//! player's row comparing those two is measuring what else was in the window, and printing it
//! beside his name says it is measuring him. That is this app inventing a number, in the worst
//! place for one: a per-person figure on a live stream.
//!
//! AND `AT PLAYHEAD` IS A SECOND CLAIM THIS BUILD CANNOT MAKE. The mock's delta is read at a point
//! in a replay; this window shows a fight that is still growing. Comparing an open pull's first
//! twenty seconds against a finished fight produces a number that moves because of the clock, not
//! because of the player, and it would be at its most wrong exactly when a reader is looking.
//!
//! WHAT WOULD SETTLE IT: something in the log that names an encounter, so two fights can be known
//! to be the same fight rather than assumed to be. An emote, a zone-wide announcement, an instance
//! id, anything the client prints on engage. Until then the honest comparison this store CAN
//! support is one fight against one named earlier fight a person picked himself, which is a
//! feature of a history page and not a column on a live meter.
use crate::fights::{FightRow, Fighter, Pulse, Who};
use crate::overlay::{Metric, Overlay, Ranked, Widget};
use crate::screens::parser::{no_fights_words, why_no_fights};
use crate::screens::Cx;
use crate::theme::*;
use egui::{Color32, RichText, Ui};
use std::time::Duration;

/// THE MARK THAT SAYS THIS FIGHT IS STILL GOING: a painted dot, the same one the Pill draws. A dot
/// and not the word "live", because the word costs four characters and says nothing the dot beside
/// a growing number does not.
///
/// IT WAS A CHARACTER, `U+25CF`, AND THE APP'S FONT HAS NO SUCH GLYPH, so every Meter and Coach drew
/// a hollow square where the dot belonged, and the owner called the square what it was. A shape the
/// painter draws needs no font.
fn live_dot(ui: &mut Ui, tint: egui::Color32) {
    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(dot.center(), 4.0, tint);
}

/// HOW LONG A FIGHT HAS TO HAVE RUN BEFORE A RATE IS PRINTED FOR IT.
///
/// The log stamps to the second, so a span printed as N seconds is truly between N minus one and N
/// plus one and the rate carries about `1/N` of error. At one second that is the engine's own
/// worst case, 40.0 dps for a fight that may have lasted an instant. At three it is about a third,
/// which is the point where the number starts telling a reader something he did not already know
/// from the bar, and it keeps improving for as long as the pull lasts.
///
/// THREE AND NOT TEN. This is a LIVE meter: a floor a reader spends most of a short pull under is
/// a meter that is blank when he looks at it, and the first seconds of a fight are when he looks.
pub(crate) const MIN_RATE_SECS: i64 = 3;

/// How wide the name column is. The bar takes what is left, so this is also what decides how much
/// bar there is to read at the 620 point default width.
const WHO_W: f32 = 116.0;

/// How wide the damage figure is. Fixed, so the numbers form a column a reader can compare down
/// rather than a ragged edge that moves as the values change.
const NUM_W: f32 = 54.0;

/// How wide the share is.
const PCT_W: f32 = 38.0;

/// The rank chip side. Drawn only when `Cols::rank` asks for it.
const RANK_W: f32 = 16.0;

/// The height of one row's bar.
const BAR_H: f32 = 16.0;

/// The column header row, shorter than a data row: it is a label and not a figure.
const HEAD_H: f32 = 12.0;

/// THE BAR COLOURS, BY RANK, AND NOT BY NAME.
///
/// A DPS meter needs its rows to be distinguishable at a glance from across a room, which one tint
/// at four brightnesses does not achieve. These four are the mockup's own ramp in this app's
/// palette.
///
/// BY RANK, BECAUSE RANK IS IN THE DATA AND A NAME'S COLOUR IS NOT. Hashing a character name to a
/// hue would be this app inventing a fact about a person, which is the rule `screens::chat` states
/// for exactly the same temptation: Twitch supplies a colour per speaker and YouTube does not, so
/// the YouTube side uses one tint rather than making them up. Rank is computed from `dealt`, it is
/// already the thing the row's position states, and colouring by it cannot disagree with the order.
const RAMP: [Color32; 4] = [
    Color32::from_rgb(0xD4, 0xA3, 0x3C),
    Color32::from_rgb(0xC7, 0x4A, 0x3C),
    Color32::from_rgb(0x5A, 0xA9, 0x6E),
    Color32::from_rgb(0x4A, 0x9E, 0xD8),
];

/// Everything past the fourth rank shares one quiet tint. Extending the ramp would mean inventing
/// six more colours to tell apart rows nobody is comparing.
const RAMP_REST: Color32 = Color32::from_rgb(0x5B, 0x51, 0x63);

/// THE COLOUR OF RANK `i`, AND THE ONE PLACE THAT DECIDES IT.
///
/// `screens::widgets` HAD ITS OWN COPY AND ITS OWN OVERFLOW RULE, which is how two surfaces
/// drawn side by side came to disagree about the same person. The four colours were duplicated
/// byte for byte, and past the fourth rank the timeline WRAPPED (`RAMP[i % 4]`, so the fifth
/// line came back gold) while the table beside it fell through to `RAMP_REST` (grey). A reader
/// who has learned that gold is the top row then finds a second gold line halfway down a chart
/// whose legend sits next to a grey row for the same name.
///
/// THE TIMELINE'S OWN DOC CLAIMED OTHERWISE IN AS MANY WORDS: "Shared with the ranked table so
/// one person is the same colour in a chart and in the list beside it." It was true for four
/// people and false for the fifth, which is the worst kind of true.
///
/// WRAPPING IS THE RULE THAT WENT, AND NOT THE OTHER WAY ROUND. A colour that repeats says two
/// rows are alike when the only thing they share is the arithmetic of a remainder; one quiet
/// tint for the tail says these are the rows nobody is comparing, which is what they are.
pub(crate) fn rank_tint(i: usize) -> Color32 {
    RAMP.get(i).copied().unwrap_or(RAMP_REST)
}

/// The DPS overlay.
///
/// NO STATE, AND THAT IS A GUARANTEE RATHER THAN AN OVERSIGHT. Everything drawn is read fresh from
/// `Cx::ingest` on the pass that draws it. A cached row here would be a third copy of the fight
/// (the engine's, the ingest's mirror, and this), able to sit on screen saying something the log no
/// longer says, and this window's whole job is to be glanced at and believed.
#[derive(Default)]
pub struct DpsScreen;

impl DpsScreen {
    /// DRAW ONE CONFIGURED OVERLAY. The window hands in what the owner built; nothing here is
    /// hardcoded to damage any more.
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, ov: &Overlay) {
        /* THE PUMP, exactly as `screens::parser::ui` does it and for the same reason. A tool
         * window's `ChildCx` builds its own `Ingest`, and a screen drawn only in that window is
         * the ONLY thing that will ever call `tail` on it. Rate limited to one stat a second
         * inside, so calling it every frame is free. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        /* THE LIVE FOLD AND NOT THE BOOTSTRAP LIST. See the module note: `fights()` is written
         * once, at launch, and never again. */
        let ig = &*cx.ingest;
        let Some(fight) = ig.current_fight() else {
            let why = why_no_fights(
                ig.scanning(),
                ig.log_dir().dir.is_some(),
                ig.active_log().is_some(),
            );
            ui.label(RichText::new(no_fights_words(why)).color(TEXT_2));
            return;
        };

        /* EVERY WIDGET THE OWNER PUT IN THIS OVERLAY, IN HIS ORDER. One arm today; the other
         * three of the four arrive with their renderers. */
        let pulse = ig.pulse();
        for w in &ov.panels() {
            /* WITH THE HISTORY, which only the coach reads: its usual is the reader's stored
             * fights against the same mob. */
            draw_widget_in(ui, fight, pulse, w, ig.history());
        }
    }
}

/// DRAW ONE WIDGET. The whole dispatch, in one place, so the overlay window, the Analysis page and
/// the builder preview cannot diverge about what an arm means.
pub fn draw_widget(ui: &mut Ui, fight: &FightRow, pulse: Pulse, w: &Widget) {
    draw(ui, fight, pulse, w, &[], false);
}

/// [`draw_widget`] WITH THE READER'S STORED FIGHTS, AND FOR THE READER ALONE: the overlay's path.
///
/// # THE READER AND HIS PETS, AND NOBODY ELSE
///
/// The always on top windows and the builder's preview of them draw through here, and the owner's
/// rule for them is his own damage and his pets', whatever the group. The defect was a Meter reading
/// `1. Nith` over a player fighting near him while his charmed pet did the killing. So a table or
/// meter drawn here ranks [`yours`], and the pages that draw the same widgets through
/// [`draw_widget`] still rank the roster until they are asked for the same.
///
/// # AND THE HISTORY
///
/// Only [`Widget::Coach`] reads it, and every surface that does not have one to hand (a dashboard
/// card over one stored fight, the Analysis page) draws exactly what it drew before. The coach says
/// nothing about a usual it was not given, rather than comparing against an empty one.
pub fn draw_widget_in(
    ui: &mut Ui,
    fight: &FightRow,
    pulse: Pulse,
    w: &Widget,
    history: &[FightRow],
) {
    draw(ui, fight, pulse, w, history, true);
}

/// THE DISPATCH FOR BOTH. `yours_only` is whether a table or meter ranks the reader and his pets
/// (an overlay) or the fight's roster (a page).
fn draw(
    ui: &mut Ui,
    fight: &FightRow,
    pulse: Pulse,
    w: &Widget,
    history: &[FightRow],
    yours_only: bool,
) {
    /* THE NARROW ANSWER FOR EVERYTHING BUT THE MARK. Only the dot has three things to say; every
     * figure on these widgets turns on the one question of whether blows are still landing. */
    match w {
        Widget::Ranked(r) => ranked_table(ui, fight, pulse, r, yours_only),
        Widget::Meter(r) => meter(ui, fight, pulse, r, yours_only),
        /* THE COUNT IS DROPPED HERE AND ASKED FOR DIRECTLY BY THE ONE SURFACE THAT WANTS IT.
         * `draw_widget` is the overlay's path and an overlay clips: see `Detail::fit`. */
        Widget::Abilities(d) => {
            crate::screens::widgets::abilities(ui, fight, d);
        }
        Widget::Targets(d) => {
            crate::screens::widgets::targets(ui, fight, d);
        }
        Widget::Elements(d) => crate::screens::widgets::elements(ui, fight, d),
        Widget::Outcomes(d) => crate::screens::widgets::outcomes(ui, fight, d),
        Widget::Timeline(t) => crate::screens::widgets::timeline(ui, fight, t),
        Widget::Coach(c) => coach(ui, fight, pulse, c, history),
        Widget::Pill(p) => pill(ui, fight, pulse, p),
    }
}

/// HOW MANY STORED FIGHTS AGAINST ONE MOB MAKE A USUAL.
///
/// One or two pulls is not a usual: a comparison against one pull says more about that pull than
/// about the reader, and a coach that told him he was 40% down on a number made of one fight would
/// be this app inventing a standard.
pub(crate) const USUAL_MIN: usize = 3;

/// THE HEIGHT OF THE COACH'S LINE, in points.
const SPARK_H: f32 = 34.0;

/// THE READER'S USUAL RATE AGAINST THIS FIGHT'S MOB, and how many stored fights it is made of.
///
/// # WHAT IS COMPARED WITH WHAT
///
/// A stored fight names its mob by its headline, and the live fight's headline is read by the same
/// rule, so like is compared with like. The usual is the mean of his rate over those fights.
///
/// THE FIGHT ON SCREEN IS LEFT OUT, by its start stamp. The live fight is stored the moment it
/// closes, and a usual that counted the fight being judged would be marking its own homework.
///
/// `None` UNDER [`USUAL_MIN`], and when the fight names no mob at all.
pub(crate) fn usual(history: &[FightRow], fight: &FightRow) -> Option<(u64, usize)> {
    let mob = fight.headline.as_deref()?;
    let rates: Vec<u64> = history
        .iter()
        .filter(|h| h.start != fight.start)
        .filter(|h| {
            h.headline
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(mob))
        })
        .filter_map(|h| {
            /* HIM AND HIS PETS, the same as the number it is compared with. */
            let mut side = h
                .fighters
                .iter()
                .filter(|g| g.who == Who::You || g.pet)
                .peekable();
            side.peek()?;
            dps(side.map(|g| g.dealt).sum(), h.secs)
        })
        .collect();
    if rates.len() < USUAL_MIN {
        return None;
    }
    let n = rates.len();
    Some((rates.iter().sum::<u64>() / n as u64, n))
}

/// THE PERSONAL COACH. See [`crate::overlay::Coach`].
fn coach(
    ui: &mut Ui,
    fight: &FightRow,
    pulse: Pulse,
    c: &crate::overlay::Coach,
    history: &[FightRow],
) {
    /* THE SAME HEADLINE RULES AS THE METER: zero out of combat, a total until the fight is old
     * enough to have a rate. `mine` is the one place they live, and it is the reader AND his pets. */
    let live = pulse.fighting();
    let me = mine(fight, live, &Ranked::default());
    let you = fight.fighters.iter().find(|f| f.who == Who::You);
    let pets: Vec<&Fighter> = fight.fighters.iter().filter(|f| f.pet).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        /* THE MARK SAYS WHICH OF THE THREE IT IS, AND IT IS ALWAYS DRAWN. It used to appear only
         * while green, so a finished pull had nothing beside it and a reader could not tell a
         * held encounter from a window that had stopped updating. */
        live_dot(ui, pulse.tint());
        ui.label(
            RichText::new(me.text())
                .color(me.tint())
                .size(28.0)
                .monospace(),
        );
        ui.label(RichText::new(me.unit()).color(TEXT_3));
        /* HIS PETS ARE A LINE AMONG HIS ABILITIES AND NOT A FIGURE UP HERE: see `coach_lines`. */
        let usual_words = match (c.usual, me, usual(history, fight)) {
            (true, Mine::Rate(now, _), Some((was, n))) if was > 0 => {
                let pct = (i128::from(now) - i128::from(was)) * 100 / i128::from(was);
                let (words, tint) = if pct >= 0 {
                    (format!("+{pct}% vs your usual"), SETTLED)
                } else {
                    (format!("{pct}% vs your usual"), TEXT_2)
                };
                let hover = format!(
                    "Your average over {n} stored fights against {}: {} dps.",
                    fight.headline.as_deref().unwrap_or_default(),
                    thousands(was)
                );
                Some((words, tint, hover))
            }
            _ => None,
        };
        if let Some((words, tint, hover)) = usual_words {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(words).color(tint).size(12.0))
                    .on_hover_text(hover);
            });
        }
    });

    /* A PET FIGHTING ALONE IS STILL HIS SIDE FIGHTING, so only neither is nothing. */
    if you.is_none() && pets.is_empty() {
        ui.label(RichText::new(empty_words(fight, Metric::Dealt, true)).color(TEXT_3));
        return;
    }

    /* HIS LAST `window` SECONDS, ONE POINT A SECOND, off `Fighter::series`, which holds a second only
     * when he dealt damage in it. A second he did nothing in is a zero, drawn as one. */
    if c.window > 0 {
        let end = u32::try_from(fight.secs.max(1)).unwrap_or(1);
        let start = end.saturating_sub(c.window);
        let mut per = vec![0u64; (end - start) as usize + 1];
        for x in you.into_iter().chain(pets.iter().copied()) {
            for (s, a) in &x.series {
                if (start..=end).contains(s) {
                    per[(s - start) as usize] = per[(s - start) as usize].saturating_add(*a);
                }
            }
        }
        let top = per.iter().copied().max().unwrap_or(0).max(1);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(40.0), SPARK_H),
            egui::Sense::hover(),
        );
        let p = ui.painter();
        p.rect_filled(rect, 2.0, PANEL_2);
        let last = (per.len() - 1).max(1) as f32;
        let pts: Vec<egui::Pos2> = per
            .iter()
            .enumerate()
            .map(|(i, v)| {
                egui::pos2(
                    rect.left() + rect.width() * i as f32 / last,
                    rect.bottom() - 2.0 - (rect.height() - 4.0) * (*v as f32 / top as f32),
                )
            })
            .collect();
        if pts.len() >= 2 {
            p.add(egui::Shape::line(
                pts,
                egui::Stroke::new(1.5, crate::theme::GOLD),
            ));
        }
    }

    /* HIS OWN SWINGS, NOT HIS PETS': a charmed mob's kicks are not his to improve. A reader who
     * cast every point of his damage has no swings, and the landed figure says so rather than
     * claiming nought per cent. */
    if let Some(you) = you {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let landed = if you.swings > 0 {
                format!("{}%", u64::from(you.landed) * 100 / u64::from(you.swings))
            } else {
                "no swings".to_owned()
            };
            coach_stat(ui, "Landed", landed);
            coach_stat(ui, "Melee crits", thousands(u64::from(you.melee_crits)));
            coach_stat(ui, "Parried", thousands(u64::from(you.outcomes.parried)));
        });
    }

    /* HIS BIGGEST ABILITIES, WITH HIS PETS AS ONE LINE AMONG THEM. See `coach_lines`. */
    let (lines, total) = coach_lines(you, &pets, c.abilities);
    if !lines.is_empty() {
        ui.add_space(4.0);
    }
    for (name, amount) in &lines {
        let pct = share(*amount, total);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(40.0), 16.0),
                egui::Sense::hover(),
            );
            let p = ui.painter().with_clip_rect(rect);
            let name_w = (rect.width() * 0.38).max(60.0);
            p.text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(11.5),
                TEXT,
            );
            let track = egui::Rect::from_min_max(
                egui::pos2(rect.left() + name_w, rect.center().y - 3.0),
                egui::pos2(rect.right() - 40.0, rect.center().y + 3.0),
            );
            p.rect_filled(track, 2.0, PANEL_2);
            let mut fill = track;
            fill.set_width(track.width() * pct as f32 / 100.0);
            p.rect_filled(fill, 2.0, crate::theme::GOLD);
            p.text(
                egui::pos2(rect.right(), rect.center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{pct}%"),
                egui::FontId::monospace(11.0),
                TEXT_3,
            );
        });
    }
}

/// THE COACH'S BOTTOM LINES: his `n` biggest abilities, and his pets as one line among them, with
/// the total every line's share is of.
///
/// # THE PET LINE IS ALWAYS THERE
///
/// The owner asked for it in those words. His pets' rate first went on the coach's right, beside
/// the usual, and he wanted it as a line here instead, every fight, and not only when the pets
/// happened to beat his third ability. So when his abilities fill all `n` lines the smallest gives
/// its place to the pets, and the lines are ordered by amount with the pet line in its place.
///
/// # THE SHARE IS OF EVERYTHING AND NOT OF THE LINES THAT FIT
///
/// Every ability of his and all of his pets' damage, so a share means the same whatever `n` is and
/// the pet's does not jump when a line is cut to make room for it.
fn coach_lines(you: Option<&Fighter>, pets: &[&Fighter], n: usize) -> (Vec<(String, u64)>, u64) {
    if n == 0 {
        return (Vec::new(), 0);
    }
    let mut lines: Vec<(String, u64)> = you
        .map(|y| {
            y.abilities
                .iter()
                .map(|a| (a.name.clone(), a.amount))
                .collect()
        })
        .unwrap_or_default();
    let mut total: u64 = lines.iter().map(|(_, a)| *a).sum();
    lines.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    lines.truncate(n);
    if !pets.is_empty() {
        let dealt: u64 = pets.iter().map(|x| x.dealt).sum();
        total += dealt;
        if lines.len() == n {
            lines.pop();
        }
        let label = if pets.len() == 1 { "Pet" } else { "Pets" };
        lines.push((label.to_owned(), dealt));
        lines.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }
    (lines, total.max(1))
}

/// ONE LABELLED FIGURE ON THE COACH'S STATS LINE.
fn coach_stat(ui: &mut Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(label).color(TEXT_3).size(11.0));
        ui.label(RichText::new(value).color(TEXT).size(13.0).monospace());
    });
}

/// WHERE THE READER STANDS AMONG HIMSELF AND HIS PETS: his place and how many there are. See
/// [`yours`]: the pill is an overlay, and another player's damage is not on it.
fn rank_of_you(fight: &FightRow) -> Option<(usize, usize)> {
    let ranked = yours(fight, Metric::Dealt);
    let at = ranked.iter().position(|f| f.who == Who::You)?;
    Some((at + 1, ranked.len()))
}

/// THE MINIMAL PILL. See [`crate::overlay::Pill`].
///
/// OUT OF COMBAT IT SAYS SO, AND WHAT THE LAST PULL WAS, dimmed: the rate is not zeroed into a
/// figure that looks current, and it is not left standing as if the fight were still going.
///
/// NO CAPSULE OF ITS OWN. Popped out, the window it sits in is the rounded box, and a capsule
/// drawn inside that box was a pill in a box. See `windows::chromeless`.
fn pill(ui: &mut Ui, fight: &FightRow, pulse: Pulse, p: &crate::overlay::Pill) {
    let live = pulse.fighting();
    let me = mine(fight, live, &Ranked::default());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        live_dot(ui, pulse.tint());
        let sep = |ui: &mut Ui| {
            ui.label(RichText::new("|").color(crate::theme::LINE));
        };
        if !live {
            let side: Vec<&Fighter> = fight
                .fighters
                .iter()
                .filter(|m| m.who == Who::You || m.pet)
                .collect();
            let last = (!side.is_empty())
                .then(|| side.iter().map(|m| m.dealt).sum::<u64>())
                .and_then(|d| dps(d, fight.secs));
            let mut words = String::from("Out of combat");
            if let Some(d) = last {
                words.push_str(&format!(" \u{b7} last pull {} dps", thousands(d)));
                if p.clock {
                    words.push_str(&format!(
                        " \u{b7} {}",
                        crate::screens::dashboards::clock(fight.secs)
                    ));
                }
            }
            ui.label(RichText::new(words).color(TEXT_3));
            return;
        }
        ui.label(
            RichText::new(me.text())
                .color(me.tint())
                .size(14.0)
                .monospace(),
        );
        ui.label(RichText::new(me.unit()).color(TEXT_3));
        if p.clock {
            sep(ui);
            ui.label(
                RichText::new(crate::screens::dashboards::clock(fight.secs))
                    .monospace()
                    .color(TEXT),
            );
        }
        if p.rank {
            /* NOT "#1 of 1": with nobody beside him there is no rank to state. */
            if let Some((k, n)) = rank_of_you(fight).filter(|(_, n)| *n > 1) {
                sep(ui);
                ui.label(RichText::new(format!("#{k} of {n}")).color(TEXT));
            }
        }
        if p.target {
            let name = fight
                .current_target()
                .map(|(n, _)| n)
                .or(fight.headline.as_deref());
            if let Some(name) = name {
                sep(ui);
                ui.label(RichText::new(name).color(crate::theme::GOLD));
            }
        }
    });
}

/// A TABLE OF PEOPLE RANKED BY ONE NUMBER, WHICH IS SIX OF THE OWNER'S ELEVEN MOCKUPS.
///
/// PURE IN (config, fight), AND THAT IS WHAT MAKES THE BUILDER'S PREVIEW FREE. Nothing here reads
/// window state or a settings key, so the builder can draw this into a framed rectangle beside the
/// controls and what it shows IS the overlay rather than a picture of one.
/// HOW TALL ONE METER ROW IS. Tighter than [`BAR_H`] plus its spacing, because the whole point
/// of the idiom is that a raid fits in a window a person is willing to leave over his game.
const METER_ROW: f32 = 17.0;

/// HOW FAR THE RANK COLOUR IS TAKEN DOWN TO MAKE THE FILL.
///
/// THE NAME IS WRITTEN ON THE FILL, so the fill has to be dark enough to read light text
/// over, and the ramp at full strength is not: `EEF1F5` on the rank one gold `D4A33C` is
/// about two to one, which is under every contrast floor there is. Taken down it stays
/// plainly four different colours (that is what the ramp is for) and the text on it reads.
/// The TABLE keeps the ramp at full strength, because nothing is written over its bar.
const FILL: f32 = 0.5;

/// THE PAD INSIDE A METER ROW, left and right, in points.
const METER_PAD: f32 = 6.0;

/// THE ROOM A DASHBOARD CARD'S `+N more` LINE NEEDS, kept clear when [`Ranked::fit`] is set.
const MORE_ROOM: f32 = 18.0;

/// IS THERE ROOM FOR ANOTHER ROW OF THIS HEIGHT, AND FOR THE LINE THAT SAYS WHAT DID NOT FIT?
///
/// ALWAYS TRUE WHEN THE TABLE IS NOT FITTING, so an overlay clips exactly as it always has.
fn room(ui: &Ui, r: &Ranked, row_h: f32) -> bool {
    !r.fit || ui.available_height() >= row_h + MORE_ROOM
}

/// A DAMAGE METER: ONE ROW PER FIGHTER, AND THE ROW IS THE BAR.
///
/// # THE SHAPE DETAILS!, RECOUNT AND SKADA SHARE
///
/// A tight stack of rows, each a track filled left to right to that person's share of the top
/// figure, the name written ON the fill at the left and the figure with its share at the right.
/// No column head, no rule between rows, no chip. It is built to be read at a glance from
/// across a room while something is trying to kill you, which is exactly what an always on top
/// overlay is for, and it is why every meter in the genre has settled on this one shape.
///
/// THE OWNER ASKED FOR IT IN THOSE WORDS: the overlay should be a DPS meter like Details, or
/// Recount, or Skada. `ranked_table` was a table with a bar in one of its columns, which is a
/// perfectly good ANALYSIS surface and is not what any of those three look like.
///
/// # WHAT THIS DOES NOT COPY FROM THEM, AND WHY
///
/// DETAILS! TINTS EACH ROW BY CLASS AND THIS TINTS BY RANK. A class here is an inference from
/// the spells the log happened to print (`class::Book`, and `Fighter::class` is `None` until
/// somebody has been seen casting something only one class can cast), so a meter coloured that
/// way would be half grey for the first half of a raid and would change colour under a reader
/// mid pull. Rank is known for every row from the first line. The NAME still carries the class
/// colour where one is proved, exactly as the table's does, so nothing is lost.
///
/// NO PER ROW SPELL BREAKDOWN. Details! expands a row into what that person cast; this app has
/// that reading and it is `Widget::Abilities`, which is a separate widget a person can put
/// under this one. An overlay row that opened a second surface would need a click, and a click
/// is the thing an overlay over a full screen game cannot ask for.
///
/// EVERY FIGURE IS THIS MODULE'S OWN, as the table's are: `ranked_dealers` picks and orders,
/// `figure` rates behind the same floor, `share` divides against the same group total. The two
/// shapes can be on screen at once over one fight and must not differ by a digit.
fn meter(ui: &mut Ui, fight: &FightRow, pulse: Pulse, r: &Ranked, yours_only: bool) {
    if r.headline {
        header(ui, fight, pulse, r, yours_only);
    }
    let ranked = rows_of(fight, r.metric, yours_only);
    if ranked.is_empty() {
        /* THE SAME FOUR WORDS THE TABLE SAYS, off the same `Metric::nothing`. */
        ui.label(RichText::new(empty_words(fight, r.metric, yours_only)).color(TEXT_3));
        return;
    }

    /* THE TOP FIGURE FILLS THE ROW AND THE GROUP'S SUM SETS THE SHARE, both for the reasons
     * written out at length over `ranked_table`'s own two. The cap is applied at the draw and
     * not to `ours`, so a share still means share of the GROUP rather than of the rows that
     * happened to fit. */
    let top = ranked.first().map_or(1, |f| r.metric.of(f)).max(1);
    let ours: u64 = ranked.iter().map(|f| r.metric.of(f)).sum();

    ui.spacing_mut().item_spacing.y = 1.0;
    let full = ui.available_width().max(40.0);
    for (i, f) in ranked.iter().take(r.cap.max(1)).enumerate() {
        if !room(ui, r, METER_ROW) {
            break;
        }
        let v = r.metric.of(f);
        let you = f.who == Who::You;
        let (rect, hit) = ui.allocate_exact_size(egui::vec2(full, METER_ROW), egui::Sense::hover());
        /* CLIPPED TO ITS OWN ROW, so a long name runs out under the figure rather than over
         * the row below it. */
        let p = ui.painter().with_clip_rect(rect);

        /* THE TRACK, THEN THE FILL. Without the track a short bar reads as an empty space and
         * the stack stops looking like a list of rows. */
        p.rect_filled(rect, 2.0, PANEL_2);
        let mut fill = rect;
        fill.set_width(bar_width(v, top, rect.width()));
        p.rect_filled(fill, 2.0, rank_tint(i).gamma_multiply(FILL));

        /* THE READER'S ROW IS EDGED AND NOT RECOLOURED, because the fill colour has to keep
         * meaning rank. The table picks him out in the same gold on his name. */
        if you {
            p.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, GOLD_HI),
                egui::StrokeKind::Inside,
            );
        }

        /* THE FIGURE FIRST, so its width is known and the name can be clipped short of it. */
        let num = format!("{}  {}%", figure(v, fight.secs, r), share(v, ours));
        let at = p.text(
            egui::pos2(rect.right() - METER_PAD, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            num,
            egui::FontId::monospace(11.0),
            TEXT,
        );

        /* THE NAME ON THE FILL, IN ITS CLASS COLOUR WHERE ONE IS PROVED. `class::tag_colour` is
         * read off the spell corpus and never hashed off the name: see `row`. */
        let name = if you {
            GOLD_HI
        } else {
            f.class
                .as_deref()
                .and_then(crate::class::tag_colour)
                .unwrap_or(TEXT)
        };
        let mut left = rect;
        left.set_right(at.left() - METER_PAD);
        p.with_clip_rect(left).text(
            egui::pos2(rect.left() + METER_PAD, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{}. {}", i + 1, f.who.text()),
            egui::FontId::proportional(11.5),
            name,
        );

        /* THE WHOLE ROW IS THE HOVER TARGET, which is what pays for the clipping above: a name
         * the row was too narrow to finish is still readable by pointing at it. */
        hit.on_hover_text(format!(
            "{}: {} {}, {}% of what {}.",
            f.who.text(),
            figure(v, fight.secs, r),
            head_unit(fight.secs, r),
            share(v, ours),
            if yours_only {
                "you and your pets did"
            } else {
                "this group did"
            }
        ));
    }

    /* THE ROSTER'S TOTAL, UNDER THE ROWS, the line the owner's meter mockup ends with. It is `ours`,
     * the same sum every share above divided by, so the rows and the total cannot disagree.
     *
     * AND SAID ONCE. On an overlay the headline's number is this same sum (`mine` is the reader and
     * his pets, and so are these rows), and the owner cut the line that repeated it. With the
     * headline off the foot is the only total and stays. A page's headline is the reader's own
     * figure and its foot the roster's, two different numbers, so a page keeps both. */
    if r.foot && !(yours_only && r.headline) && room(ui, r, METER_ROW) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(foot_words(fight, yours_only))
                    .color(TEXT_3)
                    .size(11.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        figure(ours, fight.secs, r),
                        head_unit(fight.secs, r)
                    ))
                    .monospace()
                    .size(11.0)
                    .color(GOLD_HI),
                );
            });
        });
    }
}

/// WHOSE TOTAL THE METER'S FOOT IS, in the words a foot has room for. See [`whose`] for the three
/// a page's roster can be, and [`yours`] for the overlay's.
fn foot_words(fight: &FightRow, yours_only: bool) -> &'static str {
    if yours_only {
        return if has_pet(fight) {
            "You and your pets"
        } else {
            "You"
        };
    }
    match fight.group.as_deref() {
        Some([]) => "You",
        Some(_) => "Your group",
        None => "Everyone in range",
    }
}

fn ranked_table(ui: &mut Ui, fight: &FightRow, pulse: Pulse, r: &Ranked, yours_only: bool) {
    if r.headline {
        header(ui, fight, pulse, r, yours_only);
    }
    let ranked = rows_of(fight, r.metric, yours_only);
    if r.cols.head {
        columns(ui, fight.secs, r);
    }

    /* A REAL STATE AND NOT AN ERROR, said in four words. A fight nothing named contributed to
     * is usually the reader falling down a hole, and `Fight::headline` answers `None` for it
     * too, so the header above has already said `an unnamed fight`. */
    if ranked.is_empty() {
        /* `Metric::nothing` AND NOT `Metric::unit`. See its doc: `unit` is a beside-a-number
         * abbreviation and this slot wants a verb phrase, so three of the four metrics used to
         * produce "Nobody named has any dmg." and "Nobody named has any healed by." */
        ui.label(RichText::new(empty_words(fight, r.metric, yours_only)).color(TEXT_3));
        return;
    }

    /* THE TOP DEALER AND NOT THE FIGHT'S TOTAL SETS THE FULL BAR. Scaling to the total makes
     * every bar short in a fight with many hands in it (a raid's best parse would fill a
     * fifth of the row), and the length a reader is comparing is one dealer against another,
     * which is what the share column states in numbers. */
    let top = ranked.first().map_or(1, |f| r.metric.of(f)).max(1);

    /* THE SHARE IS OF WHAT THE GROUP DID AND NOT OF WHAT THE FIGHT DID, and that changed when
     * the mobs left the chart. `FightRow::damage` is every point anybody dealt to anybody,
     * mobs included, so against it four players in the capture's first fight share 76 percent
     * between them and the missing quarter is the pull hitting back. A column that adds to 76
     * invites the reader to hunt for a row that was never going to be there.
     *
     * THE SAFETY THIS GIVES UP IS COVERED ELSEWHERE. Summing the rows on screen used to be
     * refused precisely so a mirror that LOST a fighter would show up as shares that no longer
     * reached 100. `fights.rs` asserts the whole mirror against the engine field by field on
     * every fold of the capture, at six quiet windows, so that defect is caught before it can
     * reach a screen. */
    let ours: u64 = ranked.iter().map(|f| r.metric.of(f)).sum();
    ui.spacing_mut().item_spacing.y = 2.0;
    /* THE CAP IS APPLIED HERE AND NOT IN `ranked_dealers`, so `ours` is the whole group's
     * total and a share still means share of the GROUP rather than share of the rows that
     * happened to fit. A capped table whose shares add to 100 is lying about the rest. */
    for (i, f) in ranked.iter().take(r.cap.max(1)).enumerate() {
        if !room(ui, r, BAR_H) {
            break;
        }
        row(ui, i, f, top, ours, fight.secs, r);
    }

    /* NO SCROLL AREA, WHICH IS WHY THE WINDOW CAN SHRINK TO ITS ROWS. A scroll area with
     * `auto_shrink([false, false])` claims all the height there is, so the window could never
     * report needing less than it had and would keep whatever height it was dragged to for
     * ever. A group is five and a raid is a few dozen; if a fold ever produces more rows than
     * a person wants on screen, the window is dragged shorter and the rows are clipped, which
     * is what an overlay does rather than growing a scrollbar over the game. */
}

/// THE COLUMN HEADER, WHICH IS WHERE THE UNIT LIVES. See `Cols::head` for what it is fixing.
///
/// THE WIDTHS ARE THE ROW'S OWN CONSTANTS AND THE ALIGNMENT IS THE ROW'S OWN, so a header that
/// stops lining up with its column is a compile-time change and not a drifting literal.
///
/// THE VALUE HEADER IS ASKED OF THE SAME FUNCTION THE ROW USES. `row` prints a rate only when
/// `r.rate` is set AND `dps` gives one, and it falls back to the total when the fight is too
/// young. That fallback is exactly the case a fixed `dps` header would lie about, so the header
/// is computed from the same call: for the first two seconds of a pull it reads `dmg`, and it
/// changes to `dps` on the same frame the numbers under it do.
/// WHAT THE VALUE COLUMN IS HEADED, ASKED OF THE SAME CLOCK THE ROW ASKS.
///
/// LIFTED OUT OF `columns` SO A TEST CAN DRIVE IT WITHOUT A `Ui`, which is the only reason the
/// defect it fixes went unnoticed: nothing could reach the string.
fn head_unit(secs: i64, r: &Ranked) -> &'static str {
    r.metric.unit(r.rate && dps(1, secs).is_some())
}

/// WHAT ONE ROW'S FIGURE READS, which is a rate when one was asked for AND the log can carry
/// one, and the total otherwise.
///
/// LIFTED OUT OF `row` WHEN THE METER ARRIVED. Two shapes draw the same fight and can be on
/// screen together over one pull; a meter that rounded or floored differently from the table
/// beside it would make this app disagree with itself about a number, which is the one thing
/// it must never do. `head_unit` above names the unit off the same clock for the same reason.
fn figure(v: u64, secs: i64, r: &Ranked) -> String {
    match (r.rate, dps(v, secs)) {
        (true, Some(n)) => short(n),
        /* Not asked for a rate, or the fight is too young for one: the total. */
        _ => short(v),
    }
}

fn columns(ui: &mut Ui, secs: i64, r: &Ranked) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if r.cols.rank {
            let (n, _) = ui.allocate_exact_size(egui::vec2(RANK_W, HEAD_H), egui::Sense::hover());
            ui.painter().text(
                n.center(),
                egui::Align2::CENTER_CENTER,
                "#",
                egui::FontId::proportional(9.5),
                TEXT_3,
            );
        }
        let (name, _) = ui.allocate_exact_size(egui::vec2(WHO_W, HEAD_H), egui::Sense::hover());
        ui.painter().text(
            name.left_center() + egui::vec2(2.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "NAME",
            egui::FontId::proportional(9.5),
            TEXT_3,
        );
        if r.cols.value {
            let (num, _) = ui.allocate_exact_size(egui::vec2(NUM_W, HEAD_H), egui::Sense::hover());
            ui.painter().text(
                num.right_center(),
                egui::Align2::RIGHT_CENTER,
                head_unit(secs, r).to_uppercase(),
                egui::FontId::proportional(9.5),
                TEXT_3,
            );
        }
        if r.cols.share {
            let (pct, _) = ui.allocate_exact_size(egui::vec2(PCT_W, HEAD_H), egui::Sense::hover());
            ui.painter().text(
                pct.right_center(),
                egui::Align2::RIGHT_CENTER,
                "%",
                egui::FontId::proportional(9.5),
                TEXT_3,
            );
        }
    });
}

/// ONE LINE: what is being fought, how long it has been going, and the whole fight's damage.
///
/// IT WAS FOUR LINES AND THE OWNER CUT THEM, and he was right about all four. A paragraph
/// explaining that the numbers are damage rather than a rate, a tally of who took part and dealt
/// nothing, an `ended ... at 11:40:30` stamp, and a clipped-tail warning: every one of them is
/// prose on a window that has to be read at a glance, mid-pull, over a game. The rate rule did not
/// go with the sentence, and `no_number_on_this_overlay_is_a_rate` still enforces it; what went is
/// the sentence about the rule.
///
/// THE STAMP WENT BECAUSE IT STOPPED BEING TRUE. It was there to say how STALE the table was, back
/// when this read a list that never updated. The fold follows the log now, so the honest thing is
/// a mark saying the fight is still going, and nothing at all when it is over.
fn header(ui: &mut Ui, f: &FightRow, pulse: Pulse, r: &Ranked, yours_only: bool) {
    let live = pulse.fighting();
    let me = mine(f, live, r);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        /* THE MARK SAYS WHICH OF THE THREE IT IS, AND IT IS ALWAYS DRAWN. It used to appear only
         * while green, so a finished pull had nothing beside it and a reader could not tell a
         * held encounter from a window that had stopped updating. */
        live_dot(ui, pulse.tint());

        /* THE READER'S OWN RATE, FIRST AND LARGEST, because it is the question this window is
         * open to answer and he is reading it out of the corner of his eye with both hands on
         * the keyboard. Everything else on this line is context for it. */
        ui.label(
            RichText::new(me.text())
                .color(me.tint())
                .strong()
                .size(19.0)
                .monospace(),
        );
        ui.label(RichText::new(me.unit()).color(TEXT_3));
        /* WHOSE ROWS THESE ARE, in the unit's own quiet tint and after it, ON A PAGE. See `whose`:
         * the rows under this line are the reader's group on one fight and every player in range on
         * the next, and nothing else on the window says which. NOT ON AN OVERLAY, whose rows are
         * always the reader and his pets: the owner cut the words that said so on every frame. */
        if !yours_only {
            ui.label(RichText::new(whose(f)).color(TEXT_3));
        }

        /* THE MOB'S NAME AND THE FIGHT CLOCK ARE BOTH GONE, and the owner cut both. He is looking
         * at the thing he is hitting; the overlay naming it back at him is a line of the window
         * spent telling him what he already knows. The clock is the same: a duration is not an
         * instruction and nothing on this window is decided by it. What is left is the number he
         * opened the window for, and the dot that says whether it is still moving. */
    });
}

/// One dealer's row: rank, name, bar, damage, share.
fn row(ui: &mut Ui, i: usize, f: &Fighter, top: u64, total: u64, secs: i64, r: &Ranked) {
    let tint = rank_tint(i);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        /* THE RANK CHIP IS OFF BY DEFAULT AND THE OWNER WAS RIGHT ABOUT THAT: it paints the row's
         * position as a numbered square, and the row's own POSITION already states it. It is a
         * flag rather than a deletion because two of his eleven mockups draw it, and a builder
         * that cannot reproduce his own mockups is not a builder. */
        if r.cols.rank {
            let (chip, _) = ui.allocate_exact_size(egui::vec2(RANK_W, BAR_H), egui::Sense::hover());
            ui.painter().rect_filled(chip, 2.0, tint);
            ui.painter().text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                egui::FontId::monospace(10.0),
                INK,
            );
        }

        /* THE READER IS THE ONE NAME PICKED OUT, which is the one thing about this table a person
         * is looking for before he reads any number on it. `Who::You` is the aggregator's own
         * answer to that (`Fights::with_owner` folds the character name into it), so this is not
         * a string comparison against a name a screen guessed at. */
        let you = f.who == Who::You;
        let (name, hit) = ui.allocate_exact_size(egui::vec2(WHO_W, BAR_H), egui::Sense::hover());
        /* THE NAME IS COLOURED BY CLASS WHERE THE LOG PROVED ONE.
         *
         * THE OWNER ASKED WHY ONLY FOUR PEOPLE HAVE COLOURED NAMES, and the answer was that the
         * only colour on a row came from `RAMP`, which is four deep with one grey tint after
         * it. Rank is also the wrong thing to colour a LIVE meter by: it moves mid-pull, so a
         * person the reader has learned to find by colour changes colour by overtaking
         * somebody.
         *
         * A CLASS IS READ AND NOT HASHED, which is what makes this allowed where colouring by
         * name would not be: see `class::colour`. The BAR still goes by rank, because a bar is
         * about magnitude and rank is what magnitude means here; the two never collide, because
         * one is a colour on text and the other a colour on a track.
         *
         * AND THE READER KEEPS HIS OWN GOLD. `Who::You` is the one row a person looks for
         * before he reads a number, and that is worth more than his class, which he knows. */
        let tint = if you {
            GOLD_HI
        } else {
            f.class
                .as_deref()
                .and_then(crate::class::tag_colour)
                .unwrap_or(TEXT)
        };
        let at = ui.painter().text(
            name.left_center() + egui::vec2(2.0, 0.0),
            egui::Align2::LEFT_CENTER,
            f.who.text(),
            egui::FontId::proportional(12.0),
            tint,
        );
        /* WHAT THIS PERSON HAS BEEN PROVED TO BE, after the name and in a quieter tint.
         *
         * READ AND NOT GUESSED. `class::Book` collects `X begins casting Y.` off the log and
         * `Spell::classes` is the wiki's own class table; a spell only one class can cast
         * proves that class outright. An EverQuest Legends character is a TRIO, so two proved
         * classes beside one name is the normal answer rather than a contradiction.
         *
         * ABSENT RATHER THAN GUESSED WHEN NOTHING IS PROVED. A reader who has not been seen
         * casting, or whose only spells several classes share, gets no tag at all: `Seen::tag`
         * returns `None` and this draws nothing. */
        if let Some(tag) = f.class.as_deref() {
            /* CLIPPED TO THE NAME COLUMN, so a long trio cannot push into the figures. */
            ui.painter().with_clip_rect(name).text(
                egui::pos2(at.right() + 6.0, name.center().y),
                egui::Align2::LEFT_CENTER,
                tag,
                egui::FontId::proportional(10.0),
                TEXT_3,
            );
            hit.on_hover_text(format!(
                "{tag}: read from the spells this character was seen casting. A class here is \
                 one only that class can cast; `+1` means at least one more that the log has \
                 not settled."
            ));
        }

        /* THE TWO FIGURES TAKE FIXED WIDTHS AND ARE PAINTED RIGHT ALIGNED, so they form a column
         * a reader can run an eye down. Laid out as labels they would each be as wide as their own
         * text, and the ragged edge moves every time a number gains a digit mid-fight. */
        /* EVERY ROW IS A RATE TOO, on the same floor as the headline: a meter whose top line is
         * dps and whose rows are raw damage invites the reader to compare two different units
         * down one column. `secs` is the FIGHT's span and every dealer in it shares that
         * divisor, so the ranking and the bars are unchanged by the division. */
        let v = r.metric.of(f);
        if r.cols.value {
            let (num, _) = ui.allocate_exact_size(egui::vec2(NUM_W, BAR_H), egui::Sense::hover());
            ui.painter().text(
                num.right_center(),
                egui::Align2::RIGHT_CENTER,
                figure(v, secs, r),
                egui::FontId::monospace(12.0),
                if you { GOLD_HI } else { TEXT },
            );
        }
        if r.cols.share {
            let (pct, _) = ui.allocate_exact_size(egui::vec2(PCT_W, BAR_H), egui::Sense::hover());
            ui.painter().text(
                pct.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("{}%", share(v, total)),
                egui::FontId::monospace(12.0),
                TEXT_2,
            );
        }

        /* THE BAR TAKES WHAT IS LEFT, so a narrow window loses bar and keeps every number. A
         * layout that gave the bar a fixed width would push the share off the right edge at the
         * sizes this window is actually dragged to. */
        if r.cols.bar {
            let rest = ui.available_width().max(4.0);
            let (track, _) = ui.allocate_exact_size(egui::vec2(rest, BAR_H), egui::Sense::hover());
            ui.painter().rect_filled(track, 2.0, PANEL_2);
            let w = bar_width(v, top, track.width());
            if w > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(track.min, egui::vec2(w, track.height())),
                    2.0,
                    tint,
                );
            }
        }
    });
}

/* ------------------------------------------------------------------ the rules -- */

/// EVERY PLAYER WHO DEALT DAMAGE, BIGGEST FIRST, TIES BROKEN BY NAME.
///
/// THE MOBS ARE NOT IN THIS CHART, which is the owner's rule and is obviously right once it is
/// said: a damage meter is a roster of the people fighting, and the thing being fought hits back
/// hard enough to outrank half of them. The capture's first fight is twenty-two participants of
/// which four are players, and `a lurking mummy` dealt more than two of those four.
///
/// THE TEST IS `FightRow::on_side`: `Who::player`, which is a space in the name and whose whole
/// argument is over there with the measurement, plus the reader's own pets, charmed ones included,
/// which a name cannot tell from the hostile mobs of their kind and the fold marks (`Fighter::pet`).
///
/// THE ZERO ROWS ARE DROPPED TOO. A participant is anything that was HIT, so a fight carries a row
/// for every mob in the pull whether it swung or not.
///
/// THE TIE-BREAK IS BY NAME AND IT MATTERS. `grimoire_parse` breaks its own headline tie by name
/// ascending, precisely so an answer cannot depend on the order the file happened to write things
/// in, and its test drives both orders to prove it. A sort here that left equal dealers in vector
/// order would put them in log order, which is stable per fold but changes between folds as the
/// tail moves, so two rows would swap places on screen while nothing about the fight changed.
///
/// # THE ROW AND NOT ITS FIGHTERS, SO NO CALLER CAN RANK WITHOUT THE GROUP
///
/// This took `&[Fighter]`, and a slice has no group in it. The filter is [`FightRow::ours`] now:
/// when the log proved who was in the reader's group, the people fighting beside him who were
/// not in it leave his meter, and when it did not, every player stays exactly as before. The
/// capture's fourth fight is the case: the reader is provably solo, and `Losumyda`, whose only
/// other lines are NewPlayers chat, was the one row on it. A caller handing in a bare slice
/// could not have known that, so a caller cannot hand one in.
pub(crate) fn ranked_dealers(fight: &FightRow, metric: Metric) -> Vec<&Fighter> {
    let mut out: Vec<&Fighter> = fight
        .fighters
        .iter()
        .filter(|f| metric.of(f) > 0 && fight.on_side(f))
        .collect();
    out.sort_by(|a, b| {
        metric
            .of(b)
            .cmp(&metric.of(a))
            .then_with(|| a.who.text().cmp(b.who.text()))
    });
    out
}

/// THE READER AND HIS OWN PETS, RANKED: the rows an overlay draws. See [`draw_widget_in`].
///
/// [`ranked_dealers`] FILTERED, so the order, the zero rows and the tie-break are its own and the
/// two cannot disagree about where the reader stands. A pet is [`Fighter::pet`]: charmed, or one that
/// answered him as `Master` in the fight's session.
pub(crate) fn yours(fight: &FightRow, metric: Metric) -> Vec<&Fighter> {
    ranked_dealers(fight, metric)
        .into_iter()
        .filter(|x| x.who == Who::You || x.pet)
        .collect()
}

/// THE ROWS A TABLE OR METER DRAWS: the reader's own on an overlay, the roster on a page.
fn rows_of(fight: &FightRow, metric: Metric, yours_only: bool) -> Vec<&Fighter> {
    if yours_only {
        yours(fight, metric)
    } else {
        ranked_dealers(fight, metric)
    }
}

/// DOES THIS FIGHT HOLD ONE OF THE READER'S PETS?
fn has_pet(fight: &FightRow) -> bool {
    fight.fighters.iter().any(|x| x.pet)
}

/// WHAT AN EMPTY TABLE OR METER SAYS: the reader did nothing, on an overlay, whoever else did;
/// [`nobody`] on a page.
fn empty_words(fight: &FightRow, metric: Metric, yours_only: bool) -> String {
    if yours_only {
        format!("You have not {}.", metric.nothing())
    } else {
        nobody(fight, metric)
    }
}

/// How long the bar is, in points, saturating at the track.
///
/// `top` IS THE DIVISOR AND IT CANNOT BE ZERO: [`ranked_dealers`] keeps only rows with `dealt > 0`
/// so the largest of them is at least 1, and the caller clamps anyway. Returned as an f32 rather
/// than painted here so the arithmetic is testable without a `Ui`.
fn bar_width(dealt: u64, top: u64, track: f32) -> f32 {
    if top == 0 || track <= 0.0 {
        return 0.0;
    }
    let frac = (dealt as f64 / top as f64).clamp(0.0, 1.0);
    (track as f64 * frac) as f32
}

/// WHAT THIS WINDOW IS FOR: damage per second, or `None` when the log cannot support one yet.
///
/// `None` IS A THIRD ANSWER AND NOT A ZERO. Zero means "dealt nothing", which is a true and useful
/// thing to print; a pull that is one second old has no rate the file can express, and printing 0
/// for it would say the reader is doing nothing at the exact moment he opened. See
/// [`MIN_RATE_SECS`].
pub(crate) fn dps(dealt: u64, secs: i64) -> Option<u64> {
    if secs < MIN_RATE_SECS {
        return None;
    }
    /* `secs` is at least MIN_RATE_SECS here and that is at least 1, so this cannot divide by
     * zero and cannot be reached with a negative. */
    Some(dealt / secs.max(1) as u64)
}

/// THE READER'S OWN RATE, WHICH IS THE ONE NUMBER THIS WINDOW EXISTS FOR.
///
/// ZERO OUT OF COMBAT, IN SO MANY WORDS, because that is what the owner asked for and because the
/// alternative is a meter that sits at last night's number all through a bank trip. `live` is
/// `Ingest::fight_is_live`, which is the engine's own quiet window applied to the fold's own
/// stamps.
///
/// `Who::You` AND NOT A NAME MATCH. `Fights::with_owner` folds the character name out of the log's
/// FILE NAME into `Actor::You` before the aggregator sees a line, so this is the aggregator's own
/// answer to who the reader is, and it is the same fold that stops him being two rows.
///
/// EVERYTHING HE DID IS ALREADY IN `dealt`: melee including ripostes and flurries, direct spells,
/// DoT ticks and his own damage shield. See the module note for the one kind that cannot be his.
///
/// AND HIS PETS' IS HIS. A charmed mob killing for him while this read 0 was the owner's defect: the
/// figure is every fighter the fold marked his pet (`Fighter::pet`) added to him, and never another
/// player's, whatever the group.
fn mine(fight: &FightRow, live: bool, r: &Ranked) -> Mine {
    if !live {
        return Mine::Idle(r.metric, r.rate);
    }
    let v: u64 = fight
        .fighters
        .iter()
        .filter(|f| f.who == Who::You || f.pet)
        .map(|f| r.metric.of(f))
        .sum();

    /* A TABLE THAT IS NOT A RATE PRINTS ITS TOTAL AND IS NEVER HELD BEHIND THE FLOOR. The floor
     * exists because a SPAN the log cannot express makes a RATE wrong; a total is a count the log
     * stated outright and is as true one second in as it is a minute in. */
    if !r.rate {
        return Mine::Total(v, r.metric);
    }
    match dps(v, fight.secs) {
        Some(n) => Mine::Rate(n, r.metric),
        None => Mine::TooSoon(v, r.metric),
    }
}

/// What the headline can say about the reader, which is three different things and not one number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mine {
    /// No fight is running. The owner's rule: out of combat this reads zero.
    Idle(Metric, bool),
    /// A fight is running but is younger than [`MIN_RATE_SECS`], so the total so far is shown
    /// instead of a rate the file cannot support.
    TooSoon(u64, Metric),
    /// The metric per second.
    Rate(u64, Metric),
    /// The metric, whole, because this table was not asked for a rate.
    Total(u64, Metric),
}

impl Mine {
    fn text(self) -> String {
        match self {
            Mine::Idle(..) => "0".to_owned(),
            Mine::TooSoon(d, _) | Mine::Rate(d, _) | Mine::Total(d, _) => thousands(d),
        }
    }

    /// The unit beside the number, so the three states are told apart without a sentence.
    fn unit(self) -> &'static str {
        match self {
            /* THE UNIT COMES OFF THE METRIC, so a healing overlay says `hps` and not `dps`. The
             * too-soon case deliberately shows the NON-rate unit: it is printing a total. */
            Mine::Idle(m, rate) => m.unit(rate),
            Mine::Rate(_, m) => m.unit(true),
            Mine::TooSoon(_, m) | Mine::Total(_, m) => m.unit(false),
        }
    }

    fn tint(self) -> egui::Color32 {
        match self {
            Mine::Idle(..) => TEXT_3,
            Mine::TooSoon(..) => TEXT_2,
            Mine::Rate(..) | Mine::Total(..) => GOLD_HI,
        }
    }
}

/// WHOSE ROWS A ROSTER OF THIS FIGHT DRAWS, in the few words a caption has room for.
///
/// # THE SAME LIST MEANS TWO DIFFERENT THINGS NOW, AND ONLY THIS SAYS WHICH
///
/// [`ranked_dealers`] asks `FightRow::ours`, so a meter over a fight whose group the log proved
/// lists the reader and that group, and a meter over the next fight, whose group it did not, lists
/// every player in range. Both look the same: names, bars, shares that add to a hundred. A reader
/// who cannot tell them apart reads a stranger's absence on one as a stranger doing nothing, or
/// a stranger's row on the other as a group member he did not know he had.
///
/// # THREE ANSWERS, BECAUSE `group` HAS THREE
///
///   * `Some(names)` is `your group`.
///   * `Some(vec![])` is `solo`. The reader is the whole roster and the log proved it; `your
///     group` over one row of him would claim a group that is not there.
///   * `None` is `everyone in range, group not known`, and it is the longest on purpose. It is the
///     state where the list is NOT filtered, and the words say both halves: who is shown, and why
///     it is not narrower. Never a blank, because a blank reads as the filtered case.
///
/// `in range` IS THE LOG'S OWN LIMIT: the client prints combat it is near enough to see, so every
/// player on an unfiltered list is someone within that distance and not everyone in the zone.
pub(crate) fn whose(fight: &FightRow) -> &'static str {
    match fight.group.as_deref() {
        None => "everyone in range, group not known",
        Some([]) => "solo",
        Some(_) => "your group",
    }
}

/// WHAT ONE ROW OF A LIST OF FIGHTS COUNTED, for the hover over that row.
///
/// # A COLUMN WHOSE POPULATION CHANGES FROM ROW TO ROW
///
/// The parser table's `group dmg` and `your deaths` and the dashboard fights list's death count
/// are each fight's own roster, so a fight whose group the log proved counts the reader, his pets
/// and that group, and the next fight, whose group it did not, counts every player in range. Every
/// row the store held before the group existed is the second kind. One heading sits over both, and
/// a heading cannot say which a row is; the row can, on hover, without redrawing the table.
pub(crate) fn row_population(fight: &FightRow) -> &'static str {
    match fight.group.as_deref() {
        None => "This fight's group figures count every player in range: its group was not known.",
        Some([]) => "This fight's group figures count you and your pets: the log proved you solo.",
        Some(_) => "This fight's group figures count you, your pets and your group.",
    }
}

/// WHAT AN EMPTY ROSTER SAYS, in words that claim only the rows [`ranked_dealers`] ranks.
///
/// # `Nobody has dealt any damage.` WAS TRUE UNTIL THE ROSTER COULD BE FILTERED
///
/// With every player on the meter, an empty meter meant no player had dealt any. With the group
/// known it means nobody ON THE ROSTER had, and the capture's last fight is the case: the removal at
/// 23:21:54 proved the reader solo, he struck nothing, and `Losumyda` dealt damage beside him. The
/// Live page printed `Nobody has dealt any damage.` over that fight, a sentence about the log that
/// the log contradicts. So the sentence names the population, off the same three answers `whose`
/// captions:
///
///   * `None`: `Nobody has ...`, as before, because every player is on the roster.
///   * `Some(vec![])`: `You have not ...`. Solo, so the reader is the whole roster.
///   * `Some(names)`: `Nobody in your group has ...`.
pub(crate) fn nobody(fight: &FightRow, metric: Metric) -> String {
    match fight.group.as_deref() {
        None => format!("Nobody has {}.", metric.nothing()),
        Some([]) => format!("You have not {}.", metric.nothing()),
        Some(_) => format!("Nobody in your group has {}.", metric.nothing()),
    }
}

/// A dealer's share of the fight's whole damage, as a whole percent.
///
/// THE DENOMINATOR IS WHAT THE GROUP DEALT, which is the sum of the rows on screen. It was the
/// FIGHT's own damage until the mobs left the chart, and against that total a full group shares
/// about three quarters between them, the rest being the pull hitting back. See the note at the
/// call site for what that gives up and where it is covered instead.
pub(crate) fn share(dealt: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    (dealt.saturating_mul(100)) / total
}

/// `16526` becomes `16.5k`. The bar carries the magnitude; this column is for telling two rows
/// apart, and four characters of it is what fits beside a bar in a 380 point window.
///
/// UNDER TEN THOUSAND IS PRINTED WHOLE, because `9.9k` throws away a digit a reader can still read
/// at that width and the whole point of the column is the comparison.
pub(crate) fn short(n: u64) -> String {
    if n < 10_000 {
        return n.to_string();
    }
    format!("{:.1}k", n as f64 / 1000.0)
}

/// `16526` becomes `16,526`. The header has room for the whole number and the header is the one
/// place the fight's total is stated exactly.
pub(crate) fn thousands(n: u64) -> String {
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

    fn f(who: Who, dealt: u64) -> Fighter {
        Fighter {
            who,
            dealt,
            taken: 0,
            healed: 0,
            received: 0,
            swings: 0,
            landed: 0,
            avoided: 0,
            kills: 0,
            deaths: 0,
            ..Default::default()
        }
    }

    fn named(n: &str, dealt: u64) -> Fighter {
        f(Who::Named(n.to_owned()), dealt)
    }

    /// One of the reader's own pets, charmed or summoned, as the fold marks it.
    fn a_pet(n: &str, dealt: u64) -> Fighter {
        Fighter {
            pet: true,
            ..named(n, dealt)
        }
    }

    /// The reader's own row. `Who::You` is what `Fights::with_owner` folds his character name
    /// into, so this is the shape the aggregator really produces and not a name this test invented.
    fn self_dealt(dealt: u64) -> Fighter {
        f(Who::You, dealt)
    }

    fn fight(fighters: Vec<Fighter>) -> FightRow {
        FightRow {
            start: "Wed Jul 15 23:16:50 2026".to_owned(),
            end: "Wed Jul 15 23:21:16 2026".to_owned(),
            secs: 1,
            damage: fighters.iter().map(|x| x.dealt).sum(),
            deaths: 0,
            lines: 0,
            ended: "the log stopped".to_owned(),
            headline: Some("a thunder spirit princess".to_owned()),
            fighters,
            cut: false,
            ..Default::default()
        }
    }

    /// EVERY STRING ONE WIDGET PAINTS OVER ONE FIGHT, with the history the coach reads.
    fn painted_in(w: Widget, row: &FightRow, live: bool, history: &[FightRow]) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(460.0, 400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            draw_widget_in(ui, row, Pulse::from_live(live), &w, history)
        });
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

    /// EVERY STRING ONE WIDGET PAINTS OVER ONE FIGHT DRAWN AS A PAGE DRAWS IT, through `draw_widget`.
    fn painted_on_page(w: Widget, row: &FightRow) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(460.0, 400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| draw_widget(ui, row, Pulse::Fighting, &w));
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

    /// A TEN SECOND FIGHT: the reader deals 900 (90 a second) beside Lebn's 300, against a named mob.
    fn coached() -> FightRow {
        let you = Fighter {
            who: Who::You,
            dealt: 900,
            swings: 10,
            landed: 8,
            melee_crits: 2,
            series: vec![(0, 100), (5, 200), (9, 600)],
            abilities: vec![
                crate::fights::Ability {
                    name: "slash".to_owned(),
                    family: crate::fights::Family::Melee,
                    amount: 600,
                    hits: 6,
                    crits: 2,
                },
                crate::fights::Ability {
                    name: "kick".to_owned(),
                    family: crate::fights::Family::Melee,
                    amount: 300,
                    hits: 2,
                    crits: 0,
                },
            ],
            ..Fighter::default()
        };
        let mut you = you;
        you.outcomes.parried = 1;
        let lebn = Fighter {
            who: Who::Named("Lebn".to_owned()),
            dealt: 300,
            ..Fighter::default()
        };
        FightRow {
            secs: 10,
            headline: Some("A will sapper".to_owned()),
            ..fight(vec![you, lebn])
        }
    }

    /// A STORED FIGHT AGAINST THE SAME MOB in which the reader dealt 70 a second.
    fn stored(start: &str) -> FightRow {
        FightRow {
            start: start.to_owned(),
            secs: 10,
            headline: Some("a will sapper".to_owned()),
            ..fight(vec![Fighter {
                who: Who::You,
                dealt: 700,
                ..Fighter::default()
            }])
        }
    }

    /// THE COACH SAYS WHAT THE READER DID, AND A USUAL ONLY WHEN THERE IS ONE.
    ///
    /// WHAT MUTATION MAKES THIS RED: `USUAL_MIN` lowered; `usual` counting the fight on screen; the
    /// coach dropping the landed rate, the parries or an ability.
    #[test]
    fn the_coach_says_what_the_reader_did_and_compares_only_with_a_real_usual() {
        let row = coached();
        let three: Vec<FightRow> = [
            "Mon Sep 07 16:00:00 2026",
            "Mon Sep 07 16:10:00 2026",
            "Mon Sep 07 16:20:00 2026",
        ]
        .iter()
        .map(|s| stored(s))
        .collect();
        let said = painted_in(
            Widget::Coach(crate::overlay::Coach::default()),
            &row,
            true,
            &three,
        );
        for want in [
            "90",
            "dps",
            "Landed",
            "80%",
            "Melee crits",
            "Parried",
            "slash",
            "kick",
            "66%",
            "33%",
            "+28% vs your usual",
        ] {
            assert!(
                said.iter().any(|s| s == want),
                "the coach did not say {want:?}: {said:?}"
            );
        }

        /* TWO STORED FIGHTS ARE NOT A USUAL. */
        let said = painted_in(
            Widget::Coach(crate::overlay::Coach::default()),
            &row,
            true,
            &three[..2],
        );
        assert!(
            !said.iter().any(|s| s.contains("usual")),
            "the coach compared the reader with a usual made of two fights: {said:?}"
        );

        /* AND THE FIGHT ON SCREEN IS NOT PART OF ITS OWN USUAL. */
        let mut own = three[..2].to_vec();
        own.push(FightRow {
            start: row.start.clone(),
            ..stored(&row.start)
        });
        let said = painted_in(
            Widget::Coach(crate::overlay::Coach::default()),
            &row,
            true,
            &own,
        );
        assert!(
            !said.iter().any(|s| s.contains("usual")),
            "the fight being judged was counted in the usual it was judged against: {said:?}"
        );
    }

    /// THE PILL: ONE LINE WHILE HE FIGHTS, AND SAYS IT IS OVER WHEN IT IS.
    ///
    /// His pet is on it and Lebn, a player beside him, is not: 900 of his and 500 of his pet's over ten
    /// seconds is 140.
    ///
    /// WHAT MUTATION MAKES THIS RED: the rank or the clock drawn when switched off; the rank drawn
    /// out of combat; the pill printing a current figure after the fight ended; the rank counting
    /// another player, or stated with nobody to rank against; the last pull leaving the pet out.
    #[test]
    fn the_pill_is_one_line_in_combat_and_says_so_out_of_it() {
        let mut row = coached();
        row.fighters.push(a_pet("a tormented dead", 500));
        let pill = crate::overlay::Pill::default();
        let said = painted_in(Widget::Pill(pill), &row, true, &[]);
        for want in ["140", "dps", "00:10", "#1 of 2", "A will sapper"] {
            assert!(
                said.iter().any(|s| s == want),
                "the pill did not say {want:?}: {said:?}"
            );
        }

        let bare = crate::overlay::Pill {
            clock: false,
            rank: false,
            target: false,
        };
        let said = painted_in(Widget::Pill(bare), &row, true, &[]);
        assert!(
            !said
                .iter()
                .any(|s| s == "00:10" || s == "#1 of 2" || s == "A will sapper"),
            "the pill drew something that was switched off: {said:?}"
        );

        let said = painted_in(Widget::Pill(pill), &row, false, &[]);
        assert!(
            said.iter()
                .any(|s| s == "Out of combat \u{b7} last pull 140 dps \u{b7} 00:10"),
            "out of combat the pill does not say so with the last pull: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s == "#1 of 2" || s == "140"),
            "out of combat the pill still reads as a fight in progress: {said:?}"
        );

        /* WITHOUT THE PET: Lebn is not on it, and alone the reader has no rank to state. */
        let said = painted_in(Widget::Pill(pill), &coached(), true, &[]);
        assert!(
            said.iter().any(|s| s == "90"),
            "another player's damage reached the reader's own figure: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s.starts_with("#1 of")),
            "the pill ranked the reader against another player, or against nobody: {said:?}"
        );
    }

    /// THE PILL DRAWS NO BOX OF ITS OWN: popped out, its window is the rounded box.
    ///
    /// Counted against an empty frame, so whatever the root paints on its own is not the pill's.
    ///
    /// WHAT MUTATION MAKES THIS RED: the pill filling or outlining a shape behind its line.
    #[test]
    fn the_pill_draws_no_box_inside_its_window() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let boxes = |draw: &dyn Fn(&mut Ui)| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui));
            let mut stack: Vec<egui::Shape> = std::mem::take(&mut out.shapes)
                .into_iter()
                .map(|c| c.shape)
                .collect();
            out.drop_without_applying_deltas();
            let mut n = 0;
            while let Some(sh) = stack.pop() {
                match sh {
                    egui::Shape::Vec(x) => stack.extend(x),
                    egui::Shape::Rect(_) => n += 1,
                    _ => {}
                }
            }
            n
        };
        let row = coached();
        let pill = Widget::Pill(crate::overlay::Pill::default());
        let empty = boxes(&|_| {});
        for live in [true, false] {
            assert_eq!(
                boxes(&|ui| draw_widget_in(ui, &row, Pulse::from_live(live), &pill, &[])),
                empty,
                "the pill drew a box of its own inside its window (live: {live})"
            );
        }
    }

    /// A METER SAYS ITS TOTAL ONCE: an overlay's headline is its total, so the foot under it goes.
    ///
    /// The owner, over his Meter: `the bottom line isnt needed`. On an overlay the rows are him and
    /// his pets and so is the headline's number, so the foot said the same number twice. With the
    /// headline off the foot is the only total and stays; a page's headline is his own figure and
    /// its foot the roster's, two numbers, so a page keeps both.
    ///
    /// 900 of his and 500 of his pet's over ten seconds is 140; with Lebn's 300 on a page's roster, 170.
    ///
    /// WHAT MUTATION MAKES THIS RED: the foot drawn under an overlay's headline; the foot dropped with
    /// the headline off, or drawn when switched off; a page losing its foot; the builder offering a
    /// total the headline already shows.
    #[test]
    fn a_meter_says_its_total_once() {
        let mut row = coached();
        row.fighters.push(a_pet("a tormented dead", 500));
        let head = Ranked::default();
        assert!(head.headline && head.foot, "the default meter moved");
        let said = painted_in(Widget::Meter(head.clone()), &row, true, &[]);
        assert!(
            said.iter().any(|s| s == "140"),
            "the overlay's headline is not his and his pet's 140: {said:?}"
        );
        assert!(
            !said
                .iter()
                .any(|s| s == "140 dps" || s == "You and your pets"),
            "the overlay repeated its headline in a foot: {said:?}"
        );

        let bare = Ranked {
            headline: false,
            ..head.clone()
        };
        let said = painted_in(Widget::Meter(bare.clone()), &row, true, &[]);
        assert!(
            said.iter().any(|s| s == "You and your pets") && said.iter().any(|s| s == "140 dps"),
            "with no headline the overlay dropped its only total: {said:?}"
        );
        let off = Ranked {
            foot: false,
            ..bare
        };
        let said = painted_in(Widget::Meter(off), &row, true, &[]);
        assert!(
            !said.iter().any(|s| s == "You and your pets"),
            "the foot was drawn when switched off"
        );

        let said = painted_on_page(Widget::Meter(head), &row);
        assert!(
            said.iter().any(|s| s == "Everyone in range") && said.iter().any(|s| s == "170 dps"),
            "a page lost the roster's total under its headline: {said:?}"
        );

        let builder = include_str!("parser.rs");
        let builder = &builder[..builder
            .find("mod tests {")
            .expect("the parser's test module")];
        assert!(
            builder.contains("add_enabled(!r.headline")
                && builder.contains("egui::Checkbox::new(&mut r.foot, \"total\")"),
            "the builder offers a total the headline already shows"
        );

        /* AND THE OVERLAY AND THE BUILDER'S PREVIEW BOTH HAND THE COACH ITS HISTORY. */
        let dps = include_str!("dps.rs"); /* THE CODE ABOVE THE TESTS ONLY: this test quotes the line it looks for, so a search of the whole file finds the quote and can never fail. */
        let dps = &dps[..dps.find("mod tests {").expect("the test module")];
        assert!(
            dps.contains("draw_widget_in(ui, fight, pulse, w, ig.history());"),
            "the overlay draws the coach with no history"
        );
        let parser = include_str!("parser.rs");
        assert!(
            parser.contains(
                "draw_widget_in(ui, f, crate::fights::Pulse::from_live(live), w, history);"
            ),
            "the builder preview draws the coach with no history"
        );
    }

    /// AN OVERLAY RANKS THE READER AND HIS OWN PETS AND NOBODY ELSE, WHATEVER THE GROUP; A PAGE STILL
    /// RANKS THE ROSTER.
    ///
    /// The owner, over a Meter reading `1. Nith` while Nith, a player near him, was its only row and his
    /// charmed pet did the killing: the always on top windows are his damage and his pets' and nobody
    /// else's. The pages draw the same widgets through `draw_widget` and still rank the roster.
    ///
    /// WHAT MUTATION MAKES THIS RED: `draw_widget_in` not asking for the reader's rows; `yours` dropping
    /// the pet or keeping a player; an overlay captioning its rows or its empty sentence naming a roster;
    /// `draw_widget` losing the roster or its caption.
    #[test]
    fn an_overlay_ranks_the_reader_and_his_pets_and_a_page_still_ranks_the_roster() {
        let fighters = vec![
            f(Who::You, 500),
            a_pet("a tormented dead", 300),
            named("Hert", 400),
            named("Nith", 200),
        ];
        let r = Ranked {
            headline: true,
            ..Ranked::default()
        };
        for group in [None, Some(Vec::new()), Some(vec!["Hert".to_owned()])] {
            let row = FightRow {
                group: group.clone(),
                secs: 10,
                ..fight(fighters.clone())
            };
            for shape in [Widget::Meter(r.clone()), Widget::Ranked(r.clone())] {
                let said = painted_in(shape, &row, true, &[]);
                assert!(
                    said.iter().any(|s| s == "You" || s.ends_with(" You")),
                    "{group:?}: the overlay lost the reader: {said:?}"
                );
                assert!(
                    said.iter().any(|s| s.ends_with("a tormented dead")),
                    "{group:?}: the overlay lost the reader's pet: {said:?}"
                );
                assert!(
                    !said
                        .iter()
                        .any(|s| s.ends_with("Hert") || s.ends_with("Nith")),
                    "{group:?}: the overlay ranked a player who is not the reader: {said:?}"
                );
                assert!(
                    !said.iter().any(|s| {
                        s == "you and your pets"
                            || s == "your group"
                            || s == "solo"
                            || s.starts_with("everyone in range")
                    }),
                    "{group:?}: the overlay captioned its rows, which the owner cut: {said:?}"
                );
            }
        }

        let known = FightRow {
            group: Some(vec!["Hert".to_owned()]),
            secs: 10,
            ..fight(fighters)
        };
        let said = painted_on_page(Widget::Meter(r.clone()), &known);
        assert!(
            said.iter().any(|s| s.ends_with("Hert")),
            "a page lost the reader's group from its roster: {said:?}"
        );
        assert!(
            said.iter().any(|s| s == "your group"),
            "a page lost the caption that says whose rows it drew: {said:?}"
        );

        /* AND AN EMPTY OVERLAY SAYS THE READER DID NOTHING, whoever else did. */
        let others = FightRow {
            secs: 10,
            ..fight(vec![named("Nith", 200)])
        };
        for shape in [Widget::Meter(r.clone()), Widget::Ranked(r)] {
            let said = painted_in(shape, &others, true, &[]);
            assert!(
                said.iter().any(|s| s == "You have not dealt any damage."),
                "an overlay over a fight only another player dealt damage in did not say the reader \
                 dealt none: {said:?}"
            );
        }
    }

    /// THE COACH'S BIG NUMBER IS THE READER AND HIS PETS, AND HIS PETS ARE ALWAYS A LINE AT ITS FOOT.
    ///
    /// The owner, with a charmed mob killing for him and the coach reading 0: his pets' damage is his.
    /// Their rate first went on the right beside the usual, and he asked for it as a line among the
    /// abilities instead, always.
    ///
    /// 600 of slash, 300 of kick and 500 of his pet's is 1,400: 42%, 21% and 35%.
    ///
    /// WHAT MUTATION MAKES THIS RED: `mine` leaving the pet out or counting another player; the pet
    /// line missing, or missing whenever his abilities fill the list; a pet line with no pet; a share
    /// taken over the lines that fit; the pets' rate back on the right; the usual leaving the pets out
    /// of his stored fights; a coach with only his pet fighting saying he did nothing.
    #[test]
    fn the_coach_counts_his_pets_in_his_number_and_always_lists_them() {
        let coach = |abilities: usize| {
            Widget::Coach(crate::overlay::Coach {
                abilities,
                ..crate::overlay::Coach::default()
            })
        };
        let mut row = coached();
        row.fighters.push(a_pet("a tormented dead", 500));
        let said = painted_in(coach(3), &row, true, &[]);
        assert!(
            said.iter().any(|s| s == "140"),
            "the coach's number is not his 900 and his pet's 500 over ten seconds: {said:?}"
        );
        for want in ["slash", "Pet", "kick", "42%", "35%", "21%"] {
            assert!(
                said.iter().any(|s| s == want),
                "the coach's lines are not slash 600, his pet 500 and kick 300 of 1,400, missing \
                 {want:?}: {said:?}"
            );
        }
        assert!(
            !said.iter().any(|s| s.starts_with("pet ")),
            "the pets' rate is still on the coach's right: {said:?}"
        );

        /* ALWAYS: two lines, two abilities of his to fill them, and the pet still takes one. */
        let said = painted_in(coach(2), &row, true, &[]);
        assert!(
            said.iter().any(|s| s == "Pet") && !said.iter().any(|s| s == "kick"),
            "with the list full of his abilities the pet line was left off: {said:?}"
        );
        assert!(
            said.iter().any(|s| s == "35%"),
            "the pet's share was taken over the lines that fit and not over everything: {said:?}"
        );

        let said = painted_in(coach(3), &coached(), true, &[]);
        assert!(
            !said.iter().any(|s| s == "Pet" || s == "Pets"),
            "the coach listed a pet over a fight with no pet in it: {said:?}"
        );

        /* THE USUAL IS HIM AND HIS PETS TOO: 700 of his and 100 of a pet's is 80 a second. */
        let with_pet = |start: &str| {
            let mut s = stored(start);
            s.fighters.push(a_pet("a tormented dead", 100));
            s
        };
        let three: Vec<FightRow> = [
            "Mon Sep 07 16:00:00 2026",
            "Mon Sep 07 16:10:00 2026",
            "Mon Sep 07 16:20:00 2026",
        ]
        .iter()
        .map(|s| with_pet(s))
        .collect();
        assert_eq!(
            usual(&three, &row),
            Some((80, 3)),
            "the usual left the pets out of the stored fights"
        );

        /* A PET FIGHTING ALONE IS STILL THE READER'S SIDE FIGHTING, and it is still its line. */
        let pet_only = FightRow {
            secs: 10,
            ..fight(vec![a_pet("a tormented dead", 500), named("Nith", 900)])
        };
        let said = painted_in(coach(3), &pet_only, true, &[]);
        assert!(
            said.iter().any(|s| s == "50") && said.iter().any(|s| s == "Pet"),
            "the coach over a pet fighting alone is not the pet's 50 a second on its line: {said:?}"
        );
        assert!(
            !said.iter().any(|s| s.contains("have not")),
            "a coach with his pet fighting said he did nothing: {said:?}"
        );
    }

    /// THE LIVE MARK IS A PAINTED DOT, ON THE METER AND THE COACH AS ON THE PILL, AND NOT A CHARACTER.
    ///
    /// It was `U+25CF` as text, and the app's font has no such glyph, so it drew a hollow square.
    ///
    /// WHAT MUTATION MAKES THIS RED: the mark back as a character; the dot drawn once the fight is over.
    #[test]
    fn the_live_mark_is_a_dot_and_not_a_character_the_font_lacks() {
        let dots_and_text = |w: &Widget, live: bool| -> (usize, Vec<String>) {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx);
            crate::theme::install(&ctx);
            ctx.run_ui(egui::RawInput::default(), |_| {})
                .drop_without_applying_deltas();
            let row = coached();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                draw_widget_in(ui, &row, Pulse::from_live(live), w, &[])
            });
            let mut stack: Vec<egui::Shape> = std::mem::take(&mut out.shapes)
                .into_iter()
                .map(|c| c.shape)
                .collect();
            out.drop_without_applying_deltas();
            let (mut dots, mut said) = (0, Vec::new());
            while let Some(sh) = stack.pop() {
                match sh {
                    egui::Shape::Vec(x) => stack.extend(x),
                    egui::Shape::Circle(_) => dots += 1,
                    egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            (dots, said)
        };
        for (name, w) in [
            ("meter", Widget::Meter(Ranked::default())),
            ("coach", Widget::Coach(crate::overlay::Coach::default())),
        ] {
            let (dots, said) = dots_and_text(&w, true);
            assert_eq!(dots, 1, "a live {name} drew no dot: {said:?}");
            assert!(
                !said.iter().any(|s| s.contains('\u{25cf}')),
                "the {name}'s live mark is still a character the font does not have: {said:?}"
            );
            /* AND THE MARK IS STILL THERE WHEN THE FIGHT IS OVER, IN A DIFFERENT COLOUR. It used to
             * vanish, which left a finished pull with nothing beside it: a reader could not tell
             * an encounter being held open from a window that had simply stopped updating. The
             * owner asked for three states and the dot is how all three are said. */
            let (dots, _) = dots_and_text(&w, false);
            assert_eq!(
                dots, 1,
                "the {name} drew no mark once the fight was over, so a reader cannot tell a held \n                 encounter from a dead window"
            );
        }
    }

    /// DEFECT: A RATE PRINTED FOR A FIGHT TOO SHORT FOR THE LOG TO EXPRESS ONE.
    ///
    /// This screen showed damage and no rate at all for one build, and the reason was sound:
    /// `Fight::seconds()` floors at one second because the log stamps to the second, and the
    /// engine's own test pins 40.0 dps for a fight that opened and closed inside a single printed
    /// second. The owner asked for real time DPS anyway, and he is right that a damage meter
    /// without a rate is not a damage meter; the answer is a floor, not an absence.
    ///
    /// WHAT MUTATION MAKES THIS RED: removing the `secs < MIN_RATE_SECS` guard from `dps`, which
    /// is the whole of what stops the engine's worst misreport reaching the biggest text on the
    /// window.
    #[test]
    fn a_rate_is_only_published_when_the_log_can_support_it() {
        /* THE ENGINE'S OWN WORST CASE, by its own test: a fight that opened and closed inside one
         * printed second, carrying 40 damage. `Fight::dps` answers 40.0 for it and this must
         * answer nothing at all. */
        assert_eq!(dps(40, 1), None);
        assert_eq!(dps(10_000, 2), None, "two seconds is still half an unknown");

        assert_eq!(
            dps(300, 3),
            Some(100),
            "three seconds is the floor and it publishes"
        );
        assert_eq!(dps(16_526, 266), Some(62));
        assert_eq!(
            dps(0, 60),
            Some(0),
            "dealing nothing for a minute is a real zero"
        );

        /* A NEGATIVE OR ABSURD SPAN CANNOT DIVIDE BY ZERO OR PANIC. `secs` comes from the engine
         * and is floored at one there, but this function is handed it rather than computing it. */
        assert_eq!(dps(100, 0), None);
        assert_eq!(dps(100, -5), None);
    }

    /// DEFECT: THE HEADLINE SHOWING LAST NIGHT'S NUMBER WHILE THE READER STANDS IN A BANK.
    ///
    /// The owner's rule, in his words: if not in combat this should be 0. A meter that keeps the
    /// last fight's rate on screen is worse than one that shows nothing, because the number is
    /// plausible and there is nothing on the window to say it is old.
    ///
    /// AND WHAT COUNTS AS HIS. `Participant::dealt` is the sum of every `DamageKind` that actor
    /// dealt, so melee (ripostes and flurries included, both being swings that connected), direct
    /// spells, DoT ticks and his own damage shield are all already in one number. This drives all
    /// four kinds through the real rule rather than trusting that sentence.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `live` check from `mine`, or matching the reader
    /// by name instead of by `Who::You`.
    #[test]
    fn out_of_combat_is_zero_and_in_combat_is_everything_he_did() {
        let mut f = fight(vec![
            /* One of each kind the owner named, all folded into `dealt` by the engine. */
            named("a thunder spirit princess", 100),
            self_dealt(1_200),
        ]);
        f.secs = 10;

        let r = Ranked::default();
        assert_eq!(
            mine(&f, false, &r),
            Mine::Idle(Metric::Dealt, true),
            "out of combat the headline is zero, whatever the last fight said"
        );
        assert_eq!(mine(&f, false, &r).text(), "0");

        assert_eq!(
            mine(&f, true, &r),
            Mine::Rate(120, Metric::Dealt),
            "1,200 over ten seconds is his rate, and the mob's 100 is not his"
        );

        /* A pull younger than the floor shows what he has DONE rather than a rate the file
         * cannot express, and says so with a different unit. */
        f.secs = 2;
        assert_eq!(mine(&f, true, &r), Mine::TooSoon(1_200, Metric::Dealt));
        assert_eq!(Mine::TooSoon(1_200, Metric::Dealt).unit(), "dmg");
        assert_eq!(Mine::Rate(120, Metric::Dealt).unit(), "dps");

        /* A reader who has not swung yet is a real zero and not a missing row. */
        let mut empty = fight(vec![named("a thunder spirit princess", 100)]);
        empty.secs = 10;
        assert_eq!(mine(&empty, true, &r), Mine::Rate(0, Metric::Dealt));
    }

    /// DEFECT: A CONFIG THE RENDERER DOES NOT ACTUALLY READ.
    ///
    /// This is the point of the whole stage. Six of the owner's eleven mockups are this table with
    /// a different [`Metric`], so if the renderer keeps reaching for `dealt` the config is
    /// decoration and the builder will produce five overlays that all show damage.
    ///
    /// ONE FIGHTER WITH FOUR DIFFERENT NUMBERS, so a metric that read the wrong field cannot
    /// accidentally agree. It drives the RANKING, the HEADLINE and the UNIT, because those are
    /// three separate places the metric has to reach and a fix to one would not fix the others.
    ///
    /// WHAT MUTATION MAKES THIS RED: any `f.dealt` put back into `ranked_dealers`, `mine` or `row`.
    #[test]
    fn the_table_follows_its_metric_and_not_the_word_damage() {
        /* The healer heals and swings a little; the tank swings a little and eats everything. */
        let healer = Fighter {
            who: Who::Named("Poguhy".to_owned()),
            dealt: 10,
            taken: 20,
            healed: 900,
            received: 30,
            ..f(Who::You, 0)
        };
        let tank = Fighter {
            who: Who::You,
            dealt: 500,
            taken: 4_000,
            healed: 0,
            received: 900,
            ..f(Who::You, 0)
        };
        let all = vec![healer.clone(), tank.clone()];
        let row = fight(all.clone());

        /* THE RANKING FLIPS WITH THE METRIC, which is the whole claim. */
        let by = |m: Metric| -> Vec<&'static str> {
            ranked_dealers(&row, m)
                .iter()
                .map(|x| if x.who == Who::You { "You" } else { "Poguhy" })
                .collect()
        };
        assert_eq!(by(Metric::Dealt), vec!["You", "Poguhy"]);
        assert_eq!(by(Metric::Taken), vec!["You", "Poguhy"]);
        assert_eq!(
            by(Metric::Healed),
            vec!["Poguhy"],
            "the tank healed nothing"
        );
        assert_eq!(by(Metric::Received), vec!["You", "Poguhy"]);

        /* AND THE HEADLINE READS THE READER'S OWN FIGURE FOR THAT METRIC. Ten seconds, so every
         * one of these is over the rate floor and none can hide behind `TooSoon`. */
        let mut fi = fight(all);
        fi.secs = 10;
        let head = |m: Metric, rate: bool| {
            mine(
                &fi,
                true,
                &Ranked {
                    metric: m,
                    rate,
                    ..Ranked::default()
                },
            )
        };
        assert_eq!(head(Metric::Dealt, true), Mine::Rate(50, Metric::Dealt));
        assert_eq!(head(Metric::Taken, true), Mine::Rate(400, Metric::Taken));
        assert_eq!(head(Metric::Healed, true), Mine::Rate(0, Metric::Healed));
        assert_eq!(
            head(Metric::Received, true),
            Mine::Rate(90, Metric::Received)
        );

        /* A TABLE THAT WAS NOT ASKED FOR A RATE PRINTS THE TOTAL, and is never held behind the
         * floor: a total is a count the log stated and is true one second in. */
        assert_eq!(head(Metric::Healed, false), Mine::Total(0, Metric::Healed));
        assert_eq!(
            head(Metric::Taken, false),
            Mine::Total(4_000, Metric::Taken)
        );

        /* AND THE UNIT FOLLOWS, or a healing overlay says `dps` over a healing number. */
        assert_eq!(head(Metric::Healed, true).unit(), "hps");
        assert_eq!(head(Metric::Taken, true).unit(), "dtps");
        assert_eq!(head(Metric::Taken, false).unit(), "taken");
    }

    /// DEFECT: leaving equal dealers in the order the fold happened to build them.
    ///
    /// The vector's order is the order participants first appeared in the log, which is stable for
    /// one fold and NOT stable across folds: the tail moves, the fold re-runs, and two dealers on
    /// the same damage swap rows on screen while nothing about the fight has changed. The engine
    /// breaks its own headline tie by name for exactly this reason and drives both orders to prove
    /// it, so this drives both orders too.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `.then_with` from the sort in `ranked_dealers`.
    #[test]
    fn equal_dealers_rank_by_name_whichever_order_the_fold_built_them_in() {
        for pair in [["Zeta", "Alpha"], ["Alpha", "Zeta"]] {
            let all = fight(vec![named(pair[0], 10), named(pair[1], 10)]);
            let got: Vec<&str> = ranked_dealers(&all, Metric::Dealt)
                .iter()
                .map(|x| x.who.text())
                .collect();
            assert_eq!(
                got,
                vec!["Alpha", "Zeta"],
                "built as {pair:?}, the overlay must rank the same way either way"
            );
        }
    }

    /// DEFECT: the mobs and the idle padding a DPS table with rows that are not people.
    ///
    /// TWO FILTERS AND BOTH ARE LOAD BEARING. A participant is anything that was HIT, so the
    /// capture's first fight carries twenty-two rows of which four are players; and the thing
    /// being fought hits back hard enough to outrank half the group, so `a lurking mummy` at 753
    /// would sit above two real players in the owner's own party.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping either half of the filter in `ranked_dealers`. The
    /// mob deals real damage so a `dealt > 0` test alone keeps it, and the idle player deals none
    /// so a `player()` test alone keeps him.
    #[test]
    fn only_people_who_dealt_damage_take_up_a_row() {
        let all = fight(vec![
            /* Deals plenty and is not a person: the exact row the owner asked to be rid of. */
            named("a lurking mummy", 753),
            f(Who::You, 500),
            /* A person who has not swung yet. */
            named("Fylasem", 0),
            named("Berserker", 300),
            /* No article, still not a person: fight #3 of the capture is these killing the reader. */
            named("Guard Ullindin", 568),
            /* Nobody at all, which is neither. */
            f(Who::Unknown, 25),
        ]);
        let got: Vec<&str> = ranked_dealers(&all, Metric::Dealt)
            .iter()
            .map(|x| x.who.text())
            .collect();
        assert_eq!(got, vec!["You", "Berserker"]);
    }

    /// DEFECT: A STRANGER ON THE READER'S METER IN A FIGHT WHOSE GROUP THE LOG PROVED.
    ///
    /// # THE THREE ANSWERS `FightRow::group` HAS, OVER ONE SET OF FIGHTERS
    ///
    /// A mob, the reader, a group member, and a player fighting beside them who is not in the
    /// group. `Losumyda` is the capture's own case: his only other lines are NewPlayers chat, and
    /// he is the one player in its last pull, after the removal line that proved the reader solo.
    ///
    ///   * KNOWN, `Some(["hert"])`: the reader and Hert, and the share is over the two of them. The
    ///     member is spelled in LOWER CASE on purpose, as a typed invite spells him, so a byte
    ///     comparison takes him off the meter of his own group.
    ///   * NOT KNOWN, `None`: every player, exactly as before the field existed. Not known is not
    ///     nobody, and this is the case where reading it that way would empty a roster.
    ///   * SOLO, `Some(vec![])`: the reader alone.
    ///
    /// WHAT MUTATION MAKES THIS RED: `ranked_dealers` filtering on `Who::player` again; `ours`
    /// ignoring the group; `ours` comparing names byte for byte; `ours` treating `None` as solo.
    #[test]
    fn a_known_group_ranks_the_reader_and_his_group_and_shares_over_them_alone() {
        let fighters = vec![
            named("a lurking mummy", 900),
            f(Who::You, 500),
            named("Hert", 300),
            named("Losumyda", 200),
        ];
        let ranked = |row: &FightRow| -> (Vec<String>, u64) {
            let r = ranked_dealers(row, Metric::Dealt);
            (
                r.iter().map(|x| x.who.text().to_owned()).collect(),
                r.iter().map(|x| x.dealt).sum(),
            )
        };

        let known = FightRow {
            group: Some(vec!["hert".to_owned()]),
            ..fight(fighters.clone())
        };
        let (who, ours) = ranked(&known);
        assert_eq!(
            who,
            vec!["You", "Hert"],
            "the log proved the group is Hert, and the meter ranked someone outside it or dropped \
             Hert because the group spells him `hert`"
        );
        assert_eq!(
            ours, 800,
            "the share denominator of a known group counted a player who is not in it"
        );
        let shares: Vec<u64> = ranked_dealers(&known, Metric::Dealt)
            .iter()
            .map(|x| share(x.dealt, ours))
            .collect();
        assert_eq!(shares, vec![62, 37], "500 and 300 of the group's 800");

        let unknown = fight(fighters.clone());
        assert_eq!(unknown.group, None, "the fixture default moved");
        let (who, ours) = ranked(&unknown);
        assert_eq!(
            who,
            vec!["You", "Hert", "Losumyda"],
            "a fight whose group is NOT KNOWN lost a player from its meter, which is reading not \
             known as nobody"
        );
        assert_eq!(ours, 1_000, "every player's damage, and none of the mob's");

        let solo = FightRow {
            group: Some(Vec::new()),
            ..fight(fighters)
        };
        let (who, ours) = ranked(&solo);
        assert_eq!(
            who,
            vec!["You"],
            "the log proved the reader solo and the meter still ranked somebody beside him"
        );
        assert_eq!(ours, 500);
    }

    /// DEFECT: TWO LISTS THAT MEAN DIFFERENT THINGS AND LOOK THE SAME.
    ///
    /// A meter over a known group and a meter over everyone in range are both names, bars and
    /// shares that add to a hundred. `whose` is the sentence that tells them apart, and it only
    /// counts if it reaches PAINT beside the rows it is about, so this draws both shapes over both
    /// rows and reads the text back, as `the_meter_and_the_table_state_the_same_numbers_for_one_fight`
    /// does.
    ///
    /// WHAT MUTATION MAKES THIS RED: `whose` answering one constant; `header` not drawing it; the
    /// `None` arm saying `your group`; the solo arm saying `your group`.
    #[test]
    fn the_meter_says_whether_its_rows_are_the_group_or_everyone_in_range() {
        let fighters = vec![f(Who::You, 500), named("Hert", 300), named("Losumyda", 200)];
        let known = FightRow {
            group: Some(vec!["Hert".to_owned()]),
            secs: 10,
            ..fight(fighters.clone())
        };
        let unknown = FightRow {
            secs: 10,
            ..fight(fighters.clone())
        };
        let solo = FightRow {
            group: Some(Vec::new()),
            secs: 10,
            ..fight(fighters)
        };

        let words = [whose(&known), whose(&unknown), whose(&solo)];
        assert_eq!(
            words,
            ["your group", "everyone in range, group not known", "solo"],
            "the caption does not tell the three answers of `FightRow::group` apart"
        );

        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let painted = |row: &FightRow, w: Widget| -> Vec<String> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(620.0, 400.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| draw_widget(ui, row, Pulse::Fighting, &w));
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
        };

        let r = Ranked {
            headline: true,
            ..Ranked::default()
        };
        for shape in [Widget::Meter(r.clone()), Widget::Ranked(r.clone())] {
            let said = painted(&known, shape.clone());
            assert!(
                said.iter().any(|s| s == "your group"),
                "a meter over a known group does not say so: {said:?}"
            );
            assert!(
                !said.iter().any(|s| s.ends_with("Losumyda")),
                "a meter captioned as the reader's group drew a player outside it: {said:?}"
            );

            let said = painted(&unknown, shape.clone());
            assert!(
                said.iter()
                    .any(|s| s == "everyone in range, group not known"),
                "a meter over every player in range does not say the group is not known: {said:?}"
            );
            assert!(
                said.iter().any(|s| s.ends_with("Losumyda")),
                "a meter whose group is not known left a player off: {said:?}"
            );
            assert!(
                !said.iter().any(|s| s == "your group"),
                "a meter over everyone in range called them the reader's group: {said:?}"
            );

            let said = painted(&solo, shape);
            assert!(
                said.iter().any(|s| s == "solo"),
                "a meter over a solo reader does not say so: {said:?}"
            );
        }

        /* AND AN EMPTY ROSTER SAYS WHOSE ROSTER IS EMPTY, in all three answers and on paint.
         *
         * `Nobody has dealt any damage.` over a solo fight where only a stranger dealt damage is a
         * sentence about the log that the log contradicts, and it is the capture's last fight.
         * WHAT MUTATION MAKES THIS RED: `nobody` answering `Nobody has` for every group; `meter` or
         * `ranked_table` printing the old `Nobody has` sentence instead of asking `nobody`. */
        assert_eq!(
            [
                nobody(&known, Metric::Dealt),
                nobody(&unknown, Metric::Dealt),
                nobody(&solo, Metric::Dealt)
            ],
            [
                "Nobody in your group has dealt any damage.",
                "Nobody has dealt any damage.",
                "You have not dealt any damage."
            ],
            "the empty roster's sentence does not name the roster it is about"
        );
        let stranger_only = FightRow {
            group: Some(Vec::new()),
            secs: 10,
            ..fight(vec![named("Losumyda", 200)])
        };
        for shape in [Widget::Meter(r.clone()), Widget::Ranked(r.clone())] {
            let said = painted(&stranger_only, shape);
            assert!(
                said.iter().any(|s| s == "You have not dealt any damage."),
                "an empty solo roster did not say the reader dealt nothing: {said:?}"
            );
            assert!(
                !said.iter().any(|s| s == "Nobody has dealt any damage."),
                "a solo roster said nobody dealt damage over a fight a stranger dealt damage in: \
                 {said:?}"
            );
        }
    }

    /// DEFECT: the shares still measured against the whole fight after the mobs left the chart.
    ///
    /// `FightRow::damage` counts every point anybody dealt to anybody, the pull included. With the
    /// mobs drawn it summed to 100 down the column; with them gone it sums to whatever fraction of
    /// the fight the group happened to deal, and a reader looking at 76 percent hunts for a row
    /// that was never going to be there.
    #[test]
    fn the_shares_are_of_what_the_group_dealt_and_add_up() {
        let all = fight(vec![
            named("a lurking mummy", 250),
            f(Who::You, 500),
            named("Berserker", 500),
        ]);
        let ranked = ranked_dealers(&all, Metric::Dealt);
        let ours: u64 = ranked.iter().map(|x| x.dealt).sum();
        assert_eq!(ours, 1_000, "the mob's 250 is not the group's");
        let shares: Vec<u64> = ranked.iter().map(|x| share(x.dealt, ours)).collect();
        assert_eq!(shares, vec![50, 50]);
        assert_eq!(shares.iter().sum::<u64>(), 100);
    }

    /// DEFECT: scaling every bar to the fight's TOTAL, which makes a good parse in a big group
    /// look like a sliver.
    ///
    /// The top dealer's bar fills the track and everybody else is measured against him, which is
    /// the comparison a reader of this window is actually making. Scaling to the total instead
    /// would give the best player in a raid of twenty a bar a twentieth of the width.
    #[test]
    fn the_top_dealer_fills_the_track_and_half_of_him_is_half_a_track() {
        assert_eq!(bar_width(100, 100, 200.0), 200.0);
        assert_eq!(bar_width(50, 100, 200.0), 100.0);
        assert_eq!(bar_width(0, 100, 200.0), 0.0);
        /* No divide by zero and no bar wider than its track, whatever it is handed. */
        assert_eq!(bar_width(10, 0, 200.0), 0.0);
        assert_eq!(bar_width(500, 100, 200.0), 200.0);
        assert_eq!(bar_width(10, 100, 0.0), 0.0);
    }

    /// The arithmetic of one share, on its own. What the denominator IS lives in
    /// `the_shares_are_of_what_the_group_dealt_and_add_up`; this is the rounding and the guards.
    #[test]
    fn a_share_is_a_whole_percent_and_never_divides_by_zero() {
        assert_eq!(share(50, 100), 50);
        assert_eq!(share(1, 3), 33);
        assert_eq!(share(0, 100), 0);
        assert_eq!(share(10, 0), 0, "a fight with no damage divides by nothing");
        /* Whole percent, rounded DOWN, so a column of shares can add to less than 100 by a point
         * or two and never to more. */
        assert_eq!(share(2, 3), 66);
    }

    #[test]
    fn the_numbers_read_the_way_a_person_writes_them() {
        assert_eq!(short(9_999), "9999", "under ten thousand keeps every digit");
        assert_eq!(short(16_526), "16.5k");
        assert_eq!(short(0), "0");

        assert_eq!(thousands(16_526), "16,526");
        assert_eq!(thousands(100), "100");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(0), "0");
    }

    /* THERE IS NO `size_of::<DpsScreen>() == 0` TEST HERE, DELIBERATELY. One was written and
     * removed: `screens::chat` carried exactly that assertion, and it went red the day the screen
     * grew a `String` draft, which was a correct and safe change. A test that fails on safe
     * changes and passes on the unsafe one it was aimed at is worse than no test, because it
     * trains the next person to edit the assertion instead of thinking about it. The property
     * that matters, that nothing here caches a fight across passes, is stated on `DpsScreen` and
     * is enforced by the fact that `ui` reads `cx.ingest` and has nowhere to put a copy. */

    /// DEFECT: A COLUMN OF PER-SECOND FIGURES WITH NO UNIT ANYWHERE ON THE SCREEN.
    ///
    /// # WHAT A READER SAW, MEASURED ON THE CAPTURE'S LAST FIGHT
    ///
    /// `Losumyda` dealt 20 points over 36 seconds and is the only player in that pull. The Live
    /// page, every Dashboards tab and the Analysis page drew him as `Losumyda   0   100%` under a
    /// heading reading DAMAGE DEALT. Neither figure is wrong. The `0` is a rate that rounds down
    /// and the `100%` is a share of a total, and nothing on the page said they were two different
    /// kinds of number, or that either was a rate at all.
    ///
    /// # WHY IT WAS INVISIBLE TO EVERY TEST THIS FILE HAD
    ///
    /// `Ranked::headline` gates `header`, and `header` held the ONLY `Metric::unit` call in the
    /// renderer that paints beside a figure. The overlay windows set `headline: true` and were
    /// fine; every page surface sets `headline: false` for a good reason (a page is read on
    /// purpose, not glanced at across a room) and lost the unit with the big number. The module
    /// doc asserted the opposite in as many words: "the unit printed beside every figure (`dps`,
    /// `hps`, `dtps`) is what keeps a reader from taking one for the other". It was true of the
    /// overlay and false of every page.
    ///
    /// # THE INVARIANT, WHICH IS WHAT THIS PINS RATHER THAN THE FIX
    ///
    /// A table whose figures are RATES must draw the unit somewhere: in the headline, or in the
    /// column header. Asserted over every configuration the app ships rather than over the two
    /// that happened to be wrong, so a ninth preset or a new page cannot reintroduce it.
    ///
    /// WHAT MUTATION MAKES THIS RED: setting `head: false` on any page preset, or reverting the
    /// `Cols::head` field.
    #[test]
    fn a_table_of_rates_always_says_somewhere_that_they_are_rates() {
        let mut checked = 0;
        let mut look = |what: &str, w: &crate::overlay::Widget| {
            if let crate::overlay::Widget::Ranked(r) = w {
                checked += 1;
                if r.rate {
                    assert!(
                        r.headline || r.cols.head,
                        "{what} ranks by a per-second figure and draws no unit: `headline` is \
                         what paints one beside the big number and `cols.head` is what paints one \
                         over the column, and this config has neither"
                    );
                }
            }
        };

        /* THE LIVE PAGE. */
        for (name, w) in crate::screens::live::panels_for_test() {
            look(&format!("Live / {name}"), &w);
        }
        /* EVERY DASHBOARD TILE THAT DRAWS A COMBAT PANEL. Six of the fourteen tiles read
         * something other than a fight (what is on disk, what a mob absorbs, who is casting)
         * and carry no `Widget` at all, so this walks the widgets rather than the tiles. */
        for (tile, w) in crate::screens::dashboards::widgets_for_test() {
            look(&format!("Dashboards / {tile:?}"), &w);
        }
        /* EVERY ANALYSIS TAB. This page was missed by the first version of this test AND by
         * the fix wave the test was written for, and it shipped rates with no unit for as
         * long as both. It builds its tables through `..Ranked::default()` rather than a
         * literal `Cols`, so a search for the flag did not find it and a guard listing pages
         * by hand did not cover it. */
        for tab in 0..crate::screens::analysis::TABS.len() {
            for w in crate::screens::analysis::panels_for_test(tab) {
                look(&format!("Analysis / tab {tab}"), &w);
            }
        }
        /* EVERY REPORTS TAB PANEL. */
        for (name, w) in crate::screens::reports::panels_for_test(true) {
            look(&format!("Reports / {name}"), &w);
        }
        /* THE SHIPPED OVERLAY, which is the one that has always been right and is here so the
         * test would notice if the headline stopped drawing a unit. */
        for w in &crate::overlay::Overlay::default_dps().panels() {
            look("the shipped DPS overlay", w);
        }

        assert!(
            checked >= 10,
            "only {checked} ranked configs were reached, so this is passing by not looking"
        );
    }

    /// AND THE HEADER SAYS WHAT THE ROW UNDER IT ACTUALLY PRINTS.
    ///
    /// DEFECT: a fixed `DPS` header over a column that is not yet a rate. `dps` withholds one
    /// below `MIN_RATE_SECS` because the log stamps to the second, and `row` falls back to the raw
    /// total for those first seconds. A header hard-coded to the rate would name the wrong unit on
    /// exactly the frames the fallback fires, which is the opening of every pull.
    ///
    /// WHAT MUTATION MAKES THIS RED: `r.metric.unit(r.rate)` in `columns`, ignoring the clock.
    #[test]
    fn the_column_header_changes_unit_on_the_same_frame_the_column_does() {
        let r = Ranked {
            metric: Metric::Dealt,
            rate: true,
            ..Ranked::default()
        };
        /* Too young for a rate: the row prints a total, so the header must say so. */
        let young = MIN_RATE_SECS - 1;
        assert_eq!(dps(1, young), None, "the floor moved and this test did not");
        assert_eq!(head_unit(young, &r), "dmg");
        /* Old enough: both switch together. */
        assert!(dps(1, MIN_RATE_SECS).is_some());
        assert_eq!(head_unit(MIN_RATE_SECS, &r), "dps");

        /* AND A TABLE THAT WAS NEVER ASKED FOR A RATE NEVER CLAIMS ONE. */
        let flat = Ranked {
            rate: false,
            ..r.clone()
        };
        assert_eq!(head_unit(MIN_RATE_SECS, &flat), "dmg");
    }

    /// THE METER AND THE TABLE MAY NOT DISAGREE ABOUT A NUMBER.
    ///
    /// # WHY THIS IS THE ASSERTION AND NOT `the meter draws some rows`
    ///
    /// `Widget::Meter` and `Widget::Ranked` are two renderers over one fold, and both can be on
    /// screen at the same moment: the overlay window ships the meter, the Analysis page and the
    /// dashboard can hold either, and the builder's preview draws whichever is being edited. A
    /// person reading 28 on one and 27 on the other has been shown two different answers to one
    /// question by one app, and nothing he reads afterwards is worth anything.
    ///
    /// THE SHARED ARITHMETIC IS WHAT MAKES THAT POSSIBLE RATHER THAN CERTAIN. `figure` and `share`
    /// are one function each and both shapes call them, but a caller can still hand them the
    /// wrong arguments: the fight's total instead of the group's, a per-row span instead of the
    /// fight's, `thousands` instead of `short`. Every one of those compiles. This paints both and
    /// compares what came out.
    ///
    /// # HOW IT COMPARES TWO DIFFERENT SHAPES
    ///
    /// The table paints a row's figure and its share as two separate strings in two columns; the
    /// meter paints them as one string at the right of the bar. So the meter's rows are parsed
    /// back into their two halves, and each half has to appear among the strings the TABLE
    /// painted. That is the comparison that matters and it does not care where either shape put
    /// its text.
    ///
    /// WHAT MUTATION MAKES THIS RED: sharing against `fight.damage` instead of the group's sum,
    /// printing `thousands(v)` instead of `figure(..)`, capping `ours` to the drawn rows, or
    /// rating against anything but the fight's own span.
    #[test]
    fn the_meter_and_the_table_state_the_same_numbers_for_one_fight() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let dir = crate::fights::probe::planted("meter-agrees", crate::fights::probe::CAPTURE);
        let ing = crate::fights::probe::booted(&dir);
        /* THE BIGGEST PULL, so the two configs have a fight with enough in it to disagree about.
         * `fights()[0]` is the four line remnant of the pull the capture was cut inside. */
        let fight = ing
            .fights()
            .iter()
            .max_by_key(|f| f.lines)
            .cloned()
            .expect("the capture folds fights");

        /* ONE CONFIG, DRAWN TWICE. Anything that differed between the two configs would make a
         * disagreement legitimate and the test meaningless. */
        let r = Ranked::default();
        let painted = |w: Widget| -> Vec<String> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(460.0, 400.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| draw_widget(ui, &fight, Pulse::Fighting, &w));
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
        };
        let table = painted(Widget::Ranked(r.clone()));
        let meter = painted(Widget::Meter(r.clone()));

        /* THE ROWS THE METER PAINTED, split back into figure and share. */
        let rows: Vec<(String, String)> = meter
            .iter()
            .filter_map(|s| s.split_once("  "))
            .filter(|(_, pct)| pct.ends_with('%'))
            .map(|(v, p)| (v.to_owned(), p.to_owned()))
            .collect();

        let want = ranked_dealers(&fight, r.metric).len().min(r.cap);
        assert!(want > 0, "the capture's first fight has no ranked dealers");
        assert_eq!(
            rows.len(),
            want,
            "the meter drew {} rows for {want} dealers: {meter:?}",
            rows.len()
        );

        for (v, pct) in &rows {
            assert!(
                table.iter().any(|s| s == v),
                "the meter says {v} and the table never printed it: {table:?}"
            );
            assert!(
                table.iter().any(|s| s == pct),
                "the meter says {pct} and the table never printed it: {table:?}"
            );
        }
    }

    /// DEFECT: THE POP-OUT OVERLAY'S BODY NEVER DRAWN BY ANYTHING BUT THE APP.
    ///
    /// `windows.rs` covers the overlay's LIFECYCLE well: `overlay_windows_follow_their_id_and_
    /// not_their_position` and `opening_an_overlay_asks_for_a_root_pass_and_stops_asking_after_
    /// one` both hold. What nothing covered is the thing inside the window: that the shipped
    /// configuration, drawn against a real fold, produces rows with real numbers in them.
    ///
    /// AND IT IS THE ONE SURFACE THAT CANNOT BE CHECKED BY LOOKING WITHOUT OPENING IT. The main
    /// window is on screen whenever the app is; a pop-out is a deliberate act, so a defect in it
    /// survives every casual glance at the running build.
    ///
    /// THE SHIPPED CONFIG AND NOT A FIXTURE, because `Overlay::default_dps` is what a fresh
    /// install pops out and what `or_default` hands back when the list is empty.
    ///
    /// WHAT MUTATION MAKES THIS RED: an early return in `draw_widget`, a `ranked_dealers` that
    /// filters everybody out, or a default overlay with no widgets on it.
    #[test]
    fn the_shipped_overlay_draws_the_capture_and_not_an_empty_window() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let dir = crate::fights::probe::planted("overlay-body", crate::fights::probe::CAPTURE);
        let ing = crate::fights::probe::booted(&dir);
        /* THE BIGGEST PULL AND NOT THE FIRST ONE. A rate is only published for a fight the log can
         * express one for (`MIN_RATE_SECS` is 3), and the capture now opens on the four line
         * remnant of the pull it was cut inside: one second long, so the overlay rightly prints a
         * total with no rate, and this test would be demanding a unit from a window that has no
         * business showing one. */
        let fight = ing
            .fights()
            .iter()
            .max_by_key(|f| f.lines)
            .cloned()
            .expect("the capture folds fights");

        let o = crate::overlay::Overlay::default_dps();
        assert!(!o.panels().is_empty(), "the shipped overlay has no widgets");

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(o.w, o.h.max(200.0)),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            for w in &o.panels() {
                draw_widget(ui, &fight, Pulse::Fighting, w);
            }
        });
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

        assert!(
            !said.is_empty(),
            "the overlay window painted nothing at all"
        );
        /* THE READER IS IN THE CAPTURE'S FIRST FIGHT, so his row is what a real fold produces.
         *
         * ENDS WITH AND NOT EQUALS, BECAUSE THE TWO SHAPES WRITE THE NAME DIFFERENTLY. The
         * table paints the name on its own into a name column; the meter writes it on the bar
         * after the rank, as `1. You`. What this test is for is that a REAL FOLD reached paint,
         * and either spelling proves that; pinning the exact string would make it a test of
         * which widget happens to be shipped today.
         */
        assert!(
            said.iter().any(|s| s == "You" || s.ends_with(" You")),
            "the overlay drew no row from the fight it was given: {said:?}"
        );
        /* AND A RATE, WITH ITS UNIT SOMEWHERE. The shipped overlay carries a headline, which is
         * where its unit lives; `a_table_of_rates_always_says_somewhere_that_they_are_rates` is
         * what holds that for every config, and this is the one that proves it reaches paint. */
        assert!(
            said.iter().any(|s| s == "dps"),
            "the overlay drew a per-second figure with no unit beside it: {said:?}",
            // the chosen fight, so a failure says WHICH one had no rate
        );
    }
}
