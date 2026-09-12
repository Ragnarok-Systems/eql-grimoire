//! HOW A FIGHT ACTUALLY ENDS, MEASURED BEFORE ANY RULE IS WRITTEN.
//!
//! `Fights` closes a run only on a long quiet, a zone, a clock that stepped back, or the end of
//! the text. A kill closes nothing, so a pull that is plainly over on screen stays open for the
//! whole quiet window and the reader watches `IN COMBAT` over a dead mob.
//!
//! Ending on the kill instead is a rule with two ways to be wrong, and both are measurable here
//! rather than arguable:
//!
//! * **Damage lands after the last death.** A dot that ticks on a corpse, a shield that answers a
//!   swing already in flight, a pet still finishing its round. Closing at the death line would
//!   drop every point of it out of the fight the reader was in.
//! * **A chain of pulls is one fight.** On a raid night combat never goes quiet for thirty
//!   seconds, so the aggregator's own doc says an eight minute chain is ONE fight. Closing at
//!   every death cuts it into pieces, and the count of pieces is what a dashboard's history is
//!   made of.
//!
//! This measures both over the two real captures, and prints rather than asserts: it exists to
//! decide a rule, and a measurement that asserts its own answer is a rule pretending to be a
//! reading. Run it with
//!
//! ```text
//! cargo test -p grimoire-parse --test fight_end_measure -- --ignored --nocapture
//! ```
use grimoire_parse::combat::{parse, Actor, Event};
use grimoire_parse::fights::{seconds, Ended, Fights};

const TAIL: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");
const NIGHT: &str = include_str!("../../grimoire-desktop/tests/fixtures/princess-night.txt");
const OWNER: &str = "Reviir";

/// One fight's boundary, in the log's own seconds.
struct Span {
    start: i64,
    end: i64,
}

/// What one fight looks like when you ask the deaths inside it where it should have stopped.
#[derive(Default)]
struct Reading {
    /// Deaths of anybody who is not the reader, in log order.
    kills: Vec<i64>,
    /// Total damage dealt inside the fight.
    damage: u64,
    /// Damage dealt strictly after the last such death.
    after_last: u64,
    /// Damage dealt strictly after the FIRST such death, which is what a rule that closed on any
    /// kill at all would throw away.
    after_first: u64,
}

fn spans(text: &str) -> Vec<Span> {
    let mut agg = Fights::new().with_owner(OWNER);
    for raw in text.lines() {
        if let Some(e) = parse(raw) {
            agg.push(e);
        }
    }
    agg.finish()
        .iter()
        .filter_map(|f| {
            Some(Span {
                start: seconds(f.start)?,
                end: seconds(f.end)?,
            })
        })
        .collect()
}

/// Walk the text once per fight window and read what the deaths in it did to the damage around
/// them. Deliberately a second pass over the raw lines rather than a read of `Fight`: the question
/// is about WHEN inside a fight things happened, and a fold has already thrown the clock away.
fn read(text: &str, spans: &[Span]) -> Vec<Reading> {
    let mut out: Vec<Reading> = spans.iter().map(|_| Reading::default()).collect();
    for raw in text.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Some(now) = seconds(entry.at) else {
            continue;
        };
        let grimoire_parse::combat::Reading::Event(event) = entry.reading else {
            continue;
        };
        let Some(i) = spans.iter().position(|s| now >= s.start && now <= s.end) else {
            continue;
        };
        match event {
            Event::Death { victim, .. } if victim != Actor::You => out[i].kills.push(now),
            Event::Damage(d) => {
                let r = &mut out[i];
                r.damage += u64::from(d.amount);
                if r.kills.first().is_some_and(|&k| now > k) {
                    r.after_first += u64::from(d.amount);
                }
                /* THE LAST DEATH SO FAR, WHICH IS THE ONLY ONE THIS PASS CAN KNOW ABOUT. Fixed up
                 * after the walk, because a death later in the fight moves the boundary and the
                 * damage counted here was measured against a boundary that had not arrived yet. */
            }
            _ => {}
        }
    }
    /* THE `after_last` FIGURE NEEDS THE WHOLE FIGHT'S DEATHS FIRST, so it is a second walk and not
     * a running total. Cheap, and it is the number the rule turns on. */
    for (i, span) in spans.iter().enumerate() {
        let Some(&last) = out[i].kills.last() else {
            continue;
        };
        let mut after = 0u64;
        for raw in text.lines() {
            let Some(entry) = parse(raw) else { continue };
            let Some(now) = seconds(entry.at) else {
                continue;
            };
            if now <= last || now < span.start || now > span.end {
                continue;
            }
            if let grimoire_parse::combat::Reading::Event(Event::Damage(d)) = entry.reading {
                after += u64::from(d.amount);
            }
        }
        out[i].after_last = after;
    }
    out
}

fn report(name: &str, text: &str) {
    let spans = spans(text);
    let readings = read(text, &spans);

    let mut with_kill = 0usize;
    let mut overhang: Vec<i64> = Vec::new();
    let mut lost_after_last = 0u64;
    let mut lost_after_first = 0u64;
    let mut total = 0u64;
    let mut pieces = 0usize;
    let mut fights_that_would_split = 0usize;

    for (s, r) in spans.iter().zip(readings.iter()) {
        total += r.damage;
        lost_after_last += r.after_last;
        lost_after_first += r.after_first;
        if let Some(&last) = r.kills.last() {
            with_kill += 1;
            overhang.push(s.end - last);
        }
        let n = r.kills.len().max(1);
        pieces += n;
        if n > 1 {
            fights_that_would_split += 1;
        }
    }

    overhang.sort_unstable();
    let median = overhang.get(overhang.len() / 2).copied().unwrap_or(0);
    let worst = overhang.last().copied().unwrap_or(0);
    let zero = overhang.iter().filter(|&&o| o == 0).count();

    println!("\n===== {name} =====");
    println!("fights                         {}", spans.len());
    println!("fights containing a kill       {with_kill}");
    println!("damage dealt, all fights       {total}");
    println!(
        "  dealt AFTER the last kill    {lost_after_last}  ({:.3}% of all damage)",
        pct(lost_after_last, total)
    );
    println!(
        "  dealt AFTER the FIRST kill   {lost_after_first}  ({:.3}% of all damage)",
        pct(lost_after_first, total)
    );
    println!("overhang, last kill to last combat line, in seconds:");
    println!("  zero (fight ends on the kill) {zero} of {with_kill}");
    println!("  median                        {median}");
    println!("  worst                         {worst}");
    println!("if every kill cut a fight:");
    println!("  fights become                 {pieces}");
    println!("  fights that would split       {fights_that_would_split}");
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 * 100.0 / whole as f64
}

#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn how_long_a_fight_runs_after_everything_in_it_is_dead() {
    report("eqlog-tail-200k", TAIL);
    report("princess-night", NIGHT);
}

/// WHAT A RULE THAT CAN RUN LIVE WOULD ACTUALLY DO.
///
/// `after_last` above is measured in hindsight: nothing on a live meter knows which death is the
/// last one. A rule that runs as the log arrives can only know who is still being fought, so this
/// is that rule, simulated line by line:
///
/// * A hostile joins the ENGAGED set when it deals damage to the reader's side or takes damage
///   from it. The reader, his pet and his group are never hostile.
/// * A death removes its victim from the set.
/// * When the set empties, the fight is over. That is the close.
///
/// The cost is whatever the log says after the set empties: a dot ticking on a corpse, a pet
/// finishing its round. That damage would land in no fight at all, which is the price, and it is
/// measured rather than assumed.
mod live_rule {
    use super::*;
    use std::collections::BTreeSet;

    struct Sim {
        /// Hostiles seen fighting and not yet dead.
        engaged: BTreeSet<String>,
        /// Seconds from the set emptying to the fight's last combat line.
        overhang: Vec<i64>,
        /// Damage after the set emptied, which the rule would strand.
        stranded: u64,
        total: u64,
        /// Fights this rule would produce, against the fights the quiet window produces.
        closes: usize,
        /// Fights where the set never emptied, so the quiet window still has to catch them.
        never_emptied: usize,
        /// What was still on the engaged set when a fight ended without emptying it, which is the
        /// only way to tell a real survivor from a bookkeeping miss.
        leftovers: Vec<String>,
    }

    /// Is this actor somebody on the reader's side? A hostile is everything else.
    fn friendly(a: Actor<'_>, pet: Option<&str>) -> bool {
        match a {
            Actor::You => true,
            Actor::Named(n) => pet == Some(n),
            _ => false,
        }
    }

    /// THE KEY IS CASE FOLDED, because the game sentence-capitalises inconsistently and
    /// `fights::same` folds the same way. `a thunder spirit princess` appears 29,238 times in the
    /// night capture and `A thunder spirit princess` 4,833, and they are one mob. A set that did
    /// not fold read the raid boss as two entities, one of which never died.
    fn name(a: Actor<'_>) -> Option<String> {
        match a {
            Actor::Named(n) => Some(n.to_ascii_lowercase()),
            _ => None,
        }
    }

    pub fn report(label: &str, text: &str) {
        let spans = spans(text);
        let mut sim = Sim {
            engaged: BTreeSet::new(),
            overhang: Vec::new(),
            stranded: 0,
            total: 0,
            closes: 0,
            never_emptied: 0,
            leftovers: Vec::new(),
        };
        for span in &spans {
            sim.engaged.clear();
            let mut emptied_at: Option<i64> = None;
            let mut last_line = span.start;
            for raw in text.lines() {
                let Some(entry) = parse(raw) else { continue };
                let Some(now) = seconds(entry.at) else {
                    continue;
                };
                if now < span.start || now > span.end {
                    continue;
                }
                let grimoire_parse::combat::Reading::Event(event) = entry.reading else {
                    continue;
                };
                match event {
                    Event::Damage(d) => {
                        last_line = now;
                        sim.total += u64::from(d.amount);
                        if emptied_at.is_some() {
                            sim.stranded += u64::from(d.amount);
                        }
                        /* THE HOSTILE IS WHICHEVER END OF THE LINE IS NOT OURS, and a line
                         * between two strangers engages neither: a guard killing an orc across
                         * the zone is not the reader's fight. */
                        let s_friend = friendly(d.source, None);
                        let t_friend = friendly(d.target, None);
                        if s_friend ^ t_friend {
                            let foe = if s_friend { d.target } else { d.source };
                            if let Some(n) = name(foe) {
                                sim.engaged.insert(n);
                                emptied_at = None;
                            }
                        }
                    }
                    Event::Death { victim, .. } => {
                        last_line = now;
                        if let Some(n) = name(victim) {
                            sim.engaged.remove(&n);
                        }
                        if sim.engaged.is_empty() && emptied_at.is_none() {
                            emptied_at = Some(now);
                        }
                    }
                    _ => {}
                }
            }
            match emptied_at {
                Some(t) => {
                    sim.closes += 1;
                    sim.overhang.push(last_line - t);
                }
                None => {
                    sim.never_emptied += 1;
                    if sim.leftovers.len() < 8 {
                        sim.leftovers
                            .push(sim.engaged.iter().cloned().collect::<Vec<_>>().join(", "));
                    }
                }
            }
        }
        sim.overhang.sort_unstable();
        let median = sim
            .overhang
            .get(sim.overhang.len() / 2)
            .copied()
            .unwrap_or(0);
        let worst = sim.overhang.last().copied().unwrap_or(0);
        let zero = sim.overhang.iter().filter(|&&o| o == 0).count();
        println!("\n----- live rule on {label} -----");
        println!("fights                          {}", spans.len());
        println!("  closed by the rule            {}", sim.closes);
        println!("  never emptied (quiet catches) {}", sim.never_emptied);
        println!("seconds saved, rule close to last combat line:");
        println!("  cut exactly on the kill       {zero} of {}", sim.closes);
        println!("  median overhang left          {median}");
        println!("  worst overhang left           {worst}");
        println!(
            "damage stranded after the close {}  ({:.3}% of {})",
            sim.stranded,
            pct(sim.stranded, sim.total),
            sim.total
        );
        for l in &sim.leftovers {
            println!("  still engaged at the end      [{l}]");
        }
    }
}

#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn what_a_rule_that_ends_on_the_last_thing_alive_would_cost() {
    live_rule::report("eqlog-tail-200k", TAIL);
    live_rule::report("princess-night", NIGHT);
}

/// WHAT THE `Killed` CLOSE DID TO THE BOUNDARIES, AND WHAT IT COST.
///
/// The risk a close on the kill carries is at the OTHER end: a dot ticking on a corpse is graded
/// `Beat::Opens`, so the tick after the close would open a fight of its own. A one-line fight in
/// the history is worse than a fight that ran three seconds long, so the count of them is what
/// decides whether the simple close can stand.
#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn what_closing_on_the_kill_did_to_the_boundaries() {
    for (label, text) in [("eqlog-tail-200k", TAIL), ("princess-night", NIGHT)] {
        let mut agg = Fights::new().with_owner(OWNER);
        for raw in text.lines() {
            if let Some(e) = parse(raw) {
                agg.push(e);
            }
        }
        let fights = agg.finish();
        let killed = fights.iter().filter(|f| f.ended == Ended::Killed).count();
        let tiny = fights.iter().filter(|f| f.lines <= 2).count();
        let mut follows_a_kill_within_5s = 0usize;
        for w in fights.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let (Some(ae), Some(bs)) = (seconds(a.end), seconds(b.start)) else {
                continue;
            };
            if a.ended == Ended::Killed && bs - ae <= 5 && b.lines <= 2 {
                follows_a_kill_within_5s += 1;
            }
        }
        println!("\n----- boundaries on {label} -----");
        println!("fights                          {}", fights.len());
        println!("  ended: everything died        {killed}");
        println!("  fights of 2 lines or fewer    {tiny}");
        println!("  tiny fights right after a kill {follows_a_kill_within_5s}");
        for f in fights.iter().filter(|f| f.lines <= 2) {
            println!("    [{}] {} lines, ended {:?}", f.start, f.lines, f.ended);
        }
    }
}

/// THE NEW BOUNDARIES, NAMED, so a person can check them against the log by eye.
#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn what_the_tail_capture_now_reads_as() {
    let mut agg = Fights::new().with_owner(OWNER);
    for raw in TAIL.lines() {
        if let Some(e) = parse(raw) {
            agg.push(e);
        }
    }
    for (i, f) in agg.finish().iter().enumerate() {
        let secs = seconds(f.end).unwrap_or(0) - seconds(f.start).unwrap_or(0);
        println!(
            "{i:>3}  {}  {secs:>4}s  {:>5} lines  {:>2} in it  ended {:?}",
            f.start,
            f.lines,
            f.participants.len(),
            f.ended
        );
    }
}

/// WHICH DAMAGE LINES FALL OUTSIDE EVERY FIGHT, NAMED.
///
/// Closing on the kill leaves moments with no fight open, and `push` DROPS a `Beat::Extends` line
/// that arrives then: that grading exists so falling damage cannot invent a fight, and the cost is
/// that it cannot join one either. This prints the lines that pay it.
#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn which_damage_lines_land_in_no_fight_at_all() {
    for (label, text) in [("eqlog-tail-200k", TAIL), ("princess-night", NIGHT)] {
        let windows = spans(text);
        println!("\n----- orphans in {label} -----");
        let mut lost = 0u64;
        for raw in text.lines() {
            let Some(entry) = parse(raw) else { continue };
            let Some(now) = seconds(entry.at) else {
                continue;
            };
            let grimoire_parse::combat::Reading::Event(Event::Damage(d)) = entry.reading else {
                continue;
            };
            if windows.iter().any(|w| now >= w.start && now <= w.end) {
                continue;
            }
            lost += u64::from(d.amount);
            println!("  {} points  {raw}", d.amount);
        }
        println!("  total outside every fight: {lost}");
    }
}

/// EVERY DAMAGE LINE THE FOLD GRADES `Beat::Extends`, which is the only kind that can be dropped
/// for arriving while no fight is open.
#[test]
#[ignore = "a measurement over the real captures, not a gate"]
fn which_damage_cannot_open_a_fight_of_its_own() {
    use grimoire_parse::combat::DamageKind;
    for (label, text) in [("eqlog-tail-200k", TAIL), ("princess-night", NIGHT)] {
        println!("\n----- self inflicted and environmental in {label} -----");
        let mut total = 0u64;
        for raw in text.lines() {
            let Some(entry) = parse(raw) else { continue };
            let grimoire_parse::combat::Reading::Event(Event::Damage(d)) = entry.reading else {
                continue;
            };
            if matches!(
                d.kind,
                DamageKind::SelfInflicted | DamageKind::Environmental
            ) {
                total += u64::from(d.amount);
                println!("  {:>4} points  {raw}", d.amount);
            }
        }
        println!("  total {total}");
    }
}
