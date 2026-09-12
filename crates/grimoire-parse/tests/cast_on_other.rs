//! HOW THE GRAMMAR CLASSIFIES A CAST-ON-OTHER LINE, measured before anything is built on it.
//!
//! # Why this file exists
//!
//! The wiki records three message strings per spell, and the cast-on-other one uses `Someone` as a
//! placeholder that the game substitutes a real name into. That means a landing line NAMES ITS
//! TARGET, which is the only route to three things this project had written off: which mob is
//! charmed, who a damage shield was put on, and when a buff started on somebody else.
//!
//! Matching those patterns against the raw capture recovers 33 distinct lines with real names in
//! them, and also some FALSE POSITIVES: a fade line such as `Your endurance to magic fades.` is
//! caught by any spell whose cast-on-other ends in ` fades.`, capturing "Your endurance to magic"
//! as though it were a name.
//!
//! THE QUESTION THIS FILE ANSWERS is whether the grammar already separates the two, because if it
//! does then a resolver never has to guess: it runs only on lines the parser has ALREADY called a
//! landing, and the fade lines are unreachable by construction. Guessing is the alternative, and
//! this project does not guess.
use grimoire_parse::combat::{parse, Flavour, Reading};

/// Every distinct line in the capture that a cast-on-other pattern matched with a plausible name.
/// Extracted mechanically; kept verbatim so the classification below is about real bytes.
const MATCHED: &[&str] = &[
    "Tanefilo has been diseased.",
    "A lurking mummy staggers.",
    "A lurking mummy is bathed in fire.",
    "Tanefilo feels better.",
    "A greater skeleton is bathed in fire.",
    "A greater skeleton is slashed by shards of ice.",
    "A ghoul's skin shreds as blades rain down from above.",
    "Poguhy stumbles.",
    "A ghoul's skin blisters as fire rains down from above.",
    "A tormented dead is bathed in fire.",
    "A dark boned skeleton staggers.",
    "Torklar Battlemaster's skin blisters as fire rains down from above.",
];

/// The false positives the naive matcher produced: fade lines ending in a word a landing message
/// also ends in.
const FADES: &[&str] = &[
    "Your endurance to magic fades.",
    "The intellectual advancement fades.",
    "The inner fire fades.",
];

fn reading(body: &str) -> Reading<'_> {
    let line = format!("[Wed Jul 15 23:17:00 2026] {body}");
    let entry = parse(Box::leak(line.into_boxed_str())).expect("a stamped line parses");
    entry.reading
}

/// DEFECT: a target resolver that runs on every line and calls a fade a landing.
///
/// If the grammar separates these two families, the resolver is gated on the classification and the
/// false positives are unreachable rather than filtered by a heuristic. If it does not, the resolver
/// needs its own discipline and this test says so out loud instead of letting somebody assume.
#[test]
fn the_grammar_tells_a_landing_apart_from_a_fade() {
    let mut landed = 0;
    let mut other = Vec::new();
    for body in MATCHED {
        match reading(body) {
            Reading::Flavour(Flavour::LandedOnOther) => landed += 1,
            r => other.push((*body, format!("{r:?}"))),
        }
    }

    let mut faded = 0;
    let mut fade_other = Vec::new();
    for body in FADES {
        match reading(body) {
            Reading::Flavour(Flavour::BuffFaded) => faded += 1,
            r => fade_other.push((*body, format!("{r:?}"))),
        }
    }

    /* THE MEASUREMENT IS PRINTED WHETHER IT PASSES OR NOT, because the value of this file is the
     * fact it establishes, and a silent pass teaches nobody what the grammar actually does. */
    println!(
        "of {} matched lines, {landed} read as LandedOnOther",
        MATCHED.len()
    );
    for (l, r) in &other {
        println!("  NOT a landing: {l:?} -> {r}");
    }
    println!("of {} fade lines, {faded} read as BuffFaded", FADES.len());
    for (l, r) in &fade_other {
        println!("  NOT a fade: {l:?} -> {r}");
    }

    assert!(
        faded == FADES.len(),
        "a fade line does not read as BuffFaded, so gating a resolver on the classification would \
         not keep fades out and the resolver needs its own rule"
    );
    assert!(
        landed > 0,
        "no cast-on-other line in the capture reads as LandedOnOther, so the classification cannot \
         gate a resolver at all"
    );
}
