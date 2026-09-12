//! MOB HEALTH READ OFF A REAL NIGHT, through the real fold.
//!
//! # WHY THIS FILE EXISTS AND THE UNIT TESTS ARE NOT ENOUGH
//!
//! `hp`'s own tests drive hand-built fights and prove the RULES: one death per fight, players are
//! not mobs, three kills before a figure. They cannot prove the FEATURE, which is that real kills
//! off a real log produce a number worth putting on screen. Every hand-built fixture written for
//! this agreed with whatever the code did at the time; the log did not.
//!
//! # WHAT THE LOG SAID, IN ORDER
//!
//! A first pass summed damage between slay lines and produced a spread of 167% of the mean, which
//! looked like the feature was dead. It was the measurement that was wrong: a between-slays sum
//! cannot see a double pull, and it also split the mob in two because the game capitalises
//! inconsistently and that pass keyed on the spelling.
//!
//! Through the real fold, case folded, the same twenty-six kills look like this:
//!
//! ```text
//! 16706 18199
//! 20011 20012 20012 20014 20014 20016 20018 20019 20025 20025 20140   <- twelve inside 0.1%
//! 23014 23015 23017 23018 23018                                       <- five inside 0.02%
//! 34595 39035 39195 40919 41243 42792 43555 48066                     <- eight, roughly double
//! ```
//!
//! Not a noisy measurement of one number: several exact numbers mixed together. Two level variants
//! of one mob, and eight fights that held two princesses where only one died.
use grimoire_desktop::fights::fold_text;
use grimoire_desktop::hp;

/// 44,026 lines cut from `eqlog_Reviir_neriak.txt` around the night the princess was farmed.
/// Other players' chat is stripped: it proves nothing here and this repo is headed for the open.
const NIGHT: &str = include_str!("fixtures/princess-night.txt");
const OWNER: &str = "Reviir";
const PRINCESS: &str = "a thunder spirit princess";

fn book() -> std::collections::BTreeMap<String, hp::Reading> {
    let (rows, _) = fold_text(NIGHT, 30, Some(OWNER));
    assert!(rows.len() > 20, "the cut folds a night, not a fight");
    hp::read(&rows)
}

/// THE HEADLINE: a mob's hit points, off completed kills, with no hit point line in the log.
///
/// EverQuest prints no mob health at all, which is why the Live header carries no bar. A mob that
/// was engaged whole and died absorbed exactly what it could take, and twelve kills that agree to a
/// tenth of a percent are not a guess.
///
/// WHAT MUTATION MAKES THIS RED: `expect` falling back to the median across every sample, which
/// lands at 23,016 and describes neither variant.
#[test]
fn the_princess_hit_points_come_off_the_owners_own_kills() {
    let b = book();
    let r = b.get(PRINCESS).expect("she is in this night");

    assert!(r.n() >= 20, "only {} usable kills", r.n());

    let (value, of) = r.modal().expect("twelve kills that agree");
    assert!(
        (19_900..=20_200).contains(&value),
        "the measured hit points moved to {value}"
    );
    assert!(of >= 10, "only {of} kills agreed, out of {}", r.n());
    assert_eq!(r.expect(), Some(value));
}

/// AND THE WHOLE SET IS REFUSED, WHICH IS THE POINT OF SEPARATING THE TWO.
///
/// Across all twenty-six the spread is over a hundred percent, so `settled` is false and no figure
/// may stand for the lot. The answer comes from the group that agrees, and it comes with the size
/// of that group so a screen can print its own denominator.
#[test]
fn a_figure_never_stands_for_samples_that_disagree() {
    let b = book();
    let r = b.get(PRINCESS).expect("she is in this night");

    assert!(
        !r.settled(),
        "the whole spread was accepted: {:?}",
        r.spread()
    );
    assert!(
        r.spread().is_some_and(|s| s > 100),
        "this night no longer holds the double pulls this test is about"
    );
    /* THE RANGE SURVIVES for a screen that wants to show the shape rather than one number. */
    assert!(r.low().unwrap() < 19_000 && r.high().unwrap() > 40_000);
}

/// A MOB KILLED ONCE OR TWICE OFFERS NOTHING, and says so by offering nothing.
///
/// `a thunder spirit` appears twice in this night with 121 and 15,208, which are not two readings
/// of one number. Two samples cannot show a spread, so no figure is offered at all.
#[test]
fn a_mob_with_two_kills_offers_no_figure() {
    let b = book();
    let Some(r) = b.get("a thunder spirit") else {
        /* Not a failure: if a re-cut fixture drops it, there is simply nothing to assert. */
        return;
    };
    if r.n() >= hp::ENOUGH {
        return;
    }
    assert_eq!(
        r.expect(),
        None,
        "a figure was offered from {} kills",
        r.n()
    );
    assert_eq!(r.modal(), None);
}

/// THE GAME'S INCONSISTENT CAPITALISATION IS ONE MOB, NOT TWO.
///
/// The engine's own `same` folds names with `eq_ignore_ascii_case` and its doc says why. Keying
/// this book on the display spelling did not, and the same night came back as `A thunder spirit
/// princess` with eighteen samples beside `a thunder spirit princess` with eight: two readings of
/// one mob, each too thin to settle.
///
/// WHAT MUTATION MAKES THIS RED: keying `hp::read` on `who.text()` instead of the folded name.
#[test]
fn one_mob_spelled_two_ways_is_one_reading() {
    let b = book();
    let princesses: Vec<&String> = b
        .keys()
        .filter(|k| k.contains("thunder spirit princess"))
        .collect();
    assert_eq!(
        princesses.len(),
        1,
        "the same mob came back under {} keys: {princesses:?}",
        princesses.len()
    );
    /* AND THE KEY IS FOLDED WHILE THE SHOWN NAME IS THE LOG'S OWN. */
    let r = &b[PRINCESS];
    assert!(
        r.shown.eq_ignore_ascii_case(PRINCESS),
        "the name shown is not the one the log printed: {:?}",
        r.shown
    );
}
