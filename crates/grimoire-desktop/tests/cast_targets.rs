//! THE TARGET RESOLVER, AGAINST THE REAL CORPUS AND THE REAL LOG.
//!
//! `castmsg`'s own unit tests drive hand-built spells, which prove the mechanics and cannot prove
//! the FEATURE: that the wiki's sentences and the client's sentences actually line up. Only the
//! shipped `spells.json` and the owner's own capture can show that, and they are what this file
//! uses.
//!
//! WHY IT MATTERS. This is the evidence that turned three "impossible" panels back into buildable
//! ones. If the corpus and the log ever drift apart, every one of them silently resolves nothing,
//! and a feature that quietly does nothing is exactly what this file exists to prevent.
use grimoire_desktop::castmsg::Cast;
use grimoire_desktop::data::Snapshot;
use grimoire_parse::combat::{parse, Flavour, Reading};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

/// THE SAME OPT-OUT THE LIBRARY'S OWN TESTS HAVE, because this file could not reach theirs.
///
/// # A CLONE OF THIS REPO DOES NOT CARRY THE CORPUS
///
/// `crates/grimoire-desktop/data` is ~32MB ingested from eqlwiki.com and is deliberately not
/// committed: it is derived, it is third party, and this repo is headed open source. So a fresh
/// clone has no `spells.json`, and every test in this file panicked on it.
///
/// THE LIBRARY ALREADY SOLVED THIS AND THIS FILE COULD NOT USE THE SOLUTION. `data::testdata` is
/// behind `#[cfg(test)]`, and an integration test compiles the library WITHOUT that flag, so the
/// opt-out was unreachable from here and the rule was enforced in one half of the suite only.
///
/// # THE RULE IS UNCHANGED AND THAT IS THE POINT
///
/// ABSENT DATA IS STILL A FAILURE. A suite that goes green on an empty machine certifies nothing,
/// and this file's whole subject is whether the shipped corpus and the client's own sentences line
/// up: skipping it silently is the exact outcome its module doc says it exists to prevent.
///
/// THE ONLY WAY OUT IS THE OPERATOR SAYING SO, with the same variable and the same marker on the
/// same stream: `GRIMOIRE_NO_DATA=1`, printed as SKIPPED ON PURPOSE straight to the stderr handle
/// rather than through `println!`, which the harness captures and hides on a passing test.
fn corpus() -> Option<Snapshot> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    if std::env::var_os("GRIMOIRE_NO_DATA").is_some() && !root.join("spells.json").is_file() {
        use std::io::Write;
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(
            b"SKIPPED ON PURPOSE: cast_targets needs the wiki corpus, which is not committed\n",
        );
        let _ = err.flush();
        return None;
    }
    Some(Snapshot::load(&root).unwrap_or_else(|e| {
        panic!(
            "the shipped data must load: {e:?}. That corpus is not committed; put it in \
                 crates/grimoire-desktop/data, or set GRIMOIRE_NO_DATA=1 to skip on purpose."
        )
    }))
}

/// DEFECT: THE WHOLE FEATURE SILENTLY RESOLVING NOTHING.
///
/// Every part of this can be individually correct and the feature still dead: the wiki's spacing,
/// the placeholder's exact spelling, the grammar's classification, and the client's own wording all
/// have to agree. Nothing but the real bytes can show that they do.
///
/// WHAT MUTATION MAKES THIS RED: any change to the placeholder handling, the specificity floor, or
/// the landing gate that stops the corpus lining up with the log.
#[test]
fn the_shipped_corpus_resolves_real_lines_out_of_the_owners_own_log() {
    let Some(snap) = corpus() else { return };
    let cast = Cast::new(&snap.spells);
    assert!(
        cast.len() > 500,
        "only {} cast-on-other patterns were indexed out of {} spells; the corpus is not carrying \
         the messages the ingest was supposed to add",
        cast.len(),
        snap.spells.len()
    );

    let mut hits = Vec::new();
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Flavour(f) = entry.reading else {
            continue;
        };
        /* The line with its stamp stripped, which is what `resolve` takes. */
        let Some(body) = raw.split_once("] ").map(|(_, b)| b.trim()) else {
            continue;
        };
        if let Some(landed) = cast.resolve(body, f) {
            hits.push((landed.target, landed.candidates));
        }
    }

    assert!(
        hits.len() >= 20,
        "only {} lines in the capture resolved to a spell and a target; the mechanism that makes \
         charm pets and damage-shield attribution possible is not working on real bytes",
        hits.len()
    );

    /* THE TARGETS ARE REAL ENTITIES FROM THE FIGHT, not fragments of sentences. Every one of them
     * must be a string the log actually uses as a name elsewhere. */
    for (target, candidates) in &hits {
        assert!(!target.is_empty());
        assert!(
            !candidates.is_empty(),
            "{target:?} resolved with no candidate spell"
        );
        assert!(
            target.len() < 40,
            "{target:?} is too long to be a name; a tail is carving the sentence in the wrong place"
        );
        /* A NAME AND NOT A SENTENCE FRAGMENT. The false positives the naive matcher produced all
         * began with an article or a possessive pronoun of the reader's own. */
        assert!(
            !target.starts_with("Your ") && !target.starts_with("your "),
            "{target:?} is a fragment of a fade line, so a fade reached the resolver"
        );
    }

    /* AND THE NAMES INCLUDE PEOPLE THE OWNER ACTUALLY PLAYED WITH. Measured: the capture's group is
     * Tanefilo, Poguhy, Fylasem, and the mobs include `A lurking mummy` and `A ghoul`. */
    let names: Vec<&str> = hits.iter().map(|(t, _)| t.as_str()).collect();
    assert!(
        names.contains(&"Tanefilo"),
        "the owner's own group member was never resolved: {names:?}"
    );
}

/// DEFECT: a fade line reaching the resolver through the real parser.
///
/// `castmsg`'s unit test proves the gate refuses a `BuffFaded`. This proves the real capture's fade
/// lines actually ARRIVE as `BuffFaded` and are therefore refused in practice, which is a different
/// claim and the one that matters.
#[test]
fn no_fade_line_in_the_capture_resolves_to_a_landing() {
    let Some(snap) = corpus() else { return };
    let cast = Cast::new(&snap.spells);

    let mut fades = 0;
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Flavour(Flavour::BuffFaded) = entry.reading else {
            continue;
        };
        fades += 1;
        let Some(body) = raw.split_once("] ").map(|(_, b)| b.trim()) else {
            continue;
        };
        assert_eq!(
            cast.resolve(body, Flavour::BuffFaded),
            None,
            "a fade line resolved: {body:?}"
        );
    }
    assert!(
        fades > 10,
        "only {fades} fade lines in the capture, so this proves little"
    );
}

/// DEFECT: ONE RECURRING EFFECT DRAWN AS SEVEN EFFECTS.
///
/// `Book::mine` is documented as WHAT IS CURRENTLY ON WHOM, and `screens::live` draws one row per
/// entry, so every row is a claim that a distinct effect is up. The `LandedOnSelf` arm pushed a
/// new entry per SENTENCE, and a damage shield, a regen or a recast dot prints its landing line
/// every time it fires.
///
/// MEASURED ON THE OWNER'S OWN CAPTURE, which is why this test is here and not in the unit tests:
/// `You feel your skin smolder.` appears seven times inside a 340 line span, comfortably inside
/// the 400 line window the panel reads. So the panel showed that buff seven times, in a column,
/// under a heading saying it is what is on you.
///
/// AND IT COMPOUNDS WITH THE FADE, which removes exactly one entry: after land, land, fade the
/// panel still carried a row for an effect the log had just said was gone.
///
/// WHAT MUTATION MAKES THIS RED: dropping the candidate-set check from the `LandedOnSelf` arm.
#[test]
fn a_recurring_effect_is_one_row_and_not_one_row_per_landing() {
    use grimoire_desktop::castmsg::Book;

    let Some(snap) = corpus() else { return };
    let cast = Cast::new(&snap.spells);
    let mut book = Book::default();

    /* THE WHOLE CAPTURE, THROUGH THE SAME GATE THE PANEL USES. */
    let mut landings = 0;
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Flavour(f) = entry.reading else {
            continue;
        };
        /* THE BODY IS TAKEN OFF THE RAW LINE EXACTLY AS `screens::live` TAKES IT, so this
         * test and the panel are reading the same string. */
        let Some(body) = raw.split_once("] ").map(|(_, b)| b.trim()) else {
            continue;
        };
        if matches!(f, Flavour::LandedOnSelf) {
            landings += 1;
        }
        book.read(&cast, body, f, &snap.spells);
    }
    assert!(
        landings > 10,
        "only {landings} landings on the reader in the capture, so this proves nothing"
    );

    /* NO TWO ROWS ARE THE SAME READING. The candidate set IS the identity: two spells sharing a
     * message are one ambiguous reading and the panel says so with `or N others`. */
    let mut seen: Vec<&Vec<String>> = Vec::new();
    for e in book.mine() {
        assert!(
            !seen.contains(&&e.candidates),
            "the panel draws {:?} twice, so one effect is shown as two",
            e.candidates
        );
        seen.push(&e.candidates);
    }

    /* AND THE LIST IS SHORTER THAN THE LANDINGS THAT BUILT IT, or the dedupe did nothing on a
     * capture that certainly repeats. */
    assert!(
        book.mine().len() < landings,
        "{} rows from {landings} landings: nothing was folded together",
        book.mine().len()
    );
}

/// DEFECT: AN EFFECT THE LOG SAID HAD ENDED STILL DRAWN AS ON YOU.
///
/// `Book::mine` is what the EFFECTS panel calls "on you", and the fade path is the only thing that
/// takes a row out of it. The capture roots the reader four times and lifts it four times, so a
/// fold over the whole file must end with no root on him.
///
/// WHAT MUTATION MAKES THIS RED: dropping the `remove` from the `BuffFaded` arm.
#[test]
fn an_effect_the_log_says_has_ended_is_not_still_drawn_as_on_you() {
    use grimoire_desktop::castmsg::Book;

    let Some(snap) = corpus() else { return };
    let cast = Cast::new(&snap.spells);
    let mut book = Book::default();
    let mut lands = 0;
    let mut fades = 0;

    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Flavour(f) = entry.reading else {
            continue;
        };
        let Some(body) = raw.split_once("] ").map(|(_, b)| b.trim()) else {
            continue;
        };
        if body == "Your legs feel weak." {
            lands += 1;
        }
        if body == "Strength returns to your legs." {
            fades += 1;
        }
        book.read(&cast, body, f, &snap.spells);
    }

    assert!(lands > 0 && fades > 0, "{lands} lands, {fades} fades");
    /* THE LAST OF THE TWO IS THE FADE, so nothing should be left rooted. */
    assert!(
        !book
            .mine()
            .iter()
            .any(|e| e.candidates.iter().any(|c| c == "Ghoul Root")),
        "the panel still lists Ghoul Root on the reader after the log lifted it: {:?}",
        book.mine()
    );
}
