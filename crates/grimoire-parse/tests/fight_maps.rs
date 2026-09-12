//! THE PER-PARTICIPANT MAPS, MEASURED AGAINST THE REFERENCE CAPTURE.
//!
//! `Participant` gained five collections so a detailed panel can be drawn without a second parse:
//! what each entity hit WITH, what it hit, by which element, how its swings were stopped, and how
//! many of its melee lines were criticals. This file is the measurement of all five over the only
//! real bytes that exist.
//!
//! WHY IT IS AN INTEGRATION TEST AND NOT A UNIT ONE. Every number here is a claim about 2,414 lines
//! of somebody's actual play. A hand-built three-line log can prove that a counter increments; only
//! the capture can prove the counter is counting the right thing, because only the capture contains
//! the shapes that make the naive version wrong: two spellings of one mob, a crit that rides a heal,
//! and swings stopped by the defender rather than missed by the attacker.
use grimoire_parse::combat::{parse, Actor};
use grimoire_parse::fights::{Fight, Fights, NameKind, Participant};

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
/// `fights[0]` was the whole camp while nothing but a long quiet could end a fight. `Ended::Killed`
/// cuts each pull at its kill, so index 0 is now the four line remnant of the pull the capture opens
/// inside, and these maps need the fight with enough in it to measure.
fn biggest() -> Fight<'static> {
    folded()
        .into_iter()
        .max_by_key(|f| f.lines)
        .expect("the capture has fights in it")
}

fn owner_of<'a>(f: &'a Fight<'a>) -> &'a Participant<'a> {
    f.participants
        .iter()
        .find(|p| p.who == Actor::You)
        .expect("the reader is in his own first fight")
}

/// DEFECT: `hits` counting SWINGS as well as landed damage lines.
///
/// `grimoire-forge`'s own `named` counter does exactly that, which is why its `slash` reads 148
/// against the owner's 63 landed slashes: a 2.3x overstatement that would put a hit count on the
/// Ability Breakdown panel more than twice the real one. Nothing here counts a swing.
///
/// AND THE TWO SPELLINGS ARE TWO ROWS, DELIBERATELY. `slash` is the log's base form and only ever
/// has the attacker `You`; `slashes` is the third person and belongs to everybody else. They are
/// separate rows of a group table, not a merge bug, and stemming them would be this crate asserting
/// that two words the log printed are one word.
#[test]
fn what_each_entity_hit_with_is_the_logs_own_word_and_counts_only_landed_lines() {
    let big = biggest();
    let me = owner_of(&big);

    let by = |key: &str| me.by_name.iter().find(|t| t.key == key).copied();

    let slash = by("slash").expect("the owner slashes in his own first fight");
    assert_eq!(slash.kind, NameKind::Melee);
    assert!(
        slash.hits > 0 && slash.hits < 100,
        "slash hits = {}, and the forge's swing-counting version reads 148 here",
        slash.hits
    );

    /* EVERY NAMED TALLY ADDS UP TO WHAT THE PARTICIPANT DEALT, minus the two kinds the log names as
     * nothing (falling damage and hurting yourself). A tally that double counted, or that missed an
     * arm of `DamageKind`, breaks this and nothing else here would notice. */
    let named: u64 = me.by_name.iter().map(|t| t.amount).sum();
    assert!(
        named <= me.dealt,
        "the named tallies ({named}) exceed what the owner dealt ({}), so something is counted \
         twice",
        me.dealt
    );

    /* THE CRITS ARE MELEE ONLY AND THAT IS LOAD BEARING. The `(Critical)` flag also rides heals, and
     * a `landed - crits` subtraction over a healer goes negative. */
    assert!(
        me.melee_crits <= me.landed,
        "melee crits ({}) exceed landed melee lines ({}), so a heal's crit is being counted as a \
         melee one and any Normal Hit slice built by subtraction goes negative",
        me.melee_crits,
        me.landed
    );

    /* Nothing in the file has a `NameKind::Spell` for the owner: he casts nothing outgoing in these
     * bytes. Asserted so a later change that starts inventing spell rows for him is caught. */
    assert!(
        !me.by_name.iter().any(|t| t.kind == NameKind::Spell),
        "the owner casts no direct damage spell in this capture: {:?}",
        me.by_name
    );
}

/// DEFECT: KEYING THE TARGET MAP ON A NAME, WHICH SPLITS ONE MOB INTO TWO ROWS.
///
/// This is the finding the whole map's shape rests on. The capture writes `a dry bone skeleton`
/// 121 times and `A dry bone skeleton` 168 times, because a sentence-initial article is
/// capitalised. `Fights::same` folds them with `eq_ignore_ascii_case`, so they are ALREADY one
/// participant; a map keyed on the string would split them back apart, and every percentage on a
/// Targets panel would be computed against half a mob.
///
/// WHAT MUTATION MAKES THIS RED: keying `TargetTally` on `&str` or on the actor's name.
#[test]
fn the_target_map_keys_on_a_slot_so_two_spellings_of_one_mob_are_one_row() {
    let big = biggest();
    let f = &big;
    let me = owner_of(f);

    /* The capture really does contain both spellings, or this test proves nothing. */
    assert!(
        CAPTURE.contains("a dry bone skeleton") && CAPTURE.contains("A dry bone skeleton"),
        "the fixture no longer carries both spellings, so this test cannot catch the split"
    );

    /* Every slot the owner hit is a real index into the participant vector, and no slot appears
     * twice. A name-keyed map would show two entries resolving to the same participant. */
    let mut slots: Vec<u32> = me.by_target.iter().map(|t| t.slot).collect();
    let n = slots.len();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots.len(), n, "the same target appears twice in one map");
    for t in &me.by_target {
        assert!(
            (t.slot as usize) < f.participants.len(),
            "slot {} is not a participant of this fight",
            t.slot
        );
    }

    /* WHAT HE DEALT, SPLIT BY TARGET, ADDS BACK UP TO WHAT HE DEALT. Every damage line has exactly
     * one target, so this is an identity and not an approximation. */
    let split: u64 = me.by_target.iter().map(|t| t.amount).sum();
    assert_eq!(
        split, me.dealt,
        "the per-target split ({split}) does not add up to the owner's total ({})",
        me.dealt
    );
}

/// DEFECT: A `Miss` SLICE BUILT AS `swings - landed`.
///
/// For the owner in this capture that subtraction is 110, and a fifth of it is the TARGET parrying
/// or dodging. A donut built that way credits the defender's skill to the attacker's aim and is
/// about a fifth wrong. `Outcomes` sits on the ATTACKER and names each reason, which is the only
/// way the panel can be right.
///
/// AND IT IS NOT `avoided`. That field counts swings this entity was on the RECEIVING end of; the
/// two numbers are both real and are about different people.
#[test]
fn how_a_swing_was_stopped_is_recorded_on_the_one_who_threw_it() {
    let big = biggest();
    let f = &big;
    let me = owner_of(f);

    /* THE IDENTITY THAT MAKES THE PANEL POSSIBLE: every swing either landed or was stopped, and
     * `Outcomes` accounts for every stop by name. */
    assert_eq!(
        me.landed + me.outcomes.total(),
        me.swings,
        "landed ({}) plus stopped ({}) is not swings ({}), so a Hit Results donut over these \
         numbers would not add to 100 percent",
        me.landed,
        me.outcomes.total(),
        me.swings
    );

    /* THE SUBTRACTION IS WRONG AND THIS IS THE PROOF, not an argument. The defended portion is
     * real and non-zero, so `swings - landed` is not misses. */
    let defended = me.outcomes.parried + me.outcomes.dodged + me.outcomes.blocked;
    assert!(
        defended > 0,
        "the owner's swings were never parried, dodged or blocked in this capture, so this test \
         cannot show that `swings - landed` overstates misses"
    );
    assert!(
        me.outcomes.missed < me.swings - me.landed,
        "misses ({}) is the whole of swings-minus-landed ({}), so nothing distinguishes a miss \
         from a parry and the subtraction would have been fine after all",
        me.outcomes.missed,
        me.swings - me.landed
    );

    /* The two arms the capture cannot reach are counted anyway and read zero, which is a true
     * statement about these bytes rather than a missing counter. */
    assert_eq!(me.outcomes.invulnerable, 0);
    assert_eq!(me.outcomes.rune_absorbed, 0);
}

/// DEFECT: reading `DamageKind::Spell`'s `resist` as "was this resisted".
///
/// The field is named for the CHECK and its content is the SCHOOL: `fire`, `cold`, `magic`. A panel
/// that read it as a yes/no would print the word `fire` where it wanted a boolean.
///
/// MELEE CARRIES NO ELEMENT AT ALL, so it is absent from this map rather than bucketed as "physical".
/// A pie that silently invented a physical slice would be claiming the log said something it did
/// not; the renderer states the remainder instead.
#[test]
fn the_element_map_holds_only_what_the_log_named_and_melee_is_absent() {
    let fights = folded();

    /* Somebody in this capture casts. If nobody did, the map would be empty everywhere and this
     * test would pass while proving nothing. */
    let any: usize = fights
        .iter()
        .flat_map(|f| f.participants.iter())
        .map(|p| p.by_school.len())
        .sum();
    assert!(
        any > 0,
        "no participant in the whole capture has an element tally, so nothing here is tested"
    );

    for f in &fights {
        for p in &f.participants {
            for t in &p.by_school {
                assert!(!t.school.is_empty(), "an element tally with no word");
                assert!(t.amount > 0 && t.hits > 0);
                /* The school is a lowercase word off the line, never a whole clause. */
                assert!(
                    !t.school.contains(' '),
                    "element {:?} is a phrase, so `resist` is being read as something else",
                    t.school
                );
            }
            /* The element split can never exceed what the entity dealt. */
            let by_school: u64 = p.by_school.iter().map(|t| t.amount).sum();
            assert!(by_school <= p.dealt);
        }
    }

    /* The owner casts nothing outgoing here, so his map is empty, and that is a measurement rather
     * than an oversight: it is why his damage-type pie needs a different capture. */
    let big = biggest();
    let me = owner_of(&big);
    assert!(
        me.by_school.is_empty(),
        "the owner has element tallies in a capture where he casts nothing: {:?}",
        me.by_school
    );
}
