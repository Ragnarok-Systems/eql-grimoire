//! THE FIGHT CLOCK: the per-second series, the event marks, and the zone a fight happened in.
//!
//! All three exist so a timeline can be drawn without a second parse, and all three are measured
//! here against the reference capture rather than against a hand-built log, because the shapes that
//! make the naive version wrong are only in the real bytes: ten and more lines sharing one printed
//! second, a zone line arriving over a minute after the fight it terminates, and a fight opening in
//! a zone whose name was consumed by the cut before it.
use grimoire_parse::combat::{parse, Actor};
use grimoire_parse::fights::{Ended, Fight, Fights, What};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");
const OWNER: &str = "Reviir";

fn folded() -> Vec<Fight<'static>> {
    let mut agg = Fights::new().with_owner(OWNER);
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            agg.push(e);
        }
    }
    agg.finish()
}

/// THE CAPTURE'S BIGGEST FIGHT, WHICH IS NOT ITS FIRST ONE.
///
/// These tests used `fights[0]` when the whole camp was one fight: combat never went quiet for
/// thirty seconds there, so eleven pulls folded into a single 1,626 line run and the first index
/// was also the biggest. [`Ended::Killed`] ends a fight when the last thing it was fighting dies,
/// so the camp now reads as the eleven pulls it was, and `fights[0]` is the four line remnant of
/// the pull the capture opens INSIDE: the reader cleaves `a dry bone skeleton`, slays it in the
/// first second of the file, and swings at the next mob. A remnant is the correct reading of a
/// clipped window and a useless subject for a timeline.
///
/// ASKED FOR BY SIZE RATHER THAN PINNED TO AN INDEX, so the next boundary change cannot quietly
/// re-point these at something that proves nothing.
fn longest() -> Fight<'static> {
    folded()
        .into_iter()
        .max_by_key(|f| f.lines)
        .expect("the capture has fights in it")
}

/// DEFECT: a series that carries one entry per damage LINE.
///
/// The log stamps to the second and the capture puts many lines inside one, so an event-per-entry
/// series would have duplicate x values and call it resolution, while growing the fold's cost with
/// the line count. Accumulating per second loses nothing the file could express.
///
/// WHAT MUTATION MAKES THIS RED: `series.push` instead of `add_to_series`.
#[test]
fn the_series_is_one_entry_per_second_and_never_one_per_line() {
    let f = &longest();

    let mut checked = 0;
    for p in &f.participants {
        /* STRICTLY ASCENDING, NEVER EQUAL. Equal offsets are the exact defect: two entries for one
         * second. Ascending also means a renderer can walk it without sorting. */
        for w in p.series.windows(2) {
            assert!(
                w[0].0 < w[1].0,
                "{:?} has two series entries at second {}, so the fold is appending per line",
                p.who,
                w[0].0
            );
        }
        /* THE SERIES ADDS BACK UP TO WHAT THE PARTICIPANT DEALT. Every damage event goes into
         * exactly one second, so this is an identity: a bucket that dropped or double counted
         * anything breaks it. */
        let summed: u64 = p.series.iter().map(|(_, amount)| amount).sum();
        assert_eq!(
            summed, p.dealt,
            "{:?}: the series sums to {summed} but the participant dealt {}",
            p.who, p.dealt
        );
        if !p.series.is_empty() {
            checked += 1;
        }
    }
    assert!(
        checked > 1,
        "fewer than two participants in the capture's first fight dealt anything, so this proves \
         nothing"
    );

    /* AND IT IS GENUINELY COARSER THAN THE LINES. The capture's longest fight is 423 combat lines
     * over 45 seconds, so any participant's series must be far shorter than the line count. */
    let longest = f
        .participants
        .iter()
        .map(|p| p.series.len())
        .max()
        .unwrap_or(0);
    assert!(
        longest <= 267,
        "a series has {longest} entries for a 266 second fight, so it is not per second"
    );
    assert!(
        longest > 1,
        "the longest series is {longest} entries, which cannot be a timeline"
    );
}

/// DEFECT: an event stamped with a wall clock, or with a negative offset.
///
/// The x axis of a timeline is a duration, so a mark carries seconds from the fight's own start.
/// The log has no zone offset, so anything that turned a stamp into an instant would be making a
/// timezone claim these bytes cannot support.
#[test]
fn every_mark_lands_inside_its_own_fight() {
    let fights = folded();
    let mut kinds = (0, 0, 0, 0);

    for f in &fights {
        let span = u32::try_from(f.seconds()).unwrap_or(u32::MAX);
        for e in &f.events {
            assert!(
                e.at <= span,
                "a mark at {}s in a fight that lasted {span}s",
                e.at
            );
            /* Every slot a mark names is a real participant of THAT fight. A mark carrying an index
             * into some other fight's vector would resolve to the wrong name and nothing else here
             * would catch it. */
            let slots: Vec<u32> = match e.what {
                What::Death { killer, victim } => vec![killer, victim],
                What::Ability { who, .. } | What::Berserk { who, .. } | What::Crit { who, .. } => {
                    vec![who]
                }
            };
            for s in slots {
                assert!(
                    (s as usize) < f.participants.len(),
                    "a mark names slot {s} in a fight with {} participants",
                    f.participants.len()
                );
            }
            match e.what {
                What::Death { .. } => kinds.0 += 1,
                What::Ability { .. } => kinds.1 += 1,
                What::Berserk { .. } => kinds.2 += 1,
                What::Crit { .. } => kinds.3 += 1,
            }
        }
    }

    /* THE CAPTURE REACHES ALL FOUR KINDS, or an arm could be broken and unnoticed. */
    assert!(kinds.0 > 0, "no death was marked");
    assert!(kinds.1 > 0, "no ability was marked");
    assert!(kinds.2 > 0, "no berserk was marked");
    assert!(kinds.3 > 0, "no crit was marked");

    /* THE MARKED DEATHS ARE THE COUNTED DEATHS. `Fight::deaths` and the marks are two readings of
     * one thing, and a screen showing both must not be able to disagree. */
    for f in &fights {
        let marked = f
            .events
            .iter()
            .filter(|e| matches!(e.what, What::Death { .. }))
            .count();
        assert_eq!(
            marked as u32, f.deaths,
            "the fight counts {} deaths and marked {marked}",
            f.deaths
        );
    }
}

/// DEFECT: stamping a fight with the zone named by the line that ENDED it.
///
/// A zone line is what cuts a fight, so the zone in that line is the one being entered. Reading it
/// as the fight's own zone puts every pull in the wrong place by exactly one zone.
///
/// AND THE TERMINATOR STATES ITS OWN GAP. Measured: the capture's third fight ends at 23:40:45 and
/// the zone line that closed it is at 23:41:53, sixty-eight seconds later on a sixty-one second
/// fight. A screen saying "ended: zoned" with no gap invites a reader to believe the zoning
/// interrupted the pull.
#[test]
fn a_fight_carries_the_zone_it_happened_in_and_a_zoned_end_states_its_gap() {
    let fights = folded();

    let zoned: Vec<&Fight<'_>> = fights.iter().filter(|f| f.ended == Ended::Zone).collect();
    assert!(
        !zoned.is_empty(),
        "no fight in the capture ends by zoning, so this test proves nothing"
    );
    for f in &zoned {
        let gap = f
            .zone_gap
            .expect("a fight ended by a zone line knows how long after its last swing that was");
        assert!(
            gap > 0,
            "a zone terminator with a zero gap is indistinguishable from one that interrupted the \
             pull"
        );
    }

    /* THE FIRST FIGHT OPENS BEFORE ANY ZONE LINE HAS BEEN SEEN, so its zone is honestly unknown
     * rather than guessed at from the next one. */
    assert!(
        fights[0].zone.is_none(),
        "the first fight in the file claims a zone ({:?}) that no line before it named",
        fights[0].zone
    );

    /* A fight opening AFTER a zone line carries that zone, not the next one. */
    let after: Option<&Fight<'_>> = fights.iter().skip(1).find(|f| f.zone.is_some());
    if let Some(f) = after {
        let z = f.zone.expect("checked");
        assert!(!z.is_empty());
        assert!(
            CAPTURE.contains(z),
            "the zone {z:?} is not a string that appears in the log"
        );
    }
}

/// The reader is in the capture's longest fight, and his series is the one a DPS timeline draws.
#[test]
fn the_readers_own_series_is_drawable() {
    let fight = longest();
    let me = fight
        .participants
        .iter()
        .find(|p| p.who == Actor::You)
        .expect("the reader fights in his own log");

    assert!(
        me.series.len() > 10,
        "a timeline needs more than {} points",
        me.series.len()
    );
    /* THE FIRST POINT IS AT OR AFTER ZERO, and the axis starts where the fight starts: the
     * offsets are from , so nothing can be negative and nothing can precede it. */
    let first = me
        .series
        .first()
        .map(|(at, _)| *at)
        .expect("a non-empty series");
    let last = me
        .series
        .last()
        .map(|(at, _)| *at)
        .expect("a non-empty series");
    assert!(last >= first);
    assert!(
        u64::from(last) <= me.series.len() as u64 * 300,
        "the axis is wildly out of scale"
    );
    assert!(
        me.series.iter().all(|(_, amount)| *amount > 0),
        "a second with no damage in it is not an entry; it is a gap the renderer draws"
    );
}

/// DEFECT: A TICK ON A TIMELINE FOR SOMETHING THAT HAPPENED AFTER THE FIGHT.
///
/// The liveness guard on a mark is `now >= last && now - last <= quiet`, and `last` is the last
/// COMBAT second, which is also `end_secs`. So it bounds a mark at `end_secs + quiet` and not at
/// `end_secs`. A berserk fading a few seconds after the final blow, with no further combat, is
/// inside that window and used to land past the end of a fight whose clock had stopped.
///
/// AND NOTHING DOWNSTREAM COULD TELL. `screens::widgets::timeline` divides by the span and clamps,
/// so an event past the end draws exactly on the closing edge, in the same pixel as a real
/// last-second event.
///
/// WHAT MUTATION MAKES THIS RED: `self.offset(now)` in `Fight::mark`, without the clamp.
#[test]
fn a_mark_never_lands_past_the_end_of_the_fight_it_is_in() {
    /* Two blows a second apart, then a berserk fade five seconds after the last one, then
     * nothing. The fight's clock stops at the second blow. */
    let log = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 20 points of damage.\n",
        "[Wed Jul 15 23:16:51 2026] You slash a dry bone skeleton for 20 points of damage.\n",
        "[Wed Jul 15 23:16:56 2026] Reviir is no longer berserk.\n",
    );

    /* WITH THE OWNER, or the berserk line names somebody the fold has never seen: it says
     * "Reviir", the damage lines say "You", and `find_slot` refuses to enrol a participant for
     * a mark. The first draft of this test left it off and the mark was dropped for that reason
     * rather than for the one under test, which is the shape of a fixture that passes by accident. */
    let mut f = Fights::new().with_owner(OWNER);
    for line in log.lines() {
        if let Some(e) = parse(line) {
            f.push(e);
        }
    }
    let done = f.finish();
    let fight = done.first().expect("one fight");

    assert_eq!(
        fight.seconds(),
        1,
        "the fight is the two blows and nothing after"
    );
    assert!(
        !fight.events.is_empty(),
        "the berserk fade was dropped entirely, which is the other way to get this wrong"
    );
    for e in &fight.events {
        assert!(
            i64::from(e.at) <= fight.seconds(),
            "an event is marked at second {} of a fight that lasted {}",
            e.at,
            fight.seconds()
        );
    }
}
