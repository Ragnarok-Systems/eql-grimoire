//! The coverage gate for the combat parser, run over the only real bytes there are.
//!
//! `web/fixtures/eqlog-tail-200k.txt` is 204,797 bytes of one character's play, cut from a
//! 61 MB log that no longer exists on any machine here. Read its provenance header before
//! trusting anything below it.
//!
//! Three claims are separated on purpose, because collapsing them hides the interesting one:
//!
//! * **TYPED** became an [`Event`]. Something downstream can count it.
//! * **IGNORED** was recognised and dropped on purpose, with a named reason.
//! * **FLAVOUR** was recognised as a class and cannot be typed without a message-to-spell table.
//! * **UNRECOGNISED** is a hole in the parser. This is the number that must stay near zero.
//!
//! A parser that reports flavour as unrecognised understates its own coverage; one that drops
//! it silently cannot report coverage at all. Hence four buckets and not two.
//!
//! The fixture is embedded with `include_str!` rather than read at run time: `grimoire-parse`
//! is a pure crate under the purity gate and may not name `std::fs`, and a compile-time include
//! keeps that true of the test binary as well.

use grimoire_parse::combat::{
    parse, Actor, DamageKind, Event, Flavour, Ignored, Reading, TargetKind,
};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

#[derive(Default)]
struct Tally {
    typed: usize,
    ignored: usize,
    flavour: usize,
    unrecognised: Vec<&'static str>,
}

fn tally() -> Tally {
    let mut t = Tally::default();
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        match entry.reading {
            Reading::Event(_) => t.typed += 1,
            Reading::Ignored(_) => t.ignored += 1,
            Reading::Flavour(_) => t.flavour += 1,
            Reading::Unrecognised => t.unrecognised.push(raw),
        }
    }
    t
}

/// Every line below the provenance header carries a well-formed stamp, so the count of lines
/// the parser accepted at all is itself an assertion worth making: if the stamp rule ever
/// breaks, every other number in this file becomes meaningless in the same direction.
#[test]
fn every_body_line_is_stamped() {
    let bodies = CAPTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .count();
    let stamped = CAPTURE.lines().filter(|l| parse(l).is_some()).count();
    assert_eq!(
        stamped,
        bodies,
        "{} of {bodies} body lines carried no readable timestamp",
        bodies - stamped
    );
    assert_eq!(
        bodies, 2385,
        "the fixture is not the fixture this test was written against"
    );
}

#[test]
fn the_capture_is_read_end_to_end() {
    let t = tally();
    let total = t.typed + t.ignored + t.flavour + t.unrecognised.len();
    let pct = |n: usize| 100.0 * n as f64 / total as f64;

    let mut sample = String::new();
    for line in t.unrecognised.iter().take(20) {
        sample.push_str("\n    ");
        sample.push_str(line);
    }

    let report = format!(
        "of {total} stamped lines: {} typed ({:.1}%), {} ignored ({:.1}%), \
         {} flavour ({:.1}%), {} UNRECOGNISED ({:.1}%){sample}",
        t.typed,
        pct(t.typed),
        t.ignored,
        pct(t.ignored),
        t.flavour,
        pct(t.flavour),
        t.unrecognised.len(),
        pct(t.unrecognised.len()),
    );
    println!("{report}");

    // The floor on typed lines. The capture's combat payload is 1,779 lines by hand count;
    // state, refusal, target and zone lines lift the typed total above that.
    assert!(
        t.typed >= 1900,
        "typed line count fell below its floor: {report}"
    );
    // The ceiling on holes. Zero is the standing expectation: every line in this capture has
    // been accounted for, so any regression that drops a shape shows up here as a name.
    assert!(
        t.unrecognised.is_empty(),
        "the parser has holes in it: {report}"
    );
}

/// The counts of each damage family, against the hand inventory of the same bytes.
///
/// These are exact rather than floors. A floor would pass while a shape silently halved.
#[test]
fn the_damage_families_come_out_at_their_measured_counts() {
    let (mut melee, mut shield, mut dot, mut spell, mut selfhurt, mut env) = (0, 0, 0, 0, 0, 0);
    let (mut swings, mut heals, mut deaths) = (0, 0, 0);
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        match entry.reading {
            Reading::Event(Event::Damage(d)) => match d.kind {
                DamageKind::Melee { .. } => melee += 1,
                DamageKind::Shield { .. } => shield += 1,
                DamageKind::Dot { .. } => dot += 1,
                DamageKind::Spell { .. } => spell += 1,
                DamageKind::SelfInflicted => selfhurt += 1,
                DamageKind::Environmental => env += 1,
            },
            Reading::Event(Event::Swing(_)) => swings += 1,
            Reading::Event(Event::Heal(_)) => heals += 1,
            Reading::Event(Event::Death { .. }) => deaths += 1,
            _ => {}
        }
    }
    let got = [
        melee, shield, dot, spell, selfhurt, env, swings, heals, deaths,
    ];
    let want = [712, 373, 44, 13, 97, 7, 470, 29, 34];
    assert_eq!(
        got, want,
        "damage families drifted\n  \
         [melee, shield, dot, spell, self, env, swings, heals, deaths]\n  \
         got  {got:?}\n  want {want:?}"
    );
}

/// The singular trap, measured rather than asserted in the abstract.
///
/// 135 melee lines in this capture say `1 point of damage` and 546 say `N points of damage`.
/// A matcher keyed on the plural drops all 135 and this test says so by name.
#[test]
fn every_one_damage_melee_hit_survives() {
    let singular_in_bytes = CAPTURE
        .lines()
        .filter(|l| l.contains(" for 1 point of damage."))
        .count();
    let singular_parsed = CAPTURE
        .lines()
        .filter_map(parse)
        .filter(|e| {
            matches!(
                e.reading,
                Reading::Event(Event::Damage(d))
                    if d.amount == 1 && matches!(d.kind, DamageKind::Melee { .. })
            )
        })
        .count();
    assert_eq!(
        singular_in_bytes, 135,
        "the fixture changed under this test"
    );
    assert_eq!(
        singular_parsed,
        singular_in_bytes,
        "{} of {singular_in_bytes} `1 point of damage` lines were dropped by the plural trap",
        singular_in_bytes - singular_parsed
    );
}

/// The same trap on the damage-shield line, where it is far easier to miss by eye: exactly one
/// singular against 372 plurals.
#[test]
fn the_one_singular_damage_shield_line_survives() {
    let want = "[Wed Jul 15 23:18:23 2026] A dark boned skeleton is pierced by Tanefilo's thorns \
                for 1 point of non-melee damage.";
    let line = CAPTURE
        .lines()
        .find(|l| l.contains(" for 1 point of non-melee damage."))
        .expect("the capture's single singular shield line");
    assert_eq!(line.trim_end(), want);
    let entry = parse(line).expect("stamped");
    match entry.reading {
        Reading::Event(Event::Damage(d)) => {
            assert_eq!(d.amount, 1);
            assert_eq!(d.source, Actor::Named("Tanefilo"));
            assert_eq!(d.target, Actor::Named("A dark boned skeleton"));
            assert!(matches!(d.kind, DamageKind::Shield { effect: "thorns" }));
        }
        other => panic!("the singular shield line parsed as {other:?}"),
    }
}

/// Damage shields point the other way round, and 21 percent of the capture's damage rides on
/// it. Measured here as a whole-corpus invariant rather than on one line: the log owner never
/// appears as the *victim* of his own thorns.
#[test]
fn shield_damage_is_credited_to_the_wearer_across_the_whole_capture() {
    let mut wrong_way = Vec::new();
    let mut to_you = 0usize;
    let mut from_you = 0usize;
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Event(Event::Damage(d)) = entry.reading else {
            continue;
        };
        if !matches!(d.kind, DamageKind::Shield { .. }) {
            continue;
        }
        match (d.source, d.target) {
            (Actor::You, Actor::You) => wrong_way.push(raw),
            (Actor::You, _) => from_you += 1,
            (_, Actor::You) => to_you += 1,
            _ => {}
        }
    }
    assert!(
        wrong_way.is_empty(),
        "a shield line was read as the reader hitting himself: {wrong_way:?}"
    );
    // `YOUR thorns` fires 127 times, all outbound. `YOU are pierced by ...` fires 9, all inbound.
    assert_eq!(
        (from_you, to_you),
        (127, 9),
        "shield direction drifted: {from_you} out of the reader, {to_you} into him"
    );
}

/// Names with spaces and apostrophes survive whole, and a name that is a strict prefix of
/// another never absorbs it. `Tanefi` and `Tanefilo` are both active combatants here.
#[test]
fn names_are_never_split_or_merged() {
    let mut tanefi = 0usize;
    let mut tanefilo = 0usize;
    let mut battlemaster = 0usize;
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Event(Event::Damage(d)) = entry.reading else {
            continue;
        };
        for who in [d.source, d.target] {
            match who.name() {
                Some("Tanefi") => tanefi += 1,
                Some("Tanefilo") => tanefilo += 1,
                Some("Torklar Battlemaster") => battlemaster += 1,
                _ => {}
            }
        }
    }
    assert!(tanefi > 0, "Tanefi was swallowed by Tanefilo");
    assert!(
        tanefilo > tanefi,
        "Tanefilo should be the busier of the two"
    );
    assert!(
        battlemaster > 0,
        "a two-word name did not survive as one actor"
    );
}

/// Chat is matched before anything else, so a player cannot type an event into existence.
#[test]
fn chat_is_never_read_as_combat() {
    let mut quoted = 0usize;
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        if !raw.ends_with('\'') {
            continue;
        }
        quoted += 1;
        assert!(
            matches!(
                entry.reading,
                Reading::Ignored(Ignored::Chat) | Reading::Ignored(Ignored::Speech)
            ),
            "a quoted line was read as something else: {raw}\n  -> {:?}",
            entry.reading
        );
    }
    assert_eq!(quoted, 205, "the capture's quoted-line count changed");
}

/// `, but ` is the miss rule's own delimiter and it also occurs inside free text. Nine lines in
/// this capture carry it without being a swing: four inside chat, five inside a consider line's
/// `looks quite risky, but might be worth a try` clause. An unanchored miss splitter reads all
/// nine as swings, inventing an attacker called
/// `Swiftfingers glowers at you dubiously -- looks quite risky`.
#[test]
fn a_comma_but_outside_a_swing_never_becomes_one() {
    let mut decoys = 0usize;
    for raw in CAPTURE.lines() {
        if !raw.contains(", but ") || raw.contains(" tries to ") || raw.contains("You try to ") {
            continue;
        }
        decoys += 1;
        let entry = parse(raw).expect("stamped");
        assert!(
            !matches!(entry.reading, Reading::Event(Event::Swing(_))),
            "a decoy `, but ` was read as a swing: {raw}\n  -> {:?}",
            entry.reading
        );
    }
    assert_eq!(decoys, 9, "the capture's decoy count changed");
}

/// The trailing-paren shapes that are not modifiers stay intact. All four are in this capture.
#[test]
fn a_trailing_paren_that_is_not_a_modifier_is_left_alone() {
    let cases = [
        (
            "[Wed Jul 15 23:47:12 2026] Glorin Binfurr regards you indifferently -- what would \
             you like your tombstone to say? (Lvl: 35)",
            Reading::Ignored(Ignored::Consider),
        ),
        (
            "[Wed Jul 15 23:16:50 2026] You gain experience! (1.898%)",
            Reading::Ignored(Ignored::Experience),
        ),
        (
            "[Wed Jul 15 23:44:38 2026] You have become better at Swimming! (52)",
            Reading::Ignored(Ignored::SkillUp),
        ),
    ];
    for (line, want) in cases {
        assert_eq!(parse(line).expect("stamped").reading, want, "on {line}");
    }
    let blocked = parse(
        "[Wed Jul 15 23:53:02 2026] Your Shield of Fire spell did not take hold. \
         (Blocked by Shield of Barbs.)",
    )
    .expect("stamped");
    assert_eq!(
        blocked.reading,
        Reading::Event(Event::CastBlocked {
            spell: "Shield of Fire",
            blocked_by: "Shield of Barbs"
        })
    );
}

/// Falling damage is counted once. The capture pairs `You were hit by non-melee` with
/// `YOU were injured by falling.` seven times out of seven, and emitting damage for both would
/// double every fall.
#[test]
fn falling_damage_is_not_double_counted() {
    let hits = CAPTURE
        .lines()
        .filter_map(parse)
        .filter(|e| {
            matches!(
                e.reading,
                Reading::Event(Event::Damage(d)) if d.kind == DamageKind::Environmental
            )
        })
        .count();
    let labels = CAPTURE
        .lines()
        .filter_map(parse)
        .filter(|e| e.reading == Reading::Ignored(Ignored::FallingCause))
        .count();
    assert_eq!((hits, labels), (7, 7));
}

/// The flavour lines that shadow a numeric heal are not counted as heals.
#[test]
fn heal_flavour_does_not_shadow_the_numeric_heal() {
    let flavour = CAPTURE
        .lines()
        .filter_map(parse)
        .filter(|e| e.reading == Reading::Flavour(Flavour::HealOnOther))
        .count();
    assert_eq!(flavour, 14, "the heal flavour class drifted");
}

/// `Targeted (NPC):` is the only line in the capture that declares an entity to be an NPC, so
/// it earns a type rather than an ignore.
#[test]
fn the_target_declarations_are_kept() {
    let mut npc = 0usize;
    let mut merchant = 0usize;
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            if let Reading::Event(Event::Targeted { kind, name }) = e.reading {
                assert!(!name.is_empty());
                match kind {
                    TargetKind::Npc => npc += 1,
                    TargetKind::Merchant => merchant += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((npc, merchant), (5, 5));
}

/// The line carrying the capture's three replacement characters parses without panicking and
/// without cutting a codepoint in half. Byte-index slicing is only safe here because every
/// boundary comes from a found delimiter.
#[test]
fn the_non_ascii_line_is_survived() {
    let line = CAPTURE
        .lines()
        .find(|l| l.contains('\u{fffd}'))
        .expect("the capture's one non-ASCII line");
    let entry = parse(line).expect("stamped");
    assert_eq!(entry.reading, Reading::Ignored(Ignored::Chat));
}
