//! WHAT THE HEADER SAYS AFTER A KILL, AGAINST THE OWNER'S OWN BYTES.
//!
//! # THE COMPLAINT
//!
//! "IN the log there right now can you not see that Thunder spirit princess died?"
//!
//! It was in the log, the grammar read it, and the header did not care: `current_target` was the
//! newest thing HIT, and a corpse stays the newest thing hit until something else is. So after a
//! kill the header went on naming the body through the looting and the buffing, until the next
//! pull landed.
//!
//! # THE FIXTURE IS REAL AND IT IS THE ACTUAL KILL
//!
//! `fixtures/princess-kill.txt` is 249 lines cut from `eqlog_Reviir_neriak.txt` around
//! `[Sun Sep 06 00:08:24 2026] You have slain a thunder spirit princess!`, ending in the looting.
//! Other players' chat has been stripped: it is not needed to prove anything here and this repo is
//! headed for the open.
//!
//! WHY THE REAL BYTES AND NOT A FIXTURE OF MY OWN. Every hand-built fight I wrote for this header
//! agreed with the code that was wrong. The log did not.
use grimoire_desktop::fights::fold_text;

const KILL: &str = include_str!("fixtures/princess-kill.txt");
const OWNER: &str = "Reviir";

/// DEFECT: THE HEADER NAMING A CORPSE AS THE THING BEING FOUGHT.
///
/// WHAT MUTATION MAKES THIS RED: dropping the death filter from `current_target`.
#[test]
fn a_mob_that_has_been_slain_is_not_what_you_are_fighting() {
    let (rows, _) = fold_text(KILL, 30, Some(OWNER));
    let f = rows.last().expect("the cut folds a fight");

    /* THE KILL IS IN THE FOLD. If this ever stops holding, the rest proves nothing. */
    let (slain, _) = f
        .last_slain()
        .expect("the fold kept the death the log printed");
    assert_eq!(slain, "a thunder spirit princess");

    /* AND SHE IS NOT THE CURRENT TARGET, because she is dead and nothing has been hit since. */
    assert_ne!(
        f.current_target().map(|(w, _)| w),
        Some("a thunder spirit princess"),
        "the header still names the mob the log says was slain"
    );
}

/// AND THE HEADER FALLS THROUGH TO THE KILL RATHER THAN TO THE FIGHT'S LABEL.
///
/// `headline` is the biggest thing in the fight, which after a one-mob pull is the very corpse this
/// test is about: falling through to it would print the same name in the same gold with nothing to
/// say it was dead. `last_slain` is what makes the two states look different.
#[test]
fn with_nothing_alive_left_the_header_has_a_kill_to_name() {
    let (rows, _) = fold_text(KILL, 30, Some(OWNER));
    let f = rows.last().expect("the cut folds a fight");

    assert_eq!(
        f.current_target(),
        None,
        "something is still alive in this cut, so it does not test the between-pulls state"
    );
    assert!(
        f.last_slain().is_some(),
        "with nothing alive and no kill to name, the header would fall through to the fight's \
         label and print the corpse as though it were the target"
    );
}
