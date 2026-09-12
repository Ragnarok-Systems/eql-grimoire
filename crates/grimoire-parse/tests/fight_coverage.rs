//! Fight aggregation against the only real bytes that exist.
//!
//! `web/fixtures/eqlog-tail-200k.txt` is 2,385 log lines cut from the owner's own 61 MB
//! `eqlog_Reviir_freeport.txt`, which no longer exists on any machine here. Other players' names
//! were scrubbed and replaced with same-byte-length substitutes. It is the whole verification
//! corpus, so everything below is a measurement of it rather than of a fixture written to agree
//! with the code.
//!
//! The counts are asserted EXACTLY, not as floors. A floor passes when a rule quietly stops
//! firing; an exact count goes red.

use grimoire_parse::combat::{parse, DamageKind, Event, Reading};
use grimoire_parse::fights::{seconds, Ended, Fight, Fights, QUIET_SECONDS};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

fn fold(quiet: i64) -> Vec<Fight<'static>> {
    let mut f = Fights::new().with_quiet(quiet);
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            f.push(e);
        }
    }
    f.finish()
}

/// One fight, reduced to the things a change to the aggregator would move.
#[derive(PartialEq, Eq, Debug)]
struct Summary<'a> {
    start: &'a str,
    seconds: i64,
    damage: u64,
    deaths: u32,
    ended: Ended,
    headline: Option<&'a str>,
}

fn summarise<'a>(f: &Fight<'a>) -> Summary<'a> {
    Summary {
        start: f.start,
        seconds: f.seconds(),
        damage: f.damage,
        deaths: f.deaths,
        ended: f.ended,
        headline: f.headline(),
    }
}

/// The whole capture, twelve encounters, and each one is a thing that happened.
///
/// IT READ AS FOUR UNTIL A KILL COULD CLOSE A FIGHT. The camp at the top of the file never goes
/// quiet for thirty seconds, so eleven pulls folded into one 266 second run whose DPS was the
/// average of a camp rather than of a fight. `Ended::Killed` cuts each pull where the log says it
/// ended, and `HOLD_SECONDS` keeps the ones that run into each other together: a mob that engages
/// the reader within six seconds of the last one dying is the same encounter, not the next one.
#[test]
fn the_capture_cuts_into_twelve_encounters() {
    let fights = fold(QUIET_SECONDS);
    let seen: Vec<Summary<'_>> = fights.iter().map(summarise).collect();

    assert_eq!(
        seen,
        vec![
            Summary {
                start: "Wed Jul 15 23:16:50 2026",
                seconds: 19,
                damage: 1_533,
                deaths: 3,
                ended: Ended::Killed,
                headline: Some("a lurking mummy"),
            },
            Summary {
                start: "Wed Jul 15 23:17:10 2026",
                seconds: 44,
                damage: 3_404,
                deaths: 7,
                ended: Ended::Killed,
                headline: Some("A carrion ghoul"),
            },
            Summary {
                start: "Wed Jul 15 23:17:55 2026",
                seconds: 41,
                damage: 3_928,
                deaths: 5,
                ended: Ended::Killed,
                headline: Some("a lurking mummy"),
            },
            Summary {
                start: "Wed Jul 15 23:18:37 2026",
                seconds: 68,
                damage: 3_167,
                deaths: 5,
                ended: Ended::Killed,
                headline: Some("A dry bone skeleton"),
            },
            Summary {
                start: "Wed Jul 15 23:19:47 2026",
                seconds: 27,
                damage: 1_394,
                deaths: 2,
                ended: Ended::Killed,
                headline: Some("A dry bone skeleton"),
            },
            Summary {
                start: "Wed Jul 15 23:20:16 2026",
                seconds: 49,
                damage: 2_793,
                deaths: 4,
                ended: Ended::Killed,
                headline: Some("A crazed ghoul"),
            },
            Summary {
                start: "Wed Jul 15 23:21:06 2026",
                seconds: 10,
                damage: 307,
                deaths: 0,
                ended: Ended::Quiet,
                headline: Some("A crazed ghoul"),
            },
            Summary {
                start: "Wed Jul 15 23:24:04 2026",
                seconds: 2,
                damage: 304,
                deaths: 1,
                ended: Ended::Killed,
                headline: Some("A greater skeleton"),
            },
            Summary {
                start: "Wed Jul 15 23:24:14 2026",
                seconds: 20,
                damage: 734,
                deaths: 2,
                ended: Ended::Killed,
                headline: Some("A tormented dead"),
            },
            Summary {
                start: "Wed Jul 15 23:24:38 2026",
                seconds: 4,
                damage: 310,
                deaths: 0,
                ended: Ended::Zone,
                headline: Some("A dry bone skeleton"),
            },
            Summary {
                start: "Wed Jul 15 23:39:44 2026",
                seconds: 61,
                damage: 1_700,
                deaths: 1,
                ended: Ended::Zone,
                headline: Some("Guard Ullindin"),
            },
            Summary {
                start: "Wed Jul 15 23:45:42 2026",
                seconds: 36,
                damage: 121,
                deaths: 2,
                ended: Ended::EndOfLog,
                headline: Some("a skeleton"),
            },
        ],
        "the fights the capture cuts into changed"
    );
}

/// THE BOUNDARY CONSTANT, argued rather than asserted.
///
/// [`QUIET_SECONDS`] is only defensible if it is not sitting on an edge. The gaps between combat
/// lines leave a 102-second hole from 30s to 131s, and this walks every window from 1 to 400 to
/// show where the answer actually changes. If the constant ever drifts to a value where the
/// answer is unstable, this goes red with the plateau map printed beside it.
#[test]
fn the_quiet_window_sits_in_the_middle_of_a_hole_in_the_data() {
    let mut runs: Vec<(i64, i64, usize)> = Vec::new();
    for q in 1..=400 {
        let n = fold(q).len();
        match runs.last_mut() {
            Some(r) if r.2 == n => r.1 = q,
            _ => runs.push((q, q, n)),
        }
    }
    let map: Vec<String> = runs
        .iter()
        .map(|(lo, hi, n)| format!("{lo}..={hi} -> {n}"))
        .collect();
    println!("fight count by quiet window: {}", map.join(", "));

    let plateau = runs
        .iter()
        .find(|(lo, hi, _)| *lo <= QUIET_SECONDS && QUIET_SECONDS <= *hi)
        .copied()
        .expect("the chosen window is somewhere on the map");
    assert_eq!(
        plateau,
        (29, 167, 12),
        "the plateau the constant stands on moved: {}",
        map.join(", ")
    );
    assert!(
        plateau.0 < QUIET_SECONDS && QUIET_SECONDS < plateau.1,
        "the window is on the edge of its plateau rather than inside it"
    );

    // And the edges are real: one second either side of the plateau, the answer changes.
    assert_eq!(fold(28).len(), 13, "28s splits the 29s pause inside a camp");
    assert_eq!(fold(168).len(), 11, "168s swallows a walk between zones");
}

/// The gap distribution the constant was chosen from, re-measured here so the comment on
/// [`QUIET_SECONDS`] cannot rot away from the bytes it cites.
#[test]
fn the_gap_distribution_is_what_the_constant_claims_it_is() {
    let mut times: Vec<i64> = Vec::new();
    for raw in CAPTURE.lines() {
        let Some(e) = parse(raw) else { continue };
        let Reading::Event(ev) = e.reading else {
            continue;
        };
        if matches!(
            ev,
            Event::Damage(_) | Event::Swing(_) | Event::Heal(_) | Event::Death { .. }
        ) {
            times.push(seconds(e.at).expect("every stamp in the capture reads"));
        }
    }
    assert_eq!(times.len(), 1_779, "combat activity lines");
    let gaps: Vec<i64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    assert_eq!(gaps.len(), 1_778);
    assert!(
        gaps.iter().all(|&g| g >= 0),
        "the capture is not monotonic after all"
    );

    let short = gaps.iter().filter(|&&g| g <= 1).count();
    let middle = gaps.iter().filter(|&&g| (2..=29).contains(&g)).count();
    let hole = gaps.iter().filter(|&&g| (30..=131).contains(&g)).count();
    let long = gaps.iter().filter(|&&g| g >= 132).count();
    assert_eq!(
        (short, middle, hole, long),
        (1_729, 43, 0, 6),
        "the shape of the gap distribution moved"
    );
    assert_eq!(
        gaps.iter().copied().filter(|&g| g < 132).max(),
        Some(29),
        "the largest gap inside a fight"
    );
    assert_eq!(
        gaps.iter().copied().filter(|&g| g >= 30).min(),
        Some(132),
        "the smallest gap between fights"
    );
}

/// Not one point of damage is lost, invented, or double counted.
#[test]
fn every_point_of_damage_reconciles() {
    let fights = fold(QUIET_SECONDS);
    let mut capture_damage = 0u64;
    for raw in CAPTURE.lines() {
        if let Some(Reading::Event(Event::Damage(d))) = parse(raw).map(|e| e.reading) {
            capture_damage += u64::from(d.amount);
        }
    }
    assert_eq!(capture_damage, 19_707, "damage in the capture");

    for f in &fights {
        let dealt: u64 = f.participants.iter().map(|p| p.dealt).sum();
        let taken: u64 = f.participants.iter().map(|p| p.taken).sum();
        assert_eq!(
            (dealt, taken),
            (f.damage, f.damage),
            "a fight's columns disagree with its total: [{}]",
            f.start
        );
    }

    /* NOTHING IS ORPHANED BY ENDING A FIGHT ON ITS KILL, BECAUSE THE FIGHT IS HELD OPEN FIRST.
     *
     * The first cut closed the run on the death line itself, which left gaps between pulls, and
     * self-inflicted and environmental damage is graded `Beat::Extends`: it can join a fight but
     * never open one, so a line landing in a gap was credited to nobody. One did, `You hurt
     * yourself for 2 points.` at 23:24:25, and this test recorded it as an eighth orphan.
     *
     * `HOLD_SECONDS` removed it rather than excusing it. The fight stays open for six seconds
     * after its last opponent dies, so a blow already in the air lands in the fight that earned
     * it, and the twelve points outside every fight are the seven falling-damage lines again and
     * nothing else. */
    let inside: u64 = fights.iter().map(|f| f.damage).sum();
    assert_eq!(inside, 19_695);
    assert_eq!(
        capture_damage - inside,
        12,
        "the 12 points outside every fight are the seven falling-damage lines, and nothing else"
    );
    assert_eq!(inside + 12, capture_damage, "every point is accounted for");
}

/// EXACTLY what falls outside every fight, line by line, because "some lines were dropped" is
/// not a thing anyone can check. Nine lines, and each of the three reasons is a decision this
/// module made on purpose.
#[test]
fn what_falls_outside_a_fight_is_named_and_nothing_else_does() {
    let fights = fold(QUIET_SECONDS);
    let spans: Vec<(i64, i64)> = fights
        .iter()
        .map(|f| {
            (
                seconds(f.start).expect("start"),
                seconds(f.end).expect("end"),
            )
        })
        .collect();

    let mut outside: Vec<(&str, &str)> = Vec::new();
    let mut activity = 0u32;
    for raw in CAPTURE.lines() {
        let Some(e) = parse(raw) else { continue };
        let Reading::Event(ev) = e.reading else {
            continue;
        };
        let label = match ev {
            Event::Damage(d) => match d.kind {
                DamageKind::SelfInflicted => "self damage",
                DamageKind::Environmental => "falling",
                _ => "damage",
            },
            Event::Swing(_) => "swing",
            Event::Heal(_) => "heal",
            Event::Death { .. } => "death",
            _ => continue,
        };
        activity += 1;
        let t = seconds(e.at).expect("stamp");
        if !spans.iter().any(|&(a, b)| a <= t && t <= b) {
            outside.push((label, raw));
        }
    }

    let folded: u32 = fights.iter().map(|f| f.lines).sum();
    /* TWO FEWER LINES FOLD THAN BEFORE `Ended::Killed`, AND BOTH ARE ACCOUNTED FOR. A pull now ends
     * at its kill, so there are gaps between pulls that used to be swallowed by one long run, and a
     * `Beat::Extends` line landing in one cannot open a fight to join. The pair is the 2 point
     * self-inflicted line at 23:24:25 and the falling-damage line beside it; `every_point_of_damage_reconciles`
     * names the damage they carry. */
    assert_eq!((activity, folded), (1_779, 1_768));
    let labels: Vec<&str> = outside.iter().map(|&(l, _)| l).collect();
    assert_eq!(
        labels,
        vec![
            // The reader fell down a cliff twice on the run to Dagnor's Cauldron. Seven lines,
            // twelve points, no opponent named in any of them. A cliff is not an encounter.
            "falling", "falling", "falling", "falling", "falling", "falling", "falling",
            // Two NPCs healing each other across the zone with the reader nowhere near.
            "heal",
            // `An orc centurion has been slain by Guard Topplo!`, one second before the reader's
            // own fight opens. A death alone does not start a fight, so this one is not credited
            // to a fight the reader was in.
            "death",
        ],
        "the residue outside the fights changed:\n{}",
        outside
            .iter()
            .map(|(l, raw)| format!("  {l:<12} {raw}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Every self-damage line in the capture lands inside a fight, so these bytes do not exercise
    // the rule that grades it as an extender. Stated here so nobody reads the green above as
    // evidence that they do.
    assert!(!labels.contains(&"self damage"));

    /* THE DEATHS ADD UP: 34 in the capture, 32 folded, 2 that belong to no fight of the reader's.
     *
     * BOTH OF THE TWO ARE SOMEBODY ELSE'S KILL LANDING BETWEEN HIS PULLS. `An orc centurion has
     * been slain by Guard Topplo!` is named in the residue above and always was. The second,
     * `A death beetle has been slain by Rykabe!` at 23:17:55, used to fall inside the camp while
     * the camp was one fight; now that each pull ends on its own kill it lands in the gap between
     * two of them. A death opens no fight, by the same rule that keeps Guard Topplo's kill out,
     * so it is credited to nobody rather than to whichever pull happened to be nearest.
     *
     * ONLY ONE OF THE TWO IS LABELLED, because the labelling above asks whether any fight's window
     * contains the line's second, and 23:17:55 is the second the next pull opens in. The line is
     * inside that window and was not folded into it: a death arriving a line before the swing that
     * opens a fight cannot join the fight it precedes. */
    let deaths: u32 = fights.iter().map(|f| f.deaths).sum();
    assert_eq!(
        (deaths, labels.iter().filter(|l| **l == "death").count()),
        (32, 1)
    );
}

#[test]
fn no_fight_is_empty_and_none_of_them_overlap() {
    let fights = fold(QUIET_SECONDS);
    let mut previous_end = i64::MIN;
    for f in &fights {
        assert!(f.lines > 0, "an empty fight: [{}]", f.start);
        assert!(f.participants.len() >= 2, "a solo fight: [{}]", f.start);
        assert!(f.damage > 0, "a fight with no damage in it: [{}]", f.start);
        assert!(f.headline().is_some(), "a nameless fight: [{}]", f.start);
        assert!(f.dps().is_finite() && f.dps() > 0.0);
        let start = seconds(f.start).expect("start");
        let end = seconds(f.end).expect("end");
        assert!(start <= end, "a fight that ends before it starts");
        /* SHARING THE BOUNDARY SECOND IS ALLOWED, OVERLAPPING IS NOT, and the difference is the
         * log's resolution rather than a softened rule. The file stamps to the second, and a kill
         * and the first swing at the next mob genuinely land inside one: the capture opens with
         * `You have slain a dry bone skeleton!` and `You try to cleave a barbed bone skeleton`
         * both at 23:16:50. Demanding a strictly later start would forbid the log from saying
         * what it plainly said. A start EARLIER than the previous end is still a real overlap and
         * still fails here. */
        assert!(
            start >= previous_end,
            "fights overlap: [{}] begins inside the one before it",
            f.start
        );
        previous_end = end;
    }
}

/// Every stamp in the capture is readable, so nothing was silently skipped for want of a clock.
#[test]
fn no_stamp_in_the_capture_defeats_the_reader() {
    let mut f = Fights::new();
    let mut stamped = 0u32;
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            stamped += 1;
            assert!(seconds(e.at).is_some(), "unreadable stamp: {:?}", e.at);
            f.push(e);
        }
    }
    assert_eq!(stamped, 2_385, "stamped lines in the capture");
    assert_eq!(f.unreadable(), 0);
    assert_eq!(f.finish().len(), 12);
}

/// The owner is in the log under two names, and only the caller knows they are one person.
#[test]
fn naming_the_owner_merges_the_two_ways_the_log_writes_him() {
    let mut with = Fights::new().with_owner("Reviir");
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            with.push(e);
        }
    }
    let merged = with.finish();
    assert_eq!(merged.len(), 12, "folding a name must not move a boundary");

    let reviir_rows = |fs: &[Fight<'_>]| {
        fs.iter()
            .flat_map(|f| f.participants.iter())
            .filter(|p| p.name() == Some("Reviir"))
            .count()
    };
    assert_eq!(
        reviir_rows(&fold(QUIET_SECONDS)),
        2,
        "with no owner given, Reviir is his own row in each fight the log names him in, which is \n         the honest answer"
    );
    assert_eq!(
        reviir_rows(&merged),
        0,
        "and naming him folds that row away"
    );

    // The damage is preserved by the fold, not just the row count.
    let before: u64 = fold(QUIET_SECONDS).iter().map(|f| f.damage).sum();
    let after: u64 = merged.iter().map(|f| f.damage).sum();
    assert_eq!(before, after);
}
