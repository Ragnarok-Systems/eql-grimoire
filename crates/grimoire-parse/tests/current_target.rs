//! WHAT IS BEING FOUGHT NOW, AGAINST THE REFERENCE CAPTURE.
//!
//! The owner's screen read `IN COMBAT / A SPITE GOLEM / 08:06 / 15 players named`, and every figure
//! on it was about an eight minute CHAIN rather than about the mob in front of him. A fight runs
//! until combat goes quiet for `QUIET_SECONDS`, and on a raid night it never does.
use grimoire_parse::combat::parse;
use grimoire_parse::fights::{Fight, Fights};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");
const OWNER: &str = "Reviir";

/// The capture, folded, exactly as `fight_maps` folds it.
fn folded() -> Vec<Fight<'static>> {
    let mut agg = Fights::new().with_owner(OWNER);
    for raw in CAPTURE.lines() {
        if let Some(e) = parse(raw) {
            agg.push(e);
        }
    }
    agg.finish()
}

/// DEFECT: A LIVE METER NAMING THE BIGGEST MOB OF THE LAST EIGHT MINUTES AS THE CURRENT ONE.
///
/// `headline` and `current_target` answer two different questions, and this is the fight where the
/// capture proves they diverge: the first pull is 266 seconds long with twenty-two participants,
/// so the thing that soaked the most damage over the whole run is not the thing that was being hit
/// when it ended.
///
/// WHAT MUTATION MAKES THIS RED: `current_target` returning `headline`, or the fold forgetting to
/// stamp `last_taken_at`.
#[test]
fn the_current_target_is_the_last_thing_hit_and_not_the_biggest() {
    let fights = folded();
    assert!(fights.len() >= 3, "the capture folds several fights");

    let mut differ = 0;
    for f in &fights {
        let now = f.current_target();
        /* EVERY FIGHT IN THE CAPTURE HAS ONE, and the fallback is why. The Qeynos guards
         * fight has every point of its damage taken by `You`, which carries no name, so the
         * taken rule alone answers None there; the thing hitting is the answer instead. The
         * first draft of this test asserted `is_some` without knowing that, and the capture
         * corrected it. */
        assert!(
            now.is_some(),
            "a fight with damage in it named nothing as its current target"
        );
        if now != f.headline() {
            differ += 1;
        }
    }
    assert!(
        differ >= 1,
        "the two answers agreed on every fight in the capture, so this proves nothing about the \
         defect it was written for"
    );
}

/// AND THE ANSWER IS STABLE, whatever order the file listed the participants in.
///
/// Several entities share one printed second routinely (the log stamps to the second and the
/// capture puts up to 32 lines inside one), so without a tie-break the current target would flicker
/// between them frame to frame on a live meter.
#[test]
fn two_folds_of_one_log_name_the_same_current_target() {
    let a = folded();
    let b = folded();
    let a: Vec<_> = a.iter().map(|f| f.current_target()).collect();
    let b: Vec<_> = b.iter().map(|f| f.current_target()).collect();
    assert_eq!(a, b);
}

/// AND IT IS THE LAST THING HIT, WHICH IS A CLAIM ABOUT TIME AND NOT ABOUT SIZE.
///
/// Asserted against the participants' own stamps rather than against a name typed in here, so the
/// test cannot drift from the capture the day the fixture is re-cut.
#[test]
fn nothing_was_hit_after_the_entity_named_as_the_current_target() {
    for f in folded() {
        let Some(name) = f.current_target() else {
            continue;
        };
        /* ONLY THE FIGHTS THE TAKEN RULE ANSWERS. Where nothing named was hit at all, the
         * answer is the fallback -- the thing hitting YOU -- and "nothing was hit later" is not
         * the claim being made about it. */
        let Some(at) = f
            .participants
            .iter()
            .find(|p| p.name() == Some(name))
            .and_then(|p| p.last_taken_at)
        else {
            continue;
        };
        for p in &f.participants {
            if let (Some(other), Some(n)) = (p.last_taken_at, p.name()) {
                assert!(
                    other <= at,
                    "{n} was hit at {other}s, after {name} at {at}s, yet {name} is the current \
                     target"
                );
            }
        }
    }
}
