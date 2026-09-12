//! WHAT THE OWNER'S OWN LOG PROVES ABOUT THE PEOPLE IN IT, against the shipped corpus.
//!
//! `class`'s unit tests drive five hand-built spells, which prove the covering rule and cannot
//! prove the FEATURE: that the client's spell names and the wiki's spell names are the same strings,
//! and that the wiki's class tables are populated enough to settle anybody. Only `spells.json` and
//! the capture can show that, and they are what this file uses.
//!
//! WHY IT MATTERS. This is the evidence that turned "colour the rows by rank" into "say what people
//! ARE". If the corpus and the log ever drift apart, every reading here silently comes back empty
//! and the tags disappear from the tables with nothing on screen to say why.
use grimoire_desktop::class::Book;
use grimoire_desktop::data::Snapshot;
use grimoire_parse::combat::{parse, Actor, Event, Reading};

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

/// THE SAME OPT-OUT `cast_targets` HAS, AND FOR THE SAME REASON.
///
/// The wiki corpus under `crates/grimoire-desktop/data` is not committed (it is derived, third
/// party, and this repo is headed open source), so a fresh clone has no `spells.json` and this
/// file panicked on it. `data::testdata` already solves this for the library's own tests and is
/// unreachable from here: it is behind `#[cfg(test)]`, and an integration test compiles the
/// library WITHOUT that flag.
///
/// ABSENT DATA IS STILL A FAILURE unless the operator says otherwise, because a suite that goes
/// green on an empty machine certifies nothing. `GRIMOIRE_NO_DATA=1` is the one way out and it
/// prints SKIPPED ON PURPOSE straight to the stderr handle, which the harness does not capture.
fn corpus() -> Option<Snapshot> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    if std::env::var_os("GRIMOIRE_NO_DATA").is_some() && !root.join("spells.json").is_file() {
        use std::io::Write;
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(
            b"SKIPPED ON PURPOSE: class_from_log needs the wiki corpus, which is not committed\n",
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

/// Every `X begins casting Y.` in the capture, through the grammar rather than a regex.
fn book() -> Book {
    let mut b = Book::default();
    for raw in CAPTURE.lines() {
        let Some(entry) = parse(raw) else { continue };
        let Reading::Event(Event::CastStart { caster, spell }) = entry.reading else {
            continue;
        };
        if let Actor::Named(who) = caster {
            b.saw(who, spell);
        }
    }
    b
}

/// DEFECT: THE WHOLE READING SILENTLY COMING BACK EMPTY.
///
/// Every part can be individually right and the feature still dead: the client's spelling, the
/// wiki's spelling, the grammar's `CastStart` arm and the corpus's class tables all have to line
/// up. Nothing but the real bytes shows that they do.
///
/// MEASURED, AND THESE ARE THE NUMBERS THE FEATURE RESTS ON. The capture names six casters besides
/// the reader, and three of them are settled outright by a spell only one class can cast.
///
/// WHAT MUTATION MAKES THIS RED: any change to the name match, the `CastStart` arm, or the covering
/// rule that stops the corpus lining up with the log.
#[test]
fn the_shipped_corpus_reads_real_classes_out_of_the_owners_own_log() {
    let Some(snap) = corpus() else { return };
    let b = book();

    assert!(
        b.len() >= 5,
        "only {} casters were seen in the capture, so this proves nothing",
        b.len()
    );

    let mut settled = 0;
    let mut narrowed = 0;
    for who in b.casters() {
        let seen = b.of(who, &snap.spells).expect("a caster the book has seen");
        if !seen.certain.is_empty() {
            settled += 1;
        } else if !seen.narrowed.is_empty() {
            narrowed += 1;
        }
    }
    assert!(
        settled >= 3,
        "only {settled} of {} casters were settled; the corpus and the log have drifted apart",
        b.len()
    );
    assert!(
        narrowed >= 1,
        "nothing was narrowed, which the capture does"
    );
}

/// AND THE THREE IT SETTLES ARE THE THREE THE LOG SUPPORTS, BY NAME.
///
/// Pinned against the capture rather than described, because the interesting one is Poguhy: he
/// casts `Cascade of Hail` (Druid alone) and `Rain of Blades` (Magician alone), and an intersection
/// answers EMPTY for him. He is a Druid AND a Magician, which is what an EverQuest Legends trio is
/// and what this whole module exists to read.
///
/// WHAT MUTATION MAKES THIS RED: intersecting instead of covering, which empties Poguhy and Rykabe.
#[test]
fn the_capture_names_a_wizard_an_enchanter_and_a_druid_magician() {
    let Some(snap) = corpus() else { return };
    let b = book();
    let of = |who: &str| {
        b.of(who, &snap.spells)
            .unwrap_or_else(|| panic!("{who} casts nothing in the capture"))
    };

    assert_eq!(of("Fylasem").certain, vec!["Wizard"]);
    assert_eq!(of("Losumyda").certain, vec!["Enchanter"]);

    /* THE TRIO CASE. Two classes, both proved, neither able to cast the other's spell. */
    let poguhy = of("Poguhy");
    assert_eq!(poguhy.certain, vec!["Druid", "Magician"]);
    assert_eq!(poguhy.tag().as_deref(), Some("Druid/Magician"));

    /* AND THE NARROWED CASE STAYS HONEST: a Magician who healed is a Magician plus something, and
     * this app does not pick which of the six. */
    let rykabe = of("Rykabe");
    assert_eq!(rykabe.certain, vec!["Magician"]);
    assert_eq!(rykabe.tag().as_deref(), Some("Magician +1"));
    assert!(
        rykabe.narrowed.iter().any(|l| l.len() > 1),
        "the heal narrowed to a single class, which no spell in the capture does: {rykabe:?}"
    );
}

/// A CASTER THE CORPUS CANNOT PLACE GETS NO TAG RATHER THAN A WRONG ONE.
///
/// `Tanefi` casts one spell in the capture, `Water Elemental Attack`, which the wiki page carries
/// no class table for. The honest answer is nothing at all beside the name.
#[test]
fn a_caster_whose_only_spell_is_unlisted_is_left_alone() {
    let Some(snap) = corpus() else { return };
    let b = book();
    let Some(seen) = b.of("Tanefi", &snap.spells) else {
        panic!("Tanefi casts once in the capture");
    };
    assert!(seen.certain.is_empty(), "{seen:?}");
    assert_eq!(seen.tag(), None, "an unplaceable spell became a class");
    assert!(
        !seen.unlisted.is_empty(),
        "the gap was not recorded, so a screen cannot say the reading is partial"
    );
}
